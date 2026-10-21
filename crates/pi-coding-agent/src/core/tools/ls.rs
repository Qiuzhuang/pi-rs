//! Port of `packages/coding-agent/src/core/tools/ls.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::tools::truncate::TruncationResult;
use futures::future::BoxFuture;
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub static LS_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/ls.ts:lsToolSystemPromptContribution"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LsToolInputValue {
    /// Directory to list (default: current directory)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Maximum number of entries to return (default: 500)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

pub type LsToolInput = LsToolInputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LsToolDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<TruncationResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_limit_reached: Option<i64>,
}

#[derive(Clone)]
pub struct LsOperationsResult {
    pub is_directory: Arc<dyn Fn() -> bool + Send + Sync>,
}

/// Pluggable operations for the ls tool.
/// Override these to delegate directory listing to remote systems (for example SSH).
#[derive(Clone)]
pub struct LsOperations {
    /// Check if path exists
    pub exists: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<bool>> + Send + Sync>,
    /// Get file or directory stats. Throws if not found.
    pub stat:
        Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<LsOperationsResult>> + Send + Sync>,
    /// Read directory entries
    pub readdir:
        Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Vec<String>>> + Send + Sync>,
}

#[derive(Clone)]
pub struct LsToolOptions {
    /// Custom operations for directory listing. Default: local filesystem
    pub operations: Option<LsOperations>,
}

pub fn create_ls_tool_definition(
    cwd: &str,
    options: Option<&LsToolOptions>,
) -> Arc<dyn ToolDefinition> {
    todo!("port core/tools/ls.ts:createLsToolDefinition")
}

pub fn create_ls_tool(cwd: &str, options: Option<&LsToolOptions>) -> AgentToolRef {
    todo!("port core/tools/ls.ts:createLsTool")
}
