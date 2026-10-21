//! Registry of OAuth flow implementations (port of `auth/oauth/load.ts`).
//!
//! The per-provider OAuth flows (`auth/oauth/*.ts`) are registered here by key; provider
//! definitions advertise OAuth through [`lazy_builtin_oauth`], which loads the flow on first
//! use. Keys: `anthropic`, `openaiCodex`, `openaiChatGPT`, `githubCopilot`, `openrouter`,
//! `kimiCoding`, `meta`, `xai`, `radius`.

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use super::helpers::lazy_oauth;
use super::types::OAuthAuth;

/// Arguments for parameterized flows (Radius: name + gateway).
#[derive(Debug, Clone, Default)]
pub struct OAuthFlowArgs {
    pub name: Option<String>,
    pub gateway: Option<String>,
}

pub type OAuthFlowLoader = Arc<dyn Fn(&OAuthFlowArgs) -> anyhow::Result<Arc<dyn OAuthAuth>> + Send + Sync>;

static LOADERS: Lazy<RwLock<HashMap<String, OAuthFlowLoader>>> = Lazy::new(|| RwLock::new(HashMap::new()));

/// Register (or replace) the OAuth flow implementation for `key`.
pub fn register_oauth_flow_loader(key: &str, loader: OAuthFlowLoader) {
    LOADERS.write().insert(key.to_string(), loader);
}

/// Load the OAuth flow registered under `key`.
pub fn load_oauth_flow(key: &str, args: &OAuthFlowArgs) -> anyhow::Result<Arc<dyn OAuthAuth>> {
    let loader = LOADERS.read().get(key).cloned();
    match loader {
        Some(loader) => loader(args),
        None => match super::oauth::builtin_oauth_flow(key, args) {
            Some(flow) => flow,
            None => anyhow::bail!("OAuth flow \"{key}\" is not available in this build"),
        },
    }
}

/// An `OAuthAuth` that loads the registered flow `key` on first use.
pub fn lazy_builtin_oauth(
    name: &str,
    is_subscription: Option<bool>,
    login_label: Option<&str>,
    key: &str,
    args: OAuthFlowArgs,
) -> Arc<dyn OAuthAuth> {
    let key = key.to_string();
    lazy_oauth(
        name,
        is_subscription,
        login_label,
        Arc::new(move || {
            let result = load_oauth_flow(&key, &args);
            Box::pin(async move { result })
        }),
    )
}
