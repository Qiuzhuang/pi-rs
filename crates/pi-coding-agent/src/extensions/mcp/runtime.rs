//! Port of `packages/coding-agent/src/extensions/mcp/runtime.ts`.

use crate::extensions::mcp::config::McpServerEntry;
use crate::extensions::mcp::oauth::McpOAuthSettings;
use crate::extensions::mcp::resources::McpResourceServer;
use crate::extensions::mcp::tools::McpToolCaller;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_mcp::auth_provider::AuthProvider;
use pi_mcp::client::McpClient;
use pi_mcp::client::McpRequestOptions;
use pi_mcp::oauth::types::OAuthChallenge;
use pi_mcp::protocol::content::CallToolResult;
use pi_mcp::protocol::types::ListResourceTemplatesResult;
use pi_mcp::protocol::types::ListResourcesResult;
use pi_mcp::protocol::types::ReadResourceResult;
use pi_mcp::protocol::types::Resource;
use pi_mcp::protocol::types::ResourceTemplate;
use pi_mcp::protocol::types::Tool;
use pi_mcp::transports::transport::McpTransport;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub use crate::extensions::mcp::log::McpServerLog;
pub use crate::extensions::mcp::oauth::McpOAuthCredentialStore;
pub use crate::extensions::mcp::oauth::McpSignInCancelledError;
pub use crate::extensions::mcp::oauth::sign_in_mcp_server;

/// `disconnected`: the connection dropped (for example the stdio server exited); the next call
/// reconnects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServerState {
    #[serde(rename = "connecting")]
    Connecting,
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "disconnected")]
    Disconnected,
    #[serde(rename = "needs-auth")]
    NeedsAuth,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "closed")]
    Closed,
}

impl ServerState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::NeedsAuth => "needs-auth",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

pub type McpTransportFactory = Arc<
    dyn Fn(McpServerEntry, String, Option<Arc<dyn AuthProvider>>) -> Arc<dyn McpTransport>
        + Send
        + Sync,
>;

pub fn create_default_transport(
    entry: &McpServerEntry,
    cwd: &str,
    auth_provider: Option<Arc<dyn AuthProvider>>,
) -> Arc<dyn McpTransport> {
    todo!("port extensions/mcp/runtime.ts:createDefaultTransport")
}

#[derive(Clone)]
pub struct McpServerConnectionOptions {
    pub entry: McpServerEntry,
    pub cwd: String,
    pub create_transport: McpTransportFactory,
    pub credentials: Arc<McpOAuthCredentialStore>,
    /// The current token of a pi provider, for servers with `auth.provider`.
    pub provider_token: Option<
        Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Option<String>>> + Send + Sync>,
    >,
    pub on_tools: Arc<dyn Fn(Arc<McpServerConnection>) + Send + Sync>,
    /// Called when `state`, `error`, or `tools` change.
    pub on_change: Option<Arc<dyn Fn(Arc<McpServerConnection>) + Send + Sync>>,
    /// Receives the server's log messages (`notifications/message`).
    pub log: Option<Arc<McpServerLog>>,
}

/// One configured server. Reconnects lazily when a call finds the connection gone.
/// TS: `implements McpToolCaller, McpResourceServer`
pub struct McpServerConnection {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl McpServerConnection {
    pub fn new(options: &McpServerConnectionOptions) -> Self {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.constructor")
    }

    pub fn entry(&self) -> McpServerEntry {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.entry")
    }

    pub fn state(&self) -> ServerState {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.state")
    }

    pub fn set_state(&self, value: ServerState) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.state")
    }

    pub fn error(&self) -> Option<String> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.error")
    }

    pub fn set_error(&self, value: Option<String>) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.error")
    }

    pub fn tools(&self) -> Vec<Tool> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.tools")
    }

    pub fn set_tools(&self, value: Vec<Tool>) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.tools")
    }

    /// Whether the server offers resources, which the resource tools list and read on demand. They are
    /// not listed at connect: listing every resource can take long (a large drive, say).
    pub fn has_resources(&self) -> bool {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.hasResources")
    }

    pub fn set_has_resources(&self, value: bool) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.hasResources")
    }

    /// Server instructions from `initialize`, describing its tools as a group.
    pub fn instructions(&self) -> Option<String> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.instructions")
    }

    pub fn set_instructions(&self, value: Option<String>) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.instructions")
    }

    /// Last OAuth challenge from the server; sign-in uses its resource metadata URL and scope.
    pub fn challenge(&self) -> Option<OAuthChallenge> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.challenge")
    }

    pub fn set_challenge(&self, value: Option<OAuthChallenge>) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.challenge")
    }

    /// Server URL when the server authenticates with OAuth.
    pub fn oauth_url(&self) -> Option<String> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.oauthUrl")
    }

    pub fn oauth_settings(&self) -> McpOAuthSettings {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.oauthSettings")
    }

    pub async fn get_client(&self) -> anyhow::Result<McpClient> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.getClient")
    }

    /// Connect again with fresh credentials, for example after signing in.
    pub async fn reconnect(&self) -> anyhow::Result<()> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.reconnect")
    }

    /// Disconnect after the stored credentials were removed.
    pub async fn sign_out(&self) -> anyhow::Result<()> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.signOut")
    }

    /// Resolves once no transport of this server is open, including one that was still connecting.
    pub async fn close(&self) -> anyhow::Result<()> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.close")
    }
}

#[async_trait::async_trait]
impl McpToolCaller for McpServerConnection {
    async fn call_tool(
        &self,
        name: &str,
        args: IndexMap<String, Value>,
        options: McpRequestOptions,
    ) -> anyhow::Result<CallToolResult> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.call_tool")
    }
}

#[async_trait::async_trait]
impl McpResourceServer for McpServerConnection {
    fn name(&self) -> String {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.name")
    }
    fn set_name(&self, value: String) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.set_name")
    }
    fn timeout_ms(&self) -> i64 {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.timeout_ms")
    }
    fn set_timeout_ms(&self, value: i64) {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.set_timeout_ms")
    }
    async fn resources_page(
        &self,
        cursor: Option<&str>,
        options: McpRequestOptions,
    ) -> anyhow::Result<ListResourcesResult> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.resources_page")
    }
    async fn resource_templates_page(
        &self,
        cursor: Option<&str>,
        options: McpRequestOptions,
    ) -> anyhow::Result<ListResourceTemplatesResult> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.resource_templates_page")
    }
    async fn all_resources(&self, options: McpRequestOptions) -> anyhow::Result<Vec<Resource>> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.all_resources")
    }
    async fn all_resource_templates(
        &self,
        options: McpRequestOptions,
    ) -> anyhow::Result<Vec<ResourceTemplate>> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.all_resource_templates")
    }
    async fn read_resource(
        &self,
        uri: &str,
        options: McpRequestOptions,
    ) -> anyhow::Result<ReadResourceResult> {
        todo!("port extensions/mcp/runtime.ts:McpServerConnection.read_resource")
    }
}
