//! Port of `packages/coding-agent/src/extensions/mcp/cli.ts`.

use crate::extensions::mcp::oauth::McpOAuthCredentialStore;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct McpCommandOptions {
    pub cwd: String,
    pub agent_dir: String,
    /// Defaults to `mcp-auth.json` in the agent directory.
    pub credentials: Option<Arc<McpOAuthCredentialStore>>,
    /// Defaults to the platform browser.
    pub open_url: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Defaults to console output.
    pub log: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub error: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerReport {
    pub name: String,
    pub scope: String,
    pub source: String,
    /// Project `mcp.json` that overrides `enabled`, `exposure`, or `toolExposure` of this global server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#override: Option<String>,
    pub enabled: bool,
    pub exposure: String,
    pub transport: String,
    pub state: String,
    pub tools: Vec<String>,
    /// Tools whose exposure differs from the server's, from `toolExposure`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_exposure: Option<IndexMap<String, String>>,
    /// Whether the server offers resources. They are not listed: listing every one can take long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParsedOptionsValues {
    String(String),
    Bool(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedOptions {
    pub positional: Vec<String>,
    pub values: IndexMap<String, ParsedOptionsValues>,
    /// Values of `list` options, in order.
    pub lists: IndexMap<String, Vec<String>>,
}

/// Run `pi mcp <args>` and return the exit code.
pub async fn run_mcp_command(args: &[String], options: &McpCommandOptions) -> anyhow::Result<f64> {
    todo!("port extensions/mcp/cli.ts:runMcpCommand")
}
