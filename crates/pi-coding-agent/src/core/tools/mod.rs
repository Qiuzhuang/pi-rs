//! Port of `packages/coding-agent/src/core/tools/index.ts`.

pub mod bash;
pub mod edit;
pub mod edit_diff;
pub mod file_mutation_queue;
pub mod find;
pub mod grep;
pub mod jsdiff;
pub mod js_util;
pub mod ls;
pub mod node_fs;
pub mod output_accumulator;
pub mod path_utils;
pub mod powershell;
pub mod read;
pub mod render_utils;
pub mod renderers;
pub mod tool_definition_wrapper;
pub mod truncate;
pub mod write;

use crate::core::extensions::types::ToolDefinition;
use indexmap::{IndexMap, IndexSet};
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

pub use crate::core::tools::bash::BashOperations;
pub use crate::core::tools::bash::BashSpawnContext;
pub use crate::core::tools::bash::BashSpawnHook;
pub use crate::core::tools::bash::BashToolDetails;
pub use crate::core::tools::bash::BashToolInput;
pub use crate::core::tools::bash::BashToolOptions;
pub use crate::core::tools::bash::create_bash_tool;
pub use crate::core::tools::bash::create_bash_tool_definition;
pub use crate::core::tools::bash::create_local_bash_operations;
pub use crate::core::tools::edit::EditOperations;
pub use crate::core::tools::edit::EditToolDetails;
pub use crate::core::tools::edit::EditToolInput;
pub use crate::core::tools::edit::EditToolOptions;
pub use crate::core::tools::edit::create_edit_tool;
pub use crate::core::tools::edit::create_edit_tool_definition;
pub use crate::core::tools::file_mutation_queue::with_file_mutation_queue;
pub use crate::core::tools::find::FindOperations;
pub use crate::core::tools::find::FindToolDetails;
pub use crate::core::tools::find::FindToolInput;
pub use crate::core::tools::find::FindToolOptions;
pub use crate::core::tools::find::create_find_tool;
pub use crate::core::tools::find::create_find_tool_definition;
pub use crate::core::tools::grep::GrepOperations;
pub use crate::core::tools::grep::GrepToolDetails;
pub use crate::core::tools::grep::GrepToolInput;
pub use crate::core::tools::grep::GrepToolOptions;
pub use crate::core::tools::grep::create_grep_tool;
pub use crate::core::tools::grep::create_grep_tool_definition;
pub use crate::core::tools::ls::LsOperations;
pub use crate::core::tools::ls::LsToolDetails;
pub use crate::core::tools::ls::LsToolInput;
pub use crate::core::tools::ls::LsToolOptions;
pub use crate::core::tools::ls::create_ls_tool;
pub use crate::core::tools::ls::create_ls_tool_definition;
pub use crate::core::tools::powershell::PowerShellOperations;
pub use crate::core::tools::powershell::PowerShellSpawnContext;
pub use crate::core::tools::powershell::PowerShellSpawnHook;
pub use crate::core::tools::powershell::PowerShellToolDetails;
pub use crate::core::tools::powershell::PowerShellToolInput;
pub use crate::core::tools::powershell::PowerShellToolOptions;
pub use crate::core::tools::powershell::create_local_power_shell_operations;
pub use crate::core::tools::powershell::create_power_shell_tool;
pub use crate::core::tools::powershell::create_power_shell_tool_definition;
pub use crate::core::tools::read::ReadOperations;
pub use crate::core::tools::read::ReadToolDetails;
pub use crate::core::tools::read::ReadToolInput;
pub use crate::core::tools::read::ReadToolOptions;
pub use crate::core::tools::read::create_read_tool;
pub use crate::core::tools::read::create_read_tool_definition;
pub use crate::core::tools::truncate::DEFAULT_MAX_BYTES;
pub use crate::core::tools::truncate::DEFAULT_MAX_LINES;
pub use crate::core::tools::truncate::TruncationOptions;
pub use crate::core::tools::truncate::TruncationResult;
pub use crate::core::tools::truncate::format_size;
pub use crate::core::tools::truncate::truncate_head;
pub use crate::core::tools::truncate::truncate_line;
pub use crate::core::tools::truncate::truncate_tail;
pub use crate::core::tools::write::WriteOperations;
pub use crate::core::tools::write::WriteToolInput;
pub use crate::core::tools::write::WriteToolOptions;
pub use crate::core::tools::write::create_write_tool;
pub use crate::core::tools::write::create_write_tool_definition;

pub type Tool = AgentToolRef;

pub type ToolDef = ToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolName {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "bash")]
    Bash,
    #[serde(rename = "powershell")]
    Powershell,
    #[serde(rename = "edit")]
    Edit,
    #[serde(rename = "write")]
    Write,
    #[serde(rename = "grep")]
    Grep,
    #[serde(rename = "find")]
    Find,
    #[serde(rename = "ls")]
    Ls,
}

impl ToolName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Bash => "bash",
            Self::Powershell => "powershell",
            Self::Edit => "edit",
            Self::Write => "write",
            Self::Grep => "grep",
            Self::Find => "find",
            Self::Ls => "ls",
        }
    }
}

pub static ALL_TOOL_NAMES: LazyLock<IndexSet<ToolName>> = LazyLock::new(|| {
    IndexSet::from([
        ToolName::Read,
        ToolName::Bash,
        ToolName::Powershell,
        ToolName::Edit,
        ToolName::Write,
        ToolName::Grep,
        ToolName::Find,
        ToolName::Ls,
    ])
});

impl std::str::FromStr for ToolName {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> anyhow::Result<Self> {
        ALL_TOOL_NAMES
            .iter()
            .find(|tool| tool.as_str() == name)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Unknown tool name: {name}"))
    }
}

#[derive(Clone, Default)]
pub struct ToolsOptions {
    pub read: Option<ReadToolOptions>,
    pub bash: Option<BashToolOptions>,
    pub powershell: Option<PowerShellToolOptions>,
    pub write: Option<WriteToolOptions>,
    pub edit: Option<EditToolOptions>,
    pub grep: Option<GrepToolOptions>,
    pub find: Option<FindToolOptions>,
    pub ls: Option<LsToolOptions>,
}

pub fn create_tool_definition(
    tool_name: &ToolName,
    cwd: &str,
    options: Option<&ToolsOptions>,
) -> anyhow::Result<ToolDef> {
    Ok(match tool_name {
        ToolName::Read => create_read_tool_definition(cwd, options.and_then(|o| o.read.as_ref())),
        ToolName::Bash => create_bash_tool_definition(cwd, options.and_then(|o| o.bash.as_ref())),
        ToolName::Powershell => create_power_shell_tool_definition(cwd, options.and_then(|o| o.powershell.as_ref())),
        ToolName::Edit => create_edit_tool_definition(cwd, options.and_then(|o| o.edit.as_ref())),
        ToolName::Write => create_write_tool_definition(cwd, options.and_then(|o| o.write.as_ref())),
        ToolName::Grep => create_grep_tool_definition(cwd, options.and_then(|o| o.grep.as_ref())),
        ToolName::Find => create_find_tool_definition(cwd, options.and_then(|o| o.find.as_ref())),
        ToolName::Ls => create_ls_tool_definition(cwd, options.and_then(|o| o.ls.as_ref())),
    })
}

pub fn create_tool(
    tool_name: &ToolName,
    cwd: &str,
    options: Option<&ToolsOptions>,
) -> anyhow::Result<Tool> {
    Ok(match tool_name {
        ToolName::Read => create_read_tool(cwd, options.and_then(|o| o.read.as_ref())),
        ToolName::Bash => create_bash_tool(cwd, options.and_then(|o| o.bash.as_ref())),
        ToolName::Powershell => create_power_shell_tool(cwd, options.and_then(|o| o.powershell.as_ref())),
        ToolName::Edit => create_edit_tool(cwd, options.and_then(|o| o.edit.as_ref())),
        ToolName::Write => create_write_tool(cwd, options.and_then(|o| o.write.as_ref())),
        ToolName::Grep => create_grep_tool(cwd, options.and_then(|o| o.grep.as_ref())),
        ToolName::Find => create_find_tool(cwd, options.and_then(|o| o.find.as_ref())),
        ToolName::Ls => create_ls_tool(cwd, options.and_then(|o| o.ls.as_ref())),
    })
}

pub fn create_coding_tool_definitions(cwd: &str, options: Option<&ToolsOptions>) -> Vec<ToolDef> {
    [ToolName::Read, ToolName::Bash, ToolName::Edit, ToolName::Write]
        .iter()
        .filter_map(|name| create_tool_definition(name, cwd, options).ok())
        .collect()
}

pub fn create_read_only_tool_definitions(
    cwd: &str,
    options: Option<&ToolsOptions>,
) -> Vec<ToolDef> {
    [ToolName::Read, ToolName::Grep, ToolName::Find, ToolName::Ls]
        .iter()
        .filter_map(|name| create_tool_definition(name, cwd, options).ok())
        .collect()
}

pub fn create_all_tool_definitions(
    cwd: &str,
    options: Option<&ToolsOptions>,
) -> IndexMap<ToolName, ToolDef> {
    ALL_TOOL_NAMES
        .iter()
        .filter_map(|name| create_tool_definition(name, cwd, options).ok().map(|def| (*name, def)))
        .collect()
}

pub fn create_coding_tools(cwd: &str, options: Option<&ToolsOptions>) -> Vec<Tool> {
    [ToolName::Read, ToolName::Bash, ToolName::Edit, ToolName::Write]
        .iter()
        .filter_map(|name| create_tool(name, cwd, options).ok())
        .collect()
}

pub fn create_read_only_tools(cwd: &str, options: Option<&ToolsOptions>) -> Vec<Tool> {
    [ToolName::Read, ToolName::Grep, ToolName::Find, ToolName::Ls]
        .iter()
        .filter_map(|name| create_tool(name, cwd, options).ok())
        .collect()
}

pub fn create_all_tools(cwd: &str, options: Option<&ToolsOptions>) -> IndexMap<ToolName, Tool> {
    ALL_TOOL_NAMES
        .iter()
        .filter_map(|name| create_tool(name, cwd, options).ok().map(|tool| (*name, tool)))
        .collect()
}
