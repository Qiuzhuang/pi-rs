//! Amazon Bedrock provider (port of `providers/amazon-bedrock.ts`).

use std::sync::Arc;

use async_trait::async_trait;

use super::builtin::catalog_chat_models;
use crate::api::registry::bedrock_converse_stream_api;
use crate::auth::types::{
    ApiKeyAuth, ApiKeyCredential, ApiKeyResolveInput, AuthEvent, AuthInfoLink, AuthPrompt, AuthPromptKind, AuthResult,
    AuthSelectOption, ModelAuth, ProviderAuth, ProviderAuthInteraction,
};
use crate::models::{CreateProviderOptions, Provider, ProviderApi, create_provider};
use crate::types::ProviderEnv;

struct BedrockAuth;

fn select(message: &str, options: &[(&str, &str)]) -> AuthPrompt {
    AuthPrompt {
        kind: AuthPromptKind::Select,
        message: message.into(),
        placeholder: None,
        options: options
            .iter()
            .map(|(id, label)| AuthSelectOption { id: (*id).into(), label: (*label).into(), description: None })
            .collect(),
        signal: None,
    }
}

#[async_trait]
impl ApiKeyAuth for BedrockAuth {
    fn name(&self) -> &str {
        "AWS credentials or bearer token"
    }

    fn supports_login(&self) -> bool {
        true
    }

    async fn login(&self, interaction: ProviderAuthInteraction) -> anyhow::Result<ApiKeyCredential> {
        interaction.throw_if_aborted()?;
        let method = interaction
            .interaction
            .prompt(select(
                "Select Amazon Bedrock authentication method:",
                &[
                    ("bearer-token", "Bearer token"),
                    ("aws-profile", "AWS profile"),
                    ("credential-chain", "Existing AWS credential chain"),
                ],
            ))
            .await?;
        interaction.throw_if_aborted()?;
        if method == "bearer-token" {
            let key = interaction.interaction.prompt(AuthPrompt::secret("Enter Amazon Bedrock bearer token")).await?;
            return Ok(ApiKeyCredential { key: Some(key), env: None });
        }
        interaction.interaction.notify(AuthEvent::Info {
            message: "Amazon Bedrock supports AWS profiles, IAM credentials, and role-based credentials.".into(),
            links: vec![AuthInfoLink {
                label: Some("AWS credential provider chain".into()),
                url: "https://docs.aws.amazon.com/sdkref/latest/guide/standardized-credentials.html".into(),
            }],
        });
        if method == "aws-profile" {
            let profile = interaction.interaction.prompt(AuthPrompt::text("Enter AWS profile name")).await?;
            let mut env = ProviderEnv::new();
            env.insert("AWS_PROFILE".into(), profile);
            return Ok(ApiKeyCredential { key: None, env: Some(env) });
        }
        if method != "credential-chain" {
            anyhow::bail!("Unknown Amazon Bedrock auth method: {method}");
        }
        interaction.interaction.prompt(AuthPrompt::text("Configure AWS credentials, then press Enter to continue")).await?;
        Ok(ApiKeyCredential::default())
    }

    async fn resolve(&self, input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        let env = |name: &'static str| {
            let input = input.clone();
            async move {
                input.throw_if_aborted()?;
                let value = input.ctx.env(name).await;
                input.throw_if_aborted()?;
                anyhow::Ok(value)
            }
        };
        let source = |s: &str| Some(s.to_string());
        if let Some(credential) = &input.credential {
            if let Some(key) = credential.key.as_ref().filter(|k| !k.is_empty()) {
                return Ok(Some(AuthResult {
                    auth: ModelAuth { api_key: Some(key.clone()), ..Default::default() },
                    env: credential.env.clone(),
                    source: source("stored credential"),
                }));
            }
        }
        if env("AWS_BEARER_TOKEN_BEDROCK").await?.is_some() {
            return Ok(Some(AuthResult { source: source("AWS_BEARER_TOKEN_BEDROCK"), ..Default::default() }));
        }
        let stored_profile = input.credential.as_ref().and_then(|c| c.env.as_ref()).and_then(|e| e.get("AWS_PROFILE")).cloned();
        let profile = match &stored_profile {
            Some(p) => Some(p.clone()),
            None => env("AWS_PROFILE").await?,
        };
        if profile.is_some_and(|p| !p.is_empty()) {
            return Ok(Some(AuthResult {
                auth: ModelAuth::default(),
                env: input.credential.as_ref().and_then(|c| c.env.clone()),
                source: source(if stored_profile.is_some() { "stored credential" } else { "AWS_PROFILE" }),
            }));
        }
        if env("AWS_ACCESS_KEY_ID").await?.is_some() && env("AWS_SECRET_ACCESS_KEY").await?.is_some() {
            return Ok(Some(AuthResult { source: source("AWS access keys"), ..Default::default() }));
        }
        if env("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").await?.is_some() {
            return Ok(Some(AuthResult { source: source("ECS task role"), ..Default::default() }));
        }
        if env("AWS_CONTAINER_CREDENTIALS_FULL_URI").await?.is_some() {
            return Ok(Some(AuthResult { source: source("ECS task role"), ..Default::default() }));
        }
        if env("AWS_WEB_IDENTITY_TOKEN_FILE").await?.is_some() {
            return Ok(Some(AuthResult { source: source("web identity token"), ..Default::default() }));
        }
        Ok(None)
    }
}

pub fn amazon_bedrock_provider() -> Arc<dyn Provider> {
    create_provider(CreateProviderOptions {
        id: "amazon-bedrock".into(),
        name: Some("Amazon Bedrock".into()),
        auth: ProviderAuth { api_key: Some(Arc::new(BedrockAuth)), oauth: None },
        models: catalog_chat_models("amazon-bedrock"),
        api: Some(ProviderApi::Single(bedrock_converse_stream_api())),
        ..Default::default()
    })
    .expect("bedrock provider is valid")
}
