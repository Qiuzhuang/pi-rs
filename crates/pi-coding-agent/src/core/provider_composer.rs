//! Port of `packages/coding-agent/src/core/provider-composer.ts`.

use crate::core::model_config::ModelConfig;
use crate::core::model_config::ModelsJsonProvider;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_ai::auth::extension_oauth_types::OAuthLoginCallbacks;
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

#[derive(Clone)]
pub struct ExtensionOAuthConfig {
    pub name: String,
    /// Whether access through this auth method is backed by a provider subscription.
    pub is_subscription: Option<bool>,
    /// @deprecated Retained for extension source compatibility; ignored by canonical auth flows.
    pub uses_callback_server: Option<bool>,
    pub login: Arc<
        dyn Fn(Arc<dyn OAuthLoginCallbacks>) -> BoxFuture<'static, anyhow::Result<OAuthCredentials>>
            + Send
            + Sync,
    >,
    pub refresh_token: Arc<
        dyn Fn(
                OAuthCredentials,
                CancellationToken,
            ) -> BoxFuture<'static, anyhow::Result<OAuthCredentials>>
            + Send
            + Sync,
    >,
    pub get_api_key: Arc<dyn Fn(OAuthCredentials) -> String + Send + Sync>,
    pub modify_models:
        Option<Arc<dyn Fn(Vec<Model>, OAuthCredentials) -> Vec<Model> + Send + Sync>>,
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
#[derive(Clone)]
pub struct ProviderConfigInput {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api: Option<Api>,
    pub stream_simple: Option<
        Arc<
            dyn Fn(
                    Model,
                    TranscriptContext,
                    Option<SimpleStreamOptions>,
                ) -> AssistantMessageEventStream
                + Send
                + Sync,
        >,
    >,
    pub images: Option<IndexMap<ImageApi, Arc<dyn ProviderImages>>>,
    pub classifiers: Option<IndexMap<ClassifierApi, Arc<dyn ProviderClassifier>>>,
    pub headers: Option<IndexMap<String, String>>,
    pub auth_header: Option<bool>,
    pub oauth: Option<ExtensionOAuthConfig>,
    pub models: Option<Vec<ProviderModelConfig>>,
    pub refresh_models: Option<
        Arc<
            dyn Fn(
                    RefreshModelsContext,
                ) -> BoxFuture<'static, anyhow::Result<Vec<ProviderModelConfig>>>
                + Send
                + Sync,
        >,
    >,
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
    extension: &ProviderConfigInput,
) -> anyhow::Result<()> {
    todo!("port core/provider-composer.ts:validateExtensionProvider")
}

/// Compose built-in, models.json, and extension layers without reading credentials.
pub fn compose_model_provider(
    provider_id: &str,
    base: Option<Arc<dyn Provider>>,
    model_config: Arc<ModelConfig>,
    extension: Option<&ProviderConfigInput>,
) -> anyhow::Result<Arc<dyn Provider>> {
    todo!("port core/provider-composer.ts:composeModelProvider")
}

pub fn resolve_configured_model_headers(
    model: AnyModel,
    config: Option<&ModelsJsonProvider>,
    extension: Option<&ProviderConfigInput>,
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
    extension: Option<&ProviderConfigInput>,
) -> CompatibilityRequestConfig {
    todo!("port core/provider-composer.ts:resolveCompatibilityRequestConfig")
}

pub fn configured_request_auth_status(
    config: Option<&ModelsJsonProvider>,
    extension: Option<&ProviderConfigInput>,
) -> Option<AuthStatus> {
    todo!("port core/provider-composer.ts:configuredRequestAuthStatus")
}
