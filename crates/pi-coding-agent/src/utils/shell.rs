//! Port of `packages/coding-agent/src/utils/shell.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShellConfigCommandTransport {
    #[serde(rename = "argv")]
    Argv,
    #[serde(rename = "stdin")]
    Stdin,
}

impl ShellConfigCommandTransport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Argv => "argv",
            Self::Stdin => "stdin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_transport: Option<ShellConfigCommandTransport>,
}

/// Resolve shell configuration based on platform and an optional explicit shell path.
/// Resolution order:
/// 1. User-specified shellPath
/// 2. On Windows: Git Bash in known locations, then bash on PATH
/// 3. On Unix: /bin/bash, then bash on PATH, then fallback to sh
pub fn get_shell_config(custom_shell_path: Option<&str>) -> anyhow::Result<ShellConfig> {
    todo!("port utils/shell.ts:getShellConfig")
}

pub const POWERSHELL_ARGS: &[&str] = &[
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-Command",
];

/// Resolve PowerShell on Windows, preferring PowerShell 7 when available.
pub fn get_power_shell_config() -> anyhow::Result<ShellConfig> {
    todo!("port utils/shell.ts:getPowerShellConfig")
}

/// Environment for spawned shells (TS `NodeJS.ProcessEnv`).
pub fn get_shell_env() -> indexmap::IndexMap<String, String> {
    todo!("port utils/shell.ts:getShellEnv")
}

/// Sanitize binary output for display/storage.
/// Removes characters that crash string-width or cause display issues:
/// - Control characters (except tab, newline, carriage return)
/// - Unicode interlinear annotation characters U+FFF9..U+FFFB (crash string-width due to a bug)
pub fn sanitize_binary_output(str: &str) -> String {
    todo!("port utils/shell.ts:sanitizeBinaryOutput")
}

pub fn track_detached_child_pid(pid: i64) {
    todo!("port utils/shell.ts:trackDetachedChildPid")
}

pub fn untrack_detached_child_pid(pid: i64) {
    todo!("port utils/shell.ts:untrackDetachedChildPid")
}

pub fn kill_tracked_detached_children() {
    todo!("port utils/shell.ts:killTrackedDetachedChildren")
}

/// Kill a process and all its children (cross-platform)
pub fn kill_process_tree(pid: i64) {
    todo!("port utils/shell.ts:killProcessTree")
}
