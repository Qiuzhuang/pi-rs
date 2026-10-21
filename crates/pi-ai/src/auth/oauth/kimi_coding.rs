//! Kimi Code (subscription) OAuth flow (port of `auth/oauth/kimi-coding.ts`).
//!
//! RFC 8628 device authorization grant against https://auth.kimi.com. The access token
//! authenticates requests to https://api.kimi.com/coding as an `Authorization: Bearer` header.

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
use crate::types::ProviderHeaders;
use crate::utils::provider_env::get_provider_env_value;

const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const DEFAULT_OAUTH_HOST: &str = "https://auth.kimi.com";
const DEVICE_CODE_TIMEOUT_SECONDS: f64 = 15.0 * 60.0;
const DEFAULT_POLL_INTERVAL_SECONDS: f64 = 5.0;
const REQUEST_TIMEOUT_MS: u64 = 30 * 1000;
const REFRESH_MAX_RETRIES: u32 = 3;

struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri_complete: String,
    interval_seconds: f64,
    expires_in_seconds: f64,
}

fn oauth_host() -> String {
    get_provider_env_value("KIMI_CODE_OAUTH_HOST", None)
        .or_else(|| get_provider_env_value("KIMI_OAUTH_HOST", None))
        .unwrap_or_else(|| DEFAULT_OAUTH_HOST.into())
        .trim_end_matches('/')
        .to_string()
}

fn form_request(url: String, fields: &[(&str, &str)]) -> OAuthHttpRequest {
    OAuthHttpRequest::post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(fields)
}

fn json_text(json: &Option<Map<String, Value>>) -> String {
    match json {
        Some(map) => Value::Object(map.clone()).to_string(),
        None => "null".into(),
    }
}

async fn start_device_authorization(
    fetch: &OAuthFetch,
    host: &str,
    signal: &CancellationToken,
) -> anyhow::Result<DeviceAuthorization> {
    let request = form_request(
        format!("{host}/api/oauth/device_authorization"),
        &[("client_id", CLIENT_ID)],
    );
    let response = send(fetch, request, Some(signal), Some(REQUEST_TIMEOUT_MS)).await?;
    if !response.ok() {
        let suffix = if response.body.is_empty() {
            String::new()
        } else {
            format!(": {}", response.body)
        };
        anyhow::bail!(
            "Kimi Code device authorization failed with status {}{suffix}",
            response.status
        );
    }
    let json = response.json_object();
    let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
    let device_code = field("device_code").and_then(Value::as_str);
    let user_code = field("user_code").and_then(Value::as_str);
    let verification_uri = field("verification_uri").and_then(Value::as_str);
    let complete = field("verification_uri_complete").and_then(Value::as_str);
    let (Some(device_code), Some(user_code), Some(_), Some(complete)) =
        (device_code, user_code, verification_uri, complete)
    else {
        anyhow::bail!(
            "Invalid Kimi Code device authorization response: {}",
            json_text(&json)
        );
    };
    if trusted_http_url(field("verification_uri_complete")).is_none()
        || trusted_http_url(field("verification_uri")).is_none()
    {
        anyhow::bail!(
            "Invalid Kimi Code device authorization response: {}",
            json_text(&json)
        );
    }
    Ok(DeviceAuthorization {
        device_code: device_code.to_string(),
        user_code: user_code.to_string(),
        verification_uri_complete: complete.to_string(),
        interval_seconds: positive_number(field("interval"))
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS),
        expires_in_seconds: positive_number(field("expires_in"))
            .unwrap_or(DEVICE_CODE_TIMEOUT_SECONDS),
    })
}

fn parse_token_response(
    json: &Option<Map<String, Value>>,
    operation: &str,
) -> anyhow::Result<OAuthCredential> {
    let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
    let access = field("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let refresh = field("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let expires_in = positive_number(field("expires_in"));
    let (Some(access), Some(refresh), Some(expires_in)) = (access, refresh, expires_in) else {
        anyhow::bail!(
            "Kimi Code token {operation} response missing fields: {}",
            json_text(json)
        );
    };
    Ok(OAuthCredential {
        access: access.to_string(),
        refresh: refresh.to_string(),
        expires: (crate::utils::now_ms() as f64 + expires_in * 1000.0) as i64,
        extra: Map::new(),
    })
}

async fn poll_for_token(
    fetch: &OAuthFetch,
    host: &str,
    device: &DeviceAuthorization,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    poll_oauth_device_code_flow(
        OAuthDeviceCodePollOptions {
            interval_seconds: Some(device.interval_seconds),
            expires_in_seconds: Some(device.expires_in_seconds),
            wait_before_first_poll: true,
            signal: signal.clone(),
        },
        || async {
            let request = form_request(
                format!("{host}/api/oauth/token"),
                &[
                    ("client_id", CLIENT_ID),
                    ("device_code", &device.device_code),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ],
            );
            let response = send(fetch, request, Some(signal), Some(REQUEST_TIMEOUT_MS)).await?;
            if response.status >= 500 {
                let suffix = if response.body.is_empty() {
                    String::new()
                } else {
                    format!(": {}", response.body)
                };
                return Ok(OAuthDeviceCodePollResult::Failed {
                    message: format!(
                        "Kimi Code device token request failed with status {}{suffix}",
                        response.status
                    ),
                });
            }
            let json = response.json_object();
            let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
            if response.ok() && field("access_token").is_some_and(Value::is_string) {
                return Ok(match parse_token_response(&json, "poll") {
                    Ok(credential) => OAuthDeviceCodePollResult::Complete(credential),
                    Err(error) => OAuthDeviceCodePollResult::Failed {
                        message: error.to_string(),
                    },
                });
            }
            let error = field("error").and_then(Value::as_str);
            let description = field("error_description")
                .and_then(Value::as_str)
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            Ok(match error {
                Some("authorization_pending") => OAuthDeviceCodePollResult::Pending,
                Some("slow_down") => OAuthDeviceCodePollResult::SlowDown {
                    interval_seconds: positive_number(field("interval")),
                },
                Some("expired_token") => OAuthDeviceCodePollResult::Failed {
                    message: "Kimi Code device authorization expired. Please restart login.".into(),
                },
                Some("access_denied") => OAuthDeviceCodePollResult::Failed {
                    message: "Kimi Code login was denied.".into(),
                },
                other => OAuthDeviceCodePollResult::Failed {
                    message: format!(
                        "Kimi Code device token request failed (status {}){}",
                        response.status,
                        other
                            .map(|e| format!(": {e}{description}"))
                            .unwrap_or_default()
                    ),
                },
            })
        },
    )
    .await
}

async fn refresh_token(
    fetch: &OAuthFetch,
    host: &str,
    refresh: &str,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    let mut last_error: Option<anyhow::Error> = None;
    for attempt in 0..=REFRESH_MAX_RETRIES {
        if attempt > 0 {
            crate::utils::sleep::sleep(1000 * 2u64.pow(attempt - 1), signal).await?;
        }
        if signal.is_cancelled() {
            anyhow::bail!("Kimi Code token refresh aborted");
        }
        let request = form_request(
            format!("{host}/api/oauth/token"),
            &[
                ("client_id", CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
            ],
        );
        let response = match send(fetch, request, Some(signal), Some(REQUEST_TIMEOUT_MS)).await {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        let json = response.json_object();
        if response.ok() {
            return parse_token_response(&json, "refresh");
        }
        let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
        // Unauthorized: the stored credential is dead; Models clears it and prompts re-login.
        if response.status == 401
            || response.status == 403
            || field("error").and_then(Value::as_str) == Some("invalid_grant")
        {
            let description = field("error_description")
                .and_then(Value::as_str)
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            anyhow::bail!(
                "Kimi Code token refresh unauthorized (status {}){description}",
                response.status
            );
        }
        if (response.status == 429 || response.status >= 500) && attempt < REFRESH_MAX_RETRIES {
            last_error = Some(anyhow::anyhow!(
                "Kimi Code token refresh failed with status {}",
                response.status
            ));
            continue;
        }
        anyhow::bail!(
            "Kimi Code token refresh failed with status {}: {}",
            response.status,
            json_text(&json)
        );
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Kimi Code token refresh failed")))
}

/// `kimiCodingOAuth`.
pub struct KimiCodingOAuth;

#[async_trait]
impl OAuthAuth for KimiCodingOAuth {
    fn name(&self) -> &str {
        "Kimi Code (subscription)"
    }
    fn is_subscription(&self) -> Option<bool> {
        Some(true)
    }
    fn login_label(&self) -> Option<&str> {
        Some("Sign in with Kimi Code")
    }
    async fn login(
        &self,
        interaction: ProviderAuthInteraction,
        _options: Option<LoginOptions>,
    ) -> anyhow::Result<OAuthCredential> {
        let fetch = current_oauth_fetch();
        let host = oauth_host();
        let device = start_device_authorization(&fetch, &host, &interaction.signal).await?;
        interaction.interaction.notify(AuthEvent::DeviceCode {
            user_code: device.user_code.clone(),
            verification_uri: device.verification_uri_complete.clone(),
            interval_seconds: Some(device.interval_seconds as u64),
            expires_in_seconds: Some(device.expires_in_seconds as u64),
        });
        poll_for_token(&fetch, &host, &device, &interaction.signal).await
    }
    async fn refresh(
        &self,
        credential: OAuthCredential,
        signal: CancellationToken,
    ) -> anyhow::Result<OAuthCredential> {
        refresh_token(
            &current_oauth_fetch(),
            &oauth_host(),
            &credential.refresh,
            &signal,
        )
        .await
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        let mut headers = ProviderHeaders::new();
        headers.insert(
            "Authorization".into(),
            Some(format!("Bearer {}", credential.access)),
        );
        Ok(ModelAuth {
            headers: Some(headers),
            ..Default::default()
        })
    }
}

pub fn kimi_coding_oauth() -> Arc<dyn OAuthAuth> {
    Arc::new(KimiCodingOAuth)
}
