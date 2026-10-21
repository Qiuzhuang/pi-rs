//! OpenRouter OAuth PKCE flow (port of `auth/oauth/openrouter.ts`).
//!
//! OpenRouter exchanges an authorization code for a permanent, user-controlled API key rather
//! than an expiring token pair. The callback is handled by a one-shot loopback server on an
//! ephemeral port, raced against a manual prompt for remote/headless sessions.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::callback_server::{
    CallbackOrManual, OAuthCallbackServerOptions, start_oauth_callback_server, wait_for_callback_or_manual_input,
};
use super::http::{OAuthFetch, OAuthFetchError, OAuthHttpRequest, current_oauth_fetch, form_urlencode, send};
use super::pkce::generate_pkce;
use crate::auth::types::{AuthEvent, LoginOptions, ModelAuth, OAuthAuth, OAuthCredential, ProviderAuthInteraction};
use crate::utils::provider_env::get_provider_env_value;

const AUTHORIZE_URL: &str = "https://openrouter.ai/auth";
pub const TOKEN_URL: &str = "https://openrouter.ai/api/v1/auth/keys";
const LOGIN_TIMEOUT_MS: u64 = 5 * 60 * 1000;
const TOKEN_EXCHANGE_TIMEOUT_MS: u64 = 30_000;
/// `Number.MAX_SAFE_INTEGER`: the key never expires.
const NEVER_EXPIRES: i64 = 9_007_199_254_740_991;

fn callback_host() -> String {
    get_provider_env_value("PI_OAUTH_CALLBACK_HOST", None).unwrap_or_else(|| "127.0.0.1".into())
}

fn parse_authorization_input(input: &str) -> Option<String> {
    let value = input.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(url) = url::Url::parse(value) {
        return url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned());
    }
    if value.contains("code=") {
        return url::form_urlencoded::parse(value.as_bytes()).find(|(k, _)| k == "code").map(|(_, v)| v.into_owned());
    }
    Some(value.to_string())
}

fn error_detail(body: &Map<String, Value>) -> Option<String> {
    for key in ["error_description", "message", "error"] {
        if let Some(Value::String(text)) = body.get(key) {
            return Some(text.clone());
        }
    }
    if let Some(Value::Object(error)) = body.get("error")
        && let Some(Value::String(message)) = error.get("message")
    {
        return Some(message.clone());
    }
    None
}

async fn exchange_authorization_code(
    fetch: &OAuthFetch,
    code: &str,
    verifier: &str,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    if signal.is_cancelled() {
        anyhow::bail!("Login cancelled");
    }
    let request = OAuthHttpRequest::post(TOKEN_URL)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .body(json!({ "code": code, "code_verifier": verifier, "code_challenge_method": "S256" }).to_string());
    let response = match send(fetch, request, Some(signal), Some(TOKEN_EXCHANGE_TIMEOUT_MS)).await {
        Ok(response) => response,
        Err(error) => {
            if signal.is_cancelled() {
                anyhow::bail!("Login cancelled");
            }
            if matches!(error.downcast_ref::<OAuthFetchError>(), Some(OAuthFetchError::Timeout)) {
                anyhow::bail!("OpenRouter OAuth token exchange timed out");
            }
            return Err(error);
        }
    };
    let body = match serde_json::from_str::<Value>(&response.body) {
        Ok(Value::Object(map)) => map,
        Ok(_) => Map::new(),
        Err(_) => {
            if response.ok() {
                anyhow::bail!("OpenRouter OAuth returned invalid JSON");
            }
            Map::new()
        }
    };
    if !response.ok() {
        let detail = error_detail(&body).map(|d| format!(": {d}")).unwrap_or_default();
        anyhow::bail!("OpenRouter OAuth key exchange failed (HTTP {}){detail}", response.status);
    }
    match body.get("key") {
        Some(Value::String(key)) if !key.is_empty() => {
            Ok(OAuthCredential { access: key.clone(), refresh: String::new(), expires: NEVER_EXPIRES, extra: Map::new() })
        }
        _ => anyhow::bail!("OpenRouter OAuth response carries no \"key\""),
    }
}

async fn login_openrouter(interaction: ProviderAuthInteraction) -> anyhow::Result<OAuthCredential> {
    let fetch = current_oauth_fetch();
    let pkce = generate_pkce();
    let complete_fetch = fetch.clone();
    let verifier = pkce.verifier.clone();
    let signal = interaction.signal.clone();
    // OpenRouter sends no `state`; the random path keeps stray requests from completing the sign-in.
    let callback = start_oauth_callback_server(OAuthCallbackServerOptions::<OAuthCredential> {
        provider_name: "OpenRouter".into(),
        host: callback_host(),
        port: 0,
        path: format!("/oauth/callback/{}", uuid::Uuid::new_v4()),
        redirect_host: None,
        state: None,
        complete: Arc::new(move |code| {
            let fetch = complete_fetch.clone();
            let verifier = verifier.clone();
            let signal = signal.clone();
            Box::pin(async move { exchange_authorization_code(&fetch, &code, &verifier, &signal).await })
        }),
        signal: Some(interaction.signal.clone()),
        timeout_ms: Some(LOGIN_TIMEOUT_MS),
    })
    .await?;

    let result = async {
        let authorize_url = format!(
            "{AUTHORIZE_URL}?{}",
            form_urlencode(&[
                ("callback_url", &callback.redirect_uri),
                ("code_challenge", &pkce.challenge),
                ("code_challenge_method", "S256"),
            ])
        );
        interaction.interaction.notify(AuthEvent::Progress {
            message: format!("Listening for OpenRouter OAuth callback on {}", callback.redirect_uri),
        });
        interaction.interaction.notify(AuthEvent::AuthUrl {
            url: authorize_url,
            instructions: Some(
                "Complete sign-in in your browser. If the browser is on another machine, paste the final redirect URL here."
                    .into(),
            ),
        });
        let outcome = wait_for_callback_or_manual_input(
            &interaction,
            Some(&callback),
            "Complete sign-in in your browser, or paste the authorization code / redirect URL here:",
            &callback.redirect_uri,
        )
        .await?;
        match outcome {
            CallbackOrManual::Callback(credential) => Ok(credential),
            CallbackOrManual::Manual(input) => {
                let Some(code) = parse_authorization_input(&input).filter(|c| !c.is_empty()) else {
                    anyhow::bail!("Missing authorization code");
                };
                interaction
                    .interaction
                    .notify(AuthEvent::Progress { message: "Exchanging authorization code for an API key...".into() });
                exchange_authorization_code(&fetch, &code, &pkce.verifier, &interaction.signal).await
            }
        }
    }
    .await;
    callback.close();
    result
}

/// `openRouterOAuth`.
pub struct OpenRouterOAuth;

#[async_trait]
impl OAuthAuth for OpenRouterOAuth {
    fn name(&self) -> &str {
        "OpenRouter OAuth"
    }
    fn login_label(&self) -> Option<&str> {
        Some("Sign in with OpenRouter")
    }
    async fn login(&self, interaction: ProviderAuthInteraction, _options: Option<LoginOptions>) -> anyhow::Result<OAuthCredential> {
        login_openrouter(interaction).await
    }
    async fn refresh(&self, credential: OAuthCredential, _signal: CancellationToken) -> anyhow::Result<OAuthCredential> {
        Ok(credential)
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        Ok(ModelAuth { api_key: Some(credential.access.clone()), ..Default::default() })
    }
}

pub fn openrouter_oauth() -> Arc<dyn OAuthAuth> {
    Arc::new(OpenRouterOAuth)
}
