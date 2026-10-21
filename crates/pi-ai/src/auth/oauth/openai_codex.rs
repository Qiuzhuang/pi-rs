//! OpenAI Codex (ChatGPT OAuth) flow (port of `auth/oauth/openai-codex.ts`).

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use rand::RngCore;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::callback_server::{
    CallbackOrManual, OAuthCallbackServerOptions, start_oauth_callback_server,
    wait_for_callback_or_manual_input,
};
use super::device_code::{
    OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult, poll_oauth_device_code_flow,
};
use super::http::{OAuthFetch, OAuthHttpRequest, OAuthHttpResponse, current_oauth_fetch, send};
use super::pkce::generate_pkce;
use super::{parse_authorization_input, select_prompt};
use crate::auth::types::{
    AuthEvent, LoginOptions, ModelAuth, OAuthAuth, OAuthCredential, ProviderAuthInteraction,
};
use crate::utils::provider_env::get_provider_env_value;

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTH_BASE_URL: &str = "https://auth.openai.com";
const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const DEVICE_USER_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
pub const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
pub const DEVICE_VERIFICATION_URI: &str = "https://auth.openai.com/codex/device";
pub const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const DEVICE_CODE_TIMEOUT_SECONDS: f64 = 15.0 * 60.0;
const BROWSER_LOGIN_METHOD: &str = "browser";
const DEVICE_CODE_LOGIN_METHOD: &str = "device_code";
const SCOPE: &str = "openid profile email offline_access";
const JWT_CLAIM_PATH: &str = "https://api.openai.com/auth";

fn callback_host() -> String {
    get_provider_env_value("PI_OAUTH_CALLBACK_HOST", None).unwrap_or_else(|| "127.0.0.1".into())
}

struct OAuthToken {
    access: String,
    refresh: String,
    expires: i64,
}

struct DeviceAuthInfo {
    device_auth_id: String,
    user_code: String,
    interval_seconds: f64,
}

fn create_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_jwt(token: &str) -> Option<Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let bytes = engine.decode(parts[1]).ok()?;
    let latin1: String = bytes.into_iter().map(|b| b as char).collect();
    serde_json::from_str(&latin1).ok()
}

fn get_account_id(access_token: &str) -> Option<String> {
    let payload = decode_jwt(access_token)?;
    payload
        .get(JWT_CLAIM_PATH)?
        .get("chatgpt_account_id")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

async fn fetch_with_login_cancellation(
    fetch: &OAuthFetch,
    request: OAuthHttpRequest,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthHttpResponse> {
    match send(fetch, request, Some(signal), None).await {
        Ok(response) => Ok(response),
        Err(_) if signal.is_cancelled() => anyhow::bail!("Login cancelled"),
        Err(error) => Err(error),
    }
}

fn read_token_response(
    response: &OAuthHttpResponse,
    operation: &str,
) -> anyhow::Result<OAuthToken> {
    if !response.ok() {
        let text = if response.body.is_empty() {
            response.status_text.clone()
        } else {
            response.body.clone()
        };
        anyhow::bail!(
            "OpenAI Codex token {operation} failed ({}): {text}",
            response.status
        );
    }
    let json = response.json()?;
    let access = json
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let refresh = json
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let expires_in = json.get("expires_in").and_then(Value::as_f64);
    let (Some(access), Some(refresh), Some(expires_in)) = (access, refresh, expires_in) else {
        anyhow::bail!("OpenAI Codex token {operation} response missing fields: {json}");
    };
    Ok(OAuthToken {
        access: access.to_string(),
        refresh: refresh.to_string(),
        expires: (crate::utils::now_ms() as f64 + expires_in * 1000.0) as i64,
    })
}

async fn exchange_authorization_code(
    fetch: &OAuthFetch,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthToken> {
    let request = OAuthHttpRequest::post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ]);
    let response = fetch_with_login_cancellation(fetch, request, signal).await?;
    read_token_response(&response, "exchange")
}

async fn refresh_access_token(
    fetch: &OAuthFetch,
    refresh_token: &str,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthToken> {
    let request = OAuthHttpRequest::post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
        ]);
    let response = send(fetch, request, Some(signal), None)
        .await
        .map_err(|error| anyhow::anyhow!("OpenAI Codex token refresh error: {error}"))?;
    read_token_response(&response, "refresh")
}

async fn start_device_auth(
    fetch: &OAuthFetch,
    signal: &CancellationToken,
) -> anyhow::Result<DeviceAuthInfo> {
    let request = OAuthHttpRequest::post(DEVICE_USER_CODE_URL)
        .header("Content-Type", "application/json")
        .body(json!({ "client_id": CLIENT_ID }).to_string());
    let response = fetch_with_login_cancellation(fetch, request, signal).await?;
    if !response.ok() {
        if response.status == 404 {
            anyhow::bail!(
                "OpenAI Codex device code login is not enabled for this server. Use browser login or verify the server URL."
            );
        }
        let suffix = if response.body.is_empty() {
            String::new()
        } else {
            format!(": {}", response.body)
        };
        anyhow::bail!(
            "OpenAI Codex device code request failed with status {}{suffix}",
            response.status
        );
    }
    let json = response.json()?;
    let interval = match json.get("interval") {
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Some(0.0)
            } else {
                trimmed.parse::<f64>().ok()
            }
        }
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    };
    let device_auth_id = json
        .get("device_auth_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let user_code = json
        .get("user_code")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    match (
        device_auth_id,
        user_code,
        interval.filter(|i| i.is_finite() && *i >= 0.0),
    ) {
        (Some(device_auth_id), Some(user_code), Some(interval_seconds)) => Ok(DeviceAuthInfo {
            device_auth_id: device_auth_id.to_string(),
            user_code: user_code.to_string(),
            interval_seconds,
        }),
        _ => anyhow::bail!("Invalid OpenAI Codex device code response: {json}"),
    }
}

async fn poll_device_auth(
    fetch: &OAuthFetch,
    device: &DeviceAuthInfo,
    signal: &CancellationToken,
) -> anyhow::Result<(String, String)> {
    poll_oauth_device_code_flow(
        OAuthDeviceCodePollOptions {
            interval_seconds: Some(device.interval_seconds),
            expires_in_seconds: Some(DEVICE_CODE_TIMEOUT_SECONDS),
            wait_before_first_poll: false,
            signal: signal.clone(),
        },
        || async {
            let request = OAuthHttpRequest::post(DEVICE_TOKEN_URL)
                .header("Content-Type", "application/json")
                .body(json!({ "device_auth_id": device.device_auth_id, "user_code": device.user_code }).to_string());
            let response = fetch_with_login_cancellation(fetch, request, signal).await?;
            if response.ok() {
                let json = response.json()?;
                let code = json.get("authorization_code").and_then(Value::as_str).filter(|s| !s.is_empty());
                let verifier = json.get("code_verifier").and_then(Value::as_str).filter(|s| !s.is_empty());
                return Ok(match (code, verifier) {
                    (Some(code), Some(verifier)) => {
                        OAuthDeviceCodePollResult::Complete((code.to_string(), verifier.to_string()))
                    }
                    _ => OAuthDeviceCodePollResult::Failed {
                        message: format!("Invalid OpenAI Codex device auth token response: {json}"),
                    },
                });
            }
            if response.status == 403 || response.status == 404 {
                return Ok(OAuthDeviceCodePollResult::Pending);
            }
            let error_code = serde_json::from_str::<Value>(&response.body).ok().and_then(|json| match json.get("error") {
                Some(Value::Object(error)) => error.get("code").and_then(Value::as_str).map(str::to_string),
                Some(Value::String(error)) => Some(error.clone()),
                _ => None,
            });
            Ok(match error_code.as_deref() {
                Some("deviceauth_authorization_pending") => OAuthDeviceCodePollResult::Pending,
                Some("slow_down") => OAuthDeviceCodePollResult::SlowDown { interval_seconds: None },
                _ => {
                    let suffix = if response.body.is_empty() { String::new() } else { format!(": {}", response.body) };
                    OAuthDeviceCodePollResult::Failed {
                        message: format!("OpenAI Codex device auth failed with status {}{suffix}", response.status),
                    }
                }
            })
        },
    )
    .await
}

fn credentials_from_token(token: OAuthToken) -> anyhow::Result<OAuthCredential> {
    let Some(account_id) = get_account_id(&token.access) else {
        anyhow::bail!("Failed to extract accountId from token");
    };
    let mut extra = Map::new();
    extra.insert("accountId".into(), json!(account_id));
    Ok(OAuthCredential {
        access: token.access,
        refresh: token.refresh,
        expires: token.expires,
        extra,
    })
}

/// The browser authorization URL for a PKCE challenge and state.
pub fn create_authorization_url(challenge: &str, state: &str, originator: &str) -> String {
    let mut url = url::Url::parse(AUTHORIZE_URL).expect("valid authorize URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", originator);
    url.to_string()
}

async fn login_device_code(
    fetch: OAuthFetch,
    interaction: &ProviderAuthInteraction,
) -> anyhow::Result<OAuthCredential> {
    let device = start_device_auth(&fetch, &interaction.signal).await?;
    interaction.interaction.notify(AuthEvent::DeviceCode {
        user_code: device.user_code.clone(),
        verification_uri: DEVICE_VERIFICATION_URI.into(),
        interval_seconds: Some(device.interval_seconds as u64),
        expires_in_seconds: Some(DEVICE_CODE_TIMEOUT_SECONDS as u64),
    });
    let (code, verifier) = poll_device_auth(&fetch, &device, &interaction.signal).await?;
    credentials_from_token(
        exchange_authorization_code(
            &fetch,
            &code,
            &verifier,
            DEVICE_REDIRECT_URI,
            &interaction.signal,
        )
        .await?,
    )
}

async fn login_browser(
    fetch: OAuthFetch,
    interaction: &ProviderAuthInteraction,
    options: Option<&LoginOptions>,
) -> anyhow::Result<OAuthCredential> {
    let pkce = generate_pkce();
    let state = create_state();
    let originator = options
        .and_then(|o| o.agent_name.clone())
        .unwrap_or_else(|| "pi".into());
    let url = create_authorization_url(&pkce.challenge, &state, &originator);
    // Port 1455 is shared with the Codex CLI; when it is taken, fall back to the pasted redirect URL.
    let callback = start_oauth_callback_server(OAuthCallbackServerOptions::<String> {
        provider_name: "OpenAI".into(),
        host: callback_host(),
        port: 1455,
        path: "/auth/callback".into(),
        redirect_host: None,
        state: Some(state.clone()),
        complete: Arc::new(|code| Box::pin(async move { Ok(code) })),
        signal: Some(interaction.signal.clone()),
        timeout_ms: None,
    })
    .await
    .ok();
    interaction.interaction.notify(AuthEvent::AuthUrl {
        url,
        instructions: Some("A browser window should open. Complete login to finish.".into()),
    });
    let result = async {
        let outcome = wait_for_callback_or_manual_input(
            interaction,
            callback.as_ref(),
            "Complete login in your browser, or paste the authorization code / redirect URL here:",
            REDIRECT_URI,
        )
        .await?;
        let code = match outcome {
            CallbackOrManual::Callback(code) => Some(code),
            CallbackOrManual::Manual(input) => {
                let parsed = parse_authorization_input(&input);
                if parsed.state.as_ref().is_some_and(|s| *s != state) {
                    anyhow::bail!("State mismatch");
                }
                parsed.code
            }
        };
        let Some(code) = code.filter(|c| !c.is_empty()) else {
            anyhow::bail!("Missing authorization code")
        };
        credentials_from_token(
            exchange_authorization_code(
                &fetch,
                &code,
                &pkce.verifier,
                REDIRECT_URI,
                &interaction.signal,
            )
            .await?,
        )
    }
    .await;
    if let Some(callback) = &callback {
        callback.close();
    }
    result
}

/// `openaiCodexOAuth`.
pub struct OpenAICodexOAuth;

#[async_trait]
impl OAuthAuth for OpenAICodexOAuth {
    fn name(&self) -> &str {
        "OpenAI (ChatGPT Plus/Pro)"
    }
    fn is_subscription(&self) -> Option<bool> {
        Some(true)
    }
    async fn login(
        &self,
        interaction: ProviderAuthInteraction,
        options: Option<LoginOptions>,
    ) -> anyhow::Result<OAuthCredential> {
        let fetch = current_oauth_fetch();
        let method = interaction
            .interaction
            .prompt(select_prompt(
                "Select OpenAI Codex login method:",
                &[
                    (BROWSER_LOGIN_METHOD, "Browser login (default)"),
                    (DEVICE_CODE_LOGIN_METHOD, "Device code login (headless)"),
                ],
            ))
            .await?;
        match method.as_str() {
            DEVICE_CODE_LOGIN_METHOD => login_device_code(fetch, &interaction).await,
            BROWSER_LOGIN_METHOD => login_browser(fetch, &interaction, options.as_ref()).await,
            other => anyhow::bail!("Unknown OpenAI Codex login method: {other}"),
        }
    }
    async fn refresh(
        &self,
        credential: OAuthCredential,
        signal: CancellationToken,
    ) -> anyhow::Result<OAuthCredential> {
        credentials_from_token(
            refresh_access_token(&current_oauth_fetch(), &credential.refresh, &signal).await?,
        )
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        Ok(ModelAuth {
            api_key: Some(credential.access.clone()),
            ..Default::default()
        })
    }
}

pub fn openai_codex_oauth() -> Arc<dyn OAuthAuth> {
    Arc::new(OpenAICodexOAuth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_url_shape() {
        let url = url::Url::parse(&create_authorization_url("chal", "state1", "pi")).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["redirect_uri"], REDIRECT_URI);
        assert_eq!(params["codex_cli_simplified_flow"], "true");
        assert_eq!(params["originator"], "pi");
        assert_eq!(url.origin().ascii_serialization(), AUTH_BASE_URL);
        assert_eq!(create_state().len(), 32);
    }
}
