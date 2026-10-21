//! Port of `packages/coding-agent/src/core/tools/tool-definition-wrapper.ts`.

use crate::core::extensions::types::ExtensionToolContext;
use crate::core::extensions::types::ToolDefinition;
use pi_agent::types::AgentToolRef;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Creates the context for one tool call.
pub type ToolContextFactory =
    Arc<dyn Fn(String, Option<CancellationToken>) -> Arc<dyn ExtensionToolContext> + Send + Sync>;

/// Wrap a ToolDefinition into an AgentTool for the core runtime.
pub fn wrap_tool_definition(
    definition: Arc<dyn ToolDefinition>,
    ctx_factory: Option<&ToolContextFactory>,
) -> AgentToolRef {
    todo!("port core/tools/tool-definition-wrapper.ts:wrapToolDefinition")
}

/// Wrap multiple ToolDefinitions into AgentTools for the core runtime.
pub fn wrap_tool_definitions(
    definitions: Vec<Arc<dyn ToolDefinition>>,
    ctx_factory: Option<&ToolContextFactory>,
) -> Vec<AgentToolRef> {
    todo!("port core/tools/tool-definition-wrapper.ts:wrapToolDefinitions")
}

/// Synthesize a minimal ToolDefinition from an AgentTool.
///
/// This keeps AgentSession's internal registry definition-first even when a caller
/// provides plain AgentTool overrides that do not include prompt metadata or renderers.
pub fn create_tool_definition_from_agent_tool(tool: AgentToolRef) -> Arc<dyn ToolDefinition> {
    todo!("port core/tools/tool-definition-wrapper.ts:createToolDefinitionFromAgentTool")
}
