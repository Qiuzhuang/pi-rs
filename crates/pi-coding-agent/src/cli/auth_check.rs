//! Port of `packages/coding-agent/src/cli/auth-check.ts`.

use crate::cli::args::Args;
use crate::core::model_runtime::ModelRuntime;
use pi_ai::auth::types::CredentialStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthCheckStatus {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "not_ready")]
    NotReady,
    #[serde(rename = "invalid")]
    Invalid,
}

impl AuthCheckStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NotReady => "not_ready",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthCheckReason {
    #[serde(rename = "provider_not_found")]
    ProviderNotFound,
    #[serde(rename = "credentials_not_configured")]
    CredentialsNotConfigured,
    #[serde(rename = "credential_not_available")]
    CredentialNotAvailable,
    #[serde(rename = "invalid_state")]
    InvalidState,
}

impl AuthCheckReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProviderNotFound => "provider_not_found",
            Self::CredentialsNotConfigured => "credentials_not_configured",
            Self::CredentialNotAvailable => "credential_not_available",
            Self::InvalidState => "invalid_state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthCheckResultAuthType {
    #[serde(rename = "api_key")]
    ApiKey,
    #[serde(rename = "oauth")]
    Oauth,
}

impl AuthCheckResultAuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Oauth => "oauth",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCheckResult {
    pub status: AuthCheckStatus,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<AuthCheckReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_type: Option<AuthCheckResultAuthType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckProviderAuthOptions {
    pub refresh: bool,
}

pub async fn check_provider_auth(
    args: &Args,
    model_runtime: Arc<ModelRuntime>,
    options: Option<&CheckProviderAuthOptions>,
) -> anyhow::Result<AuthCheckResult> {
    todo!("port cli/auth-check.ts:checkProviderAuth")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderCredentialOptions {
    pub refresh: bool,
}

pub async fn get_provider_credential(
    provider_id: &str,
    model_runtime: Arc<ModelRuntime>,
    credentials: Arc<dyn CredentialStore>,
    options: &GetProviderCredentialOptions,
) -> anyhow::Result<Option<String>> {
    todo!("port cli/auth-check.ts:getProviderCredential")
}

pub async fn create_auth_check_model_runtime(
    credentials: Arc<dyn CredentialStore>,
) -> anyhow::Result<Arc<ModelRuntime>> {
    todo!("port cli/auth-check.ts:createAuthCheckModelRuntime")
}
