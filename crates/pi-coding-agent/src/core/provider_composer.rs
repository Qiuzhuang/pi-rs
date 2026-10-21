//! Port of `packages/coding-agent/src/core/provider-composer.ts`.

use crate::core::model_config::ModelConfig;
use crate::core::model_config::ModelsJsonProvider;
use indexmap::IndexMap;
use pi_ai::auth::types::OAuthCredentials;
use pi_ai::models::Provider;
use pi_ai::models::ProviderClassifier;
use pi_ai::models::ProviderImages;
use pi_ai::models::RefreshModelsContext;
use pi_ai::providers::model_schema::ModelCost;
use pi_ai::providers::model_schema::ModelInputLimits;
use pi_ai::providers::model_schema::ModelPromptCache;
use pi_ai::providers::model_schema::ThinkingLevelMap;
use pi_ai::types::AnyModel;
use pi_ai::types::Api;
use pi_ai::types::ClassifierApi;
use pi_ai::types::ImageApi;
use pi_ai::types::Model;
use pi_ai::types::ProviderHeaders;
use pi_ai::types::SamplingParams;
use pi_ai::types::SamplingParamsByThinkingLevel;
use pi_ai::types::SimpleStreamOptions;
use pi_ai::types::TranscriptContext;
use pi_ai::utils::event_stream::AssistantMessageEventStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio_util::sync::CancellationToken;

#[async_trait::async_trait]
pub trait ExtensionOAuthConfig: Send + Sync {
    fn name(&self) -> String;
    fn set_name(&self, value: String);
    /// Whether access through this auth method is backed by a provider subscription.
    /// Optional in TS: the default implementation means "not provided".
    fn is_subscription(&self) -> Option<bool> {
        None
    }
    fn set_is_subscription(&self, value: Option<bool>);
    /// @deprecated Retained for extension source compatibility; ignored by canonical auth flows.
    /// Optional in TS: the default implementation means "not provided".
    fn uses_callback_server(&self) -> Option<bool> {
        None
    }
    fn set_uses_callback_server(&self, value: Option<bool>);
    async fn login(
        &self,
        callbacks: Value, /* TS: pi-ai::OAuthLoginCallbacks */
    ) -> anyhow::Result<OAuthCredentials>;
    async fn refresh_token(
        &self,
        credentials: OAuthCredentials,
        signal: CancellationToken,
    ) -> anyhow::Result<OAuthCredentials>;
    fn get_api_key(&self, credentials: OAuthCredentials) -> String;
    /// Optional in TS: the default implementation means "not provided".
    fn modify_models(&self, models: &[Model], credentials: OAuthCredentials) -> Vec<Model> {
        todo!("optional method not provided")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderModelConfigBaseInput {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "image")]
    Image,
}

impl ProviderModelConfigBaseInput {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConfigBase {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub input: Vec<ProviderModelConfigBaseInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    pub cost: ModelCost,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
}

// TS: extends ProviderModelConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChatModelConfig {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub input: Vec<ProviderModelConfigBaseInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    pub cost: ModelCost,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<Api>,
    pub reasoning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level_map: Option<ThinkingLevelMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<ModelPromptCache>,
    pub context_window: u64,
    pub max_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_params: Option<SamplingParams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_params_by_thinking_level: Option<SamplingParamsByThinkingLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
}

// TS: extends ProviderModelConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImageModelConfig {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub input: Vec<ProviderModelConfigBaseInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    pub cost: ModelCost,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Always "image".
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ImageApi>,
    pub output: Vec<ProviderModelConfigBaseInput>,
}

// TS: extends ProviderModelConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderClassifierModelConfig {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub input: Vec<ProviderModelConfigBaseInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limits: Option<ModelInputLimits>,
    pub cost: ModelCost,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    /// Always "classifier".
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ClassifierApi>,
    pub context_window: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProviderModelConfig {
    ProviderChatModelConfig(ProviderChatModelConfig),
    ProviderImageModelConfig(ProviderImageModelConfig),
    ProviderClassifierModelConfig(ProviderClassifierModelConfig),
}

/// Input type for the extension registerProvider API.
#[async_trait::async_trait]
pub trait ProviderConfigInput: Send + Sync {
    /// Optional in TS: the default implementation means "not provided".
    fn name(&self) -> Option<String> {
        None
    }
    fn set_name(&self, value: Option<String>);
    /// Optional in TS: the default implementation means "not provided".
    fn base_url(&self) -> Option<String> {
        None
    }
    fn set_base_url(&self, value: Option<String>);
    /// Optional in TS: the default implementation means "not provided".
    fn api_key(&self) -> Option<String> {
        None
    }
    fn set_api_key(&self, value: Option<String>);
    /// Optional in TS: the default implementation means "not provided".
    fn api(&self) -> Option<Api> {
        None
    }
    fn set_api(&self, value: Option<Api>);
    /// Optional in TS: the default implementation means "not provided".
    fn stream_simple(
        &self,
    ) -> Option<
        Arc<
            dyn Fn(
                    Model,
                    TranscriptContext,
                    Option<SimpleStreamOptions>,
                ) -> AssistantMessageEventStream
                + Send
                + Sync,
        >,
    > {
        None
    }
    /// Optional in TS: the default implementation means "not provided".
    fn images(&self) -> Option<IndexMap<ImageApi, Arc<dyn ProviderImages>>> {
        None
    }
    fn set_images(&self, value: Option<IndexMap<ImageApi, Arc<dyn ProviderImages>>>);
    /// Optional in TS: the default implementation means "not provided".
    fn classifiers(&self) -> Option<IndexMap<ClassifierApi, Arc<dyn ProviderClassifier>>> {
        None
    }
    fn set_classifiers(&self, value: Option<IndexMap<ClassifierApi, Arc<dyn ProviderClassifier>>>);
    /// Optional in TS: the default implementation means "not provided".
    fn headers(&self) -> Option<IndexMap<String, String>> {
        None
    }
    fn set_headers(&self, value: Option<IndexMap<String, String>>);
    /// Optional in TS: the default implementation means "not provided".
    fn auth_header(&self) -> Option<bool> {
        None
    }
    fn set_auth_header(&self, value: Option<bool>);
    /// Optional in TS: the default implementation means "not provided".
    fn oauth(&self) -> Option<Arc<dyn ExtensionOAuthConfig>> {
        None
    }
    fn set_oauth(&self, value: Option<Arc<dyn ExtensionOAuthConfig>>);
    /// Optional in TS: the default implementation means "not provided".
    fn models(&self) -> Option<Vec<ProviderModelConfig>> {
        None
    }
    fn set_models(&self, value: Option<Vec<ProviderModelConfig>>);
    /// Optional in TS: the default implementation means "not provided".
    async fn refresh_models(
        &self,
        context: RefreshModelsContext,
    ) -> anyhow::Result<Vec<ProviderModelConfig>> {
        todo!("optional method not provided")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthStatusSource {
    #[serde(rename = "stored")]
    Stored,
    #[serde(rename = "runtime")]
    Runtime,
    #[serde(rename = "environment")]
    Environment,
    #[serde(rename = "fallback")]
    Fallback,
    #[serde(rename = "models_json_key")]
    ModelsJsonKey,
    #[serde(rename = "models_json_command")]
    ModelsJsonCommand,
}

impl AuthStatusSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Runtime => "runtime",
            Self::Environment => "environment",
            Self::Fallback => "fallback",
            Self::ModelsJsonKey => "models_json_key",
            Self::ModelsJsonCommand => "models_json_command",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<AuthStatusSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

pub static CLEAR_API_KEY_CACHE: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/provider-composer.ts:clearApiKeyCache"));

pub fn validate_extension_provider(
    provider_id: &str,
    base: Option<Arc<dyn Provider>>,
    models_config: Option<&ModelsJsonProvider>,
    extension: Arc<dyn ProviderConfigInput>,
) -> anyhow::Result<()> {
    todo!("port core/provider-composer.ts:validateExtensionProvider")
}

/// Compose built-in, models.json, and extension layers without reading credentials.
pub fn compose_model_provider(
    provider_id: &str,
    base: Option<Arc<dyn Provider>>,
    model_config: Arc<ModelConfig>,
    extension: Option<Arc<dyn ProviderConfigInput>>,
) -> anyhow::Result<Arc<dyn Provider>> {
    todo!("port core/provider-composer.ts:composeModelProvider")
}

pub fn resolve_configured_model_headers(
    model: AnyModel,
    config: Option<&ModelsJsonProvider>,
    extension: Option<Arc<dyn ProviderConfigInput>>,
    env: Option<IndexMap<String, String>>,
) -> Option<IndexMap<String, String>> {
    todo!("port core/provider-composer.ts:resolveConfiguredModelHeaders")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityRequestConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<ProviderHeaders>,
    pub auth_header: bool,
}

pub fn resolve_compatibility_request_config(
    model: AnyModel,
    config: Option<&ModelsJsonProvider>,
    extension: Option<Arc<dyn ProviderConfigInput>>,
) -> CompatibilityRequestConfig {
    todo!("port core/provider-composer.ts:resolveCompatibilityRequestConfig")
}

pub fn configured_request_auth_status(
    config: Option<&ModelsJsonProvider>,
    extension: Option<Arc<dyn ProviderConfigInput>>,
) -> Option<AuthStatus> {
    todo!("port core/provider-composer.ts:configuredRequestAuthStatus")
}
