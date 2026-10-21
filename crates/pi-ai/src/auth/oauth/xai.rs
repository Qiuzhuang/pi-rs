//! xAI OAuth device-code flow (port of `auth/oauth/xai.ts`).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use super::device_code::{
    OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult, poll_oauth_device_code_flow,
};
use super::http::{OAuthFetch, OAuthHttpRequest, current_oauth_fetch, send};
use crate::auth::types::{
    AuthEvent, LoginOptions, ModelAuth, OAuthAuth, OAuthCredential, ProviderAuthInteraction,
};

const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
pub const XAI_DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
pub const XAI_TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
/// Refresh slightly before the reported expiry.
const REFRESH_SKEW_MS: f64 = 5.0 * 60.0 * 1000.0;
const DEFAULT_TOKEN_LIFETIME_SECONDS: f64 = 3600.0;

struct HttpResult {
    ok: bool,
    status: u16,
    body: Map<String, Value>,
}

struct XaiDeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    interval_seconds: Option<f64>,
    expires_in_seconds: f64,
}

fn required_string(body: &Map<String, Value>, field: &str) -> anyhow::Result<String> {
    match body.get(field) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => anyhow::bail!("Invalid xAI OAuth response field: {field}"),
    }
}

fn positive_number(body: &Map<String, Value>, field: &str) -> anyhow::Result<f64> {
    match body.get(field).and_then(Value::as_f64) {
        Some(n) if n.is_finite() && n > 0.0 => Ok(n),
        _ => anyhow::bail!("Invalid xAI OAuth response field: {field}"),
    }
}

/// Force the verification URI to https so a malicious response cannot make `open` launch something else.
fn validate_verification_uri(raw: &str) -> anyhow::Result<String> {
    match url::Url::parse(raw) {
        Ok(url) if url.scheme() == "https" => Ok(url.to_string()),
        _ => anyhow::bail!("Untrusted verification URI in xAI OAuth response"),
    }
}

async fn post_form(
    fetch: &OAuthFetch,
    url: &str,
    fields: &[(&str, &str)],
    signal: &CancellationToken,
) -> anyhow::Result<HttpResult> {
    let request = OAuthHttpRequest::post(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(fields);
    let response = match send(fetch, request, Some(signal), None).await {
        Ok(response) => response,
        Err(_) if signal.is_cancelled() => anyhow::bail!("Login cancelled"),
        Err(error) => return Err(error),
    };
    let body = match serde_json::from_str::<Value>(&response.body) {
        Ok(Value::Object(map)) => map,
        Ok(_) => Map::new(),
        Err(_) => {
            if signal.is_cancelled() {
                anyhow::bail!("Login cancelled");
            }
            anyhow::bail!("xAI OAuth returned invalid JSON (HTTP {})", response.status);
        }
    };
    Ok(HttpResult {
        ok: response.ok(),
        status: response.status,
        body,
    })
}

fn request_failure(action: &str, response: &HttpResult) -> anyhow::Error {
    let error = response
        .body
        .get("error")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let description = response
        .body
        .get("error_description")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let detail = [error, description]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(": ");
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    };
    anyhow::anyhow!(
        "xAI OAuth {action} failed (HTTP {}){suffix}",
        response.status
    )
}

fn parse_device_code(body: &Map<String, Value>) -> anyhow::Result<XaiDeviceCode> {
    // RFC 8628 allows interval 0; fall back to the poller default for non-positive values.
    let interval_seconds = body
        .get("interval")
        .and_then(Value::as_f64)
        .filter(|i| i.is_finite() && *i > 0.0);
    let verification_uri_complete = match body.get("verification_uri_complete") {
        Some(Value::String(s)) if !s.is_empty() => Some(validate_verification_uri(s)?),
        _ => None,
    };
    Ok(XaiDeviceCode {
        device_code: required_string(body, "device_code")?,
        user_code: required_string(body, "user_code")?,
        verification_uri: validate_verification_uri(&required_string(body, "verification_uri")?)?,
        verification_uri_complete,
        interval_seconds,
        expires_in_seconds: positive_number(body, "expires_in")?,
    })
}

fn credentials_from_token_response(
    body: &Map<String, Value>,
    previous_refresh: Option<&str>,
) -> anyhow::Result<OAuthCredential> {
    let access = required_string(body, "access_token")?;
    // xAI may omit refresh_token on refresh when the token is not rotated.
    let refresh = match (
        body.get("refresh_token"),
        previous_refresh.filter(|r| !r.is_empty()),
    ) {
        (None, Some(previous)) => previous.to_string(),
        _ => required_string(body, "refresh_token")?,
    };
    let expires_in = if body.contains_key("expires_in") {
        positive_number(body, "expires_in")?
    } else {
        DEFAULT_TOKEN_LIFETIME_SECONDS
    };
    Ok(OAuthCredential {
        access,
        refresh,
        expires: (crate::utils::now_ms() as f64 + expires_in * 1000.0 - REFRESH_SKEW_MS) as i64,
        extra: Map::new(),
    })
}

async fn request_device_code(
    fetch: &OAuthFetch,
    signal: &CancellationToken,
) -> anyhow::Result<XaiDeviceCode> {
    let response = post_form(
        fetch,
        XAI_DEVICE_CODE_URL,
        &[
            ("client_id", XAI_CLIENT_ID),
            ("scope", XAI_SCOPE),
            ("referrer", "pi"),
        ],
        signal,
    )
    .await?;
    if !response.ok {
        return Err(request_failure("device authorization", &response));
    }
    parse_device_code(&response.body)
}

async fn poll_for_tokens(
    fetch: &OAuthFetch,
    device: &XaiDeviceCode,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    poll_oauth_device_code_flow(
        OAuthDeviceCodePollOptions {
            interval_seconds: device.interval_seconds,
            expires_in_seconds: Some(device.expires_in_seconds),
            wait_before_first_poll: true,
            signal: signal.clone(),
        },
        || async {
            let response = post_form(
                fetch,
                XAI_TOKEN_URL,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", XAI_CLIENT_ID),
                    ("device_code", &device.device_code),
                ],
                signal,
            )
            .await?;
            if response.ok {
                return Ok(OAuthDeviceCodePollResult::Complete(
                    credentials_from_token_response(&response.body, None)?,
                ));
            }
            Ok(match response.body.get("error").and_then(Value::as_str) {
                Some("authorization_pending") => OAuthDeviceCodePollResult::Pending,
                Some("slow_down") => OAuthDeviceCodePollResult::SlowDown {
                    interval_seconds: response.body.get("interval").and_then(Value::as_f64),
                },
                Some("access_denied" | "authorization_denied") => {
                    OAuthDeviceCodePollResult::Failed {
                        message: "xAI device authorization was denied".into(),
                    }
                }
                Some("expired_token") => OAuthDeviceCodePollResult::Failed {
                    message: "xAI device code expired".into(),
                },
                _ => OAuthDeviceCodePollResult::Failed {
                    message: request_failure("device token polling", &response).to_string(),
                },
            })
        },
    )
    .await
}

async fn refresh_xai_token(
    fetch: &OAuthFetch,
    refresh: &str,
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    let response = post_form(
        fetch,
        XAI_TOKEN_URL,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", XAI_CLIENT_ID),
            ("refresh_token", refresh),
        ],
        signal,
    )
    .await?;
    if !response.ok {
        return Err(request_failure("token refresh", &response));
    }
    credentials_from_token_response(&response.body, Some(refresh))
}

/// `xaiOAuth`.
pub struct XaiOAuth;

#[async_trait]
impl OAuthAuth for XaiOAuth {
    fn name(&self) -> &str {
        "xAI (Grok/X subscription)"
    }
    fn is_subscription(&self) -> Option<bool> {
        Some(true)
    }
    fn login_label(&self) -> Option<&str> {
        Some("Sign in with SuperGrok or X Premium")
    }
    async fn login(
        &self,
        interaction: ProviderAuthInteraction,
        _options: Option<LoginOptions>,
    ) -> anyhow::Result<OAuthCredential> {
        let fetch = current_oauth_fetch();
        let device = request_device_code(&fetch, &interaction.signal).await?;
        interaction.interaction.notify(AuthEvent::DeviceCode {
            user_code: device.user_code.clone(),
            verification_uri: device
                .verification_uri_complete
                .clone()
                .unwrap_or_else(|| device.verification_uri.clone()),
            interval_seconds: device.interval_seconds.map(|i| i as u64),
            expires_in_seconds: Some(device.expires_in_seconds as u64),
        });
        poll_for_tokens(&fetch, &device, &interaction.signal).await
    }
    async fn refresh(
        &self,
        credential: OAuthCredential,
        signal: CancellationToken,
    ) -> anyhow::Result<OAuthCredential> {
        refresh_xai_token(&current_oauth_fetch(), &credential.refresh, &signal).await
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        Ok(ModelAuth {
            api_key: Some(credential.access.clone()),
            ..Default::default()
        })
    }
}

pub fn xai_oauth() -> Arc<dyn OAuthAuth> {
    Arc::new(XaiOAuth)
}
