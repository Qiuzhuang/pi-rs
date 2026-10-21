//! Port of `packages/coding-agent/src/core/model-runtime.ts`.

use crate::compat::AuthOperationOptions;
use crate::core::model_config::ModelConfig;
use crate::core::provider_composer::AuthStatus;
use crate::core::provider_composer::CompatibilityRequestConfig;
use crate::core::provider_composer::ProviderConfigInput;
use crate::core::runtime_credentials::RuntimeCredentials;
use crate::core::virtual_models::ModelRoute;
use crate::core::virtual_models::ModelRouteReason;
use crate::core::virtual_models::VirtualModelDefinition;
use indexmap::{IndexMap, IndexSet};
use pi_ai::auth::types::AuthCheck;
use pi_ai::auth::types::AuthInteraction;
use pi_ai::auth::types::AuthResult;
use pi_ai::auth::types::AuthType;
use pi_ai::auth::types::Credential;
use pi_ai::auth::types::CredentialInfo;
use pi_ai::auth::types::CredentialStore;
use pi_ai::auth::types::LoginOptions;
use pi_ai::models::ModelsRefreshOptions;
use pi_ai::models::ModelsRefreshResult;
use pi_ai::models::ModelsRequestOptions;
use pi_ai::models::Provider;
use pi_ai::models_store::ModelsStore;
use pi_ai::providers::model_schema::ModelThinkingLevel;
use pi_ai::types::AnyModel;
use pi_ai::types::AssistantImages;
use pi_ai::types::AssistantMessage;
use pi_ai::types::ClassifierContext;
use pi_ai::types::ClassifierModel;
use pi_ai::types::ClassifierResult;
use pi_ai::types::Context;
use pi_ai::types::DeferredHandle;
use pi_ai::types::ImageModel;
use pi_ai::types::ImagesContext;
use pi_ai::types::Message;
use pi_ai::types::Model;
use pi_ai::utils::event_stream::AssistantMessageEventStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredVirtualModel {
    pub model: Model,
    pub route: ModelRoute,
}

#[derive(Debug, Clone)]
pub struct ModelRuntimeSnapshot {
    pub all: Vec<Model>,
    pub available: Vec<Model>,
    pub configured_providers: IndexSet<String>,
    pub stored_providers: IndexSet<String>,
    pub auth: IndexMap<String, AuthCheck>,
}

#[derive(Clone)]
pub struct CreateModelRuntimeOptions {
    /// Credential storage. Defaults to the file at authPath.
    pub credentials: Option<Arc<dyn CredentialStore>>,
    pub auth_path: Option<String>,
    pub models_path: Option<String>,
    pub models_store: Option<Arc<dyn ModelsStore>>,
    pub models_store_path: Option<String>,
    /// Allow create() to refresh model catalogs over the network. Defaults to false.
    pub allow_model_network: Option<bool>,
    /// Timeout for the create-time network model refresh.
    pub model_refresh_timeout_ms: Option<i64>,
    pub catalog_base_url: Option<String>,
    /// Optional caller cancellation for initial cache restoration and availability checks.
    pub signal: Option<CancellationToken>,
    /// Skip initial catalog and availability refresh. Static models remain available.
    pub refresh_on_create: Option<bool>,
}

// TS: extends AuthOperationOptions (inherited fields inlined)
#[derive(Debug, Clone)]
pub struct ModelRuntimeAuthOverrides {
    pub signal: Option<CancellationToken>,
    pub api_key: Option<String>,
    pub env: Option<IndexMap<String, String>>,
    /// Require this much remaining OAuth-token validity; defaults to five minutes.
    pub min_o_auth_validity_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CredentialSynchronizationOperation {
    #[serde(rename = "login")]
    Login,
    #[serde(rename = "logout")]
    Logout,
    #[serde(rename = "setRuntimeApiKey")]
    SetRuntimeApiKey,
    #[serde(rename = "removeRuntimeApiKey")]
    RemoveRuntimeApiKey,
}

impl CredentialSynchronizationOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Logout => "logout",
            Self::SetRuntimeApiKey => "setRuntimeApiKey",
            Self::RemoveRuntimeApiKey => "removeRuntimeApiKey",
        }
    }
}

/// Credentials changed successfully, but the local model/auth snapshot could not be synchronized.
/// TS: `extends Error`
#[derive(Debug, Clone)]
pub struct CredentialSynchronizationError {
    pub message: String,
}

impl std::fmt::Display for CredentialSynchronizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CredentialSynchronizationError {}

impl CredentialSynchronizationError {
    pub fn new(
        provider_id: &str,
        operation: &CredentialSynchronizationOperation,
        credential: Option<Credential>,
        options: Value, /* TS: ErrorOptions */
    ) -> Self {
        todo!("port core/model-runtime.ts:CredentialSynchronizationError.constructor")
    }

    pub fn provider_id(&self) -> String {
        todo!("port core/model-runtime.ts:CredentialSynchronizationError.providerId")
    }

    pub fn operation(&self) -> CredentialSynchronizationOperation {
        todo!("port core/model-runtime.ts:CredentialSynchronizationError.operation")
    }

    pub fn credential(&self) -> Option<Credential> {
        todo!("port core/model-runtime.ts:CredentialSynchronizationError.credential")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelRuntimeGetAuthProviderOrModel {
    String(String),
    AnyModel(AnyModel),
}

#[derive(Debug, Clone)]
pub struct ModelRuntimeResolveModelOptions {
    pub reason: ModelRouteReason,
    pub thinking_level: ModelThinkingLevel,
    pub signal: Option<CancellationToken>,
    pub failed: Option<AssistantMessage>,
    pub state: Option<Value>,
}

/// Configured pi-ai Models collection used by coding-agent and SDK consumers.
/// TS: `implements Models`
pub struct ModelRuntime {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ModelRuntime {
    pub fn new(
        credentials: Arc<RuntimeCredentials>,
        config: Arc<ModelConfig>,
        models_path: Option<&str>,
        models_store: Arc<dyn ModelsStore>,
        providers: Vec<Arc<dyn Provider>>,
        model_network_enabled: bool,
    ) -> Self {
        todo!("port core/model-runtime.ts:ModelRuntime.constructor")
    }

    pub async fn create(
        options: Option<&CreateModelRuntimeOptions>,
    ) -> anyhow::Result<Arc<ModelRuntime>> {
        todo!("port core/model-runtime.ts:ModelRuntime.create")
    }

    pub fn get_providers(&self) -> Vec<Arc<dyn Provider>> {
        todo!("port core/model-runtime.ts:ModelRuntime.getProviders")
    }

    pub fn get_provider(&self, provider_id: &str) -> Option<Arc<dyn Provider>> {
        todo!("port core/model-runtime.ts:ModelRuntime.getProvider")
    }

    pub fn get_models(&self, provider_id: Option<&str>) -> Vec<Model> {
        todo!("port core/model-runtime.ts:ModelRuntime.getModels")
    }

    pub fn get_model(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        todo!("port core/model-runtime.ts:ModelRuntime.getModel")
    }

    pub fn get_models_of_type(&self, r#type: Value, provider_id: Option<&str>) -> Vec<Value> {
        todo!("port core/model-runtime.ts:ModelRuntime.getModelsOfType")
    }

    pub fn get_model_of_type(
        &self,
        r#type: Value,
        provider_id: &str,
        model_id: &str,
    ) -> Option<Value> {
        todo!("port core/model-runtime.ts:ModelRuntime.getModelOfType")
    }

    pub fn get_all_models(&self, provider_id: Option<&str>) -> Vec<AnyModel> {
        todo!("port core/model-runtime.ts:ModelRuntime.getAllModels")
    }

    pub async fn get_available_of_type(
        &self,
        r#type: Value,
        provider_id: Option<&str>,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Vec<Value>> {
        todo!("port core/model-runtime.ts:ModelRuntime.getAvailableOfType")
    }

    pub async fn get_all_available(
        &self,
        provider_id: Option<&str>,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Vec<AnyModel>> {
        todo!("port core/model-runtime.ts:ModelRuntime.getAllAvailable")
    }

    pub async fn check_auth(
        &self,
        provider_id: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Option<AuthCheck>> {
        todo!("port core/model-runtime.ts:ModelRuntime.checkAuth")
    }

    pub async fn get_available(
        &self,
        provider_id: Option<&str>,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Vec<Model>> {
        todo!("port core/model-runtime.ts:ModelRuntime.getAvailable")
    }

    pub fn get_available_snapshot(&self) -> Vec<Model> {
        todo!("port core/model-runtime.ts:ModelRuntime.getAvailableSnapshot")
    }

    pub fn get_error(&self) -> Option<String> {
        todo!("port core/model-runtime.ts:ModelRuntime.getError")
    }

    pub fn get_registered_provider_config(&self, provider_id: &str) -> Option<ProviderConfigInput> {
        todo!("port core/model-runtime.ts:ModelRuntime.getRegisteredProviderConfig")
    }

    pub fn get_registered_provider_ids(&self) -> Vec<String> {
        todo!("port core/model-runtime.ts:ModelRuntime.getRegisteredProviderIds")
    }

    pub fn get_registered_native_provider(&self, provider_id: &str) -> Option<Arc<dyn Provider>> {
        todo!("port core/model-runtime.ts:ModelRuntime.getRegisteredNativeProvider")
    }

    /// @internal Compatibility fallback for ModelRegistry when provider auth is unconfigured.
    pub fn get_compatibility_request_config(&self, model: Model) -> CompatibilityRequestConfig {
        todo!("port core/model-runtime.ts:ModelRuntime.getCompatibilityRequestConfig")
    }

    pub fn is_using_o_auth(&self, provider_id: &str) -> bool {
        todo!("port core/model-runtime.ts:ModelRuntime.isUsingOAuth")
    }

    pub fn is_using_subscription(&self, provider_id: &str) -> bool {
        todo!("port core/model-runtime.ts:ModelRuntime.isUsingSubscription")
    }

    pub fn has_configured_auth(&self, provider_id: &str) -> bool {
        todo!("port core/model-runtime.ts:ModelRuntime.hasConfiguredAuth")
    }

    pub async fn get_auth(
        &self,
        provider_or_model: &ModelRuntimeGetAuthProviderOrModel,
        overrides: Option<&ModelRuntimeAuthOverrides>,
    ) -> anyhow::Result<Option<AuthResult>> {
        todo!("port core/model-runtime.ts:ModelRuntime.getAuth")
    }

    pub async fn set_runtime_api_key(
        &self,
        provider_id: &str,
        api_key: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/model-runtime.ts:ModelRuntime.setRuntimeApiKey")
    }

    pub async fn remove_runtime_api_key(
        &self,
        provider_id: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/model-runtime.ts:ModelRuntime.removeRuntimeApiKey")
    }

    pub async fn list_credentials(
        &self,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<Vec<CredentialInfo>> {
        todo!("port core/model-runtime.ts:ModelRuntime.listCredentials")
    }

    pub fn get_provider_auth_status(&self, provider_id: &str) -> AuthStatus {
        todo!("port core/model-runtime.ts:ModelRuntime.getProviderAuthStatus")
    }

    pub fn stream(
        &self,
        model: Model,
        context: Context,
        options: Option<ModelsRequestOptions>,
    ) -> AssistantMessageEventStream {
        todo!("port core/model-runtime.ts:ModelRuntime.stream")
    }

    pub async fn complete(
        &self,
        model: Model,
        context: Context,
        options: Option<ModelsRequestOptions>,
    ) -> anyhow::Result<AssistantMessage> {
        todo!("port core/model-runtime.ts:ModelRuntime.complete")
    }

    pub fn stream_simple(
        &self,
        model: Model,
        context: Context,
        options: Option<ModelsRequestOptions>,
    ) -> AssistantMessageEventStream {
        todo!("port core/model-runtime.ts:ModelRuntime.streamSimple")
    }

    pub async fn complete_simple(
        &self,
        model: Model,
        context: Context,
        options: Option<ModelsRequestOptions>,
    ) -> anyhow::Result<AssistantMessage> {
        todo!("port core/model-runtime.ts:ModelRuntime.completeSimple")
    }

    pub fn stream_deferred(
        &self,
        model: Model,
        handle: DeferredHandle,
        options: Option<ModelsRequestOptions>,
    ) -> anyhow::Result<AssistantMessageEventStream> {
        todo!("port core/model-runtime.ts:ModelRuntime.streamDeferred")
    }

    pub async fn fetch_deferred(
        &self,
        model: Model,
        handle: DeferredHandle,
        options: Option<ModelsRequestOptions>,
    ) -> anyhow::Result<AssistantMessage> {
        todo!("port core/model-runtime.ts:ModelRuntime.fetchDeferred")
    }

    pub async fn cancel_deferred(
        &self,
        model: Model,
        handle: DeferredHandle,
        options: Option<ModelsRequestOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/model-runtime.ts:ModelRuntime.cancelDeferred")
    }

    pub async fn generate_images(
        &self,
        model: ImageModel,
        context: ImagesContext,
        options: Option<ModelsRequestOptions>,
    ) -> anyhow::Result<AssistantImages> {
        todo!("port core/model-runtime.ts:ModelRuntime.generateImages")
    }

    pub async fn classify(
        &self,
        model: ClassifierModel,
        context: ClassifierContext,
        options: Option<ModelsRequestOptions>,
    ) -> anyhow::Result<ClassifierResult> {
        todo!("port core/model-runtime.ts:ModelRuntime.classify")
    }

    pub async fn login(
        &self,
        provider_id: &str,
        r#type: AuthType,
        interaction: Arc<dyn AuthInteraction>,
        options: Option<LoginOptions>,
    ) -> anyhow::Result<Credential> {
        todo!("port core/model-runtime.ts:ModelRuntime.login")
    }

    pub async fn logout(
        &self,
        provider_id: &str,
        options: Option<&AuthOperationOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/model-runtime.ts:ModelRuntime.logout")
    }

    pub async fn refresh(
        &self,
        options: Option<ModelsRefreshOptions>,
    ) -> anyhow::Result<ModelsRefreshResult> {
        todo!("port core/model-runtime.ts:ModelRuntime.refresh")
    }

    pub fn register_native_provider(&self, provider: Arc<dyn Provider>) -> anyhow::Result<()> {
        todo!("port core/model-runtime.ts:ModelRuntime.registerNativeProvider")
    }

    pub fn register_provider(
        &self,
        provider_id: &str,
        config: &ProviderConfigInput,
    ) -> anyhow::Result<()> {
        todo!("port core/model-runtime.ts:ModelRuntime.registerProvider")
    }

    pub fn unregister_provider(&self, provider_id: &str) {
        todo!("port core/model-runtime.ts:ModelRuntime.unregisterProvider")
    }

    /// Register a virtual model under `definition.provider`, which may also list physical models or
    /// several virtual models. Re-registering the same provider and id replaces the virtual model.
    /// Throws when the id belongs to a physical model of that provider.
    pub fn register_virtual_model(
        &self,
        definition: &VirtualModelDefinition,
    ) -> anyhow::Result<()> {
        todo!("port core/model-runtime.ts:ModelRuntime.registerVirtualModel")
    }

    pub fn unregister_virtual_model(&self, provider_id: &str, id: &str) {
        todo!("port core/model-runtime.ts:ModelRuntime.unregisterVirtualModel")
    }

    /// Ask a virtual model's router for the model and thinking level of one request. The router must
    /// return a physical catalog model whose provider has credentials; the thinking level is clamped
    /// to that model. Throws when routing fails.
    ///
    /// `previous` reports the latest successful response in `messages`. A retry passes the failed
    /// response as `options.failed`; `messages` no longer contains it. `options.state` is the router
    /// state stored by the caller, which also stores the returned state.
    pub async fn resolve_model(
        &self,
        model: Model,
        messages: &[Message],
        options: &ModelRuntimeResolveModelOptions,
    ) -> anyhow::Result<ModelRoute> {
        todo!("port core/model-runtime.ts:ModelRuntime.resolveModel")
    }

    /// A catalog chat model that is not virtual.
    pub fn get_physical_model(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        todo!("port core/model-runtime.ts:ModelRuntime.getPhysicalModel")
    }
}

// TODO(port): impl pi-ai::Models for ModelRuntime
