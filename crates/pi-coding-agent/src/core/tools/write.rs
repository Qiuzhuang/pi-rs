//! Port of `packages/coding-agent/src/core/tools/write.ts`.

use crate::core::extensions::types::ToolDefinition;
use futures::future::BoxFuture;
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub static WRITE_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/write.ts:writeToolSystemPromptContribution"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteToolInput {
    /// Path to the file to write (relative or absolute)
    pub path: String,
    /// Content to write to the file
    pub content: String,
}

/// Pluggable operations for the write tool.
/// Override these to delegate file writing to remote systems (for example SSH).
#[derive(Clone)]
pub struct WriteOperations {
    /// Write content to a file
    pub write_file:
        Arc<dyn Fn(String, String) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    /// Create directory recursively
    pub mkdir: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
}

#[derive(Clone)]
pub struct WriteToolOptions {
    /// Custom operations for file writing. Default: local filesystem
    pub operations: Option<WriteOperations>,
}

pub fn create_write_tool_definition(
    cwd: &str,
    options: Option<&WriteToolOptions>,
) -> anyhow::Result<ToolDefinition> {
    todo!("port core/tools/write.ts:createWriteToolDefinition")
}

pub fn create_write_tool(cwd: &str, options: Option<&WriteToolOptions>) -> AgentToolRef {
    todo!("port core/tools/write.ts:createWriteTool")
}
