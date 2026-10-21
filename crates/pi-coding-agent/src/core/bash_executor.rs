//! Port of `packages/coding-agent/src/core/bash-executor.ts`.

use crate::core::tools::bash::BashOperations;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct BashExecutorOptions {
    /// Callback for streaming output chunks (already sanitized)
    pub on_chunk: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// AbortSignal for cancellation
    pub signal: Option<CancellationToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashResult {
    /// Combined stdout + stderr output (sanitized, possibly truncated)
    pub output: String,
    /// Process exit code (undefined if killed/cancelled)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// Whether the command was cancelled via signal
    pub cancelled: bool,
    /// Whether the output was truncated
    pub truncated: bool,
    /// Path to temp file containing full output (if output exceeded truncation threshold)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

/// Execute a bash command using custom BashOperations.
/// Used for remote execution (SSH, containers, etc.).
pub async fn execute_bash_with_operations(
    command: &str,
    cwd: &str,
    operations: &BashOperations,
    options: Option<&BashExecutorOptions>,
) -> anyhow::Result<BashResult> {
    todo!("port core/bash-executor.ts:executeBashWithOperations")
}
