//! Port of `packages/coding-agent/src/core/extensions/wrapper.ts`.

use crate::core::extensions::runner::ExtensionRunner;
use crate::core::extensions::types::{ExtensionToolContext, RegisteredTool};
use crate::core::tools::tool_definition_wrapper::{ToolContextFactory, wrap_tool_definition};
use pi_agent::types::AgentToolRef;
use std::sync::Arc;

/// Wrap a RegisteredTool into an AgentTool.
/// Uses the runner's createToolContext() for consistent context across tools and event handlers.
pub fn wrap_registered_tool(
    registered_tool: &RegisteredTool,
    runner: Arc<ExtensionRunner>,
) -> AgentToolRef {
    let weak = Arc::downgrade(&runner);
    let factory: ToolContextFactory = Arc::new(move |tool_call_id: String, signal| match weak.upgrade() {
        Some(runner) => runner.create_tool_context(&tool_call_id, signal),
        // The runner was replaced: hand out a stale context whose accessors fail.
        None => ExtensionToolContext {
            ctx: crate::core::extensions::runner::stale_extension_context(None),
            tool_call_id,
            signal,
        },
    });
    wrap_tool_definition(&registered_tool.definition, Some(&factory))
}

/// Wrap all registered tools into AgentTools.
/// Uses the runner's createToolContext() for consistent context across tools and event handlers.
pub fn wrap_registered_tools(
    registered_tools: &[RegisteredTool],
    runner: Arc<ExtensionRunner>,
) -> Vec<AgentToolRef> {
    registered_tools.iter().map(|tool| wrap_registered_tool(tool, runner.clone())).collect()
}

