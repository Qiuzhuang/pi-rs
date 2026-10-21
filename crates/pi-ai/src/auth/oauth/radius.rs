//! Radius gateway OAuth flow (port of `auth/oauth/radius.ts`).
//!
//! Radius is a pi-messages gateway. OAuth client APIs live on the configured gateway; only the
//! interactive browser authorization endpoint is discovered.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use super::callback_server::{OAuthCallbackServerOptions, start_oauth_callback_server};
use super::device_code::{OAuthDeviceCodePollOptions, OAuthDeviceCodePollResult, poll_oauth_device_code_flow};
use super::http::{OAuthFetch, OAuthHttpRequest, OAuthHttpResponse, current_oauth_fetch, send};
use super::pkce::generate_pkce;
use super::select_prompt;
use crate::auth::types::{AuthEvent, LoginOptions, ModelAuth, OAuthAuth, OAuthCredential, ProviderAuthInteraction};
use crate::providers::radius_config::normalize_radius_gateway_url;

const CALLBACK_HOST: &str = "127.0.0.1";
const CALLBACK_PORT: u16 = 1456;
const CALLBACK_PATH: &str = "/oauth/callback";
pub const REDIRECT_URI: &str = "http://127.0.0.1:1456/oauth/callback";
const TOKEN_EXPIRY_SKEW_MS: f64 = 60_000.0;
const LOGIN_METHOD_BROWSER: &str = "browser";
const LOGIN_METHOD_DEVICE_CODE: &str = "device-code";
const OAUTH_CLIENT_ID: &str = "pi-gateway";
const OAUTH_SCOPE: &str = "gateway offline_access";
const OAUTH_DEVICE_CODE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

fn gateway_url(gateway: &str, path: &str) -> anyhow::Result<String> {
    Ok(url::Url::parse(gateway)?.join(path)?.to_string())
}

/// An OAuth error response (`OAuthResponseError`).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct OAuthResponseError {
    pub status: u16,
    pub oauth_error: Option<String>,
    pub message: String,
}

fn read_oauth_response_error(response: &OAuthHttpResponse, message: &str) -> OAuthResponseError {
    let mut oauth_error = None;
    let mut description = None;
    if !response.body.is_empty() {
        match serde_json::from_str::<Value>(&response.body) {
            Ok(data) => {
                oauth_error = data.get("error").and_then(Value::as_str).map(str::to_string);
                description = data.get("error_description").and_then(Value::as_str).map(str::to_string);
            }
            Err(_) => description = Some(response.body.clone()),
        }
    }
    let detail = match (&oauth_error, &description) {
        (Some(error), Some(description)) => format!("{error}: {description}"),
        (Some(error), None) => error.clone(),
        (None, Some(description)) if !description.is_empty() => description.clone(),
        _ => response.status.to_string(),
    };
    OAuthResponseError { status: response.status, oauth_error, message: format!("{message}: {detail}") }
}

async fn load_authorization_endpoint(fetch: &OAuthFetch, gateway: &str, signal: &CancellationToken) -> anyhow::Result<String> {
    let request = OAuthHttpRequest::get(gateway_url(gateway, "/v1/oauth")?).header("accept", "application/json");
    let response = send(fetch, request, Some(signal), None).await?;
    if !response.ok() {
        anyhow::bail!("Could not load Radius OAuth config from {gateway}: {} {}", response.status, response.body);
    }
    match response.json()?.get("authorizationEndpoint") {
        Some(Value::String(endpoint)) => Ok(endpoint.clone()),
        _ => anyhow::bail!("Invalid Radius OAuth config from {gateway}"),
    }
}

async fn request_oauth_token(
    fetch: &OAuthFetch,
    gateway: &str,
    fields: &[(&str, &str)],
    signal: &CancellationToken,
) -> anyhow::Result<OAuthCredential> {
    let request = OAuthHttpRequest::post(gateway_url(gateway, "/v1/oauth/token")?)
        .header("accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .form(fields);
    let response = match send(fetch, request, Some(signal), None).await {
        Ok(response) => response,
        Err(_) if signal.is_cancelled() => anyhow::bail!("Login cancelled"),
        Err(error) => return Err(error),
    };
    if !response.ok() {
        return Err(read_oauth_response_error(&response, "Radius OAuth token request failed").into());
    }
    let data = response.json()?;
    let mut extra = Map::new();
    if let Some(scope) = data.get("scope").filter(|s| !s.is_null()) {
        extra.insert("scope".into(), scope.clone());
    }
    let expires_in = data.get("expires_in").and_then(Value::as_f64).unwrap_or(f64::NAN);
    Ok(OAuthCredential {
        access: data.get("access_token").and_then(Value::as_str).unwrap_or_default().to_string(),
        refresh: data.get("refresh_token").and_then(Value::as_str).unwrap_or_default().to_string(),
        expires: (crate::utils::now_ms() as f64 + expires_in * 1000.0 - TOKEN_EXPIRY_SKEW_MS) as i64,
        extra,
    })
}

async fn login_with_browser(
    fetch: OAuthFetch,
    gateway: &str,
    authorization_endpoint: &str,
    interaction: &ProviderAuthInteraction,
) -> anyhow::Result<OAuthCredential> {
    let pkce = generate_pkce();
    let state = uuid::Uuid::new_v4().to_string();
    let mut authorize_url = url::Url::parse(authorization_endpoint)?;
    authorize_url.set_query(Some(&super::http::form_urlencode(&[
        ("response_type", "code"),
        ("client_id", OAUTH_CLIENT_ID),
        ("redirect_uri", REDIRECT_URI),
        ("scope", OAUTH_SCOPE),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("handoff", "url"),
        ("state", &state),
    ])));

    let complete_fetch = fetch.clone();
    let complete_gateway = gateway.to_string();
    let verifier = pkce.verifier.clone();
    let signal = interaction.signal.clone();
    let callback = start_oauth_callback_server(OAuthCallbackServerOptions::<OAuthCredential> {
        provider_name: "Radius".into(),
        host: CALLBACK_HOST.into(),
        port: CALLBACK_PORT,
        path: CALLBACK_PATH.into(),
        redirect_host: None,
        state: Some(state),
        complete: Arc::new(move |code| {
            let fetch = complete_fetch.clone();
            let gateway = complete_gateway.clone();
            let verifier = verifier.clone();
            let signal = signal.clone();
            Box::pin(async move {
                request_oauth_token(
                    &fetch,
                    &gateway,
                    &[
                        ("grant_type", "authorization_code"),
                        ("client_id", OAUTH_CLIENT_ID),
                        ("redirect_uri", REDIRECT_URI),
                        ("code", &code),
                        ("code_verifier", &verifier),
                    ],
                    &signal,
                )
                .await
            })
        }),
        signal: Some(interaction.signal.clone()),
        timeout_ms: None,
    })
    .await?;
    interaction.interaction.notify(AuthEvent::Progress { message: format!("Listening for OAuth callback on {REDIRECT_URI}") });
    interaction.interaction.notify(AuthEvent::AuthUrl {
        url: authorize_url.to_string(),
        instructions: Some("Continue in your browser.".into()),
    });
    let result = callback.wait().await;
    callback.close();
    match result? {
        Some(credential) => Ok(credential),
        None => anyhow::bail!("OAuth callback did not complete."),
    }
}

struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: f64,
    interval: Option<f64>,
}

async fn request_device_authorization(fetch: &OAuthFetch, gateway: &str, signal: &CancellationToken) -> anyhow::Result<DeviceAuthorization> {
    let request = OAuthHttpRequest::post(gateway_url(gateway, "/v1/oauth/device")?)
        .header("accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .form(&[("client_id", OAUTH_CLIENT_ID), ("scope", OAUTH_SCOPE)]);
    let response = match send(fetch, request, Some(signal), None).await {
        Ok(response) => response,
        Err(_) if signal.is_cancelled() => anyhow::bail!("Login cancelled"),
        Err(error) => return Err(error),
    };
    if !response.ok() {
        return Err(read_oauth_response_error(&response, "Radius OAuth device authorization failed").into());
    }
    let data = response.json()?;
    let text = |name: &str| data.get(name).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let expires_in = data.get("expires_in").and_then(Value::as_f64).filter(|e| *e != 0.0);
    let (Some(device_code), Some(user_code), Some(verification_uri), Some(expires_in)) =
        (text("device_code"), text("user_code"), text("verification_uri"), expires_in)
    else {
        anyhow::bail!("Radius OAuth device authorization response is missing required fields");
    };
    Ok(DeviceAuthorization {
        device_code,
        user_code,
        verification_uri,
        expires_in,
        interval: data.get("interval").and_then(Value::as_f64),
    })
}

async fn login_with_device_code(fetch: OAuthFetch, gateway: &str, interaction: &ProviderAuthInteraction) -> anyhow::Result<OAuthCredential> {
    let device = request_device_authorization(&fetch, gateway, &interaction.signal).await?;
    interaction.interaction.notify(AuthEvent::DeviceCode {
        user_code: device.user_code.clone(),
        verification_uri: device.verification_uri.clone(),
        interval_seconds: device.interval.map(|i| i as u64),
        expires_in_seconds: Some(device.expires_in as u64),
    });
    let signal = interaction.signal.clone();
    poll_oauth_device_code_flow(
        OAuthDeviceCodePollOptions {
            interval_seconds: device.interval,
            expires_in_seconds: Some(device.expires_in),
            wait_before_first_poll: false,
            signal: signal.clone(),
        },
        || async {
            let result = request_oauth_token(
                &fetch,
                gateway,
                &[
                    ("grant_type", OAUTH_DEVICE_CODE_GRANT_TYPE),
                    ("client_id", OAUTH_CLIENT_ID),
                    ("device_code", &device.device_code),
                ],
                &signal,
            )
            .await;
            match result {
                Ok(credential) => Ok(OAuthDeviceCodePollResult::Complete(credential)),
                Err(error) => {
                    let Some(oauth) = error.downcast_ref::<OAuthResponseError>() else { return Err(error) };
                    match oauth.oauth_error.as_deref() {
                        Some("authorization_pending") => Ok(OAuthDeviceCodePollResult::Pending),
                        Some("slow_down") => Ok(OAuthDeviceCodePollResult::SlowDown { interval_seconds: None }),
                        Some("expired_token") => {
                            Ok(OAuthDeviceCodePollResult::Failed { message: "Device authorization expired.".into() })
                        }
                        Some("access_denied") => {
                            Ok(OAuthDeviceCodePollResult::Failed { message: "Device authorization was denied.".into() })
                        }
                        _ => Err(error),
                    }
                }
            }
        },
    )
    .await
}

/// Options for [`create_radius_oauth`].
#[derive(Debug, Clone)]
pub struct RadiusOAuthOptions {
    pub name: String,
    pub gateway: String,
}

/// Radius OAuth for one gateway.
pub struct RadiusOAuth {
    name: String,
    gateway: String,
}

pub fn create_radius_oauth(options: RadiusOAuthOptions) -> Arc<dyn OAuthAuth> {
    Arc::new(RadiusOAuth { name: options.name, gateway: normalize_radius_gateway_url(&options.gateway) })
}

#[async_trait]
impl OAuthAuth for RadiusOAuth {
    fn name(&self) -> &str {
        &self.name
    }
    async fn login(&self, interaction: ProviderAuthInteraction, _options: Option<LoginOptions>) -> anyhow::Result<OAuthCredential> {
        let fetch = current_oauth_fetch();
        let method = interaction
            .interaction
            .prompt(select_prompt(
                &format!("Sign in to {}:", self.name),
                &[
                    (LOGIN_METHOD_BROWSER, "Sign in with browser (recommended)"),
                    (LOGIN_METHOD_DEVICE_CODE, "Sign in with device code (when signing in from another device)"),
                ],
            ))
            .await?;
        match method.as_str() {
            LOGIN_METHOD_DEVICE_CODE => login_with_device_code(fetch, &self.gateway, &interaction).await,
            LOGIN_METHOD_BROWSER => {
                let endpoint = load_authorization_endpoint(&fetch, &self.gateway, &interaction.signal).await?;
                login_with_browser(fetch, &self.gateway, &endpoint, &interaction).await
            }
            other => anyhow::bail!("Unknown {} sign-in method: {other}", self.name),
        }
    }
    async fn refresh(&self, credential: OAuthCredential, signal: CancellationToken) -> anyhow::Result<OAuthCredential> {
        request_oauth_token(
            &current_oauth_fetch(),
            &self.gateway,
            &[("grant_type", "refresh_token"), ("client_id", OAUTH_CLIENT_ID), ("refresh_token", &credential.refresh)],
            &signal,
        )
        .await
    }
    async fn to_auth(&self, credential: &OAuthCredential) -> anyhow::Result<ModelAuth> {
        Ok(ModelAuth { api_key: Some(credential.access.clone()), ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn oauth_error_details() {
        let response = OAuthHttpResponse::new(400, json!({ "error": "authorization_pending" }).to_string());
        let error = read_oauth_response_error(&response, "Radius OAuth token request failed");
        assert_eq!(error.message, "Radius OAuth token request failed: authorization_pending");
        assert_eq!(error.oauth_error.as_deref(), Some("authorization_pending"));
        let text = read_oauth_response_error(&OAuthHttpResponse::new(500, "boom"), "x");
        assert_eq!(text.message, "x: boom");
        let empty = read_oauth_response_error(&OAuthHttpResponse::new(503, ""), "x");
        assert_eq!(empty.message, "x: 503");
        assert_eq!(gateway_url("https://radius.example/api/", "/v1/oauth").unwrap(), "https://radius.example/v1/oauth");
    }
}
