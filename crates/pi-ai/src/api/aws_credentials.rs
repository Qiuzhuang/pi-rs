//! AWS credential and region resolution for Bedrock: the parts of the AWS SDK default chain
//! (`@aws-sdk/credential-provider-node`) that a CLI relies on.
//!
//! Order (`defaultProvider`): environment keys (skipped when a profile is configured), the
//! shared config/credentials profile (`fromIni`: static keys, `role_arn` with `source_profile`
//! or `credential_source` via STS `AssumeRole`, `web_identity_token_file`, `credential_process`,
//! IAM Identity Center/SSO from the `~/.aws/sso/cache` token), `credential_process`, the web
//! identity token file from `AWS_WEB_IDENTITY_TOKEN_FILE`/`AWS_ROLE_ARN`, and finally the ECS
//! container endpoint or EC2 instance metadata (IMDSv2). MFA-protected role profiles are not
//! supported (the SDK needs an MFA code callback for them as well).

use std::collections::HashMap;
use std::path::PathBuf;
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

/// Parsed sections of an INI file: section name → key → value.
pub type IniSections = HashMap<String, HashMap<String, String>>;

/// Parse an AWS shared config/credentials file. Indented lines (nested properties) are
/// skipped; comments start with `#` or `;`.
pub fn parse_ini(text: &str) -> IniSections {
    let mut sections: IniSections = HashMap::new();
    let mut current: Option<String> = None;
    let mut nested = false;
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let name = trimmed[1..trimmed.len() - 1].trim().to_string();
            sections.entry(name.clone()).or_default();
            current = Some(name);
            nested = false;
            continue;
        }
        let indented = raw.starts_with(' ') || raw.starts_with('\t');
        if indented && nested {
            continue;
        }
        let Some(section) = &current else { continue };
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            nested = value.is_empty();
            if !nested {
                let value = value.split(" #").next().unwrap_or(value).trim().to_string();
                sections.entry(section.clone()).or_default().insert(key, value);
            }
        }
    }
    sections
}

/// The shared config files: `AWS_CONFIG_FILE` (`~/.aws/config`) and
/// `AWS_SHARED_CREDENTIALS_FILE` (`~/.aws/credentials`).
#[derive(Debug, Clone, Default)]
pub struct SharedConfig {
    /// Profiles: config-file `[profile x]`/`[default]` merged with credentials-file `[x]`
    /// (credentials win).
    pub profiles: HashMap<String, HashMap<String, String>>,
    /// `[sso-session x]` sections.
    pub sso_sessions: HashMap<String, HashMap<String, String>>,
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
        for (name, values) in parse_ini(config) {
            if let Some(profile) = name.strip_prefix("profile ") {
                shared.profiles.entry(profile.trim().to_string()).or_default().extend(values);
            } else if let Some(session) = name.strip_prefix("sso-session ") {
                shared.sso_sessions.insert(session.trim().to_string(), values);
            } else if name == "default" {
                shared.profiles.entry(name).or_default().extend(values);
            }
        }
        for (name, values) in parse_ini(credentials) {
            shared.profiles.entry(name).or_default().extend(values);
        }
        shared
    }

    pub fn profile(&self, name: &str) -> Option<&HashMap<String, String>> {
        self.profiles.get(name)
    }
}

/// The region configured for a profile in the shared config (`region` key).
pub fn profile_region(profile: Option<&str>) -> Option<String> {
    let name = profile.map(str::to_string).or_else(|| env("AWS_PROFILE")).unwrap_or_else(|| "default".into());
    SharedConfig::load().profile(&name).and_then(|p| p.get("region").cloned()).filter(|r| !r.is_empty())
}

fn credentials_error(message: impl Into<String>) -> anyhow::Error {
    anyhow::anyhow!(message.into())
}

fn static_from(values: &HashMap<String, String>) -> Option<AwsCredentials> {
    let access = values.get("aws_access_key_id").filter(|v| !v.is_empty())?;
    let secret = values.get("aws_secret_access_key").filter(|v| !v.is_empty())?;
    Some(AwsCredentials::new(access.clone(), secret.clone(), values.get("aws_session_token").filter(|v| !v.is_empty()).cloned()))
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

fn sts_error(status: u16, xml: &str) -> anyhow::Error {
    match (xml_tag(xml, "Code"), xml_tag(xml, "Message")) {
        (Some(code), Some(message)) => anyhow::anyhow!("{code}: {message}"),
        (Some(code), None) => anyhow::anyhow!("{code}"),
        _ => anyhow::anyhow!("STS request failed with status {status}"),
    }
}

fn sts_endpoint(region: &str) -> String {
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
    profile: &HashMap<String, String>,
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
    let token = std::fs::read_to_string(token_file)
        .map_err(|e| credentials_error(format!("Unable to read web identity token file {token_file}: {e}")))?;
    let session = session_name.map(str::to_string).unwrap_or_else(|| format!("aws-sdk-js-session-{}", crate::utils::now_ms()));
    let region = region.map(str::to_string).or_else(|| context.region.clone()).unwrap_or_else(|| "us-east-1".into());
    let body = form(&[
        ("Action", "AssumeRoleWithWebIdentity"),
        ("Version", "2011-06-15"),
        ("RoleArn", role_arn),
        ("RoleSessionName", &session),
        ("WebIdentityToken", token.trim()),
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

/// Run a `credential_process` command and parse its JSON output.
pub async fn from_process(command: &str, profile_name: &str) -> anyhow::Result<AwsCredentials> {
    let output = if cfg!(windows) {
        tokio::process::Command::new("cmd").arg("/C").arg(command).output().await
    } else {
        tokio::process::Command::new("sh").arg("-c").arg(command).output().await
    }
    .map_err(|e| credentials_error(format!("Error running credential_process for profile {profile_name}: {e}")))?;
    if !output.status.success() {
        anyhow::bail!("Error: credential_process for profile {profile_name} failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let data: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| credentials_error(format!("Profile {profile_name} credential_process returned invalid JSON.")))?;
    if data.get("Version").and_then(Value::as_i64) != Some(1) {
        anyhow::bail!("Profile {profile_name} credential_process did not return Version 1.");
    }
    let (Some(access), Some(secret)) =
        (data.get("AccessKeyId").and_then(Value::as_str), data.get("SecretAccessKey").and_then(Value::as_str))
    else {
        anyhow::bail!("Profile {profile_name} credential_process returned invalid credentials.");
    };
    let mut credentials =
        AwsCredentials::new(access, secret, data.get("SessionToken").and_then(Value::as_str).map(str::to_string));
    if let Some(expiration) = data.get("Expiration").and_then(Value::as_str) {
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

async fn sso_token(
    session_name: Option<&str>,
    start_url: &str,
    sso_region: &str,
    context: &CredentialContext,
) -> anyhow::Result<String> {
    let key = session_name.unwrap_or(start_url);
    let path = sso_cache_path(key).ok_or_else(|| credentials_error("Unable to locate the SSO token cache"))?;
    let text = std::fs::read_to_string(&path).map_err(|_| {
        credentials_error(
            "The SSO session associated with this profile is invalid. To refresh this SSO session run aws sso login with the corresponding profile.",
        )
    })?;
    let mut token: Value = serde_json::from_str(&text)?;
    let expires = token.get("expiresAt").and_then(Value::as_str).and_then(parse_iso8601);
    let valid = expires.is_some_and(|e| e - EXPIRY_WINDOW_MS > crate::utils::now_ms());
    if valid {
        return token.get("accessToken").and_then(Value::as_str).map(str::to_string).ok_or_else(|| credentials_error("Invalid SSO token cache"));
    }
    // sso-session tokens carry a refresh token; legacy tokens must be refreshed with `aws sso login`.
    let refresh = (
        session_name,
        token.get("refreshToken").and_then(Value::as_str),
        token.get("clientId").and_then(Value::as_str),
        token.get("clientSecret").and_then(Value::as_str),
    );
    let (Some(_), Some(refresh_token), Some(client_id), Some(client_secret)) = refresh else {
        anyhow::bail!(
            "The SSO session associated with this profile has expired. To refresh this SSO session run aws sso login with the corresponding profile."
        );
    };
    let response = context
        .client
        .post(format!("https://oidc.{sso_region}.amazonaws.com/token"))
        .json(&serde_json::json!({ "clientId": client_id, "clientSecret": client_secret, "grantType": "refresh_token", "refreshToken": refresh_token }))
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!(
            "The SSO session associated with this profile has expired. To refresh this SSO session run aws sso login with the corresponding profile."
        );
    }
    let data: Value = response.json().await?;
    let Some(access) = data.get("accessToken").and_then(Value::as_str).map(str::to_string) else {
        anyhow::bail!("SSO token refresh returned no access token");
    };
    let expires_in = data.get("expiresIn").and_then(Value::as_i64).unwrap_or(3600);
    token["accessToken"] = Value::String(access.clone());
    let stamp = super::aws_sigv4::amz_date(crate::utils::now_ms() + expires_in * 1000);
    token["expiresAt"] = Value::String(format!(
        "{}-{}-{}T{}:{}:{}Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
        &stamp[13..15]
    ));
    if let Some(new_refresh) = data.get("refreshToken").and_then(Value::as_str) {
        token["refreshToken"] = Value::String(new_refresh.to_string());
    }
    let _ = std::fs::write(&path, token.to_string());
    Ok(access)
}

async fn from_sso(profile_name: &str, profile: &HashMap<String, String>, shared: &SharedConfig, context: &CredentialContext) -> anyhow::Result<AwsCredentials> {
    let session_name = profile.get("sso_session").cloned();
    let session = session_name.as_ref().and_then(|s| shared.sso_sessions.get(s));
    if session_name.is_some() && session.is_none() {
        anyhow::bail!("Sso session \"{}\" not found in the config file", session_name.unwrap_or_default());
    }
    let start_url = session.and_then(|s| s.get("sso_start_url")).or_else(|| profile.get("sso_start_url")).cloned();
    let sso_region = session.and_then(|s| s.get("sso_region")).or_else(|| profile.get("sso_region")).cloned();
    let (Some(start_url), Some(sso_region), Some(account), Some(role)) =
        (start_url, sso_region, profile.get("sso_account_id"), profile.get("sso_role_name"))
    else {
        anyhow::bail!(
            "Profile is configured with invalid SSO credentials. Required parameters \"sso_account_id\", \"sso_region\", \"sso_role_name\", \"sso_start_url\". Got profile {profile_name}"
        );
    };
    let token = sso_token(session_name.as_deref(), &start_url, &sso_region, context).await?;
    let url = format!(
        "https://portal.sso.{sso_region}.amazonaws.com/federation/credentials?account_id={}&role_name={}",
        url::form_urlencoded::byte_serialize(account.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(role.as_bytes()).collect::<String>()
    );
    let response = context.client.get(url).header("x-amz-sso_bearer_token", token).send().await?;
    let status = response.status();
    let data: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let message = data.get("message").and_then(Value::as_str).unwrap_or("SSO GetRoleCredentials failed");
        anyhow::bail!("{message}");
    }
    let creds = &data["roleCredentials"];
    let (Some(access), Some(secret)) = (creds.get("accessKeyId").and_then(Value::as_str), creds.get("secretAccessKey").and_then(Value::as_str))
    else {
        anyhow::bail!("SSO returns an invalid temporary credential.");
    };
    let mut credentials = AwsCredentials::new(access, secret, creds.get("sessionToken").and_then(Value::as_str).map(str::to_string));
    credentials.expiration_ms = creds.get("expiration").and_then(Value::as_i64);
    Ok(credentials)
}

fn resolve_profile<'a>(
    name: &'a str,
    shared: &'a SharedConfig,
    context: &'a CredentialContext,
    visited: Vec<String>,
) -> futures::future::BoxFuture<'a, anyhow::Result<AwsCredentials>> {
    Box::pin(async move {
        let Some(profile) = shared.profile(name) else {
            anyhow::bail!("Could not resolve credentials using profile: [{name}] in configuration/credentials file(s).");
        };
        if !visited.is_empty()
            && let Some(credentials) = static_from(profile)
        {
            return Ok(credentials);
        }
        if let Some(role_arn) = profile.get("role_arn").filter(|r| !r.is_empty()) {
            if profile.contains_key("mfa_serial") {
                anyhow::bail!("Profile {name} requires multi-factor authentication, but no MFA code callback was provided.");
            }
            let source = if let Some(source_profile) = profile.get("source_profile") {
                if visited.contains(source_profile) {
                    anyhow::bail!("Detected a cycle attempting to resolve credentials for profile {name}");
                }
                let mut next = visited.clone();
                next.push(name.to_string());
                Some(resolve_profile(source_profile, shared, context, next).await?)
            } else if let Some(source) = profile.get("credential_source") {
                Some(match source.as_str() {
                    "Environment" => from_env().ok_or_else(|| credentials_error(NO_CREDENTIALS))?,
                    "Ec2InstanceMetadata" => from_imds(&context.client).await?,
                    "EcsContainer" => from_container(&context.client).await?,
                    other => anyhow::bail!("Unsupported credential source in profile {name}. Got {other}, expected EcsContainer or Ec2InstanceMetadata or Environment."),
                })
            } else if profile.contains_key("web_identity_token_file") {
                None
            } else {
                anyhow::bail!("Profile {name} did not contain credential_source or source_profile.");
            };
            if let Some(source) = source {
                return assume_role(&source, profile, role_arn, context).await;
            }
        }
        if let Some(credentials) = static_from(profile) {
            return Ok(credentials);
        }
        if let (Some(token_file), Some(role_arn)) = (profile.get("web_identity_token_file"), profile.get("role_arn")) {
            return assume_role_with_web_identity(
                role_arn,
                token_file,
                profile.get("role_session_name").map(String::as_str),
                profile.get("region").map(String::as_str),
                context,
            )
            .await;
        }
        if let Some(command) = profile.get("credential_process") {
            return from_process(command, name).await;
        }
        if profile.contains_key("sso_session") || profile.contains_key("sso_start_url") || profile.contains_key("sso_account_id") {
            return from_sso(name, profile, shared, context).await;
        }
        anyhow::bail!("Could not resolve credentials using profile: [{name}] in configuration/credentials file(s).")
    })
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
    if env("AWS_EC2_METADATA_DISABLED").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        anyhow::bail!("EC2 Instance Metadata Service access disabled");
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

/// Resolve credentials with the SDK default chain. `profile` is the client's configured profile
/// (an explicit profile, else `AWS_PROFILE`).
pub async fn default_provider(profile: Option<&str>, context: &CredentialContext) -> anyhow::Result<AwsCredentials> {
    let configured_profile = profile.map(str::to_string).or_else(|| env("AWS_PROFILE"));
    if configured_profile.is_none()
        && let Some(credentials) = from_env()
    {
        return Ok(credentials);
    }
    let profile_name = configured_profile.clone().unwrap_or_else(|| "default".into());
    let shared = SharedConfig::load();
    if shared.profile(&profile_name).is_some() {
        let key = format!("profile:{profile_name}");
        if let Some(credentials) = cached(&key) {
            return Ok(credentials);
        }
        match resolve_profile(&profile_name, &shared, context, Vec::new()).await {
            Ok(credentials) => {
                store(&key, &credentials);
                return Ok(credentials);
            }
            Err(error) => {
                // Problems with a profile that exists are terminal, as in the SDK chain.
                if !error.to_string().starts_with("Could not resolve credentials using profile") {
                    return Err(error);
                }
            }
        }
    }
    if let (Some(token_file), Some(role_arn)) = (env("AWS_WEB_IDENTITY_TOKEN_FILE"), env("AWS_ROLE_ARN")) {
        let key = format!("web-identity:{role_arn}");
        if let Some(credentials) = cached(&key) {
            return Ok(credentials);
        }
        let credentials =
            assume_role_with_web_identity(&role_arn, &token_file, env("AWS_ROLE_SESSION_NAME").as_deref(), None, context).await?;
        store(&key, &credentials);
        return Ok(credentials);
    }
    if env("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_some() || env("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_some() {
        if let Some(credentials) = cached("container") {
            return Ok(credentials);
        }
        let credentials = from_container(&context.client).await?;
        store("container", &credentials);
        return Ok(credentials);
    }
    if let Some(credentials) = cached("imds") {
        return Ok(credentials);
    }
    match from_imds(&context.client).await {
        Ok(credentials) => {
            store("imds", &credentials);
            Ok(credentials)
        }
        Err(_) => Err(credentials_error(NO_CREDENTIALS)),
    }
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
        let config = "[default]\nregion = us-west-2\n[profile dev]\nrole_arn = arn:aws:iam::1:role/r\nsource_profile = default\ns3 =\n  max_concurrent_requests = 20\nregion=eu-west-1 # comment\n[sso-session corp]\nsso_region = us-east-1\nsso_start_url = https://corp.awsapps.com/start\n";
        let credentials = "[default]\naws_access_key_id = AKID\naws_secret_access_key = SECRET\n; comment\n";
        let shared = SharedConfig::from_texts(config, credentials);
        let default = shared.profile("default").unwrap();
        assert_eq!(default.get("region").map(String::as_str), Some("us-west-2"));
        assert_eq!(static_from(default).unwrap().access_key_id, "AKID");
        let dev = shared.profile("dev").unwrap();
        assert_eq!(dev.get("region").map(String::as_str), Some("eu-west-1"));
        assert!(!dev.contains_key("max_concurrent_requests"));
        assert_eq!(shared.sso_sessions["corp"]["sso_region"], "us-east-1");
    }

    #[test]
    fn parses_sts_responses() {
        let xml = "<AssumeRoleResponse><AssumeRoleResult><Credentials><AccessKeyId>ASIA</AccessKeyId><SecretAccessKey>s</SecretAccessKey><SessionToken>t&amp;x</SessionToken><Expiration>2030-01-01T00:00:00Z</Expiration></Credentials></AssumeRoleResult></AssumeRoleResponse>";
        let credentials = sts_credentials(xml).unwrap();
        assert_eq!(credentials.session_token.as_deref(), Some("t&x"));
        assert_eq!(credentials.expiration_ms, parse_iso8601("2030-01-01T00:00:00Z"));
        assert_eq!(
            sts_error(403, "<ErrorResponse><Error><Code>AccessDenied</Code><Message>nope</Message></Error></ErrorResponse>").to_string(),
            "AccessDenied: nope"
        );
    }

    #[tokio::test]
    async fn runs_credential_processes() {
        if cfg!(windows) {
            return;
        }
        let credentials = from_process(
            r#"echo '{"Version":1,"AccessKeyId":"A","SecretAccessKey":"S","SessionToken":"T","Expiration":"2999-01-01T00:00:00Z"}'"#,
            "p",
        )
        .await
        .unwrap();
        assert_eq!((credentials.access_key_id.as_str(), credentials.session_token.as_deref()), ("A", Some("T")));
        assert!(from_process("echo '{}'", "p").await.unwrap_err().to_string().contains("Version 1"));
    }
}
