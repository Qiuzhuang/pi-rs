//! Port of `packages/coding-agent/src/core/mcp-servers.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Regular expression for a tool name pattern where `*` matches any characters, as `toolExposure`,
/// `--tools`, and `--exclude-tools` accept them.
fn tool_pattern_regex(pattern: &str) -> regex::Regex {
    let source = pattern
        .split('*')
        .map(regex::escape)
        .collect::<Vec<_>>()
        .join(".*");
    // `(?s)`: JS `.` does not match newlines either, but tool names never contain them.
    regex::Regex::new(&format!("^{source}$")).unwrap_or_else(|_| regex::Regex::new("$^").unwrap())
}

/// Whether a tool name matches any of the entries, each an exact name or a pattern.
pub fn create_tool_name_matcher(entries: &[String]) -> Arc<dyn Fn(String) -> bool + Send + Sync> {
    let names: std::collections::HashSet<String> =
        entries.iter().filter(|e| !e.contains('*')).cloned().collect();
    let patterns: Vec<regex::Regex> =
        entries.iter().filter(|e| e.contains('*')).map(|e| tool_pattern_regex(e)).collect();
    Arc::new(move |name: String| names.contains(&name) || patterns.iter().any(|p| p.is_match(&name)))
}

/// MCP resource tools, which reach every server with resources.
pub const LIST_MCP_RESOURCES_TOOL: &str = "list_mcp_resources";

pub const LIST_MCP_RESOURCE_TEMPLATES_TOOL: &str = "list_mcp_resource_templates";

pub const READ_MCP_RESOURCE_TOOL: &str = "read_mcp_resource";

/// Whether a tool comes from MCP: a server tool (`mcp__<server>__<tool>`) or a resource tool.
pub fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with("mcp__")
        || [LIST_MCP_RESOURCES_TOOL, LIST_MCP_RESOURCE_TEMPLATES_TOOL, READ_MCP_RESOURCE_TOOL].contains(&name)
}

const MCP_EXPOSURES: &[&str] = &["codemode", "deferred", "direct", "hidden"];

/// Older exposure names, accepted in configs and replaced by their current name when validated.
fn resolve_exposure_alias_name(value: &str) -> Option<&'static str> {
    match value {
        "codemode-deferred" => Some("codemode"),
        _ => None,
    }
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
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "http"
        && url.host_str().is_some_and(|h| LOOPBACK_HOSTS.contains(&h))
        && url.query().is_none_or(str::is_empty)
        && url.fragment().is_none_or(str::is_empty)
}

const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]"];

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
    format!("mcp__{}", server.replace('-', "_"))
}

impl McpServerConfig {
    /// `exposure` of either variant.
    pub fn exposure(&self) -> Option<McpExposure> {
        match self {
            Self::McpStdioServerConfig(c) => c.exposure,
            Self::McpHttpServerConfig(c) => c.exposure,
        }
    }

    /// `toolExposure` of either variant.
    pub fn tool_exposure(&self) -> Option<&IndexMap<String, McpExposure>> {
        match self {
            Self::McpStdioServerConfig(c) => c.tool_exposure.as_ref(),
            Self::McpHttpServerConfig(c) => c.tool_exposure.as_ref(),
        }
    }
}

/// Exposure of one tool of a server: its `toolExposure` entry, else the server's `exposure`.
pub fn get_mcp_tool_exposure(config: &McpServerConfig, tool_name: &str) -> McpExposure {
    if let Some(overrides) = config.tool_exposure() {
        if let Some(exact) = overrides.get(tool_name) {
            return *exact;
        }
        for (pattern, exposure) in overrides {
            if pattern.contains('*') && tool_pattern_regex(pattern).is_match(tool_name) {
                return *exposure;
            }
        }
    }
    config.exposure().unwrap_or(McpExposure::Codemode)
}

fn is_record(value: &Value) -> bool {
    value.is_object()
}

fn is_string_record(value: &Value) -> bool {
    value.as_object().is_some_and(|m| m.values().all(Value::is_string))
}

fn js_url(value: &str) -> Option<url::Url> {
    url::Url::parse(value).ok()
}

fn validate_oauth(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let Some(o) = value.as_object() else {
        return Some("oauth must be an object".into());
    };
    let present = |k: &str| o.get(k).is_some();
    let is_str = |k: &str| o.get(k).is_some_and(Value::is_string);
    if present("clientId") && !is_str("clientId") {
        return Some("oauth.clientId must be a string".into());
    }
    if present("clientSecret") && !is_str("clientSecret") {
        return Some("oauth.clientSecret must be a string".into());
    }
    let port = o.get("callbackPort");
    if let Some(port) = port {
        let ok = port.as_f64().is_some_and(|p| p.fract() == 0.0 && (1.0..=65535.0).contains(&p));
        if !ok {
            return Some("oauth.callbackPort must be a port number".into());
        }
    }
    if let Some(callback) = o.get("callbackUrl") {
        let Some(cb) = callback.as_str().filter(|s| is_loopback_redirect_uri(s)) else {
            return Some(
                "oauth.callbackUrl must be an http URI on localhost, 127.0.0.1, or [::1] without query or fragment"
                    .into(),
            );
        };
        if let (Some(url_port), Some(port)) = (js_url(cb).and_then(|u| u.port()), port.and_then(Value::as_f64))
            && f64::from(url_port) != port
        {
            return Some("oauth.callbackUrl and oauth.callbackPort name different ports".into());
        }
    }
    if present("scope") && !is_str("scope") {
        return Some("oauth.scope must be a string".into());
    }
    if let Some(name) = o.get("clientName")
        && name.as_str().is_none_or(|s| s.trim().is_empty())
    {
        return Some("oauth.clientName must be a non-empty string".into());
    }
    if let Some(reg) = o.get("clientRegistration")
        && reg.as_str() != Some("dcr")
    {
        if reg.as_str() != Some("cimd") {
            return Some(r#"oauth.clientRegistration must be "dcr" or "cimd""#.into());
        }
        if present("clientId") || present("clientName") {
            return Some(
                r#"oauth.clientRegistration "cimd" cannot be combined with oauth.clientId or oauth.clientName"#.into(),
            );
        }
        let callback = o.get("callbackUrl").and_then(Value::as_str).and_then(js_url);
        if let Some(cb) = callback
            && (cb.host_str() == Some("[::1]") || cb.path() != "/callback")
        {
            return Some(
                r#"oauth.clientRegistration "cimd" requires oauth.callbackUrl on localhost or 127.0.0.1 with path /callback"#
                    .into(),
            );
        }
    }
    if let Some(metadata) = o.get("authServerMetadataUrl") {
        let url = metadata.as_str().and_then(js_url);
        let ok = url.is_some_and(|u| {
            u.scheme() == "https" || (u.scheme() == "http" && u.host_str().is_some_and(|h| LOOPBACK_HOSTS.contains(&h)))
        });
        if !ok {
            return Some(
                "oauth.authServerMetadataUrl must be an https URL, or http on localhost, 127.0.0.1, or [::1]".into(),
            );
        }
    }
    None
}

fn is_exposure(value: &Value) -> bool {
    value.as_str().is_some_and(|s| MCP_EXPOSURES.contains(&s))
}

fn resolve_exposure_alias(value: &Value) -> Value {
    match value.as_str().and_then(resolve_exposure_alias_name) {
        Some(resolved) => Value::String(resolved.to_string()),
        None => value.clone(),
    }
}

/// A copy of the server entry with exposure aliases replaced by their current names.
fn resolve_exposure_aliases(value: &serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    let mut resolved = value.clone();
    if let Some(exposure) = value.get("exposure") {
        resolved.insert("exposure".into(), resolve_exposure_alias(exposure));
    }
    if let Some(Value::Object(tool_exposure)) = value.get("toolExposure") {
        let mapped = tool_exposure.iter().map(|(k, v)| (k.clone(), resolve_exposure_alias(v))).collect();
        resolved.insert("toolExposure".into(), Value::Object(mapped));
    }
    resolved
}

fn is_valid_server_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValidateMcpServerConfigResult {
    McpStdioServerConfig(McpStdioServerConfig),
    McpHttpServerConfig(McpHttpServerConfig),
    String(String),
}

/// Validate one server entry of the `mcpServers` shape. Returns a copy of the config with exposure
/// aliases resolved, or an error message.
pub fn validate_mcp_server_config(name: &str, raw: Value) -> ValidateMcpServerConfigResult {
    use ValidateMcpServerConfigResult as R;
    let err = |m: String| R::String(m);
    if !is_valid_server_name(name) {
        return err(format!(r#"invalid server name "{name}" (use letters, digits, "_" and "-")"#));
    }
    let Some(raw) = raw.as_object() else {
        return err(format!(r#"server "{name}" must be an object"#));
    };
    let value = resolve_exposure_aliases(raw);
    let get = |k: &str| value.get(k);
    let exposures = MCP_EXPOSURES.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join(", ");
    if let Some(exposure) = get("exposure")
        && !is_exposure(exposure)
    {
        return err(format!(r#"server "{name}": exposure must be one of {exposures}"#));
    }
    if let Some(tool_exposure) = get("toolExposure") {
        let Some(map) = tool_exposure.as_object() else {
            return err(format!(r#"server "{name}": toolExposure must map tool names to exposures"#));
        };
        for (tool, v) in map {
            if !is_exposure(v) {
                return err(format!(r#"server "{name}": toolExposure "{tool}" must be one of {exposures}"#));
            }
        }
    }
    if get("enabled").is_some_and(|v| !v.is_boolean()) {
        return err(format!(r#"server "{name}": enabled must be a boolean"#));
    }
    if get("description").is_some_and(|v| !v.is_string()) {
        return err(format!(r#"server "{name}": description must be a string"#));
    }
    if let Some(timeout) = get("timeout")
        && !timeout.as_f64().is_some_and(|t| t > 0.0)
    {
        return err(format!(r#"server "{name}": timeout must be a positive number of seconds"#));
    }
    let ty = get("type");
    let ty_str = ty.and_then(Value::as_str);
    if ty_str == Some("sse") {
        return err(format!(
            r#"server "{name}": legacy SSE transport is not supported; use the streamable HTTP URL"#
        ));
    }

    if let Some(url) = get("url").and_then(Value::as_str)
        && (ty.is_none() || ty_str == Some("http") || ty_str == Some("streamable-http"))
    {
        let parsed = js_url(url).filter(|u| u.scheme() == "http" || u.scheme() == "https");
        let Some(parsed) = parsed else {
            return err(format!(r#"server "{name}": url must be an http or https URL"#));
        };
        if get("headers").is_some_and(|h| !is_string_record(h)) {
            return err(format!(r#"server "{name}": headers must map names to strings"#));
        }
        if let Some(e) = validate_oauth(get("oauth")) {
            return err(format!(r#"server "{name}": {e}"#));
        }
        if let Some(auth) = get("auth") {
            let ok = is_record(auth) && auth.get("provider").and_then(Value::as_str).is_some_and(|p| !p.is_empty());
            if !ok {
                return err(format!(r#"server "{name}": auth.provider must be a provider name"#));
            }
            if parsed.scheme() != "https" && !parsed.host_str().is_some_and(|h| LOOPBACK_HOSTS.contains(&h)) {
                return err(format!(
                    r#"server "{name}": auth requires an https URL, or http on localhost, 127.0.0.1, or [::1]"#
                ));
            }
        }
        return match serde_json::from_value::<McpHttpServerConfig>(Value::Object(value.clone())) {
            Ok(c) => R::McpHttpServerConfig(c),
            Err(e) => err(format!(r#"server "{name}": {e}"#)),
        };
    }
    if get("command").is_some_and(Value::is_string) && (ty.is_none() || ty_str == Some("stdio")) {
        if let Some(args) = get("args")
            && !args.as_array().is_some_and(|a| a.iter().all(Value::is_string))
        {
            return err(format!(r#"server "{name}": args must be an array of strings"#));
        }
        if get("env").is_some_and(|e| !is_string_record(e)) {
            return err(format!(r#"server "{name}": env must map names to strings"#));
        }
        if get("cwd").is_some_and(|c| !c.is_string()) {
            return err(format!(r#"server "{name}": cwd must be a string"#));
        }
        return match serde_json::from_value::<McpStdioServerConfig>(Value::Object(value.clone())) {
            Ok(c) => R::McpStdioServerConfig(c),
            Err(e) => err(format!(r#"server "{name}": {e}"#)),
        };
    }
    err(format!(r#"server "{name}" needs either "command" (stdio) or "url" (streamable HTTP)"#))
}

impl ValidateMcpServerConfigResult {
    /// The validated config, or the error message.
    pub fn into_result(self) -> Result<McpServerConfig, String> {
        match self {
            Self::McpStdioServerConfig(c) => Ok(McpServerConfig::McpStdioServerConfig(c)),
            Self::McpHttpServerConfig(c) => Ok(McpServerConfig::McpHttpServerConfig(c)),
            Self::String(e) => Err(e),
        }
    }
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
    servers: parking_lot::Mutex<IndexMap<String, RegisteredMcpServer>>,
    change_listener: parking_lot::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Default for McpServerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServerRegistry {
    pub fn new() -> Self {
        Self { servers: parking_lot::Mutex::new(IndexMap::new()), change_listener: parking_lot::Mutex::new(None) }
    }

    fn notify(&self) {
        let listener = self.change_listener.lock().clone();
        if let Some(listener) = listener {
            listener();
        }
    }

    /// Register or replace a server. The caller checks ownership.
    pub fn register(&self, server: &RegisteredMcpServer) {
        {
            let mut servers = self.servers.lock();
            // JS Map.set keeps the original insertion position when replacing.
            servers.insert(server.name.clone(), server.clone());
        }
        self.notify();
    }

    /// Remove a server registered by `extensionPath`. Servers of other extensions are left alone.
    pub fn unregister(&self, name: &str, extension_path: &str) {
        {
            let mut servers = self.servers.lock();
            if servers.get(name).map(|s| s.extension_path.as_str()) != Some(extension_path) {
                return;
            }
            servers.shift_remove(name);
        }
        self.notify();
    }

    pub fn get(&self, name: &str) -> Option<RegisteredMcpServer> {
        self.servers.lock().get(name).cloned()
    }

    /// Copies of the registered servers, in registration order.
    pub fn list(&self) -> Vec<RegisteredMcpServer> {
        self.servers.lock().values().cloned().collect()
    }

    /// Called after every change. The runner sets it when it binds, to emit `mcp_servers_change`.
    pub fn set_change_listener(&self, listener: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.change_listener.lock() = listener;
    }
}
