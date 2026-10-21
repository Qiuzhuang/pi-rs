//! Port of `packages/coding-agent/src/core/tools/renderers/read.ts`.

use crate::core::extensions::types::ToolRenderContext;
use crate::core::extensions::types::ToolRenderResultOptions;
use crate::modes::interactive::theme::theme::Theme;
use pi_agent::types::AgentToolResult;
use pi_tui::tui::ComponentRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompactReadClassificationKind {
    #[serde(rename = "docs")]
    Docs,
    #[serde(rename = "resource")]
    Resource,
    #[serde(rename = "skill")]
    Skill,
}

impl CompactReadClassificationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Docs => "docs",
            Self::Resource => "resource",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactReadClassification {
    pub kind: CompactReadClassificationKind,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadRenderArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "file_path", default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Value>,
}

#[derive(Clone)]
pub struct ReadRenderersValue {
    /// Custom rendering for tool call display
    pub render_call:
        Option<Arc<dyn Fn(Value, Arc<Theme>, ToolRenderContext) -> ComponentRef + Send + Sync>>,
    /// Custom rendering for tool result display
    pub render_result: Option<
        Arc<
            dyn Fn(
                    AgentToolResult,
                    ToolRenderResultOptions,
                    Arc<Theme>,
                    ToolRenderContext,
                ) -> ComponentRef
                + Send
                + Sync,
        >,
    >,
}

pub static READ_RENDERERS: LazyLock<ReadRenderersValue> =
    LazyLock::new(|| todo!("port core/tools/renderers/read.ts:readRenderers"));
