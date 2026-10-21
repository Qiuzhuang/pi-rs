//! Port of `packages/coding-agent/src/cli/auth-command.ts`.

use crate::cli::args::Args;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl AuthCommandError {}

pub fn get_auth_command_name(kind: &AuthCommandKind) -> String {
    todo!("port cli/auth-command.ts:getAuthCommandName")
}

pub fn get_auth_command_usage(kind: &AuthCommandKind) -> String {
    todo!("port cli/auth-command.ts:getAuthCommandUsage")
}

pub fn is_auth_command_help(args: &[String]) -> bool {
    todo!("port cli/auth-command.ts:isAuthCommandHelp")
}

pub fn print_auth_command_help() {
    todo!("port cli/auth-command.ts:printAuthCommandHelp")
}

pub fn parse_auth_command(args: &[String]) -> anyhow::Result<Option<AuthCommand>> {
    todo!("port cli/auth-command.ts:parseAuthCommand")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateAuthCommandArgsValidateAuthCommandArgsResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

pub fn validate_auth_command_args(
    args: &Args,
    kind: &AuthCommandKind,
) -> anyhow::Result<ValidateAuthCommandArgsValidateAuthCommandArgsResult> {
    todo!("port cli/auth-command.ts:validateAuthCommandArgs")
}

pub fn get_auth_credential(auth: Option<AuthResult>) -> Option<String> {
    todo!("port cli/auth-command.ts:getAuthCredential")
}
