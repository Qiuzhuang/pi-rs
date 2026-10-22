//! Port of `packages/coding-agent/src/cli/auth-check.ts`.

use crate::cli::args::Args;
use crate::cli::auth_command::{
    AuthCommandError, AuthCommandKind, get_auth_credential, validate_auth_command_args,
};
use crate::core::model_resolver::{ResolveCliModelOptions, resolve_cli_model};
use crate::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntimeGetAuthProviderOrModel};
use crate::core::models_store::InMemoryCodingAgentModelsStore;
use pi_ai::auth::types::{AuthType, Credential};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCheckResult {
    pub status: AuthCheckStatus,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<AuthCheckReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_type: Option<AuthCheckResultAuthType>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckProviderAuthOptions {
    pub refresh: bool,
}

pub async fn check_provider_auth(
    args: &Args,
    model_runtime: Arc<ModelRuntime>,
    options: Option<&CheckProviderAuthOptions>,
) -> anyhow::Result<AuthCheckResult> {
    let refresh = options.map(|o| o.refresh).unwrap_or(false);
    let validated = validate_auth_command_args(args, &AuthCommandKind::Check)?;
    let mut provider = validated.provider.clone();
    if let Some(cli_model) = &validated.model {
        let resolved = resolve_cli_model(&ResolveCliModelOptions {
            cli_provider: validated.provider.clone(),
            cli_model: Some(cli_model.clone()),
            cli_thinking: None,
            inline_api_key: None,
            model_runtime: model_runtime.clone(),
        });
        match (resolved.error, resolved.model) {
            (None, Some(model)) => provider = Some(model.provider.clone()),
            (error, _) => {
                return Err(AuthCommandError::new(
                    error.unwrap_or_else(|| format!("Unable to resolve model \"{cli_model}\"")),
                )
                .into());
            }
        }
    }
    let Some(provider) = provider else {
        return Err(AuthCommandError::new("Unable to resolve an auth provider").into());
    };
    let result = |status, reason, auth_type| AuthCheckResult {
        status,
        provider: provider.clone(),
        reason,
        auth_type,
    };
    if model_runtime.get_error().is_some() {
        return Ok(result(AuthCheckStatus::Invalid, Some(AuthCheckReason::InvalidState), None));
    }
    if model_runtime.get_provider(&provider).is_none() {
        return Ok(result(AuthCheckStatus::NotReady, Some(AuthCheckReason::ProviderNotFound), None));
    }
    let checked: anyhow::Result<AuthCheckResult> = async {
        let Some(auth) = model_runtime.check_auth(&provider, None).await? else {
            return Ok(result(AuthCheckStatus::NotReady, Some(AuthCheckReason::CredentialsNotConfigured), None));
        };
        if refresh
            && model_runtime
                .get_auth(&ModelRuntimeGetAuthProviderOrModel::String(provider.clone()), None)
                .await?
                .is_none()
        {
            return Ok(result(AuthCheckStatus::NotReady, Some(AuthCheckReason::CredentialsNotConfigured), None));
        }
        let auth_type = match auth.auth_type {
            AuthType::ApiKey => AuthCheckResultAuthType::ApiKey,
            AuthType::OAuth => AuthCheckResultAuthType::Oauth,
        };
        Ok(result(AuthCheckStatus::Ready, None, Some(auth_type)))
    }
    .await;
    Ok(checked.unwrap_or_else(|_| result(AuthCheckStatus::Invalid, Some(AuthCheckReason::InvalidState), None)))
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
    let credential = credentials.read(provider_id, None).await?;
    if !options.refresh
        && let Some(Credential::OAuth(oauth)) = &credential
    {
        return Ok(Some(oauth.access.clone()));
    }
    let auth = model_runtime
        .get_auth(&ModelRuntimeGetAuthProviderOrModel::String(provider_id.to_string()), None)
        .await?;
    Ok(get_auth_credential(auth.as_ref()))
}

pub async fn create_auth_check_model_runtime(
    credentials: Arc<dyn CredentialStore>,
) -> anyhow::Result<Arc<ModelRuntime>> {
    ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(credentials),
        auth_path: None,
        models_path: None,
        models_store: Some(Arc::new(InMemoryCodingAgentModelsStore::new())),
        models_store_path: None,
        allow_model_network: Some(false),
        model_refresh_timeout_ms: None,
        catalog_base_url: None,
        signal: None,
        refresh_on_create: Some(false),
    }))
    .await
}
