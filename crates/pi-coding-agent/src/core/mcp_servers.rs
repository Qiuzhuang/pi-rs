//! Port of `packages/coding-agent/src/core/mcp-servers.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Whether a tool name matches any of the entries, each an exact name or a pattern.
pub fn create_tool_name_matcher(entries: &[String]) -> Arc<dyn Fn(String) -> bool + Send + Sync> {
    todo!("port core/mcp-servers.ts:createToolNameMatcher")
}

/// MCP resource tools, which reach every server with resources.
pub const LIST_MCP_RESOURCES_TOOL: &str = "list_mcp_resources";

pub const LIST_MCP_RESOURCE_TEMPLATES_TOOL: &str = "list_mcp_resource_templates";

pub const READ_MCP_RESOURCE_TOOL: &str = "read_mcp_resource";

/// Whether a tool comes from MCP: a server tool (`mcp__<server>__<tool>`) or a resource tool.
pub fn is_mcp_tool_name(name: &str) -> bool {
    todo!("port core/mcp-servers.ts:isMcpToolName")
}

/// - `codemode`: tools are callable from codemode scripts but neither declared to the model nor
/// listed in the codemode description, which lists only the server's namespace. Scripts find them
/// with `searchTools()`. `codemode-deferred` is accepted as an alias.
/// - `deferred`: not declared to the model until the `tool_search` tool loads them; the model then
/// calls them directly. Does not need codemode.
/// - `direct`: tools are declared to the model like any other tool (and callable from codemode).
/// - `hidden`: tools are registered but unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum McpExposure {
    #[serde(rename = "codemode")]
    Codemode,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "direct")]
    Direct,
    #[serde(rename = "hidden")]
    Hidden,
}

impl McpExposure {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Codemode => "codemode",
            Self::Deferred => "deferred",
            Self::Direct => "direct",
            Self::Hidden => "hidden",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigBase {
    /// Default: `codemode`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<McpExposure>,
    /// What the server offers, in a sentence. The `mcp_servers` system prompt section lists the server
    /// with it, tool search ranks the server's tools by it, and codemode's `describeNamespace()` returns it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Exposure of single tools, overriding `exposure`. Keys are tool names as the server offers them,
    /// or patterns where `*` matches any characters. An exact name wins over patterns; among patterns
    /// the first match in the object wins. `hidden` removes tools, so `"exposure": "hidden"` with
    /// overrides for a few tools exposes only those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_exposure: Option<IndexMap<String, McpExposure>>,
    /// Set to false to keep the entry without connecting. Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Per-request timeout in seconds. Progress notifications from the server reset it. Default: 60.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,
}

// TS: extends McpServerConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStdioServerConfig {
    /// Default: `codemode`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<McpExposure>,
    /// What the server offers, in a sentence. The `mcp_servers` system prompt section lists the server
    /// with it, tool search ranks the server's tools by it, and codemode's `describeNamespace()` returns it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Exposure of single tools, overriding `exposure`. Keys are tool names as the server offers them,
    /// or patterns where `*` matches any characters. An exact name wins over patterns; among patterns
    /// the first match in the object wins. `hidden` removes tools, so `"exposure": "hidden"` with
    /// overrides for a few tools exposes only those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_exposure: Option<IndexMap<String, McpExposure>>,
    /// Set to false to keep the entry without connecting. Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Per-request timeout in seconds. Progress notifications from the server reset it. Default: 60.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Values may reference environment variables (`${NAME}`) or commands (`!cmd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,
    /// Relative paths resolve against the session working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum McpOAuthConfigClientRegistration {
    #[serde(rename = "dcr")]
    Dcr,
    #[serde(rename = "cimd")]
    Cimd,
}

impl McpOAuthConfigClientRegistration {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dcr => "dcr",
            Self::Cimd => "cimd",
        }
    }
}

/// OAuth client settings for servers that do not support dynamic client registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthConfig {
    /// Pre-registered client id. Without it, pi registers a client with the authorization server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// May reference environment variables (`${NAME}`) or commands (`!cmd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// Port of the loopback callback server, for clients registered with a fixed redirect URI. Without
    /// `callbackUrl`, the redirect URI is `http://127.0.0.1:<port>/callback`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub callback_port: Option<f64>,
    /// Redirect URI registered for `clientId`, for example `http://localhost:8080/oauth/callback`. It must
    /// be an `http` URI on `localhost`, `127.0.0.1`, or `[::1]`. Without a port, the callback server
    /// listens on `callbackPort` or a free port, which is added to the URI (RFC 8252).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
    /// Scopes to request, separated by spaces. Default: the scopes the server advertises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// `client_name` sent with dynamic client registration, for servers that only accept known clients.
    /// Default: `pi`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// How pi identifies itself without `clientId`. `dcr` (default): dynamic client registration. `cimd`:
    /// pi's Client ID Metadata Document on pi.dev, for authorization servers that allow pi by that URL. The
    /// server must support it for public clients, and the callback must use the default path `/callback`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_registration: Option<McpOAuthConfigClientRegistration>,
    /// Authorization server metadata document (RFC 8414 or OpenID Connect discovery) to use instead of
    /// discovery through the server, for servers that advertise a wrong authorization server or none.
    /// The document is trusted as configured. Must use https, except on loopback hosts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_server_metadata_url: Option<String>,
}

/// Whether a redirect URI can be served by pi's loopback callback server.
pub fn is_loopback_redirect_uri(value: &str) -> bool {
    todo!("port core/mcp-servers.ts:isLoopbackRedirectUri")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpHttpServerConfigAuth {
    pub provider: String,
}

// TS: extends McpServerConfigBase (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpHttpServerConfig {
    /// Default: `codemode`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<McpExposure>,
    /// What the server offers, in a sentence. The `mcp_servers` system prompt section lists the server
    /// with it, tool search ranks the server's tools by it, and codemode's `describeNamespace()` returns it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Exposure of single tools, overriding `exposure`. Keys are tool names as the server offers them,
    /// or patterns where `*` matches any characters. An exact name wins over patterns; among patterns
    /// the first match in the object wins. `hidden` removes tools, so `"exposure": "hidden"` with
    /// overrides for a few tools exposes only those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_exposure: Option<IndexMap<String, McpExposure>>,
    /// Set to false to keep the entry without connecting. Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Per-request timeout in seconds. Progress notifications from the server reset it. Default: 60.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub url: String,
    /// Values may reference environment variables (`${NAME}`) or commands (`!cmd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<IndexMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOAuthConfig>,
    /// Send the token of a pi provider (`/login <provider>`) instead of using OAuth. Not allowed in project
    /// `mcp.json` files, and requires https except on loopback hosts, since it sends the credential to `url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<McpHttpServerConfigAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerConfig {
    McpStdioServerConfig(McpStdioServerConfig),
    McpHttpServerConfig(McpHttpServerConfig),
}

/// Namespace of a server's tools: `mcp__<server>` with `-` replaced by `_`, like the tool names.
pub fn mcp_namespace(server: &str) -> String {
    todo!("port core/mcp-servers.ts:mcpNamespace")
}

/// Exposure of one tool of a server: its `toolExposure` entry, else the server's `exposure`.
pub fn get_mcp_tool_exposure(config: &McpServerConfig, tool_name: &str) -> McpExposure {
    todo!("port core/mcp-servers.ts:getMcpToolExposure")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmptyValidateMcpServerConfigResult {
    McpStdioServerConfig(McpStdioServerConfig),
    McpHttpServerConfig(McpHttpServerConfig),
    String(String),
}

/// Validate one server entry of the `mcpServers` shape. Returns a copy of the config with exposure
/// aliases resolved, or an error message.
pub fn validate_mcp_server_config(name: &str, raw: Value) -> EmptyValidateMcpServerConfigResult {
    todo!("port core/mcp-servers.ts:validateMcpServerConfig")
}

/// A server an extension registered with `pi.registerMcpServer()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredMcpServer {
    pub name: String,
    pub config: McpServerConfig,
    /// Path of the extension that registered the server.
    pub extension_path: String,
}

/// Servers registered by the extensions of one runtime.
pub struct McpServerRegistry {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl McpServerRegistry {
    pub fn new() -> Self {
        todo!("port core/mcp-servers.ts:McpServerRegistry.constructor")
    }

    /// Register or replace a server. The caller checks ownership.
    pub fn register(&self, server: &RegisteredMcpServer) {
        todo!("port core/mcp-servers.ts:McpServerRegistry.register")
    }

    /// Remove a server registered by `extensionPath`. Servers of other extensions are left alone.
    pub fn unregister(&self, name: &str, extension_path: &str) {
        todo!("port core/mcp-servers.ts:McpServerRegistry.unregister")
    }

    pub fn get(&self, name: &str) -> Option<RegisteredMcpServer> {
        todo!("port core/mcp-servers.ts:McpServerRegistry.get")
    }

    /// Copies of the registered servers, in registration order.
    pub fn list(&self) -> Vec<RegisteredMcpServer> {
        todo!("port core/mcp-servers.ts:McpServerRegistry.list")
    }

    /// Called after every change. The runner sets it when it binds, to emit `mcp_servers_change`.
    pub fn set_change_listener(&self, listener: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port core/mcp-servers.ts:McpServerRegistry.setChangeListener")
    }
}
