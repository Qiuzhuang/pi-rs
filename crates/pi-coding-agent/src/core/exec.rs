//! Port of `packages/coding-agent/src/core/exec.ts`.

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Options for executing shell commands.
#[derive(Debug, Clone)]
pub struct ExecOptions {
    /// AbortSignal to cancel the command
    pub signal: Option<CancellationToken>,
    /// Timeout in milliseconds
    pub timeout: Option<i64>,
    /// Working directory
    pub cwd: Option<String>,
}

/// Result of executing a shell command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i64,
    pub killed: bool,
}

/// Execute a shell command and return stdout/stderr/code.
/// Supports timeout and abort signal.
pub async fn exec_command(
    command: &str,
    args: &[String],
    cwd: &str,
    options: Option<&ExecOptions>,
) -> anyhow::Result<ExecResult> {
    todo!("port core/exec.ts:execCommand")
}
