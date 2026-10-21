//! Built-in OAuth flows (port of `auth/oauth/*`, `oauth.ts` and `bun-oauth.ts`).
//!
//! Provider definitions reference flows by key through
//! [`lazy_builtin_oauth`](crate::auth::oauth_load::lazy_builtin_oauth); [`builtin_oauth_flow`]
//! resolves the keys `anthropic`, `openaiCodex`, `openaiChatGPT`, `githubCopilot`, `openrouter`,
//! `kimiCoding`, `meta`, `xai` and `radius`.
//!
//! Flows report the authorization URL with [`AuthEvent::AuthUrl`](crate::auth::types::AuthEvent)
//! and device codes with `AuthEvent::DeviceCode`; the app opens the browser (see
//! [`open_browser`]). Browser flows run a loopback callback server and race it against a manual
//! paste prompt.

pub mod anthropic;
pub mod callback_server;
pub mod device_code;
pub mod github_copilot;
pub mod http;
pub mod kimi_coding;
pub mod meta;
pub mod openai_chatgpt;
pub mod openai_codex;
pub mod openrouter;
pub mod pkce;
pub mod radius;
pub mod xai;

use std::sync::Arc;

use super::oauth_load::OAuthFlowArgs;
use super::types::{AuthPrompt, AuthPromptKind, AuthSelectOption, OAuthAuth};

pub use callback_server::{
    CallbackOrManual, OAuthCallbackServer, OAuthCallbackServerOptions, start_oauth_callback_server,
    wait_for_callback_or_manual_input,
};
pub use device_code::{OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult, abortable_sleep, poll_oauth_device_code_flow};
pub use http::{
    OAuthFetch, OAuthFetchError, OAuthHttpRequest, OAuthHttpResponse, current_oauth_fetch, default_oauth_fetch, with_oauth_fetch,
};
pub use pkce::{Pkce, generate_pkce};

/// Keys of the built-in OAuth flows (`OAuthFlowLoaders`).
pub const BUILTIN_OAUTH_FLOW_KEYS: [&str; 9] =
    ["anthropic", "openaiCodex", "openaiChatGPT", "githubCopilot", "openrouter", "kimiCoding", "meta", "xai", "radius"];

/// Resolve a built-in OAuth flow by key (`load.ts`). `None` for unknown keys.
pub fn builtin_oauth_flow(key: &str, args: &OAuthFlowArgs) -> Option<anyhow::Result<Arc<dyn OAuthAuth>>> {
    Some(Ok(match key {
        "anthropic" => anthropic::anthropic_oauth(),
        "openaiCodex" => openai_codex::openai_codex_oauth(),
        "openaiChatGPT" => openai_chatgpt::openai_chatgpt_oauth(),
        "githubCopilot" => github_copilot::github_copilot_oauth(),
        "openrouter" => openrouter::openrouter_oauth(),
        "kimiCoding" => kimi_coding::kimi_coding_oauth(),
        "meta" => meta::meta_oauth(),
        "xai" => xai::xai_oauth(),
        "radius" => {
            let (Some(name), Some(gateway)) = (args.name.clone(), args.gateway.clone()) else {
                return Some(Err(anyhow::anyhow!("Radius OAuth requires a name and gateway")));
            };
            radius::create_radius_oauth(radius::RadiusOAuthOptions { name, gateway })
        }
        _ => return None,
    }))
}

/// Register every built-in flow with the OAuth loader registry (`registerBunOAuthFlows`).
///
/// Not required for normal use: unregistered keys fall back to [`builtin_oauth_flow`]. Useful to
/// restore the built-ins after an app replaced a loader.
pub fn register_builtin_oauth_flows() {
    for key in BUILTIN_OAUTH_FLOW_KEYS {
        super::oauth_load::register_oauth_flow_loader(
            key,
            Arc::new(move |args: &OAuthFlowArgs| {
                builtin_oauth_flow(key, args).unwrap_or_else(|| Err(anyhow::anyhow!("Unknown OAuth flow: {key}")))
            }),
        );
    }
}

/// A `select` prompt with `(id, label)` options.
pub(crate) fn select_prompt(message: &str, options: &[(&str, &str)]) -> AuthPrompt {
    AuthPrompt {
        kind: AuthPromptKind::Select,
        message: message.to_string(),
        placeholder: None,
        options: options
            .iter()
            .map(|(id, label)| AuthSelectOption { id: id.to_string(), label: label.to_string(), description: None })
            .collect(),
        signal: None,
    }
}

/// Code and state parsed from a pasted redirect URL, `code#state`, query string or bare code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorizationInput {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub(crate) fn parse_authorization_input(input: &str) -> AuthorizationInput {
    let value = input.trim();
    if value.is_empty() {
        return AuthorizationInput::default();
    }
    if let Ok(url) = url::Url::parse(value) {
        let param = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
        return AuthorizationInput { code: param("code"), state: param("state") };
    }
    if let Some((code, state)) = value.split_once('#') {
        let state = state.split('#').next().unwrap_or_default();
        return AuthorizationInput { code: Some(code.to_string()), state: Some(state.to_string()) };
    }
    if value.contains("code=") {
        let params: Vec<(String, String)> = url::form_urlencoded::parse(value.as_bytes()).into_owned().collect();
        let param = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        return AuthorizationInput { code: param("code"), state: param("state") };
    }
    AuthorizationInput { code: Some(value.to_string()), state: None }
}

/// The verification URI is opened in the user's browser; only http(s) URLs are trusted.
pub(crate) fn trusted_http_url(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str().filter(|s| !s.is_empty())?;
    let url = url::Url::parse(text).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// A finite positive number field.
pub(crate) fn positive_number(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(serde_json::Value::as_f64).filter(|v| v.is_finite() && *v > 0.0)
}

/// Open a URL in the user's browser (port of coding-agent `utils/open-browser.ts`).
///
/// Never invokes a shell: on Windows `rundll32 url.dll,FileProtocolHandler` is used instead of
/// `cmd /c start`, which would re-parse metacharacters. Launch failures are ignored.
pub fn open_browser(target: &str) {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![target])
    } else if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler", target])
    } else {
        ("xdg-open", vec![target])
    };
    let child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        // Reap the launcher so it does not linger as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_authorization_input() {
        assert_eq!(
            parse_authorization_input("http://localhost:1/cb?code=abc&state=xyz"),
            AuthorizationInput { code: Some("abc".into()), state: Some("xyz".into()) }
        );
        assert_eq!(
            parse_authorization_input("abc#xyz"),
            AuthorizationInput { code: Some("abc".into()), state: Some("xyz".into()) }
        );
        assert_eq!(
            parse_authorization_input("code=abc&state=xyz"),
            AuthorizationInput { code: Some("abc".into()), state: Some("xyz".into()) }
        );
        assert_eq!(parse_authorization_input(" abc "), AuthorizationInput { code: Some("abc".into()), state: None });
        assert_eq!(parse_authorization_input(""), AuthorizationInput::default());
    }

    #[test]
    fn resolves_builtin_flows() {
        for key in BUILTIN_OAUTH_FLOW_KEYS {
            let args = OAuthFlowArgs { name: Some("Radius".into()), gateway: Some("https://radius.example".into()) };
            assert!(builtin_oauth_flow(key, &args).unwrap().is_ok(), "{key}");
        }
        assert!(builtin_oauth_flow("nope", &OAuthFlowArgs::default()).is_none());
        assert!(builtin_oauth_flow("radius", &OAuthFlowArgs::default()).unwrap().is_err());
    }
}
