//! Port of `packages/coding-agent/src/core/tools/grep.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::tools::truncate::TruncationResult;
use futures::future::BoxFuture;
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub static GREP_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/grep.ts:grepToolSystemPromptContribution"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepToolInput {
    /// Search pattern (regex or literal string)
    pub pattern: String,
    /// Directory or file to search (default: current directory)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Filter files by glob pattern, e.g. '*.ts' or '**/*.spec.ts'
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    /// Case-insensitive search (default: false)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_case: Option<bool>,
    /// Treat pattern as literal string instead of regex (default: false)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<bool>,
    /// Number of lines to show before and after each match (default: 0)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub context: Option<f64>,
    /// Maximum number of matches to return (default: 100)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepToolDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<TruncationResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_limit_reached: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines_truncated: Option<bool>,
}

/// Pluggable operations for the grep tool.
/// Override these to delegate search to remote systems (for example SSH).
#[derive(Clone)]
pub struct GrepOperations {
    /// Check if path is a directory. Throws if path does not exist.
    pub is_directory: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<bool>> + Send + Sync>,
    /// Read file contents for context lines
    pub read_file: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<String>> + Send + Sync>,
}

#[derive(Clone)]
pub struct GrepToolOptions {
    /// Custom operations for grep. Default: local filesystem plus ripgrep
    pub operations: Option<GrepOperations>,
}

pub fn create_grep_tool_definition(cwd: &str, options: Option<&GrepToolOptions>) -> ToolDefinition {
    todo!("port core/tools/grep.ts:createGrepToolDefinition")
}

pub fn create_grep_tool(cwd: &str, options: Option<&GrepToolOptions>) -> AgentToolRef {
    todo!("port core/tools/grep.ts:createGrepTool")
}
