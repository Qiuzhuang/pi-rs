//! Port of `packages/coding-agent/src/extensions/llama/client.ts`.

use crate::core::tools::js_util::js_to_fixed;
use futures::StreamExt;
use indexmap::IndexMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LlamaModelStatus {
    #[default]
    #[serde(rename = "unloaded")]
    Unloaded,
    #[serde(rename = "loading")]
    Loading,
    #[serde(rename = "loaded")]
    Loaded,
    #[serde(rename = "downloading")]
    Downloading,
    #[serde(rename = "sleeping")]
    Sleeping,
    /// A status this port does not know (TS keeps any string).
    #[serde(other, rename = "unknown")]
    Unknown,
}

impl LlamaModelStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unloaded => "unloaded",
            Self::Loading => "loading",
            Self::Loaded => "loaded",
            Self::Downloading => "downloading",
            Self::Sleeping => "sleeping",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoStatusProgress {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub done: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub total: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoStatus {
    pub value: LlamaModelStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<bool>,
    #[serde(rename = "exit_code", default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<IndexMap<String, LlamaModelInfoStatusProgress>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoArchitecture {
    #[serde(
        rename = "input_modalities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_modalities: Option<Vec<String>>,
    #[serde(
        rename = "output_modalities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_modalities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoMeta {
    #[serde(
        rename = "n_ctx",
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub n_ctx: Option<f64>,
    #[serde(
        rename = "n_ctx_train",
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub n_ctx_train: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "pi_ai::serde_util::opt_js_f64")]
    pub size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ftype: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    pub status: LlamaModelInfoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<LlamaModelInfoArchitecture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<LlamaModelInfoMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelsResponse {
    pub data: Vec<LlamaModelInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaServerProps {
    #[serde(
        rename = "models_autoload",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub models_autoload: Option<bool>,
    #[serde(
        rename = "chat_template",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelEvent {
    pub model: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaProgress {
    pub message: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}


/// Per-request timeout of management requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

fn error_message(payload: Option<&Value>, fallback: &str) -> String {
    payload
        .and_then(|p| p.get("error"))
        .filter(|e| e.is_object())
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

fn is_model_info(value: &Value) -> bool {
    value.get("id").is_some_and(Value::is_string)
        && value.get("status").and_then(|s| s.get("value")).is_some_and(Value::is_string)
}

/// Decode a catalog entry. Fields of unexpected types are dropped instead of failing, like the TS
/// client, which only checks `id` and `status.value`.
fn decode_model_info(value: &Value) -> LlamaModelInfo {
    serde_json::from_value(value.clone()).unwrap_or_else(|_| LlamaModelInfo {
        id: value["id"].as_str().unwrap_or_default().to_string(),
        status: LlamaModelInfoStatus {
            value: serde_json::from_value(value["status"]["value"].clone()).unwrap_or(LlamaModelStatus::Unknown),
            ..Default::default()
        },
        ..Default::default()
    })
}

/// `sleep(ms, signal)`: rejects with "Cancelled" when the signal fires.
async fn sleep(ms: u64, signal: Option<&CancellationToken>) -> anyhow::Result<()> {
    let never = CancellationToken::new();
    let signal = signal.unwrap_or(&never);
    if signal.is_cancelled() {
        anyhow::bail!("Cancelled");
    }
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => Ok(()),
        _ = signal.cancelled() => anyhow::bail!("Cancelled"),
    }
}

fn parse_load_progress(data: Option<&Value>) -> Option<LlamaProgress> {
    let progress = data?.as_object()?.get("progress")?.as_object()?;
    let stage = progress
        .get("current")
        .and_then(Value::as_str)
        .or_else(|| progress.get("stage").and_then(Value::as_str));
    let stages: Vec<&str> = match progress.get("stages") {
        Some(Value::Array(stages)) => stages.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let stage_ratio = progress.get("value").and_then(Value::as_f64).map(|v| v.clamp(0.0, 1.0));
    let mut ratio = stage_ratio;
    if let Some(stage) = stage
        && !stages.is_empty()
        && let Some(index) = stages.iter().position(|s| *s == stage)
    {
        ratio = Some((index as f64 + stage_ratio.unwrap_or(0.0)) / stages.len() as f64);
    }
    Some(LlamaProgress {
        message: match stage {
            Some(stage) => format!("Loading {}", stage.replace('_', " ")),
            None => "Loading model".to_string(),
        },
        ratio,
        detail: None,
    })
}

fn parse_download_progress(data: Option<&Value>) -> Option<LlamaProgress> {
    let data = data?.as_object()?;
    let files = match data.get("progress") {
        Some(Value::Object(nested)) => nested,
        _ => data,
    };
    let mut done = 0.0;
    let mut total = 0.0;
    for value in files.values() {
        let (Some(entry_done), Some(entry_total)) =
            (value.get("done").and_then(Value::as_f64), value.get("total").and_then(Value::as_f64))
        else {
            continue;
        };
        done += entry_done;
        total += entry_total;
    }
    if total <= 0.0 {
        return None;
    }
    Some(LlamaProgress {
        message: "Downloading model".to_string(),
        ratio: Some(done / total),
        detail: Some(format!("{} / {}", format_bytes_f64(done), format_bytes_f64(total))),
    })
}

fn js_number(value: f64) -> String {
    crate::core::tools::js_util::js_number_string(value)
}

fn format_bytes_f64(bytes: f64) -> String {
    if bytes < 1024.0 {
        return format!("{} B", js_number(bytes));
    }
    let units = ["KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes / 1024.0;
    let mut unit = units[0];
    for candidate in units.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = candidate;
    }
    format!("{} {unit}", if value >= 10.0 { js_to_fixed(value, 1) } else { js_to_fixed(value, 2) })
}

pub fn format_bytes(bytes: i64) -> String {
    format_bytes_f64(bytes as f64)
}

pub fn normalize_llama_server_url(value: &str) -> anyhow::Result<String> {
    let mut url = url::Url::parse(crate::core::tools::js_util::js_trim(value))
        .map_err(|_| anyhow::anyhow!("Invalid URL"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        anyhow::bail!("Server URL must use http or https");
    }
    url.set_fragment(None);
    url.set_query(None);
    let trimmed = url.path().trim_end_matches('/');
    let path = trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    let text = url.to_string();
    Ok(text.strip_suffix('/').unwrap_or(&text).to_string())
}

pub fn llama_inference_url(server_url: &str) -> String {
    let normalized = normalize_llama_server_url(server_url).unwrap_or_else(|_| server_url.to_string());
    format!("{normalized}/v1")
}

#[derive(Debug, Clone, Default)]
pub struct LlamaClientListOptions {
    pub reload: Option<bool>,
    pub signal: Option<CancellationToken>,
}

#[derive(Debug, Clone, Default)]
pub struct LlamaClientPropsOptions {
    pub model: Option<String>,
    pub signal: Option<CancellationToken>,
}

/// Client of the llama.cpp router management API (`/models`, `/props`, `/models/sse`).
#[derive(Debug, Clone)]
pub struct LlamaClient {
    server_url: String,
    api_key: Option<String>,
}

impl LlamaClient {
    /// Fails like `new URL()` for an invalid server URL; see [`Self::try_new`].
    pub fn new(server_url: &str, api_key: Option<&str>) -> Self {
        Self::try_new(server_url, api_key).unwrap_or_else(|_| Self {
            server_url: server_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(str::to_string),
        })
    }

    pub fn try_new(server_url: &str, api_key: Option<&str>) -> anyhow::Result<Self> {
        Ok(Self { server_url: normalize_llama_server_url(server_url)?, api_key: api_key.map(str::to_string) })
    }

    pub fn server_url(&self) -> String {
        self.server_url.clone()
    }

    fn http(&self) -> anyhow::Result<reqwest::Client> {
        pi_ai::utils::http::client_for(&self.server_url, None)
    }

    async fn request(
        &self,
        path: &str,
        method: reqwest::Method,
        body: Option<Value>,
        signal: Option<&CancellationToken>,
    ) -> anyhow::Result<Option<Value>> {
        let mut request = self.http()?.request(method, format!("{}{path}", self.server_url));
        if let Some(body) = &body {
            request = request.header("Content-Type", "application/json").body(serde_json::to_vec(body)?);
        }
        if let Some(key) = self.api_key.as_deref().filter(|k| !k.is_empty()) {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let never = CancellationToken::new();
        let signal = signal.unwrap_or(&never);
        let exchange = async {
            let response = request.send().await.map_err(|error| anyhow::anyhow!("fetch failed: {error}"))?;
            let status = response.status();
            let payload = response.bytes().await.ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            anyhow::Ok((status, payload))
        };
        let (status, payload) = tokio::select! {
            result = exchange => result?,
            _ = tokio::time::sleep(REQUEST_TIMEOUT) => anyhow::bail!("The operation was aborted due to timeout"),
            _ = signal.cancelled() => anyhow::bail!("This operation was aborted"),
        };
        if !status.is_success() {
            anyhow::bail!(error_message(payload.as_ref(), &format!("llama.cpp returned HTTP {}", status.as_u16())));
        }
        Ok(payload)
    }

    pub async fn list(&self, options: Option<&LlamaClientListOptions>) -> anyhow::Result<Vec<LlamaModelInfo>> {
        let reload = options.and_then(|o| o.reload) == Some(true);
        let payload = self
            .request(
                &format!("/models{}", if reload { "?reload=1" } else { "" }),
                reqwest::Method::GET,
                None,
                options.and_then(|o| o.signal.as_ref()),
            )
            .await?;
        let Some(Value::Array(data)) = payload.as_ref().and_then(|p| p.get("data")) else {
            anyhow::bail!("llama.cpp returned an invalid model catalog");
        };
        if !data.iter().all(is_model_info) {
            anyhow::bail!("Server is not running in llama.cpp router mode");
        }
        Ok(data.iter().map(decode_model_info).collect())
    }

    pub async fn props(&self, options: Option<&LlamaClientPropsOptions>) -> anyhow::Result<LlamaServerProps> {
        let query = match options.and_then(|o| o.model.as_deref()).filter(|m| !m.is_empty()) {
            Some(model) => format!(
                "?{}",
                url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("model", model)
                    .append_pair("autoload", "false")
                    .finish()
            ),
            None => String::new(),
        };
        let payload = self
            .request(&format!("/props{query}"), reqwest::Method::GET, None, options.and_then(|o| o.signal.as_ref()))
            .await?;
        let Some(Value::Object(payload)) = payload else { return Ok(LlamaServerProps::default()) };
        Ok(LlamaServerProps {
            models_autoload: payload.get("models_autoload").and_then(Value::as_bool),
            chat_template: payload.get("chat_template").and_then(Value::as_str).map(str::to_string),
        })
    }

    pub async fn load(&self, model: &str, signal: Option<CancellationToken>) -> anyhow::Result<()> {
        self.request("/models/load", reqwest::Method::POST, Some(json!({ "model": model })), signal.as_ref()).await?;
        Ok(())
    }

    pub async fn unload(&self, model: &str, signal: Option<CancellationToken>) -> anyhow::Result<()> {
        self.request("/models/unload", reqwest::Method::POST, Some(json!({ "model": model })), signal.as_ref())
            .await?;
        Ok(())
    }

    pub async fn unload_and_wait(&self, model: &str, signal: Option<CancellationToken>) -> anyhow::Result<()> {
        self.unload(model, signal.clone()).await?;
        let list_options = LlamaClientListOptions { reload: None, signal: signal.clone() };
        loop {
            let entry = self.list(Some(&list_options)).await?.into_iter().find(|candidate| candidate.id == model);
            match entry {
                None => return Ok(()),
                Some(entry) if entry.status.value == LlamaModelStatus::Unloaded => return Ok(()),
                Some(_) => sleep(100, signal.as_ref()).await?,
            }
        }
    }

    pub async fn download(&self, model: &str, signal: Option<CancellationToken>) -> anyhow::Result<()> {
        self.request("/models", reqwest::Method::POST, Some(json!({ "model": model })), signal.as_ref()).await?;
        Ok(())
    }

    pub async fn watch(
        &self,
        on_event: Arc<dyn Fn(LlamaModelEvent) + Send + Sync>,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<()> {
        let never = CancellationToken::new();
        let signal = signal.unwrap_or(never);
        let mut request = self.http()?.get(format!("{}/models/sse", self.server_url));
        if let Some(key) = self.api_key.as_deref().filter(|k| !k.is_empty()) {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let response = tokio::select! {
            response = request.send() => response.map_err(|error| anyhow::anyhow!("fetch failed: {error}"))?,
            _ = signal.cancelled() => anyhow::bail!("This operation was aborted"),
        };
        if !response.status().is_success() {
            anyhow::bail!("llama.cpp SSE returned HTTP {}", response.status().as_u16());
        }
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut pending: Vec<u8> = Vec::new();
        loop {
            let chunk = tokio::select! {
                chunk = stream.next() => chunk,
                _ = signal.cancelled() => anyhow::bail!("This operation was aborted"),
            };
            let Some(chunk) = chunk else { break };
            pending.extend_from_slice(&chunk?);
            // Decode complete UTF-8 sequences, keeping a split character for the next chunk.
            let valid = match std::str::from_utf8(&pending) {
                Ok(text) => text.len(),
                Err(error) => error.valid_up_to(),
            };
            buffer.push_str(&String::from_utf8_lossy(&pending[..valid]));
            pending.drain(..valid);
            buffer = buffer.replace("\r\n", "\n");
            while let Some(boundary) = buffer.find("\n\n") {
                let frame = buffer[..boundary].to_string();
                buffer = buffer[boundary + 2..].to_string();
                let data = frame
                    .split('\n')
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(|line| line.trim_start())
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    continue;
                }
                // Ignore malformed events; catalog polling remains authoritative.
                if let Ok(value) = serde_json::from_str::<Value>(&data)
                    && value.get("model").is_some_and(Value::is_string)
                    && value.get("event").is_some_and(Value::is_string)
                    && let Ok(event) = serde_json::from_value::<LlamaModelEvent>(value)
                {
                    on_event(event);
                }
            }
        }
        Ok(())
    }

    /// Run `watch` in the background until the returned token is cancelled.
    fn spawn_watch(
        &self,
        signal: Option<&CancellationToken>,
        on_event: Arc<dyn Fn(LlamaModelEvent) + Send + Sync>,
    ) -> CancellationToken {
        let watcher = match signal {
            Some(signal) => signal.child_token(),
            None => CancellationToken::new(),
        };
        let client = self.clone();
        let token = watcher.clone();
        tokio::spawn(async move {
            let _ = client.watch(on_event, Some(token)).await;
        });
        watcher
    }

    pub async fn load_and_wait(
        &self,
        model: &str,
        on_progress: Arc<dyn Fn(LlamaProgress) + Send + Sync>,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<LlamaModelInfo> {
        let event_loaded = Arc::new(AtomicBool::new(false));
        let event_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let watcher = {
            let (model, loaded, error, on_progress) =
                (model.to_string(), event_loaded.clone(), event_error.clone(), on_progress.clone());
            self.spawn_watch(
                signal.as_ref(),
                Arc::new(move |event: LlamaModelEvent| {
                    if event.model != model {
                        return;
                    }
                    if event.event != "model_status" && event.event != "status_change" {
                        return;
                    }
                    let status = event.data.as_ref().and_then(|d| d.get("status")).and_then(Value::as_str);
                    if status == Some("loaded") {
                        loaded.store(true, Ordering::SeqCst);
                    }
                    if status == Some("unloaded") {
                        *error.lock() = Some("Model failed to load".to_string());
                    }
                    if let Some(progress) = parse_load_progress(event.data.as_ref()) {
                        on_progress(progress);
                    }
                }),
            )
        };
        let result = async {
            self.load(model, signal.clone()).await?;
            on_progress(LlamaProgress { message: "Loading model".to_string(), ..Default::default() });
            let list_options = LlamaClientListOptions { reload: None, signal: signal.clone() };
            loop {
                if signal.as_ref().is_some_and(|s| s.is_cancelled()) {
                    anyhow::bail!("Cancelled");
                }
                let entry = self.list(Some(&list_options)).await?.into_iter().find(|candidate| candidate.id == model);
                if let Some(entry) = &entry
                    && entry.status.value == LlamaModelStatus::Loaded
                {
                    return Ok(entry.clone());
                }
                if event_loaded.load(Ordering::SeqCst) && entry.is_none() {
                    return Ok(LlamaModelInfo {
                        id: model.to_string(),
                        status: LlamaModelInfoStatus { value: LlamaModelStatus::Loaded, ..Default::default() },
                        ..Default::default()
                    });
                }
                let event_error = event_error.lock().clone();
                if entry.as_ref().is_some_and(|e| e.status.failed == Some(true)) || event_error.is_some() {
                    match entry.as_ref().and_then(|e| e.status.exit_code) {
                        Some(code) => anyhow::bail!("Model exited with code {code}"),
                        None => anyhow::bail!(event_error.unwrap_or_else(|| "Model failed to load".to_string())),
                    }
                }
                sleep(250, signal.as_ref()).await?;
            }
        }
        .await;
        watcher.cancel();
        result
    }

    pub async fn download_and_wait(
        &self,
        model: &str,
        on_progress: Arc<dyn Fn(LlamaProgress) + Send + Sync>,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<Vec<LlamaModelInfo>> {
        let finished = Arc::new(AtomicBool::new(false));
        let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let saw_downloading = Arc::new(AtomicBool::new(false));
        let polls = AtomicU64::new(0);
        let watcher = {
            let (model, finished, failure, saw, on_progress) = (
                model.to_string(),
                finished.clone(),
                failure.clone(),
                saw_downloading.clone(),
                on_progress.clone(),
            );
            self.spawn_watch(
                signal.as_ref(),
                Arc::new(move |event: LlamaModelEvent| {
                    if event.model != model {
                        return;
                    }
                    match event.event.as_str() {
                        "download_finished" => finished.store(true, Ordering::SeqCst),
                        "download_failed" => *failure.lock() = Some(error_message(event.data.as_ref(), "Download failed")),
                        "download_progress" => {
                            saw.store(true, Ordering::SeqCst);
                            if let Some(progress) = parse_download_progress(event.data.as_ref()) {
                                on_progress(progress);
                            }
                        }
                        _ => {}
                    }
                }),
            )
        };
        let result = async {
            self.download(model, signal.clone()).await?;
            on_progress(LlamaProgress { message: "Downloading model".to_string(), ..Default::default() });
            let list_options = LlamaClientListOptions { reload: None, signal: signal.clone() };
            loop {
                if signal.as_ref().is_some_and(|s| s.is_cancelled()) {
                    anyhow::bail!("Cancelled");
                }
                if let Some(failure) = failure.lock().clone() {
                    anyhow::bail!(failure);
                }
                let models = self.list(Some(&list_options)).await?;
                let polled = polls.fetch_add(1, Ordering::SeqCst) + 1;
                let entry = models.into_iter().find(|candidate| candidate.id == model);
                if let Some(entry) = entry.as_ref().filter(|e| e.status.value == LlamaModelStatus::Downloading) {
                    saw_downloading.store(true, Ordering::SeqCst);
                    let progress = entry.status.progress.as_ref().and_then(|p| serde_json::to_value(p).ok());
                    if let Some(progress) = parse_download_progress(progress.as_ref()) {
                        on_progress(progress);
                    }
                } else if finished.load(Ordering::SeqCst)
                    || (entry.is_some() && (saw_downloading.load(Ordering::SeqCst) || polled >= 2))
                {
                    return self
                        .list(Some(&LlamaClientListOptions { reload: Some(true), signal: signal.clone() }))
                        .await;
                }
                sleep(500, signal.as_ref()).await?;
            }
        }
        .await;
        watcher.cancel();
        result
    }
}
