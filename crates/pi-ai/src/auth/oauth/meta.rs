//! Meta Model API OAuth flow (port of `auth/oauth/meta.ts`).
//!
//! RFC 8628 device authorization grant against https://auth.meta.com. The identity token is not
//! accepted for inference, so it is exchanged for a Model API key via the Muse Code key-mint
//! endpoint (minted keys live about a day). The identity token is stored as `refresh` and the
//! minted key as `access`, so the standard OAuth scheduler re-mints the key when it expires. The
//! identity token itself is not renewable, so a 401/403 from mint means the user must sign in again.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use super::device_code::{
    OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult, poll_oauth_device_code_flow,
};
use super::http::{OAuthFetch, OAuthHttpRequest, current_oauth_fetch, send};
use super::{positive_number, trusted_http_url};
use crate::auth::types::{
    AuthEvent, LoginOptions, ModelAuth, OAuthAuth, OAuthCredential, ProviderAuthInteraction,
};

/// Muse Code CLI client id.
const CLIENT_ID: &str = "1031625952748946";
pub const DEVICE_AUTHORIZATION_URL: &str = "https://auth.meta.com/oidc/device/authorization/";
pub const DEVICE_TOKEN_URL: &str = "https://auth.meta.com/oidc/device/token/";
pub const API_KEY_MINT_URL: &str = "https://api.meta.ai/muse-code/key";
const API_KEY_LIFETIME_MS: i64 = 24 * 60 * 60 * 1000;
const REQUEST_TIMEOUT_MS: u64 = 30 * 1000;

struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval_seconds: Option<f64>,
    expires_in_seconds: Option<f64>,
}

fn error_detail(json: &Option<Map<String, Value>>) -> String {
    for key in ["error_description", "detail", "message", "error"] {
        if let Some(value) = json
            .as_ref()
            .and_then(|j| j.get(key))
            .and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            return format!(": {}", value.trim());
        }
    }
    String::new()
}

fn json_text(json: &Option<Map<String, Value>>) -> String {
    json.as_ref()
        .map(|m| Value::Object(m.clone()).to_string())
        .unwrap_or_else(|| "null".into())
}

fn form_request(url: &str, fields: &[(&str, &str)]) -> OAuthHttpRequest {
    OAuthHttpRequest::post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(fields)
}

async fn start_device_authorization(
    fetch: &OAuthFetch,
    signal: &CancellationToken,
) -> anyhow::Result<DeviceAuthorization> {
    let response = send(
        fetch,
        form_request(DEVICE_AUTHORIZATION_URL, &[("client_id", CLIENT_ID)]),
        Some(signal),
        Some(REQUEST_TIMEOUT_MS),
    )
    .await?;
    let json = response.json_object();
    if !response.ok() {
        anyhow::bail!(
            "Meta device authorization failed with status {}{}",
            response.status,
            error_detail(&json)
        );
    }
    let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
    let device_code = field("device_code")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let user_code = field("user_code")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let verification_uri = trusted_http_url(field("verification_uri_complete"))
        .or_else(|| trusted_http_url(field("verification_uri")));
    let (Some(device_code), Some(user_code), Some(verification_uri)) =
        (device_code, user_code, verification_uri)
    else {
        anyhow::bail!(
            "Invalid Meta device authorization response: {}",
            json_text(&json)
        );
    };
    Ok(DeviceAuthorization {
        device_code: device_code.to_string(),
        user_code: user_code.to_string(),
        verification_uri,
        interval_seconds: positive_number(field("interval")),
        expires_in_seconds: positive_number(field("expires_in")),
    })
}

async fn poll_for_identity_token(
    fetch: &OAuthFetch,
    device: &DeviceAuthorization,
    signal: &CancellationToken,
) -> anyhow::Result<String> {
    poll_oauth_device_code_flow(
        OAuthDeviceCodePollOptions {
            interval_seconds: device.interval_seconds,
            expires_in_seconds: device.expires_in_seconds,
            wait_before_first_poll: true,
            signal: signal.clone(),
        },
        || async {
            let request = form_request(
                DEVICE_TOKEN_URL,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", &device.device_code),
                    ("client_id", CLIENT_ID),
                ],
            );
            let response = send(fetch, request, Some(signal), Some(REQUEST_TIMEOUT_MS)).await?;
            let json = response.json_object();
            let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
            if response.ok()
                && let Some(token) = field("access_token")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            {
                return Ok(OAuthDeviceCodePollResult::Complete(token.to_string()));
            }
            Ok(match field("error").and_then(Value::as_str) {
                Some("authorization_pending") => OAuthDeviceCodePollResult::Pending,
                Some("slow_down") => OAuthDeviceCodePollResult::SlowDown {
                    interval_seconds: positive_number(field("interval")),
                },
                Some("access_denied") => OAuthDeviceCodePollResult::Failed {
                    message: "Meta login was denied.".into(),
                },
                Some("expired_token") => OAuthDeviceCodePollResult::Failed {
                    message: "Meta device authorization expired. Please restart login.".into(),
                },
                _ => OAuthDeviceCodePollResult::Failed {
                    message: format!(
                        "Meta device token request failed with status {}{}",
                        response.status,
                        error_detail(&json)
                    ),
                },
            })
        },
    )
    .await
}

/// Exchange an identity token for a Model API key. Keys are valid for about a day.
async fn mint_api_key(
    fetch: &OAuthFetch,
    identity_token: &str,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    let request = OAuthHttpRequest::post(API_KEY_MINT_URL)
        .header("Accept", "application/json")
        .header("Authorization", &format!("Bearer {identity_token}"))
        .header("Content-Type", "application/json")
        .header("x-api-version", "1.0.0")
        .body("{}");
    let response = send(fetch, request, Some(signal), Some(REQUEST_TIMEOUT_MS)).await?;
    let json = response.json_object();
    if response.status == 401 || response.status == 403 {
        anyhow::bail!(
            "Meta session expired (status {}). Run `/login meta` to sign in again.{}",
            response.status,
            error_detail(&json)
        );
    }
    if !response.ok() {
        anyhow::bail!(
            "Meta API key mint failed with status {}{}",
            response.status,
            error_detail(&json)
        );
    }
    let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
    let Some(api_key) = field("api_key")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        let action = trusted_http_url(field("action_url"))
            .map(|url| format!(" Complete setup at {url}"))
            .unwrap_or_default();
        anyhow::bail!("Meta did not issue an API key.{action}");
    };
    Ok(OAuthCredential {
        refresh: identity_token.to_string(),
        access: api_key.to_string(),
        expires: crate::utils::now_ms() + API_KEY_LIFETIME_MS,
        extra: Map::new(),
    })
}

/// `metaOAuth`.
pub struct MetaOAuth;

#[async_trait]
impl OAuthAuth for MetaOAuth {
    fn name(&self) -> &str {
        "Meta (Muse subscription)"
    }
    fn is_subscription(&self) -> Option<bool> {
        Some(true)
    }
    fn login_label(&self) -> Option<&str> {
        Some("Sign in with Meta")
    }
    async fn login(
        &self,
        interaction: ProviderAuthInteraction,
        _options: Option<LoginOptions>,
    ) -> anyhow::Result<OAuthCredential> {
        let fetch = current_oauth_fetch();
        let signal = interaction.signal.clone();
        let result = async {
            let device = start_device_authorization(&fetch, &signal).await?;
            interaction.interaction.notify(AuthEvent::DeviceCode {
                user_code: device.user_code.clone(),
                verification_uri: device.verification_uri.clone(),
                interval_seconds: device.interval_seconds.map(|i| i as u64),
                expires_in_seconds: device.expires_in_seconds.map(|e| e as u64),
            });
            let identity_token = poll_for_identity_token(&fetch, &device, &signal).await?;
            interaction.interaction.notify(AuthEvent::Progress {
                message: "Enabling Meta Model API access...".into(),
            });
            mint_api_key(&fetch, &identity_token, &signal).await
        }
        .await;
        match result {
            Err(_) if signal.is_cancelled() => anyhow::bail!("Login cancelled"),
            other => other,
        }
    }
    async fn refresh(
        &self,
        credential: OAuthCredential,
        signal: CancellationToken,
    ) -> anyhow::Result<OAuthCredential> {
        mint_api_key(&current_oauth_fetch(), &credential.refresh, &signal).await
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        Ok(ModelAuth {
            api_key: Some(credential.access.clone()),
            ..Default::default()
        })
    }
}

pub fn meta_oauth() -> Arc<dyn OAuthAuth> {
    Arc::new(MetaOAuth)
}
