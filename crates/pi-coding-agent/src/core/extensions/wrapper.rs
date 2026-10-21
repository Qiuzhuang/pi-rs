//! Port of `packages/coding-agent/src/core/extensions/wrapper.ts`.

use crate::core::extensions::runner::ExtensionRunner;
use crate::core::extensions::types::RegisteredTool;
use pi_agent::types::AgentToolRef;
use std::sync::Arc;

/// Wrap a RegisteredTool into an AgentTool.
/// Uses the runner's createToolContext() for consistent context across tools and event handlers.
pub fn wrap_registered_tool(
    registered_tool: &RegisteredTool,
    runner: Arc<ExtensionRunner>,
) -> AgentToolRef {
    todo!("port core/extensions/wrapper.ts:wrapRegisteredTool")
}

/// Wrap all registered tools into AgentTools.
/// Uses the runner's createToolContext() for consistent context across tools and event handlers.
pub fn wrap_registered_tools(
    registered_tools: &[RegisteredTool],
    runner: Arc<ExtensionRunner>,
) -> Vec<AgentToolRef> {
    todo!("port core/extensions/wrapper.ts:wrapRegisteredTools")
}
