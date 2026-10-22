//! Port of `packages/coding-agent/src/cli/auth-command.ts`.

use std::sync::LazyLock;

use crate::cli::args::Args;
use crate::config::APP_NAME;
use pi_ai::auth::types::AuthResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthCommandKind {
    #[serde(rename = "check")]
    Check,
    #[serde(rename = "api_key")]
    ApiKey,
    #[serde(rename = "bearer_token")]
    BearerToken,
}

impl AuthCommandKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::ApiKey => "api_key",
            Self::BearerToken => "bearer_token",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCommand {
    pub kind: AuthCommandKind,
    pub args: Vec<String>,
    pub json: bool,
    pub credentials: bool,
    pub no_refresh: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_expiry_ms: Option<i64>,
}

/// TS: `extends Error`
#[derive(Debug, Clone)]
pub struct AuthCommandError {
    pub message: String,
}

impl std::fmt::Display for AuthCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AuthCommandError {}

impl AuthCommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

fn auth_error(message: impl Into<String>) -> anyhow::Error {
    AuthCommandError::new(message).into()
}

pub fn get_auth_command_name(kind: &AuthCommandKind) -> String {
    match kind {
        AuthCommandKind::Check => "auth check",
        AuthCommandKind::ApiKey => "auth print-api-key",
        AuthCommandKind::BearerToken => "auth print-bearer-token",
    }
    .to_string()
}

pub fn get_auth_command_usage(kind: &AuthCommandKind) -> String {
    match kind {
        AuthCommandKind::Check => {
            format!("{APP_NAME} auth check --provider <provider> [--json] [--credentials] [--no-refresh]")
        }
        AuthCommandKind::ApiKey => format!("{APP_NAME} auth print-api-key --provider <provider> [--model <model>]"),
        AuthCommandKind::BearerToken => format!(
            "{APP_NAME} auth print-bearer-token --provider <provider> [--model <model>] [--min-expiry <duration>]"
        ),
    }
}

pub fn is_auth_command_help(args: &[String]) -> bool {
    args.first().map(String::as_str) == Some("auth")
        && (args.get(1).is_none()
            || args.get(1).map(String::as_str) == Some("help")
            || args.iter().any(|a| a == "--help" || a == "-h"))
}

pub const AUTH_COMMAND_HELP: &str = "Usage:
  pi auth print-api-key [--provider <provider>] [--model <model>]
  pi auth print-bearer-token [--provider <provider>] [--model <model>] [--min-expiry <duration>]
  pi auth check [--provider <provider>] [--model <model>] [--json] [--credentials] [--no-refresh]

Auth commands require at least one of --provider or --model. Checks refresh expired OAuth credentials by default; --no-refresh prevents this. --credentials emits the credential, or includes it in JSON output.";

pub fn print_auth_command_help() {
    println!("{AUTH_COMMAND_HELP}");
}

static MIN_EXPIRY_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)^(\d+)(ms|s|m|h)$").expect("valid regex"));

pub fn parse_auth_command(args: &[String]) -> anyhow::Result<Option<AuthCommand>> {
    if args.first().map(String::as_str) != Some("auth") {
        return Ok(None);
    }

    let kind = match args.get(1).map(String::as_str) {
        Some("check") => AuthCommandKind::Check,
        Some("print-api-key") => AuthCommandKind::ApiKey,
        Some("print-bearer-token") => AuthCommandKind::BearerToken,
        other => {
            return Err(auth_error(format!(
                "Unknown auth command \"{}\". Use \"{APP_NAME} auth print-api-key\", \"{APP_NAME} auth print-bearer-token\", or \"{APP_NAME} auth check\".",
                other.unwrap_or("")
            )));
        }
    };

    let mut command_args: Vec<String> = Vec::new();
    let mut json = false;
    let mut credentials = false;
    let mut no_refresh = false;
    let mut min_expiry_ms: Option<i64> = None;
    let mut index = 2;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--min-expiry" {
            if kind != AuthCommandKind::BearerToken {
                return Err(auth_error("--min-expiry is only supported by print-bearer-token"));
            }
            index += 1;
            let value = args.get(index).filter(|v| !v.is_empty());
            let captures = value.and_then(|v| MIN_EXPIRY_RE.captures(v));
            let Some(captures) = captures else {
                return Err(auth_error("--min-expiry must use a duration such as 30m or 1h"));
            };
            let amount: i64 = captures[1].parse().unwrap_or(i64::MAX);
            // Units are matched case-insensitively but compared exactly, as in TS.
            let multiplier = match &captures[2] {
                "ms" => 1,
                "s" => 1_000,
                "m" => 60_000,
                _ => 3_600_000,
            };
            min_expiry_ms = Some(amount.saturating_mul(multiplier));
            index += 1;
            continue;
        }
        if arg == "--json" || arg == "--credentials" || arg == "--no-refresh" {
            if kind != AuthCommandKind::Check {
                return Err(auth_error(format!("{arg} is only supported by auth check")));
            }
            match arg {
                "--json" => json = true,
                "--credentials" => credentials = true,
                _ => no_refresh = true,
            }
            index += 1;
            continue;
        }
        command_args.push(arg.to_string());
        index += 1;
    }

    Ok(Some(AuthCommand { kind, args: command_args, json, credentials, no_refresh, min_expiry_ms }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateAuthCommandArgsResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

pub fn validate_auth_command_args(
    args: &Args,
    kind: &AuthCommandKind,
) -> anyhow::Result<ValidateAuthCommandArgsResult> {
    let provider = args.provider.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let model = args.model.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if let Some(option) = args.unknown_flags.keys().next() {
        return Err(auth_error(format!(
            "Unknown option --{option} for \"{}\".",
            get_auth_command_name(kind)
        )));
    }
    if args.api_key.is_some() || !args.messages.is_empty() || !args.file_args.is_empty() {
        return Err(auth_error("Auth commands only accept --provider and --model"));
    }
    if provider.is_none() && model.is_none() {
        return Err(auth_error(if *kind == AuthCommandKind::Check {
            "Auth checks require --provider <provider> or --model <model>"
        } else {
            "Credential printing requires --provider <provider> or --model <model>"
        }));
    }
    Ok(ValidateAuthCommandArgsResult { provider, model })
}

static BEARER_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?is)^Bearer\s+(.+)$").expect("valid regex"));

pub fn get_auth_credential(auth: Option<&AuthResult>) -> Option<String> {
    let auth = auth?;
    if let Some(api_key) = auth.auth.api_key.as_ref().filter(|k| !k.is_empty()) {
        return Some(api_key.clone());
    }
    let authorization = auth
        .auth
        .headers
        .as_ref()?
        .iter()
        .find(|(name, _)| name.to_lowercase() == "authorization")
        .map(|(_, value)| value)?;
    let authorization = authorization.as_ref()?;
    BEARER_RE.captures(authorization).map(|caps| caps[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_auth_commands() {
        assert_eq!(parse_auth_command(&strings(&["chat"])).unwrap(), None);
        let command = parse_auth_command(&strings(&["auth", "check", "--provider", "openai", "--json", "--no-refresh"]))
            .unwrap()
            .unwrap();
        assert_eq!(command.kind, AuthCommandKind::Check);
        assert_eq!(command.args, strings(&["--provider", "openai"]));
        assert!(command.json && command.no_refresh && !command.credentials);
        let command = parse_auth_command(&strings(&["auth", "print-bearer-token", "--min-expiry", "2h"]))
            .unwrap()
            .unwrap();
        assert_eq!(command.min_expiry_ms, Some(7_200_000));
        let err = parse_auth_command(&strings(&["auth", "print-api-key", "--min-expiry", "1h"])).unwrap_err();
        assert_eq!(err.to_string(), "--min-expiry is only supported by print-bearer-token");
        let err = parse_auth_command(&strings(&["auth", "print-bearer-token", "--min-expiry", "soon"])).unwrap_err();
        assert_eq!(err.to_string(), "--min-expiry must use a duration such as 30m or 1h");
        let err = parse_auth_command(&strings(&["auth", "print-api-key", "--json"])).unwrap_err();
        assert_eq!(err.to_string(), "--json is only supported by auth check");
        let err = parse_auth_command(&strings(&["auth", "nope"])).unwrap_err();
        assert!(err.downcast_ref::<AuthCommandError>().is_some());
        assert_eq!(
            err.to_string(),
            "Unknown auth command \"nope\". Use \"pi auth print-api-key\", \"pi auth print-bearer-token\", or \"pi auth check\"."
        );
    }

    #[test]
    fn detects_help() {
        assert!(is_auth_command_help(&strings(&["auth"])));
        assert!(is_auth_command_help(&strings(&["auth", "help"])));
        assert!(is_auth_command_help(&strings(&["auth", "check", "-h"])));
        assert!(!is_auth_command_help(&strings(&["auth", "check"])));
    }

    #[test]
    fn extracts_credentials() {
        let mut auth = AuthResult { auth: Default::default(), env: None, source: None };
        assert_eq!(get_auth_credential(Some(&auth)), None);
        let mut headers = pi_ai::types::ProviderHeaders::new();
        headers.insert("Authorization".into(), Some("Bearer tok-1".into()));
        auth.auth.headers = Some(headers);
        assert_eq!(get_auth_credential(Some(&auth)).as_deref(), Some("tok-1"));
        auth.auth.api_key = Some("key".into());
        assert_eq!(get_auth_credential(Some(&auth)).as_deref(), Some("key"));
        assert_eq!(get_auth_credential(None), None);
    }
}
