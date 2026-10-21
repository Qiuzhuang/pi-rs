//! Anthropic OAuth flow, Claude Pro/Max (port of `auth/oauth/anthropic.ts`).

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::callback_server::{
    CallbackOrManual, OAuthCallbackServerOptions, start_oauth_callback_server, wait_for_callback_or_manual_input,
};
use super::http::{OAuthFetch, OAuthHttpRequest, current_oauth_fetch, form_urlencode, send};
use super::pkce::generate_pkce;
use super::{parse_authorization_input, select_prompt};
use crate::auth::types::{
    AuthEvent, AuthPrompt, AuthPromptKind, LoginOptions, ModelAuth, OAuthAuth, OAuthCredential, ProviderAuthInteraction,
};
use crate::utils::provider_env::get_provider_env_value;

fn client_id() -> String {
    let bytes = base64::engine::general_purpose::STANDARD.decode("OWQxYzI1MGEtZTYxYi00NGQ5LTg4ZWQtNTk0NGQxOTYyZjVl").unwrap_or_default();
    String::from_utf8(bytes).unwrap_or_default()
}

const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
pub(crate) const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// Preferred so the port can be forwarded into containers or over SSH; login falls back to a free port.
const CALLBACK_PORT: u16 = 53692;
const CALLBACK_PATH: &str = "/callback";
const COPY_CODE_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const BROWSER_LOGIN_METHOD: &str = "browser";
const COPY_CODE_LOGIN_METHOD: &str = "copy_code";
const SCOPES: &str =
    "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

fn callback_host() -> String {
    get_provider_env_value("PI_OAUTH_CALLBACK_HOST", None).unwrap_or_else(|| "127.0.0.1".into())
}

fn redirect_uri() -> String {
    format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}")
}

fn format_error_details(error: &anyhow::Error) -> String {
    let mut details = vec![format!("Error: {error}")];
    if let Some(cause) = error.chain().nth(1) {
        details.push(format!("cause={cause}"));
    }
    details.join("; ")
}

async fn post_json(fetch: &OAuthFetch, url: &str, body: &Value, signal: &CancellationToken) -> anyhow::Result<String> {
    let request = OAuthHttpRequest::post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(body.to_string());
    let response = send(fetch, request, Some(signal), Some(30_000)).await?;
    if !response.ok() {
        anyhow::bail!("HTTP request failed. status={}; url={url}; body={}", response.status, response.body);
    }
    Ok(response.body)
}

fn credential_from_tokens(body: &str, url: &str, invalid_prefix: &str) -> anyhow::Result<OAuthCredential> {
    let data: Value = serde_json::from_str(body).map_err(|e| {
        anyhow::anyhow!("{invalid_prefix} returned invalid JSON. url={url}; body={body}; details=Error: {e}")
    })?;
    let expires_in = data.get("expires_in").and_then(Value::as_f64).unwrap_or(f64::NAN);
    Ok(OAuthCredential {
        refresh: data.get("refresh_token").and_then(Value::as_str).unwrap_or_default().to_string(),
        access: data.get("access_token").and_then(Value::as_str).unwrap_or_default().to_string(),
        expires: (crate::utils::now_ms() as f64 + expires_in * 1000.0 - 5.0 * 60.0 * 1000.0) as i64,
        extra: Map::new(),
    })
}

async fn exchange_authorization_code(
    fetch: &OAuthFetch,
    code: &str,
    state: &str,
    verifier: &str,
    redirect_uri: &str,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    let body = json!({
        "grant_type": "authorization_code",
        "client_id": client_id(),
        "code": code,
        "state": state,
        "redirect_uri": redirect_uri,
        "code_verifier": verifier,
    });
    let response = post_json(fetch, TOKEN_URL, &body, signal).await.map_err(|error| {
        anyhow::anyhow!(
            "Token exchange request failed. url={TOKEN_URL}; redirect_uri={redirect_uri}; response_type=authorization_code; details={}",
            format_error_details(&error)
        )
    })?;
    credential_from_tokens(&response, TOKEN_URL, "Token exchange")
}

fn authorize_url(challenge: &str, verifier: &str, redirect_uri: &str) -> String {
    let client_id = client_id();
    format!(
        "{AUTHORIZE_URL}?{}",
        form_urlencode(&[
            ("code", "true"),
            ("client_id", &client_id),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri),
            ("scope", SCOPES),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", verifier),
        ])
    )
}

async fn login_browser(fetch: OAuthFetch, interaction: &ProviderAuthInteraction) -> anyhow::Result<OAuthCredential> {
    let pkce = generate_pkce();
    let start = |port: u16| {
        start_oauth_callback_server(OAuthCallbackServerOptions::<String> {
            provider_name: "Anthropic".into(),
            host: callback_host(),
            port,
            path: CALLBACK_PATH.into(),
            redirect_host: Some("localhost".into()),
            state: Some(pkce.verifier.clone()),
            complete: Arc::new(|code| Box::pin(async move { Ok(code) })),
            signal: Some(interaction.signal.clone()),
            timeout_ms: None,
        })
    };
    let callback = match start(CALLBACK_PORT).await {
        Ok(server) => Some(server),
        Err(_) => start(0).await.ok(),
    };
    let redirect_uri = callback.as_ref().map(|c| c.redirect_uri.clone()).unwrap_or_else(redirect_uri);
    let result = async {
        interaction.interaction.notify(AuthEvent::AuthUrl {
            url: authorize_url(&pkce.challenge, &pkce.verifier, &redirect_uri),
            instructions: Some(
                "Complete login in your browser. If the browser is on another machine, paste the final redirect URL here."
                    .into(),
            ),
        });
        let outcome = wait_for_callback_or_manual_input(
            interaction,
            callback.as_ref(),
            "Complete login in your browser, or paste the authorization code / redirect URL here:",
            &redirect_uri,
        )
        .await?;
        let (code, state) = match outcome {
            CallbackOrManual::Callback(code) => (Some(code), pkce.verifier.clone()),
            CallbackOrManual::Manual(input) => {
                let parsed = parse_authorization_input(&input);
                if parsed.state.as_ref().is_some_and(|s| *s != pkce.verifier) {
                    anyhow::bail!("OAuth state mismatch");
                }
                (parsed.code, parsed.state.unwrap_or_else(|| pkce.verifier.clone()))
            }
        };
        let Some(code) = code.filter(|c| !c.is_empty()) else { anyhow::bail!("Missing authorization code") };
        interaction.interaction.notify(AuthEvent::Progress { message: "Exchanging authorization code for tokens...".into() });
        exchange_authorization_code(&fetch, &code, &state, &pkce.verifier, &redirect_uri, &interaction.signal).await
    }
    .await;
    if let Some(callback) = &callback {
        callback.close();
    }
    result
}

async fn login_copy_code(fetch: OAuthFetch, interaction: &ProviderAuthInteraction) -> anyhow::Result<OAuthCredential> {
    let pkce = generate_pkce();
    interaction.interaction.notify(AuthEvent::AuthUrl {
        url: authorize_url(&pkce.challenge, &pkce.verifier, COPY_CODE_REDIRECT_URI),
        instructions: Some("Complete login in your browser, then copy the code Anthropic shows and paste it here.".into()),
    });
    let input = interaction
        .interaction
        .prompt(AuthPrompt {
            kind: AuthPromptKind::ManualCode,
            message: "Paste the code Anthropic shows after you sign in:".into(),
            placeholder: Some("code#state".into()),
            options: Vec::new(),
            signal: Some(interaction.signal.clone()),
        })
        .await?;
    let parsed = parse_authorization_input(&input);
    if parsed.state.as_ref().is_some_and(|s| *s != pkce.verifier) {
        anyhow::bail!("OAuth state mismatch");
    }
    let Some(code) = parsed.code.filter(|c| !c.is_empty()) else { anyhow::bail!("Missing authorization code") };
    interaction.interaction.notify(AuthEvent::Progress { message: "Exchanging authorization code for tokens...".into() });
    let state = parsed.state.unwrap_or_else(|| pkce.verifier.clone());
    exchange_authorization_code(&fetch, &code, &state, &pkce.verifier, COPY_CODE_REDIRECT_URI, &interaction.signal).await
}

async fn refresh_anthropic_token(fetch: &OAuthFetch, refresh_token: &str, signal: &CancellationToken) -> anyhow::Result<OAuthCredential> {
    let body = json!({ "grant_type": "refresh_token", "client_id": client_id(), "refresh_token": refresh_token });
    let response = post_json(fetch, TOKEN_URL, &body, signal).await.map_err(|error| {
        anyhow::anyhow!("Anthropic token refresh request failed. url={TOKEN_URL}; details={}", format_error_details(&error))
    })?;
    credential_from_tokens(&response, TOKEN_URL, "Anthropic token refresh")
}

/// `anthropicOAuth`.
pub struct AnthropicOAuth;

#[async_trait]
impl OAuthAuth for AnthropicOAuth {
    fn name(&self) -> &str {
        "Anthropic (Claude Pro/Max)"
    }
    fn is_subscription(&self) -> Option<bool> {
        Some(true)
    }
    async fn login(&self, interaction: ProviderAuthInteraction, _options: Option<LoginOptions>) -> anyhow::Result<OAuthCredential> {
        let fetch = current_oauth_fetch();
        let method = interaction
            .interaction
            .prompt(select_prompt(
                "Select Anthropic login method:",
                &[(BROWSER_LOGIN_METHOD, "Browser login (default)"), (COPY_CODE_LOGIN_METHOD, "Copy code login (headless)")],
            ))
            .await?;
        match method.as_str() {
            COPY_CODE_LOGIN_METHOD => login_copy_code(fetch, &interaction).await,
            BROWSER_LOGIN_METHOD => login_browser(fetch, &interaction).await,
            other => anyhow::bail!("Unknown Anthropic login method: {other}"),
        }
    }
    async fn refresh(&self, credential: OAuthCredential, signal: CancellationToken) -> anyhow::Result<OAuthCredential> {
        refresh_anthropic_token(&current_oauth_fetch(), &credential.refresh, &signal).await
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        Ok(ModelAuth { api_key: Some(credential.access.clone()), ..Default::default() })
    }
}

pub fn anthropic_oauth() -> Arc<dyn OAuthAuth> {
    Arc::new(AnthropicOAuth)
}
