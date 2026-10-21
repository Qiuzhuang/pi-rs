//! Port of `packages/coding-agent/src/core/tools/renderers/ls.ts`.

use crate::core::extensions::types::ToolRenderContext;
use crate::core::extensions::types::ToolRenderResultOptions;
use crate::modes::interactive::theme::theme::Theme;
use pi_agent::types::AgentToolResult;
use pi_tui::tui::ComponentRef;
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

#[derive(Clone)]
pub struct LsRenderersValue {
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

pub static LS_RENDERERS: LazyLock<LsRenderersValue> =
    LazyLock::new(|| todo!("port core/tools/renderers/ls.ts:lsRenderers"));
