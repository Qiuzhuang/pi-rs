//! Port of `packages/coding-agent/src/core/virtual-models.ts`.

use crate::core::session_manager::SessionEntry;
use pi_agent::types::AgentMessage;
use pi_ai::models::Provider;
use pi_ai::providers::model_schema::ModelThinkingLevel;
use pi_ai::types::AssistantMessage;
use pi_ai::types::Message;
use pi_ai::types::Model;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VirtualModelDefinitionInput {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "image")]
    Image,
}

impl VirtualModelDefinitionInput {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}

#[async_trait::async_trait]
pub trait VirtualModelDefinition: Send + Sync {
    /// Provider the virtual model is listed under. May be a provider with physical models.
    fn provider(&self) -> String;
    fn set_provider(&self, value: String);
    /// Model id. Must not be the id of a physical model of `provider`.
    fn id(&self) -> String;
    fn set_id(&self, value: String);
    fn name(&self) -> String;
    fn set_name(&self, value: String);
    /// Thinking levels offered for selection. Defaults to `["off"]`.
    /// Optional in TS: the default implementation means "not provided".
    fn thinking_levels(&self) -> Option<Vec<ModelThinkingLevel>> {
        None
    }
    fn set_thinking_levels(&self, value: Option<Vec<ModelThinkingLevel>>);
    /// Limits shown before the first response. Afterwards, Pi uses the limits of the physical model
    /// that answered. Unset limits are unknown (0).
    /// Optional in TS: the default implementation means "not provided".
    fn context_window(&self) -> Option<u64> {
        None
    }
    fn set_context_window(&self, value: Option<u64>);
    /// Optional in TS: the default implementation means "not provided".
    fn max_tokens(&self) -> Option<u64> {
        None
    }
    fn set_max_tokens(&self, value: Option<u64>);
    /// Input types accepted for selection. Defaults to text and images; routed models without image support get placeholders.
    /// Optional in TS: the default implementation means "not provided".
    fn input(&self) -> Option<Vec<VirtualModelDefinitionInput>> {
        None
    }
    fn set_input(&self, value: Option<Vec<VirtualModelDefinitionInput>>);
    /// Pick the physical model, which must have credentials, and thinking level for one request.
    async fn route(&self, request: &ModelRouteRequest) -> anyhow::Result<ModelRoute>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsVirtualModelModel {
    pub api: String,
}

/// Whether a model or message names a virtual model. Failed routing leaves the virtual model on its message.
pub fn is_virtual_model(model: &IsVirtualModelModel) -> bool {
    todo!("port core/virtual-models.ts:isVirtualModel")
}

/// Latest successful response. Its model is physical: failed or aborted requests, including failed routing, are skipped.
pub fn find_latest_response(messages: &[AgentMessage]) -> Option<AssistantMessage> {
    todo!("port core/virtual-models.ts:findLatestResponse")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBranchSelectionGetBranchSelectionResult {
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
) -> Option<GetBranchSelectionGetBranchSelectionResult> {
    todo!("port core/virtual-models.ts:getBranchSelection")
}

/// Latest router state a session branch stores for a virtual model.
pub fn get_virtual_model_state(branch: &[SessionEntry], provider: &str, model_id: &str) -> Value {
    todo!("port core/virtual-models.ts:getVirtualModelState")
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
    todo!("port core/virtual-models.ts:createVirtualModel")
}

/// Add virtual models to a provider's catalog. Without a provider, the result is a keyless provider
/// that only lists the virtual models. A virtual model hides a physical chat model with the same id,
/// which a catalog refresh can add after registration. Availability follows the provider's auth.
pub fn with_virtual_models(
    provider_id: &str,
    provider: Option<Arc<dyn Provider>>,
    virtual_models: &[Model],
) -> Arc<dyn Provider> {
    todo!("port core/virtual-models.ts:withVirtualModels")
}
