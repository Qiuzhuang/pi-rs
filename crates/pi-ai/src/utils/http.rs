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
