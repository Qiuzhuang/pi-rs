//! Port of `packages/coding-agent/src/core/tools/powershell.ts`.

use crate::core::tools::bash::{
    BashOperations, BashSpawnContext, BashSpawnHook, BashToolDetails, BashToolInput, BashToolOptions,
    ShellToolConfig, create_local_shell_operations, create_shell_tool_definition,
};
use crate::core::tools::tool_definition_wrapper::wrap_tool_definition;
use crate::utils::shell::get_power_shell_config;
use serde_json::{Value, json};
use std::sync::{Arc, LazyLock};

const UTF8_OUTPUT_PREFIX: &str = "try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}\n";

const POWERSHELL_SNIPPET: &str = "Execute PowerShell commands";
const POWERSHELL_GUIDELINES: &[&str] =
    &["You can inspect PI_* environment variables for current model and session details."];

pub static POWERSHELL_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| json!({ "snippet": POWERSHELL_SNIPPET, "guidelines": POWERSHELL_GUIDELINES }));

pub type PowerShellOperations = BashOperations;

pub type PowerShellSpawnContext = BashSpawnContext;

pub type PowerShellSpawnHook = BashSpawnHook;

pub type PowerShellToolDetails = BashToolDetails;

pub type PowerShellToolInput = BashToolInput;

// TS: extends Pick<BashToolOptions, "operations" | "exposeSessionEnvironment" | "spawnHook"> (inherited fields inlined)
#[derive(Clone, Default)]
pub struct PowerShellToolOptions {
    /// Custom operations for command execution. Default: local shell
    pub operations: Option<BashOperations>,
    /// Expose current Pi session metadata as PI_* environment variables. Default: true
    pub expose_session_environment: Option<bool>,
    /// Hook to adjust command, cwd, or env before execution
    pub spawn_hook: Option<BashSpawnHook>,
}

pub fn create_local_power_shell_operations() -> PowerShellOperations {
    let operations = create_local_shell_operations("PowerShell", Arc::new(get_power_shell_config));
    BashOperations {
        exec: Arc::new(move |command, cwd, options| {
            (operations.exec)(format!("{UTF8_OUTPUT_PREFIX}{command}"), cwd, options)
        }),
    }
}

fn powershell_tool_config() -> ShellToolConfig {
    ShellToolConfig {
        name: "powershell".to_string(),
        label: "powershell".to_string(),
        shell_name: "PowerShell".to_string(),
        prompt: "PS>".to_string(),
        prompt_snippet: POWERSHELL_SNIPPET.to_string(),
        prompt_guidelines: Some(POWERSHELL_GUIDELINES.iter().map(|s| s.to_string()).collect()),
        temp_file_prefix: "pi-powershell".to_string(),
    }
}

pub fn create_power_shell_tool_definition(
    cwd: &str,
    options: Option<&PowerShellToolOptions>,
) -> crate::core::extensions::types::ToolDefinition {
    let bash_options = BashToolOptions {
        operations: Some(
            options
                .and_then(|o| o.operations.clone())
                .unwrap_or_else(create_local_power_shell_operations),
        ),
        command_prefix: None,
        shell_path: None,
        expose_session_environment: options.and_then(|o| o.expose_session_environment),
        spawn_hook: options.and_then(|o| o.spawn_hook.clone()),
    };
    create_shell_tool_definition(cwd, &powershell_tool_config(), Some(&bash_options))
}

pub fn create_power_shell_tool(
    cwd: &str,
    options: Option<&PowerShellToolOptions>,
) -> pi_agent::types::AgentToolRef {
    wrap_tool_definition(&create_power_shell_tool_definition(cwd, options), None)
}
