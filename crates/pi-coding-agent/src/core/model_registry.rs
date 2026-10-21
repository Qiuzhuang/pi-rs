//! Port of `packages/coding-agent/src/core/model-registry.ts`.

use crate::core::model_runtime::ModelRuntime;
use crate::core::provider_composer::AuthStatus;
use crate::core::virtual_models::VirtualModelDefinition;
use indexmap::IndexMap;
use pi_ai::auth::types::AuthResult;
use pi_ai::models::ModelsRefreshOptions;
use pi_ai::models::ModelsRefreshResult;
use pi_ai::models::Provider;
use pi_ai::types::AssistantImages;
use pi_ai::types::AssistantMessage;
use pi_ai::types::ClassifierContext;
use pi_ai::types::ClassifierModel;
use pi_ai::types::ClassifierResult;
use pi_ai::types::Context;
use pi_ai::types::ImageModel;
use pi_ai::types::ImagesContext;
use pi_ai::types::Model;
use pi_ai::types::ProviderHeaders;
use pi_ai::utils::event_stream::AssistantMessageEventStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub use crate::core::provider_composer::CLEAR_API_KEY_CACHE;
pub use crate::core::provider_composer::ProviderConfigInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRequestAuthVariant1 {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<ProviderHeaders>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRequestAuthVariant2 {
    pub ok: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResolvedRequestAuth {
    ResolvedRequestAuthVariant1(ResolvedRequestAuthVariant1),
    ResolvedRequestAuthVariant2(ResolvedRequestAuthVariant2),
}

#[derive(Clone)]
pub enum ModelRegistryProviderOrName {
    Arc(Arc<dyn Provider>),
    String(String),
}

/// Synchronous compatibility facade exposed to extensions.
/// Coding-agent internals use ModelRuntime directly.
pub struct ModelRegistry {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ModelRegistry {
    pub fn new(runtime: Arc<ModelRuntime>) -> Self {
        todo!("port core/model-registry.ts:ModelRegistry.constructor")
    }

    /// Reload models.json asynchronously. Await before making synchronous registry reads.
    pub async fn refresh(
        &self,
        options: Option<ModelsRefreshOptions>,
    ) -> anyhow::Result<ModelsRefreshResult> {
        todo!("port core/model-registry.ts:ModelRegistry.refresh")
    }

    pub fn get_error(&self) -> Option<String> {
        todo!("port core/model-registry.ts:ModelRegistry.getError")
    }

    pub fn get_all(&self) -> Vec<Model> {
        todo!("port core/model-registry.ts:ModelRegistry.getAll")
    }

    pub fn get_available(&self) -> Vec<Model> {
        todo!("port core/model-registry.ts:ModelRegistry.getAvailable")
    }

    pub fn find(&self, provider: &str, model_id: &str) -> Option<Model> {
        todo!("port core/model-registry.ts:ModelRegistry.find")
    }

    /// Find a model of a non-chat type, e.g. `findOfType("classifier", "typesafe", "jev-latest")`.
    pub fn find_of_type(&self, r#type: Value, provider: &str, model_id: &str) -> Option<Value> {
        todo!("port core/model-registry.ts:ModelRegistry.findOfType")
    }

    pub fn has_configured_auth(&self, model: Model) -> bool {
        todo!("port core/model-registry.ts:ModelRegistry.hasConfiguredAuth")
    }

    pub async fn get_api_key_and_headers(
        &self,
        model: Model,
    ) -> anyhow::Result<ResolvedRequestAuth> {
        todo!("port core/model-registry.ts:ModelRegistry.getApiKeyAndHeaders")
    }

    pub fn get_provider_auth_status(&self, provider: &str) -> AuthStatus {
        todo!("port core/model-registry.ts:ModelRegistry.getProviderAuthStatus")
    }

    pub fn get_provider(&self, provider: &str) -> Option<Arc<dyn Provider>> {
        todo!("port core/model-registry.ts:ModelRegistry.getProvider")
    }

    /// Stream through the configured provider with request-time authentication.
    pub fn stream(
        &self,
        model: Model,
        context: Context,
        options: Option<Value /* TS: pi-ai::ModelsApiStreamOptions */>,
    ) -> AssistantMessageEventStream {
        todo!("port core/model-registry.ts:ModelRegistry.stream")
    }

    /// Stream with provider-neutral options and request-time authentication.
    pub fn stream_simple(
        &self,
        model: Model,
        context: Context,
        options: Option<Value /* TS: pi-ai::ModelsSimpleStreamOptions */>,
    ) -> AssistantMessageEventStream {
        todo!("port core/model-registry.ts:ModelRegistry.streamSimple")
    }

    pub async fn complete(
        &self,
        model: Model,
        context: Context,
        options: Option<Value /* TS: pi-ai::ModelsApiStreamOptions */>,
    ) -> anyhow::Result<AssistantMessage> {
        todo!("port core/model-registry.ts:ModelRegistry.complete")
    }

    /// Every known model of a type (chat, image, classifier), optionally for one provider.
    pub fn get_models_of_type(&self, r#type: Value, provider: Option<&str>) -> Vec<Value> {
        todo!("port core/model-registry.ts:ModelRegistry.getModelsOfType")
    }

    /// Models of a type whose provider has working credentials.
    pub async fn get_available_of_type(
        &self,
        r#type: Value,
        provider: Option<&str>,
        options: Option<Value /* TS: pi-ai::AuthOperationOptions */>,
    ) -> anyhow::Result<Vec<Value>> {
        todo!("port core/model-registry.ts:ModelRegistry.getAvailableOfType")
    }

    pub fn get_model_of_type(
        &self,
        r#type: Value,
        provider: &str,
        model_id: &str,
    ) -> Option<Value> {
        todo!("port core/model-registry.ts:ModelRegistry.getModelOfType")
    }

    /// Classify structured state with request-time authentication. Never rejects.
    pub async fn classify(
        &self,
        model: ClassifierModel,
        context: ClassifierContext,
        options: Option<Value /* TS: pi-ai::ModelsClassifierOptions */>,
    ) -> anyhow::Result<ClassifierResult> {
        todo!("port core/model-registry.ts:ModelRegistry.classify")
    }

    /// Generate images with request-time authentication. Never rejects.
    pub async fn generate_images(
        &self,
        model: ImageModel,
        context: ImagesContext,
        options: Option<Value /* TS: pi-ai::ModelsImagesOptions */>,
    ) -> anyhow::Result<AssistantImages> {
        todo!("port core/model-registry.ts:ModelRegistry.generateImages")
    }

    pub fn get_provider_display_name(&self, provider: &str) -> String {
        todo!("port core/model-registry.ts:ModelRegistry.getProviderDisplayName")
    }

    pub async fn get_provider_auth(&self, provider: &str) -> anyhow::Result<Option<AuthResult>> {
        todo!("port core/model-registry.ts:ModelRegistry.getProviderAuth")
    }

    pub async fn get_api_key_for_provider(&self, provider: &str) -> anyhow::Result<Option<String>> {
        todo!("port core/model-registry.ts:ModelRegistry.getApiKeyForProvider")
    }

    pub fn is_using_o_auth(&self, model: Model) -> bool {
        todo!("port core/model-registry.ts:ModelRegistry.isUsingOAuth")
    }

    pub fn register_provider(
        &self,
        provider_or_name: &ModelRegistryProviderOrName,
        config: Option<Arc<dyn ProviderConfigInput>>,
    ) -> anyhow::Result<()> {
        todo!("port core/model-registry.ts:ModelRegistry.registerProvider")
    }

    pub fn unregister_provider(&self, provider_name: &str) {
        todo!("port core/model-registry.ts:ModelRegistry.unregisterProvider")
    }

    pub fn register_virtual_model(&self, definition: Arc<dyn VirtualModelDefinition>) {
        todo!("port core/model-registry.ts:ModelRegistry.registerVirtualModel")
    }

    pub fn unregister_virtual_model(&self, provider_name: &str, id: &str) {
        todo!("port core/model-registry.ts:ModelRegistry.unregisterVirtualModel")
    }

    pub fn get_registered_provider_config(
        &self,
        provider_name: &str,
    ) -> Option<Arc<dyn ProviderConfigInput>> {
        todo!("port core/model-registry.ts:ModelRegistry.getRegisteredProviderConfig")
    }

    pub fn get_registered_native_provider(&self, provider_name: &str) -> Option<Arc<dyn Provider>> {
        todo!("port core/model-registry.ts:ModelRegistry.getRegisteredNativeProvider")
    }

    pub fn get_registered_provider_ids(&self) -> Vec<String> {
        todo!("port core/model-registry.ts:ModelRegistry.getRegisteredProviderIds")
    }
}
