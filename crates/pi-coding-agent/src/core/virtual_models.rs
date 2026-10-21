//! Port of `packages/coding-agent/src/core/virtual-models.ts`.
//!
//! Virtual models are catalog entries that route each request to a physical model.
//!
//! The selection (`model_change`, `agent.state.model`, `ctx.model`) may name a virtual model.
//! Everything below the routing step only sees physical models: providers stream them and
//! assistant messages record them. A virtual model never reaches a provider.
//!
//! Virtual models belong to a provider id but are not provider models. `ModelRuntime` keeps them
//! separately and adds them to the provider's catalog with `with_virtual_models()`, so any provider,
//! including one with physical models, can list several virtual models.

use crate::core::session_manager::SessionEntry;
use async_trait::async_trait;
use futures::future::BoxFuture;
use pi_agent::types::AgentMessage;
use pi_ai::api::lazy::error_stream;
use pi_ai::auth::types::{
    ApiKeyAuth, ApiKeyResolveInput, AuthResult, Credential, ModelAuth, ProviderAuth,
};
use pi_ai::models::{Provider, RefreshModelsContext};
use pi_ai::providers::model_schema::{
    MODEL_THINKING_LEVELS, ModelCost, ModelInputModality, ModelThinkingLevel, ThinkingLevelMap,
};
use pi_ai::types::{
    AnyModel, AssistantImages, AssistantMessage, AssistantMessageEventStream, ClassifierContext,
    ClassifierModel, ClassifierResult, DeferredHandle, ImageModel, ImagesContext, Message, Model,
    ModelType, ProviderHeaders, StopReason, StreamOptions, TranscriptContext,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// API id of virtual catalog entries. Requests for it fail unless routed first.
pub const VIRTUAL_MODEL_API: &str = "pi-virtual";

/// Custom entry type that stores router state on the session branch.
pub const VIRTUAL_MODEL_STATE_ENTRY: &str = "pi.virtual-model-state";

/// Data of a `pi.virtual-model-state` custom entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualModelStateData {
    pub provider: String,
    pub model_id: String,
    pub state: Value,
}

/// Why a request is being routed.
/// - `user`: first request after a message the user wrote (prompt, steering, or follow-up)
/// - `continuation`: any other request in the agent loop, e.g. after tool results or extension messages
/// - `retry`: automatic retry after a failed request, including after compaction for a context overflow
/// - `direct`: a request outside the agent loop, e.g. a compaction summary or an extension call
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelRouteReason {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "continuation")]
    Continuation,
    #[serde(rename = "retry")]
    Retry,
    #[serde(rename = "direct")]
    Direct,
}

impl ModelRouteReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Continuation => "continuation",
            Self::Retry => "retry",
            Self::Direct => "direct",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRouteRequestPrevious {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ModelThinkingLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRouteRequestFailed {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ModelThinkingLevel>,
    pub message: AssistantMessage,
}

#[derive(Debug, Clone)]
pub struct ModelRouteRequest {
    /// The selected virtual model.
    pub model: Model,
    /// The selected thinking level. Its meaning is up to the router.
    pub thinking_level: ModelThinkingLevel,
    pub reason: ModelRouteReason,
    /// Physical model and thinking level of the latest successful response in `messages`.
    pub previous: Option<ModelRouteRequestPrevious>,
    /// For `retry`: the failed request, which `messages` no longer contains. `message` carries its
    /// `stopReason` and `errorMessage`. Absent when the router itself failed.
    pub failed: Option<ModelRouteRequestFailed>,
    /// Router state last returned on this session branch. Undefined before the first state and for `direct` requests.
    pub state: Option<Value>,
    /// Conversation for this request, including system messages.
    pub messages: Vec<Message>,
    pub signal: Option<CancellationToken>,
}

/// Physical model and thinking level for one request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoute {
    pub model: Model,
    pub thinking_level: ModelThinkingLevel,
    /// New router state, stored on the session branch unless it is `request.state` itself. Return
    /// `request.state` or undefined to keep the current state. Must be JSON-serializable. Ignored for
    /// `direct` requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
}

/// Input types of a virtual model (`"text" | "image"`).
pub type VirtualModelDefinitionInput = ModelInputModality;

/// `route(request)` of a virtual model.
pub type VirtualModelRouteFn =
    Arc<dyn Fn(ModelRouteRequest) -> BoxFuture<'static, anyhow::Result<ModelRoute>> + Send + Sync>;

#[derive(Clone)]
pub struct VirtualModelDefinition {
    /// Provider the virtual model is listed under. May be a provider with physical models.
    pub provider: String,
    /// Model id. Must not be the id of a physical model of `provider`.
    pub id: String,
    pub name: String,
    /// Thinking levels offered for selection. Defaults to `["off"]`.
    pub thinking_levels: Option<Vec<ModelThinkingLevel>>,
    /// Limits shown before the first response. Afterwards, Pi uses the limits of the physical model
    /// that answered. Unset limits are unknown (0).
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    /// Input types accepted for selection. Defaults to text and images; routed models without image support get placeholders.
    pub input: Option<Vec<VirtualModelDefinitionInput>>,
    /// Pick the physical model, which must have credentials, and thinking level for one request.
    pub route: VirtualModelRouteFn,
}

impl VirtualModelDefinition {
    /// The definition without its router (`Omit<VirtualModelDefinition, "route">`).
    pub fn without_route(&self) -> CreateVirtualModelDefinition {
        CreateVirtualModelDefinition {
            name: self.name.clone(),
            input: self.input.clone(),
            provider: self.provider.clone(),
            id: self.id.clone(),
            thinking_levels: self.thinking_levels.clone(),
            context_window: self.context_window,
            max_tokens: self.max_tokens,
        }
    }
}

/// Anything that names an API (`{ api: string }`).
pub trait HasApi {
    fn api_name(&self) -> &str;
}

impl HasApi for str {
    fn api_name(&self) -> &str {
        self
    }
}
impl HasApi for String {
    fn api_name(&self) -> &str {
        self
    }
}
impl HasApi for Model {
    fn api_name(&self) -> &str {
        &self.api
    }
}
impl HasApi for ImageModel {
    fn api_name(&self) -> &str {
        &self.api
    }
}
impl HasApi for ClassifierModel {
    fn api_name(&self) -> &str {
        &self.api
    }
}
impl HasApi for AnyModel {
    fn api_name(&self) -> &str {
        self.api()
    }
}
impl HasApi for AssistantMessage {
    fn api_name(&self) -> &str {
        &self.api
    }
}

/// `{ api }` of a model or message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsVirtualModelModel {
    pub api: String,
}

impl HasApi for IsVirtualModelModel {
    fn api_name(&self) -> &str {
        &self.api
    }
}

/// Whether a model or message names a virtual model. Failed routing leaves the virtual model on its message.
pub fn is_virtual_model<M: HasApi + ?Sized>(model: &M) -> bool {
    model.api_name() == VIRTUAL_MODEL_API
}

fn is_successful_response(message: &AssistantMessage) -> bool {
    !matches!(message.stop_reason, StopReason::Error | StopReason::Aborted)
}

/// Latest successful response. Its model is physical: failed or aborted requests, including failed routing, are skipped.
pub fn find_latest_response(messages: &[AgentMessage]) -> Option<AssistantMessage> {
    messages.iter().rev().find_map(|message| match message {
        AgentMessage::Assistant(message) if is_successful_response(message) => Some(message.clone()),
        _ => None,
    })
}

/// [`find_latest_response`] over LLM messages.
pub fn find_latest_llm_response(messages: &[Message]) -> Option<AssistantMessage> {
    messages.iter().rev().find_map(|message| match message {
        Message::Assistant(message) if is_successful_response(message) => Some(message.clone()),
        _ => None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBranchSelectionResult {
    pub provider: String,
    pub model_id: String,
}

/// The model selection a session branch records. A virtual `model_change` holds until the next
/// `model_change`, because responses name the physical models it routed to. Otherwise the latest
/// physical response wins, as in sessions without virtual models. A virtual model that is no longer
/// registered does not hold, so the selection falls back to the physical model that answered last.
///
/// Only the last `model_change` can hold, so this looks up at most one model in the catalog.
pub fn get_branch_selection(
    branch: &[SessionEntry],
    get_model: Arc<dyn Fn(String, String) -> Option<Model> + Send + Sync>,
) -> Option<GetBranchSelectionResult> {
    for i in (0..branch.len()).rev() {
        match &branch[i] {
            SessionEntry::ModelChange(entry) => {
                return Some(GetBranchSelectionResult {
                    provider: entry.provider.clone(),
                    model_id: entry.model_id.clone(),
                });
            }
            SessionEntry::Message(entry) => {
                let AgentMessage::Assistant(message) = &entry.message else {
                    continue;
                };
                if is_virtual_model(message) {
                    continue;
                }
                let response = GetBranchSelectionResult {
                    provider: message.provider.clone(),
                    model_id: message.model.clone(),
                };
                let change = find_last_model_change(branch, i);
                if let Some(change) = change {
                    let model = get_model(change.provider.clone(), change.model_id.clone());
                    if model.is_some_and(|model| is_virtual_model(&model)) {
                        return Some(change);
                    }
                }
                return Some(response);
            }
            _ => {}
        }
    }
    None
}

fn find_last_model_change(branch: &[SessionEntry], before: usize) -> Option<GetBranchSelectionResult> {
    branch[..before].iter().rev().find_map(|entry| match entry {
        SessionEntry::ModelChange(entry) => Some(GetBranchSelectionResult {
            provider: entry.provider.clone(),
            model_id: entry.model_id.clone(),
        }),
        _ => None,
    })
}

/// Latest router state a session branch stores for a virtual model.
pub fn get_virtual_model_state(
    branch: &[SessionEntry],
    provider: &str,
    model_id: &str,
) -> Option<Value> {
    for entry in branch.iter().rev() {
        let SessionEntry::Custom(entry) = entry else {
            continue;
        };
        if entry.custom_type != VIRTUAL_MODEL_STATE_ENTRY {
            continue;
        }
        let Some(Value::Object(data)) = &entry.data else {
            continue;
        };
        if data.get("provider").and_then(Value::as_str) == Some(provider)
            && data.get("modelId").and_then(Value::as_str) == Some(model_id)
        {
            return data.get("state").cloned();
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVirtualModelDefinition {
    pub name: String,
    /// Input types accepted for selection. Defaults to text and images; routed models without image support get placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<VirtualModelDefinitionInput>>,
    /// Provider the virtual model is listed under. May be a provider with physical models.
    pub provider: String,
    /// Model id. Must not be the id of a physical model of `provider`.
    pub id: String,
    /// Thinking levels offered for selection. Defaults to `["off"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_levels: Option<Vec<ModelThinkingLevel>>,
    /// Limits shown before the first response. Afterwards, Pi uses the limits of the physical model
    /// that answered. Unset limits are unknown (0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

/// Build the catalog entry of a virtual model.
pub fn create_virtual_model(definition: &CreateVirtualModelDefinition) -> Model {
    let levels = definition
        .thinking_levels
        .clone()
        .unwrap_or_else(|| vec![ModelThinkingLevel::Off]);
    let mut thinking_level_map = ThinkingLevelMap::new();
    for level in MODEL_THINKING_LEVELS {
        thinking_level_map.insert(
            level,
            levels.contains(&level).then(|| level.as_str().to_string()),
        );
    }
    Model {
        id: definition.id.clone(),
        name: definition.name.clone(),
        api: VIRTUAL_MODEL_API.to_string(),
        provider: definition.provider.clone(),
        base_url: String::new(),
        reasoning: levels.iter().any(|level| *level != ModelThinkingLevel::Off),
        thinking_level_map: Some(thinking_level_map),
        input: definition
            .input
            .clone()
            .unwrap_or_else(|| vec![ModelInputModality::Text, ModelInputModality::Image]),
        cost: ModelCost::default(),
        context_window: definition.context_window.unwrap_or(0),
        max_tokens: definition.max_tokens.unwrap_or(0),
        headers: None,
        compat: None,
        prompt_cache: None,
        input_limits: None,
        sampling_params: None,
        sampling_params_by_thinking_level: None,
        model_type: None,
        extra: Default::default(),
    }
}

/// Stream for a virtual model that was not routed, e.g. `stream()` with API-specific options.
fn unrouted_stream(model: &Model) -> AssistantMessageEventStream {
    error_stream(
        model,
        anyhow::anyhow!(
            "Virtual model {}/{} must be routed before streaming",
            model.provider,
            model.id
        ),
    )
}

/// Api-key auth of a provider that only lists virtual models: always configured, no key.
struct VirtualModelAuth;

#[async_trait]
impl ApiKeyAuth for VirtualModelAuth {
    fn name(&self) -> &str {
        "Virtual model"
    }
    async fn resolve(&self, _input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        Ok(Some(AuthResult {
            auth: ModelAuth::default(),
            env: None,
            source: Some("virtual".to_string()),
        }))
    }
}

/// Keyless provider that lists only virtual models.
struct VirtualOnlyProvider {
    id: String,
    auth: ProviderAuth,
    virtual_models: Vec<Model>,
}

#[async_trait]
impl Provider for VirtualOnlyProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.id
    }
    fn auth(&self) -> &ProviderAuth {
        &self.auth
    }
    fn get_models(&self) -> Vec<Model> {
        self.virtual_models.clone()
    }
    fn stream(&self, model: &Model, _context: &TranscriptContext, _options: StreamOptions) -> AssistantMessageEventStream {
        unrouted_stream(model)
    }
    fn stream_simple(
        &self,
        model: &Model,
        _context: &TranscriptContext,
        _options: StreamOptions,
    ) -> AssistantMessageEventStream {
        unrouted_stream(model)
    }
}

/// A provider's catalog plus virtual models.
struct WithVirtualModels {
    inner: Arc<dyn Provider>,
    virtual_models: Vec<Model>,
    ids: HashSet<String>,
}

impl WithVirtualModels {
    fn physical_chat(&self, models: &[Model]) -> Vec<Model> {
        models
            .iter()
            .filter(|model| !is_virtual_model(*model) && !self.ids.contains(&model.id))
            .cloned()
            .collect()
    }

    fn physical_any(&self, models: &[AnyModel]) -> Vec<AnyModel> {
        models
            .iter()
            .filter(|model| {
                !is_virtual_model(*model)
                    && !(model.model_type() == ModelType::Chat && self.ids.contains(model.id()))
            })
            .cloned()
            .collect()
    }
}

#[async_trait]
impl Provider for WithVirtualModels {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn base_url(&self) -> Option<&str> {
        self.inner.base_url()
    }
    fn headers(&self) -> Option<&ProviderHeaders> {
        self.inner.headers()
    }
    fn auth(&self) -> &ProviderAuth {
        self.inner.auth()
    }
    fn get_models(&self) -> Vec<Model> {
        let mut models = self.physical_chat(&self.inner.get_models());
        models.extend(self.virtual_models.iter().cloned());
        models
    }
    fn get_all_models(&self) -> Vec<AnyModel> {
        let mut models = self.physical_any(&self.inner.get_all_models());
        models.extend(self.virtual_models.iter().cloned().map(AnyModel::Chat));
        models
    }
    fn supports_refresh(&self) -> bool {
        self.inner.supports_refresh()
    }
    async fn refresh_models(&self, context: RefreshModelsContext) -> anyhow::Result<()> {
        self.inner.refresh_models(context).await
    }
    fn filter_models(&self, models: &[Model], credential: Option<&Credential>) -> Option<Vec<Model>> {
        let real = self.physical_chat(models);
        let mut filtered = self
            .inner
            .filter_models(&real, credential)
            .unwrap_or(real);
        filtered.extend(models.iter().filter(|model| is_virtual_model(*model)).cloned());
        Some(filtered)
    }
    fn filter_all_models(&self, models: &[AnyModel], credential: Option<&Credential>) -> Option<Vec<AnyModel>> {
        let mut filtered = self
            .inner
            .filter_all_models(&self.physical_any(models), credential)?;
        filtered.extend(models.iter().filter(|model| is_virtual_model(*model)).cloned());
        Some(filtered)
    }
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        if is_virtual_model(model) {
            return unrouted_stream(model);
        }
        self.inner.stream(model, context, options)
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        if is_virtual_model(model) {
            return unrouted_stream(model);
        }
        self.inner.stream_simple(model, context, options)
    }
    fn supports_fetch_deferred(&self) -> bool {
        self.inner.supports_fetch_deferred()
    }
    fn fetch_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> AssistantMessageEventStream {
        self.inner.fetch_deferred(model, handle, options)
    }
    fn supports_cancel_deferred(&self) -> bool {
        self.inner.supports_cancel_deferred()
    }
    async fn cancel_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> anyhow::Result<()> {
        self.inner.cancel_deferred(model, handle, options).await
    }
    fn supports_images(&self) -> bool {
        self.inner.supports_images()
    }
    async fn generate_images(&self, model: &ImageModel, context: &ImagesContext, options: StreamOptions) -> AssistantImages {
        self.inner.generate_images(model, context, options).await
    }
    fn supports_classify(&self) -> bool {
        self.inner.supports_classify()
    }
    async fn classify(&self, model: &ClassifierModel, context: &ClassifierContext, options: StreamOptions) -> ClassifierResult {
        self.inner.classify(model, context, options).await
    }
}

/// Add virtual models to a provider's catalog. Without a provider, the result is a keyless provider
/// that only lists the virtual models. A virtual model hides a physical chat model with the same id,
/// which a catalog refresh can add after registration. Availability follows the provider's auth.
pub fn with_virtual_models(
    provider_id: &str,
    provider: Option<Arc<dyn Provider>>,
    virtual_models: &[Model],
) -> Arc<dyn Provider> {
    match provider {
        None => Arc::new(VirtualOnlyProvider {
            id: provider_id.to_string(),
            auth: ProviderAuth {
                api_key: Some(Arc::new(VirtualModelAuth)),
                oauth: None,
            },
            virtual_models: virtual_models.to_vec(),
        }),
        Some(inner) => Arc::new(WithVirtualModels {
            inner,
            ids: virtual_models.iter().map(|model| model.id.clone()).collect(),
            virtual_models: virtual_models.to_vec(),
        }),
    }
}
