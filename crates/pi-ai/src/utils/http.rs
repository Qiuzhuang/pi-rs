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

fn build_client(proxy: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(60));
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

/// Stainless SDK platform headers (`X-Stainless-*`) sent by the OpenAI and Anthropic SDKs.
pub fn stainless_headers(package_version: &str, timeout_ms: u64) -> Vec<(String, String)> {
    let os = match std::env::consts::OS {
        "macos" => "MacOS",
        "linux" => "Linux",
        "windows" => "Windows",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "android" => "Android",
        "ios" => "iOS",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x32",
        "arm" => "arm",
        other => other,
    };
    vec![
        ("X-Stainless-Retry-Count".into(), "0".into()),
        ("X-Stainless-Timeout".into(), (timeout_ms / 1000).to_string()),
        ("X-Stainless-Lang".into(), "js".into()),
        ("X-Stainless-Package-Version".into(), package_version.into()),
        ("X-Stainless-OS".into(), os.into()),
        ("X-Stainless-Arch".into(), arch.into()),
        ("X-Stainless-Runtime".into(), "unknown".into()),
        ("X-Stainless-Runtime-Version".into(), "unknown".into()),
    ]
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
