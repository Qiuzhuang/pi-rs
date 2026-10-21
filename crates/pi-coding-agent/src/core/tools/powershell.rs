//! Port of `packages/coding-agent/src/core/tools/powershell.ts`.

use crate::core::tools::bash::BashOperations;
use crate::core::tools::bash::BashSpawnContext;
use crate::core::tools::bash::BashSpawnHook;
use crate::core::tools::bash::BashToolDetails;
use crate::core::tools::bash::BashToolInput;
use serde_json::Value;
use std::sync::LazyLock;

pub static POWERSHELL_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/powershell.ts:powershellToolSystemPromptContribution"));

pub type PowerShellOperations = BashOperations;

pub type PowerShellSpawnContext = BashSpawnContext;

pub type PowerShellSpawnHook = BashSpawnHook;

pub type PowerShellToolDetails = BashToolDetails;

pub type PowerShellToolInput = BashToolInput;

// TS: extends Pick<BashToolOptions, "operations" | "exposeSessionEnvironment" | "spawnHook"> (inherited fields inlined)
#[derive(Clone)]
pub struct PowerShellToolOptions {
    /// Custom operations for command execution. Default: local shell
    pub operations: Option<BashOperations>,
    /// Expose current Pi session metadata as PI_* environment variables. Default: true
    pub expose_session_environment: Option<bool>,
    /// Hook to adjust command, cwd, or env before execution
    pub spawn_hook: Option<BashSpawnHook>,
}

pub fn create_local_power_shell_operations() -> PowerShellOperations {
    todo!("port core/tools/powershell.ts:createLocalPowerShellOperations")
}

pub fn create_power_shell_tool_definition(
    cwd: &str,
    options: Option<&PowerShellToolOptions>,
) -> Value {
    todo!("port core/tools/powershell.ts:createPowerShellToolDefinition")
}

pub fn create_power_shell_tool(cwd: &str, options: Option<&PowerShellToolOptions>) -> Value {
    todo!("port core/tools/powershell.ts:createPowerShellTool")
}
