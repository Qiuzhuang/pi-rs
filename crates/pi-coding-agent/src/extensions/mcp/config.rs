//! Port of `packages/coding-agent/src/extensions/mcp/config.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub use crate::core::mcp_servers::McpExposure;
pub use crate::core::mcp_servers::McpHttpServerConfig;
pub use crate::core::mcp_servers::McpOAuthConfig;
pub use crate::core::mcp_servers::McpServerConfig;
pub use crate::core::mcp_servers::McpStdioServerConfig;
pub use crate::core::mcp_servers::get_mcp_tool_exposure;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum McpServerEntryScope {
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "extension")]
    Extension,
}

impl McpServerEntryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Extension => "extension",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerEntry {
    pub name: String,
    pub config: McpServerConfig,
    /// Config file that defined the entry, or the path of the extension that registered it.
    pub source: String,
    /// The global or the project `mcp.json`, or `extension` for servers registered with
    /// `pi.registerMcpServer()`. Changes to extension servers are not saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<McpServerEntryScope>,
    /// Project `mcp.json` with an override of this global server's `enabled`, `exposure`, or `toolExposure`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedMcpConfig {
    pub servers: Vec<McpServerEntry>,
    /// Activate the codemode tool when `codemode` servers connect. Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_enable_codemode: Option<bool>,
    pub errors: Vec<String>,
    /// The project `mcp.json` when the project is trusted, where `/mcp` saves project overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_config: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigState {
    pub servers: IndexMap<String, McpServerEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_enable_codemode: Option<bool>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadMcpConfigOptions {
    pub agent_dir: String,
    pub cwd: String,
    pub project_trusted: bool,
}

/// Load global and (when trusted) project MCP configuration. Disabled servers are included with
/// `enabled: false`, so they can be enabled again.
pub fn load_mcp_config(options: &LoadMcpConfigOptions) -> LoadedMcpConfig {
    todo!("port extensions/mcp/config.ts:loadMcpConfig")
}

/// Settings `/mcp` changes. `enabled: true` and `exposure: "codemode"` are the defaults and remove the key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<McpExposure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMcpServerConfigOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#override: Option<bool>,
}

/// Change one server's settings in the `mcp.json` that defines or overrides it. With `override`, a
/// missing entry is added as an override. Overrides keep default values, since they replace the global
/// server's. Other content is kept; the file is rewritten with its indentation.
pub fn update_mcp_server_config(
    path: &str,
    name: &str,
    patch: &McpServerConfigPatch,
    options: Option<&UpdateMcpServerConfigOptions>,
) -> anyhow::Result<()> {
    todo!("port extensions/mcp/config.ts:updateMcpServerConfig")
}

/// Add a server to an `mcp.json`, creating the file when missing. An existing entry with the same
/// name is replaced. Returns true when an entry was replaced.
pub fn add_mcp_server_config(path: &str, name: &str, config: &McpServerConfig) -> bool {
    todo!("port extensions/mcp/config.ts:addMcpServerConfig")
}

/// Remove a server from an `mcp.json`. Returns false when the file does not define it.
pub fn remove_mcp_server_config(path: &str, name: &str) -> bool {
    todo!("port extensions/mcp/config.ts:removeMcpServerConfig")
}
