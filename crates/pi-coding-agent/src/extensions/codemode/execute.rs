//! Port of `packages/coding-agent/src/extensions/codemode/execute.ts`.

use crate::core::extensions::types::ExtensionToolContext;
use crate::core::session_manager::SessionEntry;
use crate::extensions::codemode::tool::CodemodeToolInput;
use crate::extensions::codemode::tool::CodemodeToolOptions;
use indexmap::IndexMap;
use pi_agent::types::AgentToolResult;
use pi_ai::types::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// A JSON value from a script, read field by field: values that are not objects have no fields.
pub type ScriptValue = Option<IndexMap<String, Value>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelCallResultStopReason {
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "aborted")]
    Aborted,
}

impl ModelCallResultStopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Error => "error",
            Self::Aborted => "aborted",
        }
    }
}

/// The fields of `ClassifierResult` and `AssistantImages` that a nested call row reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCallResult {
    pub stop_reason: ModelCallResultStopReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Values of `load()`: the `codemode-store` entries on the branch, applied from the root.
pub fn read_codemode_store(branch: &[SessionEntry]) -> IndexMap<String, Value> {
    todo!("port extensions/codemode/execute.ts:readCodemodeStore")
}

/// Run one script. Without a session context (a plain Agent or a direct call) scripts cannot call
/// tools, `store()` starts empty, and writes are dropped.
pub async fn execute_codemode(
    tool_call_id: &str,
    input: &CodemodeToolInput,
    signal: Option<CancellationToken>,
    on_update: Option<Arc<dyn Fn(AgentToolResult) + Send + Sync>>,
    ctx: Option<Arc<dyn ExtensionToolContext>>,
    options: Option<&CodemodeToolOptions>,
) -> anyhow::Result<AgentToolResult> {
    todo!("port extensions/codemode/execute.ts:executeCodemode")
}
