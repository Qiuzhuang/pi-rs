//! Port of `packages/coding-agent/src/core/tools/bash.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::tools::truncate::TruncationResult;
use crate::utils::shell::ShellConfig;
use futures::future::BoxFuture;
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio_util::sync::CancellationToken;

pub static BASH_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/bash.ts:bashToolSystemPromptContribution"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashToolInputValue {
    /// Shell command to execute
    pub command: String,
    /// Timeout in seconds (optional, no default timeout)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,
}

pub type BashToolInput = BashToolInputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashToolOutputValue {
    /// Combined stdout and stderr, possibly truncated
    pub output: String,
    pub truncated: bool,
    /// Full output, when truncated
    #[serde(
        rename = "full_output_path",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub full_output_path: Option<String>,
    #[serde(rename = "exit_code")]
    pub exit_code: i64,
    #[serde(rename = "wall_time_seconds")]
    pub wall_time_seconds: i64,
}

pub type BashToolOutput = BashToolOutputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashToolDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<TruncationResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

#[derive(Clone)]
pub struct BashOperationsOptions {
    pub on_data: Arc<dyn Fn(Vec<u8>) + Send + Sync>,
    pub signal: Option<CancellationToken>,
    pub timeout: Option<i64>,
    pub env: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashOperationsResult {
    #[serde(default)]
    pub exit_code: Option<i64>,
}

/// Pluggable operations for the bash tool.
/// Override these to delegate command execution to remote systems (for example SSH).
#[derive(Clone)]
pub struct BashOperations {
    /// Execute a command and stream output.
    /// @param command The command to execute
    /// @param cwd Working directory
    /// @param options Execution options
    /// @returns Promise resolving to the exit code. Report signal terminations as 128 + signal number;
    /// a null exit code is treated as a failed command.
    pub exec: Arc<
        dyn Fn(
                String,
                String,
                BashOperationsOptions,
            ) -> BoxFuture<'static, anyhow::Result<BashOperationsResult>>
            + Send
            + Sync,
    >,
}

/// Shared process execution used by the built-in shell tools.
pub fn create_local_shell_operations(
    shell_name: &str,
    resolve_shell_config: Arc<dyn Fn() -> ShellConfig + Send + Sync>,
) -> anyhow::Result<BashOperations> {
    todo!("port core/tools/bash.ts:createLocalShellOperations")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLocalBashOperationsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_path: Option<String>,
}

/// Create bash operations using pi's built-in local shell execution backend.
///
/// This is useful for extensions that intercept user_bash and still want pi's
/// standard local shell behavior while wrapping or rewriting commands.
pub fn create_local_bash_operations(
    options: Option<&CreateLocalBashOperationsOptions>,
) -> BashOperations {
    todo!("port core/tools/bash.ts:createLocalBashOperations")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashSpawnContext {
    pub command: String,
    pub cwd: String,
    pub env: Value,
}

pub type BashSpawnHook = Arc<dyn Fn(BashSpawnContext) -> BashSpawnContext + Send + Sync>;

#[derive(Clone)]
pub struct BashToolOptions {
    /// Custom operations for command execution. Default: local shell
    pub operations: Option<BashOperations>,
    /// Command prefix prepended to every command (for example shell setup commands)
    pub command_prefix: Option<String>,
    /// Optional explicit shell path from settings
    pub shell_path: Option<String>,
    /// Expose current Pi session metadata as PI_* environment variables. Default: true
    pub expose_session_environment: Option<bool>,
    /// Hook to adjust command, cwd, or env before execution
    pub spawn_hook: Option<BashSpawnHook>,
}

#[derive(Debug, Clone)]
pub struct BashRenderState {
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub interval: Option<Arc<tokio::task::JoinHandle<()>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellToolConfig {
    pub name: String,
    pub label: String,
    pub shell_name: String,
    pub prompt: String,
    pub prompt_snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_guidelines: Option<Vec<String>>,
    pub temp_file_prefix: String,
}

pub fn create_shell_tool_definition(
    cwd: &str,
    config: &ShellToolConfig,
    options: Option<&BashToolOptions>,
) -> anyhow::Result<Arc<dyn ToolDefinition>> {
    todo!("port core/tools/bash.ts:createShellToolDefinition")
}

pub fn create_bash_tool_definition(
    cwd: &str,
    options: Option<&BashToolOptions>,
) -> Arc<dyn ToolDefinition> {
    todo!("port core/tools/bash.ts:createBashToolDefinition")
}

pub fn create_bash_tool(cwd: &str, options: Option<&BashToolOptions>) -> AgentToolRef {
    todo!("port core/tools/bash.ts:createBashTool")
}
