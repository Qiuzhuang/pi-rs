//! Port of `packages/coding-agent/src/core/tools/renderers/bash.ts`.

use crate::core::extensions::types::ToolRenderContext;
use crate::core::extensions::types::ToolRenderResultOptions;
use crate::modes::interactive::theme::theme::Theme;
use pi_agent::types::AgentToolResult;
use pi_tui::tui::ComponentRef;
use serde_json::Value;
use std::sync::Arc;

pub const BASH_UPDATE_THROTTLE_MS: i64 = 100;

#[derive(Clone)]
pub struct EmptyCreateShellRenderersResult {
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

/// Shell renderers are shared by bash and powershell, which differ only in the prompt they display.
pub fn create_shell_renderers(prompt: &str) -> EmptyCreateShellRenderersResult {
    todo!("port core/tools/renderers/bash.ts:createShellRenderers")
}
