//! Port of `packages/coding-agent/src/cli/credential-print.ts`.

use crate::cli::args::Args;
use crate::cli::auth_command::{AuthCommandError, AuthCommandKind, get_auth_credential, validate_auth_command_args};
use crate::compat::AuthOperationOptions;
use crate::core::model_resolver::{
    ModelReferenceRequest, ModelReferenceRequestThinkingSuffix, ModelReferenceResult, ResolveCliModelOptions,
    resolve_cli_model, resolve_model_reference,
};
use crate::core::model_runtime::{ModelRuntimeAuthOverrides, ModelRuntimeGetAuthProviderOrModel};
use indexmap::IndexMap;
use pi_ai::auth::types::AuthType;
use pi_ai::types::{AnyModel, Model};
use crate::core::model_runtime::ModelRuntime;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CredentialPrintKindValue {
    #[serde(rename = "api_key")]
    ApiKey,
    #[serde(rename = "bearer_token")]
    BearerToken,
}

impl CredentialPrintKindValue {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::BearerToken => "bearer_token",
        }
    }
}

pub type CredentialPrintKind = CredentialPrintKindValue;

const DEFAULT_BEARER_TOKEN_MIN_EXPIRY_MS: i64 = 30 * 60_000;

impl From<CredentialPrintKindValue> for AuthCommandKind {
    fn from(value: CredentialPrintKindValue) -> Self {
        match value {
            CredentialPrintKindValue::ApiKey => AuthCommandKind::ApiKey,
            CredentialPrintKindValue::BearerToken => AuthCommandKind::BearerToken,
        }
    }
}

fn auth_error(message: impl Into<String>) -> anyhow::Error {
    AuthCommandError::new(message).into()
}

/// Resolve one configured provider credential.
///
/// This intentionally calls ModelRuntime.getAuth(), which refreshes and persists
/// OAuth credentials with less than five minutes remaining through the normal request-auth path.
pub async fn resolve_credential_for_print(
    args: &Args,
    model_runtime: Arc<ModelRuntime>,
    kind: &CredentialPrintKind,
    min_expiry_ms: Option<i64>,
    signal: Option<CancellationToken>,
) -> anyhow::Result<String> {
    let validated = validate_auth_command_args(args, &AuthCommandKind::from(*kind))?;
    let cli_provider = validated.provider;
    let cli_model = validated.model;
    let credential_types: IndexMap<String, AuthType> = model_runtime
        .list_credentials(Some(&AuthOperationOptions { signal: signal.clone() }))
        .await?
        .into_iter()
        .map(|credential| (credential.provider_id, credential.credential_type))
        .collect();

    let mut providers: Vec<(String, Option<Model>)> = Vec::new();
    if let Some(cli_provider) = &cli_provider {
        let Some(provider) = model_runtime.get_provider(cli_provider) else {
            return Err(auth_error(format!(
                "Unknown provider \"{cli_provider}\". Use --list-models to see available providers."
            )));
        };
        let provider_id = provider.id().to_string();
        if let Some(cli_model) = &cli_model {
            let resolved = resolve_cli_model(&ResolveCliModelOptions {
                cli_provider: Some(provider_id.clone()),
                cli_model: Some(cli_model.clone()),
                cli_thinking: None,
                inline_api_key: None,
                model_runtime: model_runtime.clone(),
            });
            match (resolved.error, resolved.model) {
                (None, Some(model)) => providers.push((provider_id, Some(model))),
                (error, _) => {
                    return Err(auth_error(
                        error.unwrap_or_else(|| "Unable to resolve the requested provider/model".to_string()),
                    ));
                }
            }
        } else {
            providers.push((provider_id, None));
        }
    } else {
        let cli_model = cli_model.clone().unwrap_or_default();
        let models = model_runtime.get_models(None);
        for provider in model_runtime.get_providers() {
            let provider_id = provider.id().to_string();
            if !credential_types.contains_key(&provider_id) {
                continue;
            }
            let resolved = resolve_model_reference(
                &ModelReferenceRequest {
                    text: cli_model.clone(),
                    provider: Some(provider_id.clone()),
                    thinking_suffix: ModelReferenceRequestThinkingSuffix::Strict,
                    fuzzy: true,
                },
                &models,
                &|_| true,
            );
            if let ModelReferenceResult::Resolved { model, .. } = resolved {
                providers.push((provider_id, Some(model)));
            }
        }
        if providers.is_empty() {
            return Err(auth_error(format!(
                "Model \"{cli_model}\" not found. Use --list-models to see available models."
            )));
        }
    }

    let mut credentials: Vec<(String, String)> = Vec::new();
    for (provider_id, model) in &providers {
        let credential_type = credential_types.get(provider_id).copied();
        if *kind == CredentialPrintKindValue::ApiKey && credential_type == Some(AuthType::OAuth) {
            continue;
        }
        if *kind == CredentialPrintKindValue::BearerToken && credential_type != Some(AuthType::OAuth) {
            continue;
        }
        let overrides = ModelRuntimeAuthOverrides {
            signal: signal.clone(),
            api_key: None,
            env: None,
            min_o_auth_validity_ms: if *kind == CredentialPrintKindValue::BearerToken {
                Some(min_expiry_ms.unwrap_or(DEFAULT_BEARER_TOKEN_MIN_EXPIRY_MS))
            } else {
                None
            },
        };
        let target = match model {
            Some(model) => ModelRuntimeGetAuthProviderOrModel::AnyModel(AnyModel::from(model.clone())),
            None => ModelRuntimeGetAuthProviderOrModel::String(provider_id.clone()),
        };
        let auth = model_runtime.get_auth(&target, Some(&overrides)).await?;
        if let Some(value) = get_auth_credential(auth.as_ref()) {
            credentials.push((provider_id.clone(), value));
        }
    }

    if credentials.len() == 1 {
        return Ok(credentials.remove(0).1);
    }
    if credentials.is_empty() {
        let provider_id = providers.first().map(|(id, _)| id.clone());
        let credential_type = provider_id.as_ref().and_then(|id| credential_types.get(id).copied());
        if cli_provider.is_some() && *kind == CredentialPrintKindValue::ApiKey && credential_type == Some(AuthType::OAuth)
        {
            return Err(auth_error(format!(
                "Provider \"{}\" is configured with OAuth, not an API key",
                provider_id.unwrap_or_default()
            )));
        }
        if cli_provider.is_some()
            && *kind == CredentialPrintKindValue::BearerToken
            && credential_type != Some(AuthType::OAuth)
        {
            return Err(auth_error(format!(
                "Provider \"{}\" is not configured with an OAuth bearer token",
                provider_id.unwrap_or_default()
            )));
        }
        return Err(auth_error(format!(
            "No usable {} is configured",
            if *kind == CredentialPrintKindValue::ApiKey { "API key" } else { "OAuth bearer token" }
        )));
    }
    Err(auth_error(format!(
        "Multiple configured providers matched ({}). Specify --provider.",
        credentials.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(", ")
    )))
}
