//! AWS credential and region resolution for Bedrock: the parts of the AWS SDK default chain
//! (`@aws-sdk/credential-provider-node`) that a CLI relies on.
//!
//! Order (`defaultProvider`): environment keys (skipped when a profile is configured), the
//! shared config/credentials profile (`fromIni`: static keys, `role_arn` with `source_profile`
//! or `credential_source` via STS `AssumeRole`, `web_identity_token_file`, `credential_process`,
//! IAM Identity Center/SSO from the `~/.aws/sso/cache` token), `credential_process`, the web
//! identity token file from `AWS_WEB_IDENTITY_TOKEN_FILE`/`AWS_ROLE_ARN`, and finally the ECS
//! container endpoint or EC2 instance metadata (IMDSv2). MFA-protected role profiles fail like
//! the SDK's default chain, which has no MFA code callback. `aws login` sessions
//! (`login_session`) use the cached credentials and refresh them with a DPoP-bound
//! `CreateOAuth2Token` request.

use std::collections::HashMap;
use std::path::PathBuf;

use indexmap::IndexMap;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;
use sha1::Digest as _;

use super::aws_sigv4::{AwsCredentials, SignableRequest, sign_request};

/// Error raised when no source produced credentials.
pub const NO_CREDENTIALS: &str = "Could not load credentials from any providers";
/// Refresh temporary credentials this long before they expire.
const EXPIRY_WINDOW_MS: i64 = 5 * 60 * 1000;

static CACHE: Lazy<Mutex<HashMap<String, AwsCredentials>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// `CredentialsProviderError`: a chain link failure. With `try_next_link` the default chain moves
/// on to the next link; otherwise it stops with this error.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct CredentialsProviderError {
    pub message: String,
    pub try_next_link: bool,
}

/// A `CredentialsProviderError` that lets the chain continue.
fn provider_error(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CredentialsProviderError { message: message.into(), try_next_link: true })
}

/// A `CredentialsProviderError` with `tryNextLink: false`.
fn terminal(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CredentialsProviderError { message: message.into(), try_next_link: false })
}

/// One profile's settings in file order.
pub type Profile = IndexMap<String, String>;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn home_dir() -> Option<PathBuf> {
    env("HOME").map(PathBuf::from).or_else(|| env("USERPROFILE").map(PathBuf::from)).or_else(dirs::home_dir)
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home_dir().map(|h| h.join(rest)).unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

/// Parsed sections of an INI file: section name → key → value, in file order.
pub type IniSections = IndexMap<String, Profile>;

/// `parseIni` of `@smithy/shared-ini-file-loader`: `[profile x]`/`[sso-session x]`/`[services x]`
/// sections become `profile.x`/`sso-session.x`/`services.x`; comments start at `#`/`;` at the
/// line start or after whitespace; nested properties are stored as `parent.key`.
pub fn parse_ini(text: &str) -> IniSections {
    let comment = regex::Regex::new(r"(^|\s)[;#]").expect("valid regex");
    let prefixed = regex::Regex::new(r#"^([\w-]+)\s(["'])?([\w\-@+.%:/]+)(["'])?$"#).expect("valid regex");
    let mut sections: IniSections = IndexMap::new();
    let mut current: Option<String> = None;
    let mut sub_section: Option<String> = None;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = comment.splitn(line, 2).next().unwrap_or_default().trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
            current = None;
            sub_section = None;
            let name = &trimmed[1..trimmed.len() - 1];
            match prefixed.captures(name) {
                Some(captures) => {
                    let quotes_match = captures.get(2).map(|m| m.as_str()) == captures.get(4).map(|m| m.as_str());
                    let prefix = &captures[1];
                    if quotes_match && matches!(prefix, "profile" | "sso-session" | "services") {
                        current = Some(format!("{prefix}.{}", &captures[3]));
                    }
                }
                None => current = Some(name.to_string()),
            }
            continue;
        }
        let Some(section) = &current else { continue };
        let Some(equals) = trimmed.find('=').filter(|i| *i != 0) else { continue };
        let name = trimmed[..equals].trim().to_string();
        let value = trimmed[equals + 1..].trim().to_string();
        if value.is_empty() {
            sub_section = Some(name);
            continue;
        }
        if sub_section.is_some() && line.trim_start() == line {
            sub_section = None;
        }
        let key = match &sub_section {
            Some(parent) => format!("{parent}.{name}"),
            None => name,
        };
        sections.entry(section.clone()).or_default().insert(key, value);
    }
    sections
}

/// The shared config files: `AWS_CONFIG_FILE` (`~/.aws/config`) and
/// `AWS_SHARED_CREDENTIALS_FILE` (`~/.aws/credentials`).
#[derive(Debug, Clone, Default)]
pub struct SharedConfig {
    /// Profiles: config-file `[profile x]`/`[default]` merged with credentials-file `[x]`
    /// (credentials win).
    pub profiles: IndexMap<String, Profile>,
    /// `[sso-session x]` sections.
    pub sso_sessions: IndexMap<String, Profile>,
}

impl SharedConfig {
    pub fn load() -> SharedConfig {
        let config_path = env("AWS_CONFIG_FILE").map(|p| expand_home(&p)).or_else(|| home_dir().map(|h| h.join(".aws/config")));
        let credentials_path = env("AWS_SHARED_CREDENTIALS_FILE")
            .map(|p| expand_home(&p))
            .or_else(|| home_dir().map(|h| h.join(".aws/credentials")));
        let read = |path: Option<PathBuf>| path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        SharedConfig::from_texts(&read(config_path), &read(credentials_path))
    }

    pub fn from_texts(config: &str, credentials: &str) -> SharedConfig {
        let mut shared = SharedConfig::default();
        // `getConfigData`: `[default]` plus prefixed sections; `[profile x]` becomes `x`.
        let mut config_profiles: IndexMap<String, Profile> = IndexMap::new();
        for (name, values) in parse_ini(config) {
            if name == "default" {
                config_profiles.entry(name).or_insert(values);
            } else if let Some(profile) = name.strip_prefix("profile.") {
                config_profiles.insert(profile.to_string(), values);
            } else if let Some(session) = name.strip_prefix("sso-session.") {
                shared.sso_sessions.insert(session.to_string(), values);
            }
        }
        shared.profiles = config_profiles;
        // `mergeConfigFiles`: credentials-file values win.
        for (name, values) in parse_ini(credentials) {
            shared.profiles.entry(name).or_default().extend(values);
        }
        shared
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }
}

/// The region configured for a profile in the shared config (`region` key).
pub fn profile_region(profile: Option<&str>) -> Option<String> {
    let name = profile.map(str::to_string).or_else(|| env("AWS_PROFILE")).unwrap_or_else(|| "default".into());
    SharedConfig::load().profile(&name).and_then(|p| p.get("region").cloned()).filter(|r| !r.is_empty())
}

/// Credentials from `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`/`AWS_SESSION_TOKEN`.
pub fn from_env() -> Option<AwsCredentials> {
    let access = env("AWS_ACCESS_KEY_ID")?;
    let secret = env("AWS_SECRET_ACCESS_KEY")?;
    let mut credentials = AwsCredentials::new(access, secret, env("AWS_SESSION_TOKEN"));
    credentials.expiration_ms = env("AWS_CREDENTIAL_EXPIRATION").and_then(|e| parse_iso8601(&e));
    Some(credentials)
}

/// Parse an ISO 8601 UTC timestamp to epoch milliseconds.
pub fn parse_iso8601(text: &str) -> Option<i64> {
    let text = text.trim().trim_end_matches('Z').trim_end_matches("+00:00");
    let (date, time) = text.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.split(':').map(|p| p.parse::<i64>().ok());
    let (h, mi, s) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
    let millis = format!("{frac:0<3}").get(..3)?.parse::<i64>().ok()?;
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + s) * 1000 + millis)
}

fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let start = xml.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(xml[start..end].trim().replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'"))
}

fn sts_credentials(xml: &str) -> anyhow::Result<AwsCredentials> {
    let (Some(access), Some(secret)) = (xml_tag(xml, "AccessKeyId"), xml_tag(xml, "SecretAccessKey")) else {
        anyhow::bail!("Invalid response from STS: missing credentials");
    };
    let mut credentials = AwsCredentials::new(access, secret, xml_tag(xml, "SessionToken"));
    credentials.expiration_ms = xml_tag(xml, "Expiration").and_then(|e| parse_iso8601(&e));
    Ok(credentials)
}

/// The STS (AWS Query) service error: the SDK error's message is the `Message` element.
fn sts_error(_status: u16, xml: &str) -> anyhow::Error {
    anyhow::anyhow!(xml_tag(xml, "Message").unwrap_or_else(|| "Unknown".to_string()))
}

/// A configured endpoint for a service (`AWS_ENDPOINT_URL_<SERVICE>`, then `AWS_ENDPOINT_URL`),
/// unless `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS=true`.
pub fn configured_endpoint(service_env_suffix: &str) -> Option<String> {
    if env("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        return None;
    }
    env(&format!("AWS_ENDPOINT_URL_{service_env_suffix}")).or_else(|| env("AWS_ENDPOINT_URL"))
}

fn sts_endpoint(region: &str) -> String {
    if let Some(endpoint) = configured_endpoint("STS") {
        return format!("{}/", endpoint.trim_end_matches('/'));
    }
    if region.starts_with("cn-") {
        format!("https://sts.{region}.amazonaws.com.cn/")
    } else {
        format!("https://sts.{region}.amazonaws.com/")
    }
}

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs.iter()).finish()
}

/// Inputs shared by every credential source.
#[derive(Clone)]
pub struct CredentialContext {
    pub client: reqwest::Client,
    /// The region of the calling client, used for STS when the profile names none.
    pub region: Option<String>,
}

async fn assume_role(
    source: &AwsCredentials,
    profile: &Profile,
    role_arn: &str,
    context: &CredentialContext,
) -> anyhow::Result<AwsCredentials> {
    let region = profile.get("region").cloned().or_else(|| context.region.clone()).unwrap_or_else(|| "us-east-1".into());
    let session_name = profile.get("role_session_name").cloned().unwrap_or_else(|| format!("aws-sdk-js-{}", crate::utils::now_ms()));
    let mut pairs: Vec<(&str, &str)> =
        vec![("Action", "AssumeRole"), ("Version", "2011-06-15"), ("RoleArn", role_arn), ("RoleSessionName", &session_name)];
    if let Some(external) = profile.get("external_id") {
        pairs.push(("ExternalId", external));
    }
    if let Some(duration) = profile.get("duration_seconds") {
        pairs.push(("DurationSeconds", duration));
    }
    let body = form(&pairs);
    let url = url::Url::parse(&sts_endpoint(&region))?;
    let host = url.host_str().unwrap_or_default().to_string();
    let headers =
        vec![("host".to_string(), host), ("content-type".to_string(), "application/x-www-form-urlencoded".to_string())];
    let signed = sign_request(
        &SignableRequest { method: "POST", url: &url, headers: &headers, body: body.as_bytes() },
        source,
        &region,
        "sts",
        crate::utils::now_ms(),
    );
    let mut request = context.client.post(url.clone()).body(body);
    for (name, value) in headers.iter().chain(signed.iter()) {
        if name != "host" {
            request = request.header(name.as_str(), value.as_str());
        }
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(sts_error(status, &text));
    }
    sts_credentials(&text)
}

async fn assume_role_with_web_identity(
    role_arn: &str,
    token_file: &str,
    session_name: Option<&str>,
    region: Option<&str>,
    context: &CredentialContext,
) -> anyhow::Result<AwsCredentials> {
    // `readFileSync(file, { encoding: "ascii" })`: a plain (chain-stopping) Node error, untrimmed.
    let token = std::fs::read(token_file)
        .map_err(|e| anyhow::anyhow!(node_fs_error(&e, "open", std::path::Path::new(token_file))))?;
    let token: String = token.iter().map(|b| (b & 0x7f) as char).collect();
    let session = session_name.map(str::to_string).unwrap_or_else(|| format!("aws-sdk-js-session-{}", crate::utils::now_ms()));
    let region = region.map(str::to_string).or_else(|| context.region.clone()).unwrap_or_else(|| "us-east-1".into());
    let body = form(&[
        ("Action", "AssumeRoleWithWebIdentity"),
        ("Version", "2011-06-15"),
        ("RoleArn", role_arn),
        ("RoleSessionName", &session),
        ("WebIdentityToken", token.as_str()),
    ]);
    let response = context
        .client
        .post(sts_endpoint(&region))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(sts_error(status, &text));
    }
    sts_credentials(&text)
}

/// Run a `credential_process` command and parse its JSON output (`resolveProcessCredentials`).
/// Callers turn every failure into a chain-continuing `CredentialsProviderError`.
pub async fn from_process(command: &str, profile_name: &str, profile: &Profile) -> anyhow::Result<AwsCredentials> {
    let _ = profile;
    let output = if cfg!(windows) {
        tokio::process::Command::new("cmd").arg("/d").arg("/s").arg("/c").arg(command).output().await
    } else {
        tokio::process::Command::new("/bin/sh").arg("-c").arg(command).output().await
    }
    .map_err(|e| anyhow::anyhow!("spawn /bin/sh ENOENT ({e})"))?;
    if !output.status.success() {
        // Node `exec`: `Command failed: <cmd>\n<stderr>`.
        anyhow::bail!("Command failed: {command}\n{}", String::from_utf8_lossy(&output.stderr));
    }
    let data: Value = serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
        .map_err(|_| anyhow::anyhow!("Profile {profile_name} credential_process returned invalid JSON."))?;
    if data.get("Version").and_then(Value::as_f64) != Some(1.0) {
        anyhow::bail!("Profile {profile_name} credential_process did not return Version 1.");
    }
    let (Some(access), Some(secret)) = (data.get("AccessKeyId"), data.get("SecretAccessKey")) else {
        anyhow::bail!("Profile {profile_name} credential_process returned invalid credentials.");
    };
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let session_token = data.get("SessionToken").filter(|v| !v.is_null() && v.as_str() != Some("")).map(text);
    let mut credentials = AwsCredentials::new(text(access), text(secret), session_token);
    if let Some(expiration) = data.get("Expiration").and_then(Value::as_str).filter(|e| !e.is_empty()) {
        let expires = parse_iso8601(expiration);
        if expires.is_some_and(|e| e < crate::utils::now_ms()) {
            anyhow::bail!("Profile {profile_name} credential_process returned expired credentials.");
        }
        credentials.expiration_ms = expires;
    }
    Ok(credentials)
}

fn sso_cache_path(key: &str) -> Option<PathBuf> {
    let digest = sha1::Sha1::digest(key.as_bytes());
    let name: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    home_dir().map(|h| h.join(".aws").join("sso").join("cache").join(format!("{name}.json")))
}

const SSO_REFRESH_MESSAGE: &str = "To refresh this SSO session run 'aws sso login' with the corresponding profile.";
const SSO_LEGACY_REFRESH_MESSAGE: &str = "To refresh this SSO session run aws sso login with the corresponding profile.";
/// Tokens are refreshed this long before they expire (`@aws-sdk/token-providers`).
const SSO_EXPIRE_WINDOW_MS: i64 = 5 * 60 * 1000;

static SSO_LAST_REFRESH_ATTEMPT: Lazy<Mutex<HashMap<String, i64>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn iso_timestamp(epoch_ms: i64) -> String {
    let stamp = super::aws_sigv4::amz_date(epoch_ms);
    format!(
        "{}-{}-{}T{}:{}:{}.{:03}Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
        &stamp[13..15],
        epoch_ms.rem_euclid(1000)
    )
}

/// `getSSOTokenFromFile`: the cached token JSON for an sso-session name or legacy start URL.
fn read_sso_token(key: &str) -> anyhow::Result<(PathBuf, Value)> {
    let path = sso_cache_path(key).ok_or_else(|| anyhow::anyhow!("Unable to locate the SSO token cache"))?;
    let text = std::fs::read_to_string(&path)?;
    Ok((path, serde_json::from_str(&text)?))
}

/// `@aws-sdk/token-providers` `fromSso` for an sso-session profile: the cached token, refreshed
/// through SSO OIDC within five minutes of expiry. Returns the token and its expiry.
async fn sso_session_token(
    profile_name: &str,
    session_name: &str,
    sso_region: &str,
    context: &CredentialContext,
) -> Result<(String, Option<i64>), String> {
    let token_error = |message: String| message;
    let (path, token) = read_sso_token(session_name).map_err(|_| {
        token_error(format!(
            "The SSO session token associated with profile={profile_name} was not found or is invalid. {SSO_REFRESH_MESSAGE}"
        ))
    })?;
    let value_missing = |key: &str, for_refresh: bool| {
        format!(
            "Value not present for '{key}' in SSO Token{}. {SSO_REFRESH_MESSAGE}",
            if for_refresh { ". Cannot refresh" } else { "" }
        )
    };
    let Some(access_token) = token.get("accessToken").and_then(Value::as_str).map(str::to_string) else {
        return Err(value_missing("accessToken", false));
    };
    let Some(expires_at) = token.get("expiresAt").and_then(Value::as_str).map(str::to_string) else {
        return Err(value_missing("expiresAt", false));
    };
    let expiration = parse_iso8601(&expires_at);
    let now = crate::utils::now_ms();
    if expiration.is_some_and(|e| e - now > SSO_EXPIRE_WINDOW_MS) {
        return Ok((access_token, expiration));
    }
    let validate_expiry = || {
        if expiration.is_some_and(|e| e < crate::utils::now_ms()) {
            Err(format!("Token is expired. {SSO_REFRESH_MESSAGE}"))
        } else {
            Ok((access_token.clone(), expiration))
        }
    };
    let last_attempt = SSO_LAST_REFRESH_ATTEMPT.lock().get(session_name).copied().unwrap_or(0);
    if now - last_attempt < 30_000 {
        return validate_expiry();
    }
    let field = |key: &str| token.get(key).and_then(Value::as_str).map(str::to_string);
    let Some(client_id) = field("clientId") else { return Err(value_missing("clientId", true)) };
    let Some(client_secret) = field("clientSecret") else { return Err(value_missing("clientSecret", true)) };
    let Some(refresh_token) = field("refreshToken") else { return Err(value_missing("refreshToken", true)) };
    SSO_LAST_REFRESH_ATTEMPT.lock().insert(session_name.to_string(), now);
    let endpoint = configured_endpoint("SSO_OIDC").unwrap_or_else(|| format!("https://oidc.{sso_region}.amazonaws.com"));
    let refreshed = async {
        let response = context
            .client
            .post(format!("{}/token", endpoint.trim_end_matches('/')))
            .json(&serde_json::json!({
                "clientId": client_id,
                "clientSecret": client_secret,
                "grantType": "refresh_token",
                "refreshToken": refresh_token,
            }))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let data: Value = response.json().await.ok()?;
        let access = data.get("accessToken").and_then(Value::as_str)?.to_string();
        let expires_in = data.get("expiresIn").and_then(Value::as_f64)?;
        Some((access, expires_in, data.get("refreshToken").cloned()))
    }
    .await;
    let Some((access, expires_in, new_refresh)) = refreshed else {
        return validate_expiry();
    };
    let expiration = crate::utils::now_ms() + (expires_in * 1000.0) as i64;
    let mut updated = token.clone();
    updated["accessToken"] = Value::String(access.clone());
    updated["expiresAt"] = Value::String(iso_timestamp(expiration));
    match new_refresh {
        Some(value) => updated["refreshToken"] = value,
        None => {
            if let Some(map) = updated.as_object_mut() {
                map.remove("refreshToken");
            }
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&updated) {
        let _ = std::fs::write(&path, text);
    }
    Ok((access, Some(expiration)))
}

/// The AWS SDK's `${name}: ${message}` rendering of a REST-JSON service error.
async fn aws_json_error(response: reqwest::Response) -> String {
    let error_type = response
        .headers()
        .get("x-amzn-errortype")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(':').next().unwrap_or(v).to_string());
    let data: Value = response.json().await.unwrap_or(Value::Null);
    let name = error_type
        .or_else(|| data.get("__type").or_else(|| data.get("code")).and_then(Value::as_str).map(|t| t.rsplit('#').next().unwrap_or(t).to_string()))
        .unwrap_or_else(|| "UnknownError".to_string());
    let message = data
        .get("message")
        .or_else(|| data.get("Message"))
        .and_then(Value::as_str)
        .unwrap_or("UnknownError")
        .to_string();
    format!("{name}: {message}")
}

/// `fromSSO` for a profile (`@aws-sdk/credential-provider-sso`).
async fn from_sso(
    profile_name: &str,
    profile: &Profile,
    shared: &SharedConfig,
    context: &CredentialContext,
) -> anyhow::Result<AwsCredentials> {
    let mut profile = profile.clone();
    let session_name = profile.get("sso_session").cloned();
    if let Some(session_name) = &session_name {
        let Some(session) = shared.sso_sessions.get(session_name) else {
            // `profile.sso_region = session.sso_region` on an undefined session.
            anyhow::bail!("Cannot read properties of undefined (reading 'sso_region')");
        };
        // The session's values replace the profile's (undefined ones too).
        for key in ["sso_region", "sso_start_url"] {
            match session.get(key) {
                Some(value) => {
                    profile.insert(key.to_string(), value.clone());
                }
                None => {
                    profile.shift_remove(key);
                    profile.insert(key.to_string(), String::new());
                }
            }
        }
    }
    let required = |key: &str| profile.get(key).filter(|v| !v.is_empty()).cloned();
    let (Some(start_url), Some(account), Some(sso_region), Some(role)) =
        (required("sso_start_url"), required("sso_account_id"), required("sso_region"), required("sso_role_name"))
    else {
        let keys: Vec<&str> = profile.keys().map(String::as_str).collect();
        return Err(terminal(format!(
            "Profile is configured with invalid SSO credentials. Required parameters \"sso_account_id\", \"sso_region\", \"sso_role_name\", \"sso_start_url\". Got {}\nReference: https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sso.html",
            keys.join(", ")
        )));
    };
    let (token, expires_at) = match &session_name {
        Some(session_name) => sso_session_token(profile_name, session_name, &sso_region, context).await.map_err(terminal)?,
        None => {
            let (_, token) = read_sso_token(&start_url).map_err(|_| {
                terminal(format!("The SSO session associated with this profile is invalid. {SSO_LEGACY_REFRESH_MESSAGE}"))
            })?;
            let access = token.get("accessToken").and_then(Value::as_str).unwrap_or_default().to_string();
            let expires = token.get("expiresAt").and_then(Value::as_str).and_then(parse_iso8601);
            (access, expires)
        }
    };
    if expires_at.is_none_or(|e| e - crate::utils::now_ms() <= 0) {
        return Err(terminal(format!(
            "The SSO session associated with this profile has expired. {SSO_LEGACY_REFRESH_MESSAGE}"
        )));
    }
    let url = format!(
        "{}/federation/credentials?account_id={}&role_name={}",
        configured_endpoint("SSO").unwrap_or_else(|| format!("https://portal.sso.{sso_region}.amazonaws.com")).trim_end_matches('/'),
        url::form_urlencoded::byte_serialize(account.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(role.as_bytes()).collect::<String>()
    );
    let response = context
        .client
        .get(url)
        .header("x-amz-sso_bearer_token", token)
        .send()
        .await
        .map_err(|e| terminal(e.to_string()))?;
    if !response.status().is_success() {
        return Err(terminal(aws_json_error(response).await));
    }
    let data: Value = response.json().await.unwrap_or(Value::Null);
    let creds = &data["roleCredentials"];
    let field = |key: &str| creds.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    let expiration = creds.get("expiration").and_then(Value::as_i64).filter(|e| *e != 0);
    let (Some(access), Some(secret), Some(session_token), Some(expiration)) =
        (field("accessKeyId"), field("secretAccessKey"), field("sessionToken"), expiration)
    else {
        return Err(terminal("SSO returns an invalid temporary credential."));
    };
    let mut credentials = AwsCredentials::new(access, secret, Some(session_token));
    credentials.expiration_ms = Some(expiration);
    Ok(credentials)
}

/// The `aws login` token cache directory (`AWS_LOGIN_CACHE_DIRECTORY`).
fn login_token_path(login_session: &str) -> Option<PathBuf> {
    use sha2::Digest as _;
    let directory = env("AWS_LOGIN_CACHE_DIRECTORY")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".aws").join("login").join("cache")))?;
    let digest = sha2::Sha256::digest(login_session.as_bytes());
    Some(directory.join(format!("{}.json", super::aws_sigv4::hex(&digest))))
}

fn load_login_token(path: &std::path::Path) -> anyhow::Result<Value> {
    let fail = |error: String| terminal(format!("Failed to load token from {}: {error}", path.display()));
    let text = std::fs::read_to_string(path).map_err(|e| fail(format!("Error: {}", node_fs_error(&e, "open", path))))?;
    let token: Value = serde_json::from_str(&text).map_err(|e| fail(format!("SyntaxError: {e}")))?;
    let truthy = |v: Option<&Value>| v.is_some_and(|v| !v.is_null() && v != &Value::Bool(false) && v != &Value::String(String::new()));
    let mut missing: Vec<&str> =
        ["accessToken", "clientId", "refreshToken", "dpopKey"].into_iter().filter(|k| !truthy(token.get(*k))).collect();
    if !truthy(token.get("accessToken").and_then(|a| a.get("accountId"))) {
        missing.push("accountId");
    }
    if !missing.is_empty() {
        return Err(fail(format!(
            "CredentialsProviderError: Token validation failed, missing fields: {}",
            missing.join(", ")
        )));
    }
    Ok(token)
}

fn login_credentials(access_token: &Value) -> AwsCredentials {
    let field = |k: &str| access_token.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let mut credentials = AwsCredentials::new(field("accessKeyId"), field("secretAccessKey"), Some(field("sessionToken")));
    credentials.expiration_ms = access_token.get("expiresAt").and_then(Value::as_str).and_then(parse_iso8601);
    credentials
}

/// The `signin` endpoint of a region's partition (`CreateOAuth2Token` endpoint rules).
fn signin_endpoint(region: &str) -> String {
    if let Some(endpoint) = configured_endpoint("SIGNIN") {
        return endpoint.trim_end_matches('/').to_string();
    }
    let suffix = if region.starts_with("cn-") {
        "signin.amazonaws.cn"
    } else if region.starts_with("us-gov-") {
        "signin.amazonaws-us-gov.com"
    } else if region.starts_with("us-isob-") {
        "signin.sc2shome.sgov.gov"
    } else if region.starts_with("us-isof-") {
        "signin.csphome.hci.ic.gov"
    } else if region.starts_with("us-iso-") {
        "signin.c2shome.ic.gov"
    } else if region.starts_with("eu-isoe-") {
        "signin.csphome.adc-e.uk"
    } else if region.starts_with("eusc-") {
        "signin.amazonaws-eusc.eu"
    } else {
        "signin.aws.amazon.com"
    };
    format!("https://{region}.{suffix}")
}

/// A DPoP proof (ES256 JWT) for the refresh request.
fn dpop_proof(dpop_key_pem: &str, method: &str, endpoint: &str) -> anyhow::Result<String> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
    let fail = |message: String| terminal(format!("Failed to generate Dpop proof: {message}"));
    let body: String = dpop_key_pem.lines().filter(|l| !l.starts_with("-----")).collect::<Vec<_>>().join("");
    let sec1 = base64::engine::general_purpose::STANDARD.decode(body.trim()).map_err(|e| fail(e.to_string()))?;
    let pkcs8 = sec1_to_pkcs8(&sec1);
    let rng = ring::rand::SystemRandom::new();
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &pkcs8, &rng).map_err(|e| fail(e.to_string()))?;
    let point = key.public_key().as_ref();
    if point.len() != 65 {
        return Err(fail("unexpected public key encoding".into()));
    }
    let header = serde_json::json!({
        "alg": "ES256",
        "typ": "dpop+jwt",
        "jwk": { "kty": "EC", "crv": "P-256", "x": URL_SAFE_NO_PAD.encode(&point[1..33]), "y": URL_SAFE_NO_PAD.encode(&point[33..65]) },
    });
    let payload = serde_json::json!({
        "jti": uuid::Uuid::new_v4().to_string(),
        "htm": method,
        "htu": endpoint,
        "iat": crate::utils::now_ms() / 1000,
    });
    let message = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    );
    let signature = key.sign(&rng, message.as_bytes()).map_err(|e| fail(e.to_string()))?;
    Ok(format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref())))
}

/// Wrap a SEC1 `ECPrivateKey` (P-256) in a PKCS#8 `PrivateKeyInfo`.
fn sec1_to_pkcs8(sec1: &[u8]) -> Vec<u8> {
    fn der_len(len: usize) -> Vec<u8> {
        if len < 0x80 {
            vec![len as u8]
        } else if len < 0x100 {
            vec![0x81, len as u8]
        } else {
            vec![0x82, (len >> 8) as u8, len as u8]
        }
    }
    // AlgorithmIdentifier { id-ecPublicKey, prime256v1 }.
    let algorithm: [u8; 21] = [
        0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03,
        0x01, 0x07,
    ];
    let mut content = vec![0x02, 0x01, 0x00];
    content.extend_from_slice(&algorithm);
    content.push(0x04);
    content.extend(der_len(sec1.len()));
    content.extend_from_slice(sec1);
    let mut out = vec![0x30];
    out.extend(der_len(content.len()));
    out.extend(content);
    out
}

/// `fromLoginCredentials` (`aws login` sessions): cached credentials, refreshed with a DPoP-bound
/// `CreateOAuth2Token` call within five minutes of expiry.
async fn from_login(profile: &Profile, context: &CredentialContext) -> anyhow::Result<AwsCredentials> {
    const REFRESH_THRESHOLD_MS: i64 = 5 * 60 * 1000;
    let session = profile.get("login_session").cloned().unwrap_or_default();
    let Some(path) = login_token_path(&session) else {
        return Err(terminal(format!(
            "Failed to load a token for session {session}, please re-authenticate using aws login"
        )));
    };
    let token = load_login_token(&path)?;
    let expiry = |token: &Value| token["accessToken"].get("expiresAt").and_then(Value::as_str).and_then(parse_iso8601).unwrap_or(0);
    if expiry(&token) - crate::utils::now_ms() > REFRESH_THRESHOLD_MS {
        return Ok(login_credentials(&token["accessToken"]));
    }
    // Another process may have refreshed the cache meanwhile.
    let disk = load_login_token(&path).unwrap_or_else(|_| token.clone());
    let now = crate::utils::now_ms();
    let fresh = if expiry(&disk) <= now && expiry(&token) > now { token } else { disk };
    if expiry(&fresh) - crate::utils::now_ms() > REFRESH_THRESHOLD_MS {
        return Ok(login_credentials(&fresh["accessToken"]));
    }
    let region = profile.get("region").cloned().or_else(|| context.region.clone()).or_else(|| env("AWS_REGION")).unwrap_or_default();
    let endpoint = format!("{}/v1/token", signin_endpoint(&region));
    let dpop = dpop_proof(fresh["dpopKey"].as_str().unwrap_or_default(), "POST", &endpoint)?;
    let body = serde_json::json!({
        "clientId": fresh["clientId"],
        "grantType": "refresh_token",
        "refreshToken": fresh["refreshToken"],
    });
    let response = context
        .client
        .post(&endpoint)
        .header("content-type", "application/json")
        .header("DPoP", dpop)
        .body(body.to_string())
        .send()
        .await;
    let fresh_unexpired = |error: String| -> anyhow::Result<AwsCredentials> {
        if expiry(&fresh) > crate::utils::now_ms() {
            return Ok(login_credentials(&fresh["accessToken"]));
        }
        Err(provider_error(format!("Failed to refresh token: {error}. Please re-authenticate using aws login")))
    };
    let response = match response {
        Ok(response) => response,
        Err(error) => return fresh_unexpired(format!("Error: {error}")),
    };
    if !response.status().is_success() {
        let header_type = response.headers().get("x-amzn-errortype").and_then(|v| v.to_str().ok()).map(str::to_string);
        let data: Value = response.json().await.unwrap_or(Value::Null);
        let name = header_type
            .map(|t| t.split(':').next().unwrap_or_default().to_string())
            .or_else(|| data.get("__type").and_then(Value::as_str).map(|t| t.rsplit('#').next().unwrap_or(t).to_string()))
            .unwrap_or_else(|| "UnknownError".into());
        let message = data.get("message").and_then(Value::as_str).unwrap_or("UnknownError").to_string();
        if name == "AccessDeniedException" {
            let text = match data.get("error").and_then(Value::as_str) {
                Some("TOKEN_EXPIRED") => "Your session has expired. Please reauthenticate.".to_string(),
                Some("USER_CREDENTIALS_CHANGED") => "Unable to refresh credentials because of a change in your password. Please reauthenticate with your new password.".to_string(),
                Some("INSUFFICIENT_PERMISSIONS") => "Unable to refresh credentials due to insufficient permissions. You may be missing permission for the 'CreateOAuth2Token' action.".to_string(),
                _ => format!("Failed to refresh token: {name}: {message}. Please re-authenticate using `aws login`"),
            };
            return Err(terminal(text));
        }
        return fresh_unexpired(format!("{name}: {message}"));
    }
    let data: Value = response.json().await.unwrap_or(Value::Null);
    let access = &data["accessToken"];
    let field = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let (Some(access_key_id), Some(secret_access_key), Some(session_token), Some(refresh_token)) = (
        field(access, "accessKeyId"),
        field(access, "secretAccessKey"),
        field(access, "sessionToken"),
        field(&data, "refreshToken"),
    ) else {
        return Err(terminal("Token refresh response missing required fields"));
    };
    let expires_in = data.get("expiresIn").and_then(Value::as_f64).unwrap_or(900.0);
    let mut updated = fresh.clone();
    updated["accessToken"]["accessKeyId"] = Value::String(access_key_id);
    updated["accessToken"]["secretAccessKey"] = Value::String(secret_access_key);
    updated["accessToken"]["sessionToken"] = Value::String(session_token);
    updated["accessToken"]["expiresAt"] = Value::String(iso_timestamp(crate::utils::now_ms() + (expires_in * 1000.0) as i64));
    updated["refreshToken"] = Value::String(refresh_token);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, serde_json::to_string_pretty(&updated)?)?;
    Ok(login_credentials(&updated["accessToken"]))
}

/// `String(err)` of a Node fs error.
fn node_fs_error(error: &std::io::Error, syscall: &str, path: &std::path::Path) -> String {
    let (code, text) = match error.kind() {
        std::io::ErrorKind::NotFound => ("ENOENT", "no such file or directory"),
        std::io::ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
        std::io::ErrorKind::IsADirectory => ("EISDIR", "illegal operation on a directory"),
        _ => return error.to_string(),
    };
    format!("{code}: {text}, {syscall} '{}'", path.display())
}

fn is_assume_role_profile(profile: &Profile) -> bool {
    profile.contains_key("role_arn") && (profile.contains_key("source_profile") != profile.contains_key("credential_source"))
}

fn is_credential_source_without_role_arn(profile: Option<&Profile>) -> bool {
    profile.is_some_and(|p| p.get("role_arn").is_none_or(String::is_empty) && p.get("credential_source").is_some_and(|s| !s.is_empty()))
}

fn is_static_profile(profile: &Profile) -> bool {
    profile.contains_key("aws_access_key_id") && profile.contains_key("aws_secret_access_key")
}

fn is_sso_profile(profile: &Profile) -> bool {
    ["sso_start_url", "sso_account_id", "sso_session", "sso_region", "sso_role_name"].iter().any(|k| profile.contains_key(*k))
}

fn static_credentials(profile: &Profile) -> AwsCredentials {
    AwsCredentials::new(
        profile.get("aws_access_key_id").cloned().unwrap_or_default(),
        profile.get("aws_secret_access_key").cloned().unwrap_or_default(),
        profile.get("aws_session_token").cloned(),
    )
}

/// `fromEnv` as a chain link.
fn env_provider() -> anyhow::Result<AwsCredentials> {
    from_env().ok_or_else(|| provider_error("Unable to find environment variable credentials."))
}

/// The ECS container chain (`fromHttp`, `fromContainerMetadata`) or EC2 instance metadata.
async fn remote_provider(client: &reqwest::Client) -> anyhow::Result<AwsCredentials> {
    let result = if env("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_some() || env("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_some() {
        from_container(client).await
    } else {
        from_imds(client).await
    };
    // Remote failures are `CredentialsProviderError`s: the chain moves on.
    result.map_err(|error| match error.downcast::<CredentialsProviderError>() {
        Ok(error) => anyhow::Error::new(error),
        Err(other) => provider_error(other.to_string()),
    })
}

/// Credentials of a `credential_source` (`resolveCredentialSource`).
fn credential_source_provider<'a>(
    source: &'a str,
    profile_name: &'a str,
    context: &'a CredentialContext,
) -> anyhow::Result<futures::future::BoxFuture<'a, anyhow::Result<AwsCredentials>>> {
    Ok(match source {
        "EcsContainer" => Box::pin(async move {
            from_container(&context.client).await.map_err(|e| provider_error(e.to_string()))
        }),
        "Ec2InstanceMetadata" => {
            Box::pin(async move { from_imds(&context.client).await.map_err(|e| provider_error(e.to_string())) })
        }
        "Environment" => Box::pin(async move { env_provider() }),
        other => {
            return Err(provider_error(format!(
                "Unsupported credential source in profile {profile_name}. Got {other}, expected EcsContainer or Ec2InstanceMetadata or Environment."
            )));
        }
    })
}

/// `resolveProfileData` (`@aws-sdk/credential-provider-ini`). `visited` lists the source
/// profiles followed so far, in order; `top` is the profile the chain started from.
fn resolve_profile<'a>(
    name: &'a str,
    shared: &'a SharedConfig,
    context: &'a CredentialContext,
    visited: Vec<String>,
    assume_role_recursive: bool,
    top: &'a str,
) -> futures::future::BoxFuture<'a, anyhow::Result<AwsCredentials>> {
    Box::pin(async move {
        let data = shared.profile(name);
        if let Some(profile) = data
            && !visited.is_empty()
            && is_static_profile(profile)
        {
            return Ok(static_credentials(profile));
        }
        if let Some(profile) = data
            && (assume_role_recursive || is_assume_role_profile(profile))
        {
            return resolve_assume_role(name, profile, shared, context, visited, top).await;
        }
        let Some(profile) = data else {
            return Err(provider_error(format!(
                "Could not resolve credentials using profile: [{name}] in configuration/credentials file(s)."
            )));
        };
        if is_static_profile(profile) {
            return Ok(static_credentials(profile));
        }
        if let (Some(token_file), Some(role_arn)) = (profile.get("web_identity_token_file"), profile.get("role_arn")) {
            return assume_role_with_web_identity(
                role_arn,
                token_file,
                profile.get("role_session_name").map(String::as_str),
                None,
                context,
            )
            .await;
        }
        if let Some(command) = profile.get("credential_process") {
            return from_process(command, name, profile).await.map_err(|e| provider_error(e.to_string()));
        }
        if is_sso_profile(profile) {
            return from_sso(name, profile, shared, context).await;
        }
        if profile.get("login_session").is_some_and(|s| !s.is_empty()) {
            return from_login(profile, context).await;
        }
        Err(provider_error(format!("Could not resolve credentials using profile: [{name}] in configuration/credentials file(s).")))
    })
}

/// `resolveAssumeRoleCredentials`.
async fn resolve_assume_role(
    name: &str,
    profile: &Profile,
    shared: &SharedConfig,
    context: &CredentialContext,
    visited: Vec<String>,
    top: &str,
) -> anyhow::Result<AwsCredentials> {
    let source_profile = profile.get("source_profile").cloned();
    if let Some(source) = &source_profile
        && visited.contains(source)
    {
        return Err(provider_error(format!(
            "Detected a cycle attempting to resolve credentials for profile {top}. Profiles visited: {}",
            visited.join(", ")
        )));
    }
    let source_credentials: futures::future::BoxFuture<'_, anyhow::Result<AwsCredentials>> = match &source_profile {
        Some(source) => {
            let mut next = visited.clone();
            next.push(source.clone());
            let recursive = is_credential_source_without_role_arn(shared.profile(source));
            resolve_profile(source, shared, context, next, recursive, top)
        }
        None => credential_source_provider(profile.get("credential_source").map(String::as_str).unwrap_or_default(), name, context)?,
    };
    if is_credential_source_without_role_arn(Some(profile)) {
        return source_credentials.await;
    }
    if profile.get("mfa_serial").is_some_and(|s| !s.is_empty()) {
        // The SDK prompts through `mfaCodeProvider`; the default chain has none, so a
        // non-interactive client fails here and stops the chain.
        return Err(terminal(format!(
            "Profile {name} requires multi-factor authentication, but no MFA code callback was provided."
        )));
    }
    let source = source_credentials.await?;
    assume_role(&source, profile, profile.get("role_arn").map(String::as_str).unwrap_or_default(), context).await
}

/// ECS/EKS container credentials (`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI`/`_FULL_URI`).
pub async fn from_container(client: &reqwest::Client) -> anyhow::Result<AwsCredentials> {
    let url = if let Some(relative) = env("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI") {
        format!("http://169.254.170.2{relative}")
    } else if let Some(full) = env("AWS_CONTAINER_CREDENTIALS_FULL_URI") {
        full
    } else {
        anyhow::bail!("The container metadata credential provider cannot be used unless the AWS_CONTAINER_CREDENTIALS_RELATIVE_URI or AWS_CONTAINER_CREDENTIALS_FULL_URI environment variable is set");
    };
    let mut request = client.get(&url).timeout(Duration::from_millis(1000));
    let token = match env("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE") {
        Some(file) => Some(std::fs::read_to_string(file)?.trim().to_string()),
        None => env("AWS_CONTAINER_AUTHORIZATION_TOKEN"),
    };
    if let Some(token) = token {
        request = request.header("Authorization", token);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Container credentials request failed with status {}", response.status().as_u16());
    }
    let data: Value = response.json().await?;
    parse_metadata_credentials(&data)
}

fn parse_metadata_credentials(data: &Value) -> anyhow::Result<AwsCredentials> {
    let (Some(access), Some(secret)) = (data.get("AccessKeyId").and_then(Value::as_str), data.get("SecretAccessKey").and_then(Value::as_str))
    else {
        anyhow::bail!("Invalid response received from the credentials endpoint");
    };
    let mut credentials = AwsCredentials::new(access, secret, data.get("Token").and_then(Value::as_str).map(str::to_string));
    credentials.expiration_ms = data.get("Expiration").and_then(Value::as_str).and_then(parse_iso8601);
    Ok(credentials)
}

/// EC2 instance metadata credentials (IMDSv2 with an IMDSv1 fallback).
pub async fn from_imds(client: &reqwest::Client) -> anyhow::Result<AwsCredentials> {
    if env("AWS_EC2_METADATA_DISABLED").is_some_and(|v| v != "false") {
        return Err(provider_error("EC2 Instance Metadata Service access disabled"));
    }
    let host = env("AWS_EC2_METADATA_SERVICE_ENDPOINT").unwrap_or_else(|| "http://169.254.169.254".into());
    let host = host.trim_end_matches('/').to_string();
    let timeout = Duration::from_millis(1000);
    let token = client
        .put(format!("{host}/latest/api/token"))
        .header("x-aws-ec2-metadata-token-ttl-seconds", "21600")
        .timeout(timeout)
        .send()
        .await
        .ok()
        .filter(|r| r.status().is_success());
    let token = match token {
        Some(response) => Some(response.text().await?),
        None => None,
    };
    let get = |path: String| {
        let mut request = client.get(format!("{host}{path}")).timeout(timeout);
        if let Some(token) = &token {
            request = request.header("x-aws-ec2-metadata-token", token.as_str());
        }
        request.send()
    };
    let profile = get("/latest/meta-data/iam/security-credentials/".into()).await?;
    if !profile.status().is_success() {
        anyhow::bail!("Could not load credentials from the EC2 instance metadata service");
    }
    let name = profile.text().await?.lines().next().unwrap_or_default().trim().to_string();
    let response = get(format!("/latest/meta-data/iam/security-credentials/{name}")).await?;
    if !response.status().is_success() {
        anyhow::bail!("Could not load credentials from the EC2 instance metadata service");
    }
    let data: Value = response.json().await?;
    parse_metadata_credentials(&data)
}

fn cached(key: &str) -> Option<AwsCredentials> {
    CACHE.lock().get(key).filter(|c| c.expiration_ms.is_some_and(|e| e - EXPIRY_WINDOW_MS > crate::utils::now_ms())).cloned()
}

fn store(key: &str, credentials: &AwsCredentials) {
    if credentials.expiration_ms.is_some() {
        CACHE.lock().insert(key.to_string(), credentials.clone());
    }
}

/// Continue with the next chain link unless the error stops the chain.
fn next_link(error: anyhow::Error) -> anyhow::Result<()> {
    match error.downcast_ref::<CredentialsProviderError>() {
        Some(provider) if provider.try_next_link => Ok(()),
        _ => Err(error),
    }
}

/// Resolve credentials with the SDK default chain (`defaultProvider`): `fromEnv` (skipped when a
/// profile is configured), `fromIni`, `fromProcess`, `fromTokenFile`, then the container or
/// instance metadata. A link's `CredentialsProviderError` moves on to the next link unless it
/// stops the chain; any other error is returned. `profile` is the client's configured profile.
pub async fn default_provider(profile: Option<&str>, context: &CredentialContext) -> anyhow::Result<AwsCredentials> {
    let configured_profile = profile.map(str::to_string).or_else(|| env("AWS_PROFILE"));
    if configured_profile.is_none() {
        match env_provider() {
            Ok(credentials) => return Ok(credentials),
            Err(error) => next_link(error)?,
        }
    }
    let profile_name = configured_profile.clone().unwrap_or_else(|| "default".into());
    let shared = SharedConfig::load();

    let key = format!("profile:{profile_name}");
    if let Some(credentials) = cached(&key) {
        return Ok(credentials);
    }
    match resolve_profile(&profile_name, &shared, context, Vec::new(), false, &profile_name).await {
        Ok(credentials) => {
            store(&key, &credentials);
            return Ok(credentials);
        }
        Err(error) => next_link(error)?,
    }

    // `fromProcess`: the profile's `credential_process` once more.
    let process = match shared.profile(&profile_name) {
        Some(profile) => match profile.get("credential_process") {
            Some(command) => from_process(command, &profile_name, profile).await.map_err(|e| provider_error(e.to_string())),
            None => Err(provider_error(format!("Profile {profile_name} did not contain credential_process."))),
        },
        None => Err(provider_error(format!("Profile {profile_name} could not be found in shared credentials file."))),
    };
    match process {
        Ok(credentials) => return Ok(credentials),
        Err(error) => next_link(error)?,
    }

    // `fromTokenFile`.
    match (env("AWS_WEB_IDENTITY_TOKEN_FILE"), env("AWS_ROLE_ARN")) {
        (Some(token_file), Some(role_arn)) => {
            let key = format!("web-identity:{role_arn}");
            if let Some(credentials) = cached(&key) {
                return Ok(credentials);
            }
            let credentials =
                assume_role_with_web_identity(&role_arn, &token_file, env("AWS_ROLE_SESSION_NAME").as_deref(), None, context)
                    .await?;
            store(&key, &credentials);
            return Ok(credentials);
        }
        _ => next_link(provider_error("Web identity configuration not specified"))?,
    }

    let remote_key = if env("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_some() || env("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_some() {
        "container"
    } else {
        "imds"
    };
    if let Some(credentials) = cached(remote_key) {
        return Ok(credentials);
    }
    match remote_provider(&context.client).await {
        Ok(credentials) => {
            store(remote_key, &credentials);
            return Ok(credentials);
        }
        Err(error) => next_link(error)?,
    }
    Err(terminal(NO_CREDENTIALS))
}

/// Read a path relative to the home directory (tests).
#[doc(hidden)]
pub fn sso_cache_file_for(key: &str) -> Option<PathBuf> {
    sso_cache_path(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shared_config_files() {
        let config = "[default]\nregion = us-west-2\n[profile dev]\nrole_arn = arn:aws:iam::1:role/r\nsource_profile = default\ns3 =\n  max_concurrent_requests = 20\nregion=eu-west-1 # comment\n[sso-session corp]\nsso_region = us-east-1\nsso_start_url = https://corp.awsapps.com/start\n[ignored]\nkey = value\n";
        let credentials = "[default]\naws_access_key_id = AKID\naws_secret_access_key = SECRET\n; comment\n[Mixed]\nAWS_Key = v;still-value\n";
        let shared = SharedConfig::from_texts(config, credentials);
        let default = shared.profile("default").unwrap();
        assert_eq!(default.get("region").map(String::as_str), Some("us-west-2"));
        assert_eq!(static_credentials(default).access_key_id, "AKID");
        let dev = shared.profile("dev").unwrap();
        assert_eq!(dev.get("region").map(String::as_str), Some("eu-west-1"));
        assert_eq!(dev.get("s3.max_concurrent_requests").map(String::as_str), Some("20"));
        assert_eq!(dev.keys().collect::<Vec<_>>(), ["role_arn", "source_profile", "s3.max_concurrent_requests", "region"]);
        assert_eq!(shared.sso_sessions["corp"]["sso_region"], "us-east-1");
        // Config-file sections without a known prefix are ignored; keys keep their case.
        assert!(shared.profile("ignored").is_none());
        assert_eq!(shared.profile("Mixed").unwrap().get("AWS_Key").map(String::as_str), Some("v;still-value"));
    }

    #[test]
    fn parses_sts_responses() {
        let xml = "<AssumeRoleResponse><AssumeRoleResult><Credentials><AccessKeyId>ASIA</AccessKeyId><SecretAccessKey>s</SecretAccessKey><SessionToken>t&amp;x</SessionToken><Expiration>2030-01-01T00:00:00Z</Expiration></Credentials></AssumeRoleResult></AssumeRoleResponse>";
        let credentials = sts_credentials(xml).unwrap();
        assert_eq!(credentials.session_token.as_deref(), Some("t&x"));
        assert_eq!(credentials.expiration_ms, parse_iso8601("2030-01-01T00:00:00Z"));
        assert_eq!(
            sts_error(403, "<ErrorResponse><Error><Code>AccessDenied</Code><Message>nope</Message></Error></ErrorResponse>").to_string(),
            "nope"
        );
        assert_eq!(sts_error(500, "<ErrorResponse/>").to_string(), "Unknown");
    }

    #[tokio::test]
    async fn runs_credential_processes() {
        if cfg!(windows) {
            return;
        }
        let profile = Profile::new();
        let credentials = from_process(
            r#"echo '{"Version":1,"AccessKeyId":"A","SecretAccessKey":"S","SessionToken":"T","Expiration":"2999-01-01T00:00:00Z"}'"#,
            "p",
            &profile,
        )
        .await
        .unwrap();
        assert_eq!((credentials.access_key_id.as_str(), credentials.session_token.as_deref()), ("A", Some("T")));
        assert!(from_process("echo '{}'", "p", &profile).await.unwrap_err().to_string().contains("Version 1"));
        assert_eq!(
            from_process("echo oops >&2; exit 3", "p", &profile).await.unwrap_err().to_string(),
            "Command failed: echo oops >&2; exit 3\noops\n"
        );
    }

    fn context() -> CredentialContext {
        CredentialContext { client: reqwest::Client::new(), region: Some("us-east-1".into()) }
    }

    fn chain_error(error: anyhow::Error) -> (String, Option<bool>) {
        let flag = error.downcast_ref::<CredentialsProviderError>().map(|e| e.try_next_link);
        (error.to_string(), flag)
    }

    #[tokio::test]
    async fn mfa_role_profiles_stop_the_chain_without_an_mfa_callback() {
        let shared = SharedConfig::from_texts(
            "[profile mfa]\nrole_arn = arn:aws:iam::1:role/r\nsource_profile = base\nmfa_serial = arn:aws:iam::1:mfa/me\n",
            "[base]\naws_access_key_id = A\naws_secret_access_key = S\n",
        );
        let error = resolve_profile("mfa", &shared, &context(), Vec::new(), false, "mfa").await.unwrap_err();
        assert_eq!(
            chain_error(error),
            ("Profile mfa requires multi-factor authentication, but no MFA code callback was provided.".into(), Some(false))
        );
    }

    #[tokio::test]
    async fn profile_errors_follow_the_sdk_chain_semantics() {
        let shared = SharedConfig::from_texts(
            "[profile a]\nrole_arn = arn:aws:iam::1:role/a\nsource_profile = b\n\
[profile b]\nrole_arn = arn:aws:iam::1:role/b\nsource_profile = a\n\
[profile bad-source]\nrole_arn = arn:aws:iam::1:role/r\ncredential_source = Nowhere\nmfa_serial = x\n\
[profile role-only]\nrole_arn = arn:aws:iam::1:role/r\naws_access_key_id = K\naws_secret_access_key = S\n\
[profile sso-missing]\nsso_session = gone\n\
[profile sso-incomplete]\nsso_start_url = https://x\nsso_region = us-east-1\n",
            "",
        );
        let ctx = context();
        let resolve = |name: &'static str| resolve_profile(name, &shared, &ctx, Vec::new(), false, name);
        assert_eq!(
            chain_error(resolve("a").await.unwrap_err()),
            ("Detected a cycle attempting to resolve credentials for profile a. Profiles visited: b, a".into(), Some(true))
        );
        // The credential source is validated before the MFA check.
        assert_eq!(
            chain_error(resolve("bad-source").await.unwrap_err()),
            (
                "Unsupported credential source in profile bad-source. Got Nowhere, expected EcsContainer or Ec2InstanceMetadata or Environment.".into(),
                Some(true)
            )
        );
        // `role_arn` without a source is not an assume-role profile: static keys apply.
        assert_eq!(resolve("role-only").await.unwrap().access_key_id, "K");
        assert_eq!(
            chain_error(resolve("sso-missing").await.unwrap_err()),
            ("Cannot read properties of undefined (reading 'sso_region')".into(), None)
        );
        assert_eq!(
            chain_error(resolve("sso-incomplete").await.unwrap_err()),
            (
                "Profile is configured with invalid SSO credentials. Required parameters \"sso_account_id\", \"sso_region\", \"sso_role_name\", \"sso_start_url\". Got sso_start_url, sso_region\nReference: https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sso.html".into(),
                Some(false)
            )
        );
        assert_eq!(
            chain_error(resolve("missing").await.unwrap_err()),
            ("Could not resolve credentials using profile: [missing] in configuration/credentials file(s).".into(), Some(true))
        );
    }

    #[test]
    fn wraps_sec1_keys_for_dpop() {
        use base64::Engine;
        use ring::signature::{ECDSA_P256_SHA256_FIXED, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair, UnparsedPublicKey};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pkcs8 = pkcs8.as_ref();
        // PrivateKeyInfo: SEQUENCE { version, AlgorithmIdentifier (21 bytes), OCTET STRING sec1 }.
        let header_len = if pkcs8[1] & 0x80 != 0 { 2 + (pkcs8[1] & 0x7f) as usize } else { 2 };
        let mut offset = header_len + 3 + 21;
        assert_eq!(pkcs8[offset], 0x04);
        offset += 1;
        let (len, skip) = if pkcs8[offset] & 0x80 != 0 { (pkcs8[offset + 1] as usize, 2) } else { (pkcs8[offset] as usize, 1) };
        let sec1 = &pkcs8[offset + skip..offset + skip + len];
        assert_eq!(sec1_to_pkcs8(sec1), pkcs8);
        let pem = format!(
            "-----BEGIN EC PRIVATE KEY-----\n{}\n-----END EC PRIVATE KEY-----\n",
            base64::engine::general_purpose::STANDARD.encode(sec1)
        );
        let proof = dpop_proof(&pem, "POST", "https://us-east-1.signin.aws.amazon.com/v1/token").unwrap();
        let parts: Vec<&str> = proof.split('.').collect();
        let decode = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).unwrap();
        let header: Value = serde_json::from_slice(&decode(parts[0])).unwrap();
        let payload: Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        assert_eq!((header["alg"].as_str(), header["typ"].as_str(), header["jwk"]["crv"].as_str()), (Some("ES256"), Some("dpop+jwt"), Some("P-256")));
        assert_eq!(payload["htm"], "POST");
        assert_eq!(payload["htu"], "https://us-east-1.signin.aws.amazon.com/v1/token");
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8, &rng).unwrap();
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, key.public_key().as_ref())
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &decode(parts[2]))
            .unwrap();
        assert_eq!(signin_endpoint("us-west-2"), "https://us-west-2.signin.aws.amazon.com");
        assert_eq!(signin_endpoint("cn-north-1"), "https://cn-north-1.signin.amazonaws.cn");
    }
}
