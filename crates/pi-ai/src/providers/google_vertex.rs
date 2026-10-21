//! Google Vertex AI provider (port of `providers/google-vertex.ts`).

use std::sync::Arc;

use async_trait::async_trait;

use super::builtin::catalog_chat_models;
use crate::api::registry::google_vertex_api;
use crate::auth::types::{
    ApiKeyAuth, ApiKeyCredential, ApiKeyResolveInput, AuthEvent, AuthInfoLink, AuthPrompt, AuthPromptKind, AuthResult,
    AuthSelectOption, ModelAuth, ProviderAuth, ProviderAuthInteraction,
};
use crate::models::{CreateProviderOptions, Provider, ProviderApi, create_provider};
use crate::types::ProviderEnv;

const VERTEX_ADC_PATH: &str = "~/.config/gcloud/application_default_credentials.json";

struct VertexAuth;

#[async_trait]
impl ApiKeyAuth for VertexAuth {
    fn name(&self) -> &str {
        "Google Cloud credentials"
    }

    fn supports_login(&self) -> bool {
        true
    }

    async fn login(&self, interaction: ProviderAuthInteraction) -> anyhow::Result<ApiKeyCredential> {
        interaction.throw_if_aborted()?;
        let prompt = AuthPrompt {
            kind: AuthPromptKind::Select,
            message: "Select Google Vertex AI authentication method:".into(),
            placeholder: None,
            options: [
                ("api-key", "Google Cloud API key"),
                ("adc", "Application Default Credentials"),
                ("service-account", "Service account credentials file"),
            ]
            .iter()
            .map(|(id, label)| AuthSelectOption { id: (*id).into(), label: (*label).into(), description: None })
            .collect(),
            signal: None,
        };
        let method = interaction.interaction.prompt(prompt).await?;
        interaction.throw_if_aborted()?;
        if method == "api-key" {
            let key = interaction.interaction.prompt(AuthPrompt::secret("Enter Google Cloud API key")).await?;
            return Ok(ApiKeyCredential { key: Some(key), env: None });
        }
        if method != "adc" && method != "service-account" {
            anyhow::bail!("Unknown Google Vertex AI auth method: {method}");
        }
        interaction.interaction.notify(AuthEvent::Info {
            message: if method == "adc" {
                "Run `gcloud auth application-default login`, then provide the project and location.".into()
            } else {
                "Provide a service account credentials file, project, and location.".into()
            },
            links: vec![AuthInfoLink {
                label: Some("Application Default Credentials".into()),
                url: "https://cloud.google.com/docs/authentication/provide-credentials-adc".into(),
            }],
        });
        let project = interaction.interaction.prompt(AuthPrompt::text("Enter Google Cloud project ID")).await?;
        let location = interaction.interaction.prompt(AuthPrompt::text("Enter Google Cloud location")).await?;
        let credentials_path = if method == "service-account" {
            Some(interaction.interaction.prompt(AuthPrompt::text("Enter service account credentials file path")).await?)
        } else {
            None
        };
        let mut env = ProviderEnv::new();
        env.insert("GOOGLE_CLOUD_PROJECT".into(), project);
        env.insert("GOOGLE_CLOUD_LOCATION".into(), location);
        if let Some(path) = credentials_path.filter(|p| !p.is_empty()) {
            env.insert("GOOGLE_APPLICATION_CREDENTIALS".into(), path);
        }
        Ok(ApiKeyCredential { key: None, env: Some(env) })
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
        let credential = input.credential.clone();
        let stored_key = credential.as_ref().and_then(|c| c.key.clone());
        let key = match &stored_key {
            Some(k) => Some(k.clone()),
            None => env("GOOGLE_CLOUD_API_KEY").await?,
        };
        if let Some(key) = key.filter(|k| !k.is_empty()) {
            return Ok(Some(AuthResult {
                auth: ModelAuth { api_key: Some(key), ..Default::default() },
                env: None,
                source: Some(if stored_key.is_some() { "stored credential" } else { "GOOGLE_CLOUD_API_KEY" }.into()),
            }));
        }
        let stored_env = |name: &str| credential.as_ref().and_then(|c| c.env.as_ref()).and_then(|e| e.get(name)).cloned();
        let adc_path = match stored_env("GOOGLE_APPLICATION_CREDENTIALS") {
            Some(p) => Some(p),
            None => env("GOOGLE_APPLICATION_CREDENTIALS").await?,
        };
        input.throw_if_aborted()?;
        let has_credentials = input.ctx.file_exists(adc_path.as_deref().unwrap_or(VERTEX_ADC_PATH)).await;
        input.throw_if_aborted()?;
        let project = match stored_env("GOOGLE_CLOUD_PROJECT") {
            Some(p) => Some(p),
            None => match env("GOOGLE_CLOUD_PROJECT").await? {
                Some(p) => Some(p),
                None => env("GCLOUD_PROJECT").await?,
            },
        };
        let location = match stored_env("GOOGLE_CLOUD_LOCATION") {
            Some(l) => Some(l),
            None => env("GOOGLE_CLOUD_LOCATION").await?,
        };
        if has_credentials && project.is_some_and(|p| !p.is_empty()) && location.is_some_and(|l| !l.is_empty()) {
            return Ok(Some(AuthResult {
                auth: ModelAuth::default(),
                env: credential.as_ref().and_then(|c| c.env.clone()),
                source: Some(
                    if credential.is_some() { "stored credential" } else { "gcloud application default credentials" }.into(),
                ),
            }));
        }
        Ok(None)
    }
}

pub fn google_vertex_provider() -> Arc<dyn Provider> {
    create_provider(CreateProviderOptions {
        id: "google-vertex".into(),
        name: Some("Google Vertex AI".into()),
        auth: ProviderAuth { api_key: Some(Arc::new(VertexAuth)), oauth: None },
        models: catalog_chat_models("google-vertex"),
        api: Some(ProviderApi::Single(google_vertex_api())),
        ..Default::default()
    })
    .expect("vertex provider is valid")
}
