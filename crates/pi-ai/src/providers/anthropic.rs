//! Anthropic provider (port of `providers/anthropic.ts`).

use std::sync::Arc;

use async_trait::async_trait;

use super::builtin::catalog_chat_models;
use crate::api::registry::anthropic_messages_api;
use crate::auth::oauth_load::{OAuthFlowArgs, lazy_builtin_oauth};
use crate::auth::types::{
    ApiKeyAuth, ApiKeyCredential, ApiKeyResolveInput, AuthPrompt, AuthResult, ModelAuth, ProviderAuth,
    ProviderAuthInteraction,
};
use crate::env_api_keys::*;
use crate::models::{CreateProviderOptions, Provider, ProviderApi, create_provider};
use crate::types::{ProviderEnv, ProviderHeaders};

struct AnthropicApiKeyAuth;

#[async_trait]
impl ApiKeyAuth for AnthropicApiKeyAuth {
    fn name(&self) -> &str {
        "Anthropic API key"
    }

    fn supports_login(&self) -> bool {
        true
    }

    async fn login(&self, interaction: ProviderAuthInteraction) -> anyhow::Result<ApiKeyCredential> {
        interaction.throw_if_aborted()?;
        let key = interaction.interaction.prompt(AuthPrompt::secret("Enter Anthropic API key")).await?;
        interaction.throw_if_aborted()?;
        Ok(ApiKeyCredential { key: Some(key), env: None })
    }

    async fn resolve(&self, input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        input.throw_if_aborted()?;
        if let Some(credential) = &input.credential
            && let Some(key) = credential.key.as_ref().filter(|k| !k.is_empty()) {
                return Ok(Some(AuthResult {
                    auth: ModelAuth { api_key: Some(key.clone()), ..Default::default() },
                    env: credential.env.clone(),
                    source: Some("stored credential".into()),
                }));
            }
        let auth_token = input.ctx.env(ANTHROPIC_AUTH_TOKEN_ENV).await;
        input.throw_if_aborted()?;
        if let Some(token) = auth_token {
            let mut headers = ProviderHeaders::new();
            headers.insert("Authorization".into(), Some(format!("Bearer {token}")));
            return Ok(Some(AuthResult {
                auth: ModelAuth { headers: Some(headers), ..Default::default() },
                env: None,
                source: Some(ANTHROPIC_AUTH_TOKEN_ENV.into()),
            }));
        }
        for env_var in [ANTHROPIC_OAUTH_TOKEN_ENV, ANTHROPIC_API_KEY_ENV] {
            let key = input.ctx.env(env_var).await;
            input.throw_if_aborted()?;
            if let Some(key) = key {
                return Ok(Some(AuthResult {
                    auth: ModelAuth { api_key: Some(key), ..Default::default() },
                    env: None,
                    source: Some(env_var.into()),
                }));
            }
        }
        // Workload identity federation: the ids are provider config and travel in `env`.
        let mut federation = ProviderEnv::new();
        for env_var in [ANTHROPIC_FEDERATION_RULE_ID_ENV, ANTHROPIC_ORGANIZATION_ID_ENV, ANTHROPIC_IDENTITY_TOKEN_FILE_ENV] {
            let value = input.ctx.env(env_var).await;
            input.throw_if_aborted()?;
            match value {
                Some(value) => {
                    federation.insert(env_var.into(), value);
                }
                None => return Ok(None),
            }
        }
        for env_var in [ANTHROPIC_SERVICE_ACCOUNT_ID_ENV, ANTHROPIC_WORKSPACE_ID_ENV] {
            let value = input.ctx.env(env_var).await;
            input.throw_if_aborted()?;
            if let Some(value) = value {
                federation.insert(env_var.into(), value);
            }
        }
        Ok(Some(AuthResult {
            auth: ModelAuth::default(),
            env: Some(federation),
            source: Some("workload identity federation".into()),
        }))
    }
}

pub fn anthropic_provider() -> Arc<dyn Provider> {
    create_provider(CreateProviderOptions {
        id: "anthropic".into(),
        name: Some("Anthropic".into()),
        base_url: Some("https://api.anthropic.com".into()),
        auth: ProviderAuth {
            api_key: Some(Arc::new(AnthropicApiKeyAuth)),
            oauth: Some(lazy_builtin_oauth("Anthropic (Claude Pro/Max)", Some(true), None, "anthropic", OAuthFlowArgs::default())),
        },
        models: catalog_chat_models("anthropic"),
        api: Some(ProviderApi::Single(anthropic_messages_api())),
        ..Default::default()
    })
    .expect("anthropic provider is valid")
}
