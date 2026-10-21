//! Port of `packages/coding-agent/src/extensions/mcp/index.ts`.

pub mod cli;
pub mod config;
pub mod log;
pub mod oauth;
pub mod resources;
pub mod runtime;
pub mod tools;
pub mod ui;

use crate::core::extensions::types::ExtensionContext;
use crate::core::extensions::types::ExtensionFactory;
use crate::extensions::mcp::config::LoadedMcpConfig;
use crate::extensions::mcp::config::McpServerConfigPatch;
use crate::extensions::mcp::config::McpServerEntry;
use crate::extensions::mcp::oauth::McpOAuthCredentialStore;
use crate::extensions::mcp::runtime::McpServerConnection;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use crate::extensions::mcp::runtime::McpTransportFactory;

#[derive(Clone)]
pub struct McpExtensionOptions {
    /// Defaults to reading `mcp.json` from the agent directory and the trusted project.
    pub load_config:
        Option<Arc<dyn Fn(Arc<dyn ExtensionContext>) -> LoadedMcpConfig + Send + Sync>>,
    /// Defaults to stdio and streamable HTTP transports built from the server config.
    pub create_transport: Option<McpTransportFactory>,
    /// Defaults to `mcp-auth.json` in the agent directory.
    pub credentials: Option<Arc<McpOAuthCredentialStore>>,
    /// File server log messages are appended to. Defaults to `mcp.log` in the agent directory.
    pub log_path: Option<String>,
    /// Opens the OAuth authorization URL. Defaults to the platform browser.
    pub open_url: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Saves `/mcp` changes to the server's config file: its project `override` when set, else its
    /// `source`. Defaults to editing that `mcp.json`.
    pub update_config: Option<Arc<dyn Fn(McpServerEntry, McpServerConfigPatch) + Send + Sync>>,
    /// How long the first prompt waits for servers with `direct` tools that are still connecting at
    /// startup, in milliseconds. Their tools become available when they connect. Other servers are
    /// waited for when a script or search needs them. Default: 10000.
    pub startup_wait_ms: Option<i64>,
}

/// A configured server. Disabled servers have no connection.
#[derive(Clone)]
pub struct McpServer {
    pub entry: McpServerEntry,
    pub connection: Option<Arc<McpServerConnection>>,
    /// For servers extensions registered: the config as registered, to detect re-registrations.
    pub registered_config: Option<String>,
    /// Result of the last `/mcp` action that failed, shown in the manager.
    pub message: Option<String>,
    /// Identifies the current connection attempt; disabling or replacing it invalidates earlier work.
    pub attempt: Option<String>,
    /// Settles when the connection started for the server connected or failed.
    pub ready: Option<futures::future::Shared<BoxFuture<'static, Result<(), Arc<anyhow::Error>>>>>,
    /// Cleanup of a detached connection; a replacement waits for it before opening a transport.
    pub closing:
        Option<futures::future::Shared<BoxFuture<'static, Result<(), Arc<anyhow::Error>>>>>,
}

/// Name of the system prompt section that lists the servers whose tools are not declared.
pub const MCP_SERVERS_SECTION: &str = "mcp_servers";

/// Characters of the whole section. Descriptions shrink to fit; when the server lines alone do not
/// fit, the last servers are left out and counted in a closing line.
pub const MAX_SERVERS_SECTION_CHARS: u64 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerListingConnection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// What the `mcp_servers` section needs of a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerListing {
    pub entry: McpServerEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<McpServerListingConnection>,
}

/// The `mcp_servers` section: every enabled server with codemode or deferred tools, with how its
/// tools are reached and a one-line summary. The model learns of the servers from it, since neither
/// codemode nor tool_search lists them. Undefined when there are no such servers.
pub fn render_servers_section(servers: &[McpServerListing]) -> Option<String> {
    todo!("port extensions/mcp/index.ts:renderServersSection")
}

pub fn create_mcp_extension(
    options: Option<&McpExtensionOptions>,
) -> anyhow::Result<ExtensionFactory> {
    todo!("port extensions/mcp/index.ts:createMcpExtension")
}
