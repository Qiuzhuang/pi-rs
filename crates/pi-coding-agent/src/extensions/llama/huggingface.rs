//! Port of `packages/coding-agent/src/extensions/llama/huggingface.ts`.

use crate::core::tools::js_util::locale_compare;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const DEFAULT_HUGGING_FACE_URL: &str = "https://huggingface.co";
static QUANTIZATION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[-_.])((?:UD-)?(?:IQ\d(?:_[A-Z0-9]+)+|Q\d(?:_[A-Z0-9]+)+|BF16|F16|F32|MXFP\d(?:_[A-Z0-9]+)*))$")
        .unwrap()
});
static SHARD_SUFFIX_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-\d{5}-of-\d{5}$").unwrap());
static RATE_LIMIT_DELAY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|;)t=(\d+)").unwrap());
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceModel {
    pub id: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub downloads: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceQuantization {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
}

/// `false | "auto" | "manual"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HuggingFaceModelDetailsGated {
    Bool(bool),
    String(String),
}

impl HuggingFaceModelDetailsGated {
    /// Whether access must be requested (`"auto"` or `"manual"`).
    pub fn is_gated(&self) -> bool {
        !matches!(self, Self::Bool(false))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceModelDetails {
    pub id: String,
    pub gated: HuggingFaceModelDetailsGated,
    pub quantizations: Vec<HuggingFaceQuantization>,
}

fn payload_error(payload: Option<&Value>, fallback: &str) -> String {
    payload
        .and_then(|p| p.get("error"))
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

fn parse_rate_limit_delay(value: Option<&str>) -> Option<f64> {
    RATE_LIMIT_DELAY.captures(value?).and_then(|c| c[1].parse().ok())
}

async fn read_token(path: &str) -> Option<String> {
    let token = tokio::fs::read_to_string(path).await.ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Find a Hugging Face token: `HF_TOKEN`, then the token files `huggingface_hub` uses. `env` is a
/// JSON object of environment variables; `None` reads the process environment.
pub async fn find_hugging_face_token(env: Option<Value>) -> anyhow::Result<Option<String>> {
    let get = |name: &str| -> Option<String> {
        match &env {
            Some(env) => env.get(name).and_then(Value::as_str).map(str::to_string),
            None => std::env::var(name).ok(),
        }
    };
    if let Some(token) = get("HF_TOKEN").map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        return Ok(Some(token));
    }
    let join = |base: &str, parts: &[&str]| {
        let mut path = std::path::PathBuf::from(base);
        for part in parts {
            path.push(part);
        }
        path.to_string_lossy().into_owned()
    };
    let home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
    let paths: Vec<String> = [
        get("HF_TOKEN_PATH"),
        get("HF_HOME").filter(|h| !h.is_empty()).map(|h| join(&h, &["token"])),
        get("XDG_CACHE_HOME").filter(|h| !h.is_empty()).map(|h| join(&h, &["huggingface", "token"])),
        Some(join(&home, &[".cache", "huggingface", "token"])),
    ]
    .into_iter()
    .flatten()
    .filter(|path| !path.is_empty())
    .collect();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Some(token) = read_token(&path).await {
            return Ok(Some(token));
        }
    }
    Ok(None)
}

/// `encodeURIComponent`.
fn encode_uri_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub struct HuggingFaceClient {
    token: Option<String>,
    base_url: String,
}

impl HuggingFaceClient {
    pub fn new(token: Option<&str>, base_url: Option<&str>) -> Self {
        Self {
            token: token.map(str::to_string),
            base_url: base_url.unwrap_or(DEFAULT_HUGGING_FACE_URL).trim_end_matches('/').to_string(),
        }
    }

    async fn request(&self, path: &str, signal: Option<&CancellationToken>) -> anyhow::Result<Option<Value>> {
        let url = format!("{}{path}", self.base_url);
        let mut request = pi_ai::utils::http::client_for(&url, None)?.get(&url);
        if let Some(token) = self.token.as_deref().filter(|t| !t.is_empty()) {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let never = CancellationToken::new();
        let signal = signal.unwrap_or(&never);
        let exchange = async {
            let response = request.send().await.map_err(|error| anyhow::Error::new(pi_ai::utils::http::FetchFailed::new(&error)))?;
            let status = response.status();
            let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
            let (retry_after, rate_limit) = (header("retry-after"), header("ratelimit"));
            let payload = response.bytes().await.ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            anyhow::Ok((status, retry_after, rate_limit, payload))
        };
        let (status, retry_after, rate_limit, payload) = tokio::select! {
            result = exchange => result?,
            _ = tokio::time::sleep(Duration::from_secs(15)) => anyhow::bail!("The operation was aborted due to timeout"),
            _ = signal.cancelled() => anyhow::bail!("This operation was aborted"),
        };
        if !status.is_success() {
            let fallback = format!("Hugging Face returned HTTP {}", status.as_u16());
            if status.as_u16() == 429 {
                let delay = retry_after
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .filter(|d| *d != 0.0 && !d.is_nan())
                    .or_else(|| parse_rate_limit_delay(rate_limit.as_deref()));
                match delay.filter(|d| *d != 0.0) {
                    Some(delay) => anyhow::bail!(
                        "Hugging Face rate limit reached; retry in {}s",
                        crate::core::tools::js_util::js_number_string(delay)
                    ),
                    None => anyhow::bail!("Hugging Face rate limit reached"),
                }
            }
            anyhow::bail!(payload_error(payload.as_ref(), &fallback));
        }
        Ok(payload)
    }

    pub async fn search(&self, query: &str, signal: Option<CancellationToken>) -> anyhow::Result<Vec<HuggingFaceModel>> {
        let params = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("search", query)
            .append_pair("filter", "gguf")
            .append_pair("sort", "downloads")
            .append_pair("direction", "-1")
            .append_pair("limit", "20")
            .finish();
        let payload = self.request(&format!("/api/models?{params}"), signal.as_ref()).await?;
        let Some(Value::Array(items)) = payload else {
            anyhow::bail!("Hugging Face returned invalid search results");
        };
        Ok(items
            .iter()
            .filter_map(|value| {
                let id = value.get("id")?.as_str()?.to_string();
                Some(HuggingFaceModel { id, downloads: value.get("downloads").and_then(Value::as_f64).unwrap_or(0.0) })
            })
            .collect())
    }

    pub async fn details(&self, id: &str, signal: Option<CancellationToken>) -> anyhow::Result<HuggingFaceModelDetails> {
        let encoded_id = id.split('/').map(encode_uri_component).collect::<Vec<_>>().join("/");
        let payload = self.request(&format!("/api/models/{encoded_id}?blobs=true"), signal.as_ref()).await?;
        let Some(Value::Object(model)) = payload else {
            anyhow::bail!("Hugging Face returned invalid model details");
        };
        // Insertion order of the quantizations, then their sizes.
        let mut order: Vec<String> = Vec::new();
        let mut sizes: HashMap<String, (f64, bool)> = HashMap::new();
        if let Some(Value::Array(siblings)) = model.get("siblings") {
            for file in siblings {
                let Some(name) = file.get("rfilename").and_then(Value::as_str) else { continue };
                if !name.to_lowercase().ends_with(".gguf") {
                    continue;
                }
                let filename = name.rsplit('/').next().unwrap_or(name);
                if filename.to_lowercase().starts_with("mmproj") {
                    continue;
                }
                let stem = SHARD_SUFFIX_PATTERN.replace(&filename[..filename.len() - 5], "").into_owned();
                let Some(quantization) = QUANTIZATION_PATTERN.captures(&stem).map(|c| c[1].to_uppercase()) else {
                    continue;
                };
                let current = sizes.entry(quantization.clone()).or_insert_with(|| {
                    order.push(quantization.clone());
                    (0.0, true)
                });
                match file.get("size").and_then(Value::as_f64) {
                    Some(size) => current.0 += size,
                    None => current.1 = false,
                }
            }
        }
        let mut quantizations: Vec<HuggingFaceQuantization> = order
            .into_iter()
            .map(|name| {
                let (total, complete) = sizes[&name];
                HuggingFaceQuantization { name, size: complete.then_some(total as i64) }
            })
            .collect();
        quantizations.sort_by(|left, right| {
            use std::cmp::Ordering;
            if left.name == "Q4_K_M" {
                return Ordering::Less;
            }
            if right.name == "Q4_K_M" {
                return Ordering::Greater;
            }
            let size = |q: &HuggingFaceQuantization| q.size.map(|s| s as f64).unwrap_or(MAX_SAFE_INTEGER);
            size(left).partial_cmp(&size(right)).unwrap_or(Ordering::Equal).then_with(|| locale_compare(&left.name, &right.name))
        });
        let gated = match model.get("gated").and_then(Value::as_str) {
            Some(gated @ ("auto" | "manual")) => HuggingFaceModelDetailsGated::String(gated.to_string()),
            _ => HuggingFaceModelDetailsGated::Bool(false),
        };
        Ok(HuggingFaceModelDetails {
            id: model.get("id").and_then(Value::as_str).unwrap_or(id).to_string(),
            gated,
            quantizations,
        })
    }
}
