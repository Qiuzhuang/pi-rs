//! Shared HTTP client construction (the `fetch` equivalent).
//!
//! Clients honor `HTTP(S)_PROXY`/`ALL_PROXY`/`NO_PROXY` (with provider-scoped env overrides
//! taking precedence) using the same resolution rules as `node-http-proxy.ts`.

use std::collections::HashMap;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use super::node_http_proxy::get_proxy_url_for_target;
use crate::types::ProviderEnv;

static CLIENTS: Lazy<Mutex<HashMap<String, reqwest::Client>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// SDK default request timeout (10 minutes).
pub const DEFAULT_TIMEOUT_MS: u64 = 600_000;

/// Idle timeout for response headers and body reads (`None` = disabled).
static IDLE_TIMEOUT: Lazy<Mutex<Option<Duration>>> = Lazy::new(|| Mutex::new(None));

/// Set the idle timeout applied to every shared client: a request fails when no response data
/// arrives for this long (undici's `headersTimeout`/`bodyTimeout`). `None` or zero disables it.
/// Cached clients are rebuilt.
pub fn set_http_idle_timeout(timeout: Option<Duration>) {
    *IDLE_TIMEOUT.lock() = timeout.filter(|t| !t.is_zero());
    CLIENTS.lock().clear();
}

/// The idle timeout set with [`set_http_idle_timeout`].
pub fn http_idle_timeout() -> Option<Duration> {
    *IDLE_TIMEOUT.lock()
}

fn build_client(proxy: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(60));
    if let Some(timeout) = http_idle_timeout() {
        builder = builder.read_timeout(timeout);
    }
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?);
    }
    Ok(builder.build()?)
}

/// A client for requests to `target_url`, with the proxy configured from the environment.
/// Clients are cached per proxy URL so connections are pooled.
pub fn client_for(target_url: &str, env: Option<&ProviderEnv>) -> anyhow::Result<reqwest::Client> {
    let proxy = get_proxy_url_for_target(target_url, env);
    let key = proxy.clone().unwrap_or_default();
    if let Some(client) = CLIENTS.lock().get(&key) {
        return Ok(client.clone());
    }
    let client = build_client(proxy.as_deref())?;
    CLIENTS.lock().insert(key, client.clone());
    Ok(client)
}

/// Ordered header list with JavaScript `Headers`-like case-insensitive merging.
/// Later entries replace earlier ones with the same name; `None` removes a header.
#[derive(Debug, Clone, Default)]
pub struct HeaderList {
    entries: Vec<(String, String)>,
}

impl HeaderList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: &str, value: Option<&str>) {
        self.entries.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        if let Some(value) = value {
            self.entries.push((name.to_string(), value.to_string()));
        }
    }

    pub fn extend_strings<'a>(&mut self, headers: impl IntoIterator<Item = (&'a String, &'a String)>) {
        for (name, value) in headers {
            self.set(name, Some(value));
        }
    }

    pub fn extend_optional<'a>(&mut self, headers: impl IntoIterator<Item = (&'a String, &'a Option<String>)>) {
        for (name, value) in headers {
            self.set(name, value.as_deref());
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    pub fn to_header_map(&self) -> anyhow::Result<reqwest::header::HeaderMap> {
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in &self.entries {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())?;
            let value = reqwest::header::HeaderValue::from_str(value)?;
            map.append(name, value);
        }
        Ok(map)
    }
}

/// `process.version` reported in the Stainless runtime headers.
const STAINLESS_NODE_VERSION: &str = "v22.19.0";

/// Stainless `normalizePlatform` applied to the Node `process.platform` of this target.
fn stainless_os() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        "solaris" | "illumos" => "sunos",
        other => other,
    };
    match platform {
        p if p.contains("ios") => "iOS".into(),
        "android" => "Android".into(),
        "darwin" => "MacOS".into(),
        "win32" => "Windows".into(),
        "freebsd" => "FreeBSD".into(),
        "openbsd" => "OpenBSD".into(),
        "linux" => "Linux".into(),
        "" => "Unknown".into(),
        other => format!("Other:{other}"),
    }
}

/// Stainless `normalizeArch` applied to the Node `process.arch` of this target.
fn stainless_arch() -> String {
    let arch = match std::env::consts::ARCH {
        "x86" => "ia32",
        "powerpc64" => "ppc64",
        "powerpc" => "ppc",
        "loongarch64" => "loong64",
        other => other,
    };
    match arch {
        "x32" => "x32".into(),
        "x86_64" | "x64" => "x64".into(),
        "arm" => "arm".into(),
        "aarch64" | "arm64" => "arm64".into(),
        "" => "unknown".into(),
        other => format!("other:{other}"),
    }
}

/// Stainless SDK platform headers (`X-Stainless-*`) sent by the OpenAI and Anthropic SDKs.
pub fn stainless_headers(package_version: &str, timeout_ms: u64) -> Vec<(String, String)> {
    let os = stainless_os();
    let arch = stainless_arch();
    vec![
        ("X-Stainless-Retry-Count".into(), "0".into()),
        ("X-Stainless-Timeout".into(), (timeout_ms / 1000).to_string()),
        ("X-Stainless-Lang".into(), "js".into()),
        ("X-Stainless-Package-Version".into(), package_version.into()),
        ("X-Stainless-OS".into(), os),
        ("X-Stainless-Arch".into(), arch),
        // The TS SDKs run on Node (or Bun, which reports itself as Node).
        ("X-Stainless-Runtime".into(), "node".into()),
        ("X-Stainless-Runtime-Version".into(), STAINLESS_NODE_VERSION.into()),
    ]
}

/// The `openai` SDK's `readEnv`: the trimmed process env value, empty counting as unset.
fn openai_read_env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// `OpenAI-Organization`/`OpenAI-Project` that every `openai` SDK client sends from
/// `OPENAI_ORG_ID`/`OPENAI_PROJECT_ID` (set after the platform headers, before auth).
pub fn apply_openai_sdk_tenant_headers(headers: &mut HeaderList) {
    if let Some(org) = openai_read_env("OPENAI_ORG_ID") {
        headers.set("OpenAI-Organization", Some(&org));
    }
    if let Some(project) = openai_read_env("OPENAI_PROJECT_ID") {
        headers.set("OpenAI-Project", Some(&project));
    }
}

/// `OPENAI_CUSTOM_HEADERS` (`Name: value` lines) that the `openai` SDK client merges into its
/// default headers ahead of the caller's default headers (set after auth).
pub fn apply_openai_custom_headers_env(headers: &mut HeaderList) -> anyhow::Result<()> {
    let Some(value) = openai_read_env("OPENAI_CUSTOM_HEADERS") else { return Ok(()) };
    for line in value.split('\n') {
        let Some(colon) = line.find(':') else { continue };
        let name = line[..colon].trim();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
            anyhow::bail!("Header name must be a valid HTTP token [\"{name}\"]");
        }
        headers.set(name, Some(line[colon + 1..].trim()));
    }
    Ok(())
}

/// Send a JSON POST request; non-2xx responses become [`ProviderHttpError`]s in the SDK's style.
pub async fn post_json(
    client: &reqwest::Client,
    url: &str,
    headers: &HeaderList,
    body: &serde_json::Value,
    timeout_ms: Option<u64>,
    signal: Option<&tokio_util::sync::CancellationToken>,
    style: super::error_body::SdkErrorStyle,
) -> anyhow::Result<reqwest::Response> {
    use super::error_body::ProviderHttpError;
    let mut request = client
        .post(url)
        .headers(headers.to_header_map()?)
        .body(serde_json::to_vec(body)?);
    if let Some(timeout) = timeout_ms {
        request = request.timeout(Duration::from_millis(timeout));
    }
    let send = request.send();
    let result = match signal {
        Some(signal) => {
            if signal.is_cancelled() {
                anyhow::bail!(super::provider_retry::AbortError);
            }
            tokio::select! {
                r = send => r,
                _ = signal.cancelled() => anyhow::bail!(super::provider_retry::AbortError),
            }
        }
        None => send.await,
    };
    let response = result.map_err(|e| anyhow::Error::new(ProviderHttpError::from_reqwest(&e)))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let response_headers = response.headers().clone();
    let text = response.text().await.unwrap_or_default();
    Err(ProviderHttpError::from_response(status.as_u16(), response_headers, text, style).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stainless_headers_report_a_node_runtime() {
        let headers = stainless_headers("1.2.3", 600_000);
        let get = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
        assert_eq!(get("X-Stainless-Runtime"), Some("node"));
        assert_eq!(get("X-Stainless-Runtime-Version"), Some("v22.19.0"));
        assert_eq!(get("X-Stainless-Timeout"), Some("600"));
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(get("X-Stainless-OS"), Some("Linux"));
            assert_eq!(get("X-Stainless-Arch"), Some("x64"));
        }
    }
}
