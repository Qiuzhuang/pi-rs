//! Port of `packages/coding-agent/src/core/tools/find.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::tools::truncate::TruncationResult;
use futures::future::BoxFuture;
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

/// Relativize a find result against the search root and normalize it to posix separators.
pub fn relativize_find_result_path(
    result_path: &str,
    search_path: &str,
    path_module: Option<Value>,
) -> String {
    todo!("port core/tools/find.ts:relativizeFindResultPath")
}

pub static FIND_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/find.ts:findToolSystemPromptContribution"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindToolInputValue {
    /// Glob pattern to match files, e.g. '*.ts', '**/*.json', or 'src/**/*.spec.ts'
    pub pattern: String,
    /// Directory to search in (default: current directory)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Maximum number of results (default: 1000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

pub type FindToolInput = FindToolInputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindToolDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<TruncationResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_limit_reached: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindOperationsOptions {
    pub ignore: Vec<String>,
    pub limit: i64,
}

/// Pluggable operations for the find tool.
/// Override these to delegate file search to remote systems (for example SSH).
#[derive(Clone)]
pub struct FindOperations {
    /// Check if path exists
    pub exists: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<bool>> + Send + Sync>,
    /// Find files matching glob pattern. Returns relative or absolute paths.
    pub glob: Arc<
        dyn Fn(
                String,
                String,
                FindOperationsOptions,
            ) -> BoxFuture<'static, anyhow::Result<Vec<String>>>
            + Send
            + Sync,
    >,
}

#[derive(Clone)]
pub struct FindToolOptions {
    /// Custom operations for find. Default: local filesystem plus fd
    pub operations: Option<FindOperations>,
}

pub fn create_find_tool_definition(
    cwd: &str,
    options: Option<&FindToolOptions>,
) -> Arc<dyn ToolDefinition> {
    todo!("port core/tools/find.ts:createFindToolDefinition")
}

pub fn create_find_tool(cwd: &str, options: Option<&FindToolOptions>) -> AgentToolRef {
    todo!("port core/tools/find.ts:createFindTool")
}
