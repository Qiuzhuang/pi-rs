//! Port of `packages/coding-agent/src/core/tools/renderers/index.ts`.

pub mod bash;
pub mod edit;
pub mod find;
pub mod grep;
pub mod ls;
pub mod read;
pub mod write;

use crate::core::extensions::types::ToolRenderers;
use crate::core::tools::ToolName;
use indexmap::IndexMap;
use serde_json::Value;

/// Renderers for every built-in tool, keyed by tool name.
pub fn create_all_tool_renderers() -> IndexMap<ToolName, ToolRenderers> {
    todo!("port core/tools/renderers/index.ts:createAllToolRenderers")
}

#[derive(Clone)]
pub enum WithBuiltInRenderersResult {
    Value(Value),
    ToolRenderers(ToolRenderers),
}

/// Merge built-in renderers into a tool definition that does not supply its own.
///
/// `ToolExecutionComponent` used to do this lookup itself, which forced every presentation to import
/// the tool implementations. Callers do it now, so a process that renders can import renderers alone.
pub fn with_built_in_renderers(
    tool_name: &str,
    definition: Option<Value>,
) -> Option<WithBuiltInRenderersResult> {
    todo!("port core/tools/renderers/index.ts:withBuiltInRenderers")
}
