//! Tests of the MCP extension (ports of `test/mcp-extension.test.ts`, `test/mcp-command.test.ts`,
//! `test/mcp-oauth-refresh.test.ts`, `test/mcp-manager-view.test.ts`, and the extension behavior
//! of `test/suite/agent-session-mcp.test.ts` against a fake `pi`).

use super::config::*;
use super::runtime::*;
use super::tools::*;
use super::*;
use indexmap::IndexMap;
use pi_agent::types::AgentToolResult;
use pi_mcp::protocol::content::CallToolResult;
use pi_tui::tui::Component;
use crate::core::auth_storage::InMemoryAuthStorageBackend;
use crate::core::extensions::types::{
    BeforeAgentStartKind, ExtensionAPI, ExtensionContextActions, ExtensionEvent,
    ExtensionEventResult, ExtensionHandler, ExtensionUIContext, ToolInfo, Unsubscribe,
};
use crate::core::mcp_servers::RegisteredMcpServer;
use crate::core::source_info::{SourceInfo, SourceOrigin, SourceScope};
use pi_mcp::protocol::jsonrpc::JsonRpcMessage;
use pi_mcp::testing::{InMemoryTransport, create_in_memory_transport_pair};
use pi_mcp::transports::transport::{McpTransport, McpTransportSendOptions, TransportEvents};
use pi_mcp::{Error as McpError, LATEST_PROTOCOL_VERSION};
use serde_json::{Value, json};
use std::sync::atomic::AtomicUsize;

// ------------------------------------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------------------------------------

/// A temporary directory removed on drop.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "{prefix}{}-{}",
            std::process::id(),
            next_id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    fn join(&self, path: &str) -> String {
        self.0.join(path).to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read_json(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn config_value(config: &McpServerConfig) -> Value {
    serde_json::to_value(config).unwrap()
}

fn stdio_entry(name: &str) -> McpServerEntry {
    McpServerEntry {
        name: name.to_string(),
        config: serde_json::from_value(json!({ "command": "unused" })).unwrap(),
        source: "test".to_string(),
        scope: None,
        r#override: None,
    }
}

fn entry_with(name: &str, config: Value) -> McpServerEntry {
    McpServerEntry {
        name: name.to_string(),
        config: serde_json::from_value(config).unwrap(),
        source: "test".to_string(),
        scope: None,
        r#override: None,
    }
}

fn credentials() -> Arc<McpOAuthCredentialStore> {
    Arc::new(McpOAuthCredentialStore::new(
        Some(Arc::new(InMemoryAuthStorageBackend::new())),
        None,
    ))
}

// ------------------------------------------------------------------------------------------------
// MCP config
// ------------------------------------------------------------------------------------------------

// Config values are resolved at connect time, so the literal reference must survive loading.
const TOKEN_HEADER: &str = "Bearer ${TOKEN}";

struct ConfigPaths {
    _root: TempDir,
    agent_dir: String,
    cwd: String,
}

fn setup_config(global: Value, project: Value) -> ConfigPaths {
    let root = TempDir::new("pi-mcp-config-");
    let agent_dir = root.join("agent");
    let cwd = root.join("project");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::create_dir_all(Path::new(&cwd).join(".pi")).unwrap();
    std::fs::write(Path::new(&agent_dir).join("mcp.json"), global.to_string()).unwrap();
    std::fs::write(Path::new(&cwd).join(".pi/mcp.json"), project.to_string()).unwrap();
    ConfigPaths {
        _root: root,
        agent_dir,
        cwd,
    }
}

fn load(paths: &ConfigPaths, project_trusted: bool) -> LoadedMcpConfig {
    load_mcp_config(&LoadMcpConfigOptions {
        agent_dir: paths.agent_dir.clone(),
        cwd: paths.cwd.clone(),
        project_trusted,
    })
}

#[test]
fn config_merges_global_and_trusted_project_servers_and_validates_entries() {
    let paths = setup_config(
        json!({
            "mcpServers": {
                "shared": { "command": "global-cmd" },
                "remote": { "url": "https://example.com/mcp", "headers": { "Authorization": TOKEN_HEADER } },
                "off": { "command": "x", "enabled": false },
                "bad": { "args": ["no command"] },
                "legacy": { "type": "sse", "url": "https://example.com/sse" },
                "badUrl": { "url": "example.com/mcp" },
                "bad name": { "command": "x" },
            }
        }),
        json!({ "mcpServers": { "shared": { "command": "project-cmd", "exposure": "direct" } } }),
    );
    let trusted = load(&paths, true);
    // Disabled servers are kept so /mcp can enable them again.
    let servers: Vec<(String, Option<McpServerEntryScope>, Value)> = trusted
        .servers
        .iter()
        .map(|server| (server.name.clone(), server.scope, config_value(&server.config)))
        .collect();
    assert_eq!(
        servers,
        vec![
            (
                "shared".to_string(),
                Some(McpServerEntryScope::Project),
                json!({ "command": "project-cmd", "exposure": "direct" })
            ),
            (
                "remote".to_string(),
                Some(McpServerEntryScope::Global),
                json!({ "url": "https://example.com/mcp", "headers": { "Authorization": TOKEN_HEADER } })
            ),
            (
                "off".to_string(),
                Some(McpServerEntryScope::Global),
                json!({ "command": "x", "enabled": false })
            ),
        ]
    );
    assert_eq!(trusted.errors.len(), 4, "{:?}", trusted.errors);
    assert!(trusted.errors[0].contains("server \"bad\" needs either \"command\""));
    assert!(trusted.errors[1].contains("legacy SSE transport is not supported"));
    assert!(trusted.errors[2].contains("server \"badUrl\": url must be an http or https URL"));
    assert!(trusted.errors[3].contains("invalid server name \"bad name\""));

    // Untrusted projects cannot add or override servers, since stdio servers run commands.
    let untrusted = load(&paths, false);
    let shared = untrusted
        .servers
        .iter()
        .find(|server| server.name == "shared")
        .unwrap();
    assert_eq!(config_value(&shared.config), json!({ "command": "global-cmd" }));
}

// #10277
#[test]
fn config_lets_project_entries_override_enabled_and_exposure_of_global_servers() {
    let paths = setup_config(
        json!({ "mcpServers": { "tools": { "command": "x", "env": { "TOKEN": "secret" } } } }),
        // An override cannot change the command, which would run with the global env.
        json!({ "mcpServers": { "tools": { "enabled": false, "args": ["y"] }, "missing": { "enabled": false } } }),
    );
    let project = Path::new(&paths.cwd)
        .join(".pi/mcp.json")
        .to_string_lossy()
        .into_owned();
    let loaded = load(&paths, true);
    assert_eq!(loaded.servers.len(), 1);
    assert_eq!(loaded.servers[0].r#override, None);
    assert_eq!(
        config_value(&loaded.servers[0].config),
        json!({ "command": "x", "env": { "TOKEN": "secret" } })
    );
    assert_eq!(loaded.errors.len(), 2);
    assert!(loaded.errors[0]
        .contains("server \"tools\": an override can only set enabled, exposure, toolExposure"));
    assert!(loaded.errors[1]
        .contains("server \"missing\" needs \"command\" or \"url\", or a global server to override"));

    std::fs::write(
        &project,
        json!({ "mcpServers": { "tools": { "enabled": false } } }).to_string(),
    )
    .unwrap();
    let loaded = load(&paths, true);
    let tools = &loaded.servers[0];
    assert_eq!(tools.r#override.as_deref(), Some(project.as_str()));
    assert_eq!(
        config_value(&tools.config),
        json!({ "command": "x", "env": { "TOKEN": "secret" }, "enabled": false })
    );

    // Overrides keep `enabled: true`, since it replaces the global value.
    update_mcp_server_config(
        &project,
        "tools",
        &McpServerConfigPatch {
            enabled: Some(true),
            exposure: None,
        },
        None,
    )
    .unwrap();
    assert_eq!(read_json(&project)["mcpServers"]["tools"], json!({ "enabled": true }));
}

// Regression: #11191.
#[test]
fn config_reads_and_edits_files_that_start_with_a_bom() {
    let paths = setup_config(json!({}), json!({}));
    let global = Path::new(&paths.agent_dir)
        .join("mcp.json")
        .to_string_lossy()
        .into_owned();
    let project = Path::new(&paths.cwd)
        .join(".pi/mcp.json")
        .to_string_lossy()
        .into_owned();
    std::fs::write(
        &global,
        "\u{FEFF}{\"mcpServers\":{\"demo\":{\"command\":\"echo\",\"enabled\":false}}}\n",
    )
    .unwrap();
    std::fs::write(
        &project,
        "\u{FEFF}{\"mcpServers\":{\"local\":{\"command\":\"local-cmd\"}}}\n",
    )
    .unwrap();
    let loaded = load(&paths, true);
    assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
    let names: Vec<(String, Option<McpServerEntryScope>)> = loaded
        .servers
        .iter()
        .map(|server| (server.name.clone(), server.scope))
        .collect();
    assert_eq!(
        names,
        vec![
            ("demo".to_string(), Some(McpServerEntryScope::Global)),
            ("local".to_string(), Some(McpServerEntryScope::Project)),
        ]
    );
    let other: McpServerConfig = serde_json::from_value(json!({ "command": "other-cmd" })).unwrap();
    assert!(!add_mcp_server_config(&global, "other", &other));
    assert_eq!(
        read_json(&global)["mcpServers"],
        json!({ "demo": { "command": "echo", "enabled": false }, "other": { "command": "other-cmd" } })
    );
}

// Regression: #10239.
#[test]
fn config_rejects_server_names_that_differ_only_in_dash_and_underscore() {
    let paths = setup_config(
        json!({ "mcpServers": { "work-files": { "command": "a" }, "work_files": { "command": "b" } } }),
        json!({}),
    );
    let loaded = load(&paths, false);
    assert_eq!(
        loaded
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect::<Vec<_>>(),
        vec!["work-files"]
    );
    assert_eq!(loaded.errors.len(), 1);
    assert!(loaded.errors[0].contains("server \"work_files\" conflicts with \"work-files\""));
}

#[test]
fn config_validates_exposure_and_reads_auto_enable_codemode_with_project_precedence() {
    let paths = setup_config(
        json!({
            "autoEnableCodemode": false,
            "mcpServers": {
                "later": { "command": "x", "exposure": "deferred" },
                // `codemode-deferred` is an alias for `codemode`.
                "scripts": { "command": "x", "exposure": "codemode-deferred", "toolExposure": { "a": "codemode-deferred" } },
                "off": { "command": "x", "exposure": "hidden" },
                "wrong": { "command": "x", "exposure": "model-only" },
                "described": { "command": "x", "description": "Docs search" },
                "badDescription": { "command": "x", "description": 1 },
            }
        }),
        json!({ "autoEnableCodemode": "yes", "mcpServers": {} }),
    );
    let untrusted = load(&paths, false);
    assert_eq!(untrusted.auto_enable_codemode, Some(false));
    let exposures: Vec<(String, Option<McpExposure>)> = untrusted
        .servers
        .iter()
        .map(|server| (server.name.clone(), server.config.exposure()))
        .collect();
    assert_eq!(
        exposures,
        vec![
            ("later".to_string(), Some(McpExposure::Deferred)),
            ("scripts".to_string(), Some(McpExposure::Codemode)),
            ("off".to_string(), Some(McpExposure::Hidden)),
            ("described".to_string(), None),
        ]
    );
    assert_eq!(
        config_value(&untrusted.servers[1].config)["toolExposure"],
        json!({ "a": "codemode" })
    );
    assert_eq!(
        config_value(&untrusted.servers[3].config)["description"],
        json!("Docs search")
    );
    assert_eq!(untrusted.errors.len(), 2);
    assert!(untrusted.errors[0].contains("server \"wrong\": exposure must be one of"));
    assert!(untrusted.errors[1].contains("server \"badDescription\": description must be a string"));

    let trusted = load(&paths, true);
    assert_eq!(trusted.auto_enable_codemode, Some(false));
    assert!(trusted
        .errors
        .iter()
        .any(|error| error.contains("autoEnableCodemode must be a boolean")));
}

#[test]
fn config_validates_the_oauth_callback_url_scope_and_client_name() {
    let paths = setup_config(
        json!({
            "mcpServers": {
                "ok": { "url": "https://a.example/mcp", "oauth": { "callbackUrl": "http://localhost:8080/callback", "scope": "a b" } },
                "ipv6": { "url": "https://a.example/mcp", "oauth": { "callbackUrl": "http://[::1]/cb", "callbackPort": 9000 } },
                "same": { "url": "https://a.example/mcp", "oauth": { "callbackUrl": "http://127.0.0.1:2/cb", "callbackPort": 2 } },
                "remote": { "url": "https://a.example/mcp", "oauth": { "callbackUrl": "https://example.com/callback" } },
                "both": { "url": "https://a.example/mcp", "oauth": { "callbackUrl": "http://127.0.0.1:1/cb", "callbackPort": 2 } },
                "scope": { "url": "https://a.example/mcp", "oauth": { "scope": ["a"] } },
                "named": { "url": "https://a.example/mcp", "oauth": { "clientName": "Claude Code" } },
                "unnamed": { "url": "https://a.example/mcp", "oauth": { "clientName": " " } },
                "metadata": { "url": "https://a.example/mcp", "oauth": { "authServerMetadataUrl": "https://idp.example/m" } },
                "plainMetadata": { "url": "https://a.example/mcp", "oauth": { "authServerMetadataUrl": "http://idp.example/m" } },
                "cimd": { "url": "https://a.example/mcp", "oauth": { "clientRegistration": "cimd", "callbackUrl": "http://localhost/callback" } },
                "badRegistration": { "url": "https://a.example/mcp", "oauth": { "clientRegistration": "auto" } },
                "cimdClient": { "url": "https://a.example/mcp", "oauth": { "clientRegistration": "cimd", "clientId": "x" } },
                "cimdPath": { "url": "https://a.example/mcp", "oauth": { "clientRegistration": "cimd", "callbackUrl": "http://127.0.0.1/cb" } },
            }
        }),
        json!({}),
    );
    let loaded = load(&paths, false);
    assert_eq!(
        loaded
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect::<Vec<_>>(),
        vec!["ok", "ipv6", "same", "named", "metadata", "cimd"]
    );
    let expected = [
        "server \"remote\": oauth.callbackUrl must be an http URI on localhost",
        "server \"both\": oauth.callbackUrl and oauth.callbackPort name different ports",
        "server \"scope\": oauth.scope must be a string",
        "server \"unnamed\": oauth.clientName must be a non-empty string",
        "server \"plainMetadata\": oauth.authServerMetadataUrl must be an https URL",
        "server \"badRegistration\": oauth.clientRegistration must be \"dcr\" or \"cimd\"",
        "server \"cimdClient\": oauth.clientRegistration \"cimd\" cannot be combined",
        "server \"cimdPath\": oauth.clientRegistration \"cimd\" requires oauth.callbackUrl",
    ];
    assert_eq!(loaded.errors.len(), expected.len(), "{:?}", loaded.errors);
    for (error, expected) in loaded.errors.iter().zip(expected) {
        assert!(error.contains(expected), "{error} should contain {expected}");
    }
}

#[test]
fn config_resolves_per_tool_exposure_from_exact_names_then_patterns_in_order() {
    let paths = setup_config(
        json!({
            "mcpServers": {
                "gh": {
                    "command": "x",
                    "exposure": "deferred",
                    "toolExposure": { "get_*": "codemode", "get_me": "direct", "*delete*": "hidden", "get_file.*": "direct" },
                },
                "bad": { "command": "x", "toolExposure": { "a": "visible" } },
            }
        }),
        json!({}),
    );
    let loaded = load(&paths, false);
    assert_eq!(loaded.errors.len(), 1);
    assert!(loaded.errors[0].contains("server \"bad\": toolExposure \"a\" must be one of"));
    let config = &loaded.servers[0].config;
    assert_eq!(get_mcp_tool_exposure(config, "get_me"), McpExposure::Direct);
    assert_eq!(get_mcp_tool_exposure(config, "get_issue"), McpExposure::Codemode);
    assert_eq!(get_mcp_tool_exposure(config, "get_delete_hint"), McpExposure::Codemode);
    assert_eq!(get_mcp_tool_exposure(config, "delete_repo"), McpExposure::Hidden);
    assert_eq!(get_mcp_tool_exposure(config, "list_issues"), McpExposure::Deferred);
    // Only `*` is special.
    let only_star: McpServerConfig =
        serde_json::from_value(json!({ "command": "x", "toolExposure": { "get_file.*": "direct" } }))
            .unwrap();
    assert_eq!(get_mcp_tool_exposure(&only_star, "get_file_x"), McpExposure::Codemode);
}

#[test]
fn config_accepts_provider_auth_only_in_the_global_file() {
    let paths = setup_config(
        json!({
            "mcpServers": {
                "radius": { "url": "https://radius.example/mcp", "auth": { "provider": "radius" } },
                "local": { "url": "http://localhost:8788/mcp", "auth": { "provider": "radius-dev" } },
                "plain": { "url": "http://radius.example/mcp", "auth": { "provider": "radius" } },
                "empty": { "url": "https://radius.example/mcp", "auth": { "provider": "" } },
            }
        }),
        json!({ "mcpServers": { "radius": { "url": "https://evil.example/mcp", "auth": { "provider": "radius" } } } }),
    );
    let loaded = load(&paths, true);
    // The project entry cannot replace the global one: it would send the credential to its own URL.
    let servers: Vec<(String, Option<McpServerEntryScope>, Value)> = loaded
        .servers
        .iter()
        .map(|server| {
            (
                server.name.clone(),
                server.scope,
                config_value(&server.config)["url"].clone(),
            )
        })
        .collect();
    assert_eq!(
        servers,
        vec![
            (
                "radius".to_string(),
                Some(McpServerEntryScope::Global),
                json!("https://radius.example/mcp")
            ),
            (
                "local".to_string(),
                Some(McpServerEntryScope::Global),
                json!("http://localhost:8788/mcp")
            ),
        ]
    );
    let expected = [
        "server \"plain\": auth requires an https URL",
        "server \"empty\": auth.provider must be a provider name",
        "server \"radius\": auth is only allowed in the global mcp.json",
    ];
    assert_eq!(loaded.errors.len(), expected.len(), "{:?}", loaded.errors);
    for (error, expected) in loaded.errors.iter().zip(expected) {
        assert!(error.contains(expected), "{error} should contain {expected}");
    }
}

#[test]
fn config_add_and_remove_keep_other_content_and_indentation() {
    let dir = TempDir::new("pi-mcp-edit-");
    let path = dir.join("mcp.json");
    std::fs::write(
        &path,
        "{\n    \"other\": 1,\n    \"mcpServers\": {\n        \"a\": { \"command\": \"a\" }\n    }\n}\n",
    )
    .unwrap();
    let b: McpServerConfig = serde_json::from_value(json!({ "command": "b" })).unwrap();
    assert!(!add_mcp_server_config(&path, "b", &b));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("{\n    \"other\": 1,\n    \"mcpServers\": {\n        \"a\""));
    assert!(remove_mcp_server_config(&path, "a"));
    assert!(!remove_mcp_server_config(&path, "a"));
    assert_eq!(
        read_json(&path),
        json!({ "other": 1, "mcpServers": { "b": { "command": "b" } } })
    );
    assert!(!remove_mcp_server_config(&dir.join("missing.json"), "a"));
    let error = update_mcp_server_config(
        &path,
        "missing",
        &McpServerConfigPatch {
            enabled: Some(false),
            exposure: None,
        },
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{path} does not define MCP server \"missing\"")
    );
    // Defaults are removed from entries that define a server.
    update_mcp_server_config(
        &path,
        "b",
        &McpServerConfigPatch {
            enabled: Some(false),
            exposure: Some(McpExposure::Direct),
        },
        None,
    )
    .unwrap();
    assert_eq!(
        read_json(&path)["mcpServers"]["b"],
        json!({ "command": "b", "enabled": false, "exposure": "direct" })
    );
    update_mcp_server_config(
        &path,
        "b",
        &McpServerConfigPatch {
            enabled: Some(true),
            exposure: Some(McpExposure::Codemode),
        },
        None,
    )
    .unwrap();
    assert_eq!(read_json(&path)["mcpServers"]["b"], json!({ "command": "b" }));
}

// ------------------------------------------------------------------------------------------------
// MCP tools
// ------------------------------------------------------------------------------------------------

#[test]
fn tools_create_provider_safe_tool_names() {
    assert_eq!(create_mcp_tool_name("docs", "search", None), "mcp__docs__search");
    assert_eq!(
        create_mcp_tool_name("my-server", "get.item/v2", None),
        "mcp__my_server__get_item_v2"
    );
    let long = create_mcp_tool_name("server", &"x".repeat(100), None);
    assert_eq!(long.len(), 64);
    let pattern = regex::Regex::new(r"^mcp__server__x+_[0-9a-f]{8}$").unwrap();
    assert!(pattern.is_match(&long), "{long}");
    assert_ne!(
        create_mcp_tool_name("server", &format!("{}y", "x".repeat(100)), None),
        long
    );
    // Names that sanitize to one already taken by another tool get a hash suffix.
    let taken = create_mcp_tool_name("s", "a_b", None);
    let second = create_mcp_tool_name(
        "s",
        "a-b",
        Some(Arc::new(move |name: String| name == taken)),
    );
    assert!(
        regex::Regex::new(r"^mcp__s__a_b_[0-9a-f]{8}$")
            .unwrap()
            .is_match(&second),
        "{second}"
    );
}

#[test]
fn tools_declare_the_call_tool_result_keeping_references_in_the_output_schema() {
    use pi_codemode::declarations::render_tool_output_type;
    let content = "content: Array<{ data?: string; mimeType?: string; text?: string; type: \"text\" | \"image\" | \"audio\" | \"resource_link\" | \"resource\"; [key: string]: unknown; }>; isError?: boolean;";
    // #10416: without an output schema, structured content may be any JSON value.
    assert_eq!(
        render_tool_output_type(Some(&create_mcp_result_schema(None))),
        format!("{{ {content} structuredContent?: unknown; [key: string]: unknown; }}")
    );

    let hit = json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] });
    let mut hit_v1 = hit.clone();
    hit_v1["default"] = json!({ "$ref": "#/stays" });
    let structured: IndexMap<String, Value> = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "hits": { "type": "array", "items": { "$ref": "#/$defs/Hit~1v1" } },
            "again": { "$ref": "#/properties/hits" },
            "children": { "type": "array", "items": { "$ref": "#" } },
            // A property named like a keyword whose value is data.
            "default": { "$ref": "#/definitions/Hit" },
            "remote": { "$ref": "https://example.com/schema.json" },
        },
        "required": ["hits"],
        "$defs": { "Hit/v1": hit_v1 },
        "definitions": { "Hit": hit },
    }))
    .unwrap();
    let schema = create_mcp_result_schema(Some(structured));
    // Data that looks like a reference is kept as is.
    assert_eq!(
        schema["properties"]["structuredContent"]["$defs"]["Hit/v1"]["default"],
        json!({ "$ref": "#/stays" })
    );
    assert_eq!(
        schema["properties"]["structuredContent"]["properties"]["children"]["items"],
        json!({ "$ref": "#/properties/structuredContent" })
    );
    assert_eq!(
        render_tool_output_type(Some(&schema)),
        format!(
            "{{ {content} structuredContent?: {{ again?: Array<{{ id: string; }}>; children?: Array<{{ again?: Array<{{ id: string; }}>; children?: Array<unknown>; default?: {{ id: string; }}; hits: Array<{{ id: string; }}>; remote?: unknown; }}>; default?: {{ id: string; }}; hits: Array<{{ id: string; }}>; remote?: unknown; }}; [key: string]: unknown; }}"
        )
    );
}

fn call_result(value: Value) -> CallToolResult {
    serde_json::from_value(value).unwrap()
}

fn result_json(result: &AgentToolResult) -> Value {
    serde_json::to_value(result).unwrap()
}

#[tokio::test]
async fn tools_convert_results_passing_the_call_tool_result_to_scripts() {
    let blocks = json!([
        { "type": "resource_link", "uri": "file:///a", "name": "a" },
        { "type": "resource", "resource": { "uri": "file:///b", "text": "b text" } },
        { "type": "audio", "data": "", "mimeType": "audio/wav" },
    ]);
    let converted = convert_mcp_result(
        "docs",
        "t",
        call_result(json!({ "content": blocks, "structuredContent": { "ok": true }, "_meta": { "trace": "x" } })),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        result_json(&converted),
        json!({
            "content": [
                { "type": "text", "text": "[Resource file:///a \"a\"]" },
                { "type": "text", "text": "b text" },
                { "type": "text", "text": "[audio audio/wav omitted]" },
            ],
            "details": { "server": "docs", "tool": "t" },
            // Scripts get the server's blocks as sent, without `_meta`.
            "structuredContent": { "content": blocks, "structuredContent": { "ok": true } },
        })
    );
    let converted = convert_mcp_result(
        "docs",
        "t",
        call_result(json!({ "content": [], "structuredContent": { "n": 1 } })),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        result_json(&converted)["content"],
        json!([{ "type": "text", "text": "{\n  \"n\": 1\n}" }])
    );
    // #10416: array structured content reaches the model as JSON and scripts as sent.
    let converted = convert_mcp_result(
        "docs",
        "t",
        call_result(json!({ "content": [], "structuredContent": [{ "id": 1 }] })),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        result_json(&converted),
        json!({
            "content": [{ "type": "text", "text": "[\n  {\n    \"id\": 1\n  }\n]" }],
            "details": { "server": "docs", "tool": "t" },
            "structuredContent": { "content": [], "structuredContent": [{ "id": 1 }] },
        })
    );
    let converted = convert_mcp_result(
        "docs",
        "t",
        call_result(json!({ "content": [{ "type": "text", "text": "nope" }], "isError": true })),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        result_json(&converted),
        json!({
            "content": [{ "type": "text", "text": "nope" }],
            "details": { "server": "docs", "tool": "t" },
            "structuredContent": { "content": [{ "type": "text", "text": "nope" }], "isError": true },
            "isError": true,
        })
    );
    let converted = convert_mcp_result(
        "docs",
        "t",
        call_result(json!({ "content": [], "isError": true })),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        result_json(&converted)["content"],
        json!([{ "type": "text", "text": "MCP tool docs/t returned an error" }])
    );
}

fn recording_saver(
    saved: Arc<Mutex<Vec<(McpOutputSaverData, String)>>>,
    path: Option<&'static str>,
) -> McpOutputSaver {
    Arc::new(move |data, extension| {
        let saved = saved.clone();
        Box::pin(async move {
            let result = match path {
                Some(path) => path.to_string(),
                None => format!("/tmp/saved{extension}"),
            };
            saved.lock().push((data, extension));
            Ok(result)
        })
    })
}

#[tokio::test]
async fn tools_point_resource_links_to_read_mcp_resource_and_save_binary_resources() {
    let saved: Arc<Mutex<Vec<(McpOutputSaverData, String)>>> = Arc::default();
    let converted = convert_mcp_result(
        "docs",
        "t",
        call_result(json!({
            "content": [
                {
                    "type": "resource_link",
                    "uri": "docs://guide",
                    "name": "guide",
                    "title": "The Guide",
                    "mimeType": "text/markdown",
                    "size": 2048,
                    "description": "How to use it",
                },
                { "type": "resource", "resource": { "uri": "file:///r/report.pdf", "mimeType": "application/pdf", "blob": "JVBERg==" } },
                { "type": "resource", "resource": { "uri": "docs://logo", "mimeType": "image/png", "blob": "AAAA" } },
            ]
        })),
        Some(&ConvertMcpResultOptions {
            save_output: Some(recording_saver(saved.clone(), None)),
            readable_resources: Some(true),
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        result_json(&converted)["content"],
        json!([
            {
                "type": "text",
                "text": "[Resource docs://guide \"The Guide\" (text/markdown, 2.0KB): How to use it. Read it with read_mcp_resource (server \"docs\")]",
            },
            { "type": "text", "text": "[Binary resource file:///r/report.pdf (application/pdf, 4B) saved to /tmp/saved.pdf]" },
            { "type": "image", "data": "AAAA", "mimeType": "image/png" },
        ])
    );
    assert_eq!(
        *saved.lock(),
        vec![(McpOutputSaverData::List(b"%PDF".to_vec()), ".pdf".to_string())]
    );
}

#[tokio::test]
async fn tools_cut_the_middle_of_text_over_20kb_and_keep_the_full_result_for_scripts() {
    let saved: Arc<Mutex<Vec<(McpOutputSaverData, String)>>> = Arc::default();
    let saver = recording_saver(saved.clone(), Some("/tmp/full.txt"));
    let full = (1..=3000)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let image = json!({ "type": "image", "data": "AAAA", "mimeType": "image/png" });
    let result = json!({ "content": [{ "type": "text", "text": full }, image] });
    let converted = convert_mcp_result(
        "docs",
        "snapshot",
        call_result(result.clone()),
        Some(&ConvertMcpResultOptions {
            save_output: Some(saver.clone()),
            readable_resources: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(converted.content.len(), 2);
    let text = converted.content[0].as_text().unwrap().text.clone();
    let tokens = (full.len() as f64 / 4.0).ceil() as i64;
    assert!(
        text.starts_with(&format!(
            "Warning: truncated output (original token count: {tokens})\nTotal output lines: 3000\n\nline 1\nline 2\n"
        )),
        "{}",
        &text[..200]
    );
    assert!(regex::Regex::new(r"…\d+ chars truncated…").unwrap().is_match(&text));
    assert!(text.ends_with("line 3000\n\n[Full output: /tmp/full.txt (read it with offset/limit)]"));
    assert!(text.len() < 21 * 1024);
    assert_eq!(result_json(&converted)["content"][1], image);
    assert_eq!(
        converted.details,
        Some(json!({ "server": "docs", "tool": "snapshot", "fullOutputPath": "/tmp/full.txt" }))
    );
    assert_eq!(
        *saved.lock(),
        vec![(McpOutputSaverData::String(full.clone()), ".txt".to_string())]
    );
    assert_eq!(converted.structured_content, Some(result));

    // Text within the limit is not saved.
    convert_mcp_result(
        "docs",
        "small",
        call_result(json!({ "content": [{ "type": "text", "text": "ok" }] })),
        Some(&ConvertMcpResultOptions {
            save_output: Some(saver),
            readable_resources: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(saved.lock().len(), 1);
}

// ------------------------------------------------------------------------------------------------
// MCP connections
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct FakeServerOptions {
    methods: Option<Arc<Mutex<Vec<String>>>>,
    no_tools: bool,
    resources: bool,
    tools: Vec<Value>,
    instructions: Option<String>,
    /// Requests of these methods are received but never answered.
    silent: Vec<String>,
}

/// In-memory server that answers initialize, tools/list, resources, and tools/call with "ok".
/// Returns the client end and the server end.
fn fake_server(options: FakeServerOptions) -> (InMemoryTransport, InMemoryTransport) {
    let (client, server) = create_in_memory_transport_pair();
    let responder = server.clone();
    let _ = server.on_message(move |message| {
        let JsonRpcMessage::Request(request) = &message else {
            return;
        };
        let method = request.method.clone();
        let id = request.id.clone();
        if let Some(methods) = &options.methods {
            methods.lock().push(method.clone());
        }
        if options.silent.contains(&method) {
            return;
        }
        let response = match method.as_str() {
            "initialize" => {
                let mut capabilities = serde_json::Map::new();
                if options.no_tools {
                    capabilities.insert("prompts".into(), json!({}));
                } else {
                    capabilities.insert("tools".into(), json!({}));
                }
                if options.resources {
                    capabilities.insert("resources".into(), json!({}));
                }
                let mut result = json!({
                    "protocolVersion": LATEST_PROTOCOL_VERSION,
                    "capabilities": capabilities,
                    "serverInfo": { "name": "fake", "version": "1.0.0" },
                });
                if let Some(instructions) = &options.instructions {
                    result["instructions"] = json!(instructions);
                }
                JsonRpcMessage::result(id, result)
            }
            "tools/list" if options.no_tools => {
                JsonRpcMessage::error(id, -32601, "Method not found", None)
            }
            "tools/list" => JsonRpcMessage::result(id, json!({ "tools": options.tools })),
            "resources/read" => JsonRpcMessage::result(
                id,
                json!({ "contents": [{ "uri": "docs://a", "text": "ok" }] }),
            ),
            "resources/list" => JsonRpcMessage::result(
                id,
                json!({ "resources": [
                    { "uri": "docs://a", "name": "a", "_meta": { "x": 1 }, "icons": [] },
                    { "uri": "ui://app", "name": "app" },
                ] }),
            ),
            "resources/templates/list" => {
                JsonRpcMessage::error(id, -32601, "Method not found", None)
            }
            _ => JsonRpcMessage::result(
                id,
                json!({ "content": [{ "type": "text", "text": "ok" }] }),
            ),
        };
        // In-memory sends and starts happen at the call; the futures only carry the result.
        drop(responder.send(response, McpTransportSendOptions::default()));
    });
    let server_start = server.clone();
    drop(server_start.start());
    (client, server)
}

type SendHook = Arc<dyn Fn(&JsonRpcMessage) -> Option<McpError> + Send + Sync>;

/// A transport that fails the sends `hook` returns an error for.
struct HookTransport {
    inner: InMemoryTransport,
    hook: SendHook,
}

impl McpTransport for HookTransport {
    fn start(&self) -> BoxFuture<'static, pi_mcp::Result<()>> {
        self.inner.start()
    }

    fn send(
        &self,
        message: JsonRpcMessage,
        options: McpTransportSendOptions,
    ) -> BoxFuture<'static, pi_mcp::Result<()>> {
        if let Some(error) = (self.hook)(&message) {
            return Box::pin(async move { Err(error) });
        }
        self.inner.send(message, options)
    }

    fn close(&self) -> BoxFuture<'static, pi_mcp::Result<()>> {
        self.inner.close()
    }

    fn events(&self) -> &TransportEvents {
        self.inner.events()
    }
}

fn hooked(inner: InMemoryTransport, hook: impl Fn(&JsonRpcMessage) -> Option<McpError> + Send + Sync + 'static) -> McpTransportHandle {
    McpTransportHandle::new(HookTransport {
        inner,
        hook: Arc::new(hook),
    })
}

fn method_of(message: &JsonRpcMessage) -> String {
    message.method().unwrap_or("").to_string()
}

type TransportMaker = Box<dyn Fn() -> McpTransportHandle + Send + Sync>;

fn connect(
    entry: McpServerEntry,
    transports: Vec<TransportMaker>,
    log: Option<Arc<McpServerLog>>,
) -> (Arc<McpServerConnection>, Arc<AtomicUsize>) {
    let opened = Arc::new(AtomicUsize::new(0));
    let counter = opened.clone();
    let transports = Arc::new(transports);
    let connection = McpServerConnection::new(&McpServerConnectionOptions {
        entry,
        cwd: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        create_transport: Arc::new(move |_, _, _| {
            let index = counter.fetch_add(1, Ordering::SeqCst);
            Ok(transports[index]())
        }),
        credentials: credentials(),
        provider_token: None,
        on_tools: Arc::new(|_| {}),
        on_change: None,
        log,
    });
    (connection, opened)
}

fn plain(transport: InMemoryTransport) -> McpTransportHandle {
    McpTransportHandle::new(transport)
}

fn ok_result() -> CallToolResult {
    call_result(json!({ "content": [{ "type": "text", "text": "ok" }] }))
}

#[tokio::test]
async fn connections_start_a_new_session_and_retry_once_when_the_session_expired() {
    let (connection, opened) = connect(
        stdio_entry("fake"),
        vec![
            Box::new(|| {
                hooked(fake_server(Default::default()).0, |message| {
                    (method_of(message) == "tools/call").then(|| McpError::SessionExpired {
                        body: "gone".into(),
                    })
                })
            }),
            Box::new(|| plain(fake_server(Default::default()).0)),
        ],
        None,
    );
    let (first, second) = tokio::join!(
        connection.call_tool("echo", IndexMap::new(), Default::default()),
        connection.call_tool("echo", IndexMap::new(), Default::default()),
    );
    assert_eq!(first.unwrap(), ok_result());
    assert_eq!(second.unwrap(), ok_result());
    assert_eq!(opened.load(Ordering::SeqCst), 2);
    connection.close().await.unwrap();
}

#[test]
fn connections_expand_home_in_stdio_command_arguments_and_cwd() {
    let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();
    let entry = entry_with(
        "home",
        json!({ "command": "~/bin/server", "args": ["~/server.mjs", "x~"], "cwd": "~/work" }),
    );
    let handle = create_default_transport(&entry, "/tmp", None).unwrap();
    let stdio = handle.stdio.unwrap();
    let options = stdio.options();
    assert_eq!(
        options.command,
        Path::new(&home).join("bin/server").to_string_lossy()
    );
    assert_eq!(
        options.args,
        vec![
            Path::new(&home)
                .join("server.mjs")
                .to_string_lossy()
                .into_owned(),
            "x~".to_string()
        ]
    );
    assert_eq!(options.cwd.as_deref(), Some(Path::new(&home).join("work").as_path()));
    let relative = create_default_transport(
        &entry_with("rel", json!({ "command": "x", "cwd": "sub/../dir" })),
        "/base",
        None,
    )
    .unwrap();
    assert_eq!(
        relative.stdio.unwrap().options().cwd.as_deref(),
        Some(Path::new("/base/dir"))
    );
}

#[tokio::test]
async fn connections_connect_to_servers_without_the_tools_capability_without_listing_tools() {
    let methods: Arc<Mutex<Vec<String>>> = Arc::default();
    let server_methods = methods.clone();
    let (connection, _) = connect(
        stdio_entry("fake"),
        vec![Box::new(move || {
            plain(
                fake_server(FakeServerOptions {
                    no_tools: true,
                    methods: Some(server_methods.clone()),
                    ..Default::default()
                })
                .0,
            )
        })],
        None,
    );
    connection.get_client().await.unwrap();
    assert_eq!(connection.state(), ServerState::Connected);
    assert!(connection.tools().is_empty());
    assert_eq!(*methods.lock(), vec!["initialize".to_string()]);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn connections_mark_a_dropped_connection_and_reconnect_on_the_next_call() {
    let servers: Arc<Mutex<Vec<InMemoryTransport>>> = Arc::default();
    let make = |servers: Arc<Mutex<Vec<InMemoryTransport>>>| -> TransportMaker {
        Box::new(move || {
            let (client, server) = fake_server(Default::default());
            servers.lock().push(server);
            plain(client)
        })
    };
    let (connection, opened) = connect(
        stdio_entry("fake"),
        vec![make(servers.clone()), make(servers.clone())],
        None,
    );
    connection.get_client().await.unwrap();
    let server = servers.lock().last().cloned().unwrap();
    let _ = server.close().await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(connection.state(), ServerState::Disconnected);
    assert_eq!(connection.error().as_deref(), Some("Connection closed"));
    assert_eq!(
        connection
            .call_tool("echo", IndexMap::new(), Default::default())
            .await
            .unwrap(),
        ok_result()
    );
    assert_eq!(connection.state(), ServerState::Connected);
    assert_eq!(opened.load(Ordering::SeqCst), 2);
    connection.close().await.unwrap();
}

fn http_error(status: u16, message: &str) -> McpError {
    McpError::Http {
        status,
        message: message.to_string(),
        body: String::new(),
    }
}

fn http_entry() -> McpServerEntry {
    entry_with(
        "fake",
        json!({ "url": "http://unused.invalid", "headers": { "Authorization": "x" } }),
    )
}

#[tokio::test]
async fn connections_retry_http_connections_that_fail_with_a_transient_error() {
    let (connection, opened) = connect(
        http_entry(),
        vec![
            Box::new(|| {
                hooked(fake_server(Default::default()).0, |_| {
                    Some(http_error(503, "MCP HTTP request failed with status 503"))
                })
            }),
            Box::new(|| plain(fake_server(Default::default()).0)),
        ],
        None,
    );
    connection.get_client().await.unwrap();
    assert_eq!(connection.state(), ServerState::Connected);
    assert_eq!(opened.load(Ordering::SeqCst), 2);
    connection.close().await.unwrap();

    let (failing, opened) = connect(
        http_entry(),
        vec![Box::new(|| {
            hooked(fake_server(Default::default()).0, |_| {
                Some(http_error(400, "MCP HTTP request failed with status 400: bad"))
            })
        })],
        None,
    );
    let error = failing.get_client().await.unwrap_err();
    assert!(error.to_string().contains("status 400: bad"), "{error}");
    assert_eq!(failing.state(), ServerState::Failed);
    assert_eq!(opened.load(Ordering::SeqCst), 1);
}

// #10249
#[tokio::test]
async fn connections_close_a_connection_that_is_still_initializing_before_close_returns() {
    let (client, server) = create_in_memory_transport_pair();
    let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
    let sender = Mutex::new(Some(sender));
    // The server receives initialize but never answers it.
    let _ = server.on_message(move |_| {
        if let Some(sender) = sender.lock().take() {
            let _ = sender.send(());
        }
    });
    // The in-memory start happens at the call; the future only carries the result.
    drop(server.start());
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = closed.clone();
    let _ = client.events().on_close(move || flag.store(true, Ordering::SeqCst));
    let transport = Mutex::new(Some(client));
    let (connection, _) = connect(
        stdio_entry("fake"),
        vec![Box::new(move || plain(transport.lock().take().unwrap()))],
        None,
    );
    let pending = {
        let connection = connection.clone();
        tokio::spawn(async move { connection.get_client().await })
    };
    receiver.await.unwrap();
    connection.close().await.unwrap();
    assert!(closed.load(Ordering::SeqCst));
    let failure = pending.await.unwrap().unwrap_err();
    assert!(failure.to_string().contains("failed to connect"), "{failure}");
}

// #10249
#[tokio::test]
async fn connections_stop_waiting_to_retry_a_connection_when_closed() {
    let (connection, _) = connect(
        http_entry(),
        vec![Box::new(|| {
            hooked(fake_server(Default::default()).0, |_| {
                Some(http_error(503, "MCP HTTP request failed with status 503"))
            })
        })],
        None,
    );
    let pending = {
        let connection = connection.clone();
        tokio::spawn(async move { connection.get_client().await })
    };
    // The first attempt failed; the retry is 250ms away.
    tokio::time::sleep(Duration::from_millis(10)).await;
    let started = std::time::Instant::now();
    connection.close().await.unwrap();
    assert!(started.elapsed() < Duration::from_millis(200));
    let failure = pending.await.unwrap().unwrap_err();
    assert!(failure.to_string().contains("status 503"), "{failure}");
}

#[tokio::test]
async fn connections_retry_resource_reads_but_not_tool_calls_after_a_transient_error() {
    let failed: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = failed.clone();
    let transport = Mutex::new(Some(hooked(
        fake_server(Default::default()).0,
        move |message| {
            let method = method_of(message);
            if (method == "resources/read" || method == "tools/call")
                && !seen.lock().contains(&method)
            {
                seen.lock().push(method);
                return Some(http_error(502, "MCP HTTP request failed with status 502"));
            }
            None
        },
    )));
    let (connection, _) = connect(
        stdio_entry("fake"),
        vec![Box::new(move || transport.lock().take().unwrap())],
        None,
    );
    let read = connection
        .read_resource("docs://a", Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(read).unwrap(),
        json!({ "contents": [{ "uri": "docs://a", "text": "ok" }] })
    );
    let error = connection
        .call_tool("echo", IndexMap::new(), Default::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("status 502"), "{error}");
    connection.close().await.unwrap();
}

#[tokio::test]
async fn connections_ask_oauth_servers_that_keep_rejecting_requests_for_a_new_sign_in() {
    let (connection, _) = connect(
        entry_with("fake", json!({ "url": "http://unused.invalid" })),
        vec![Box::new(|| {
            hooked(fake_server(Default::default()).0, |_| {
                Some(McpError::AuthRequired {
                    body: String::new(),
                    www_authenticate: None,
                })
            })
        })],
        None,
    );
    let error = connection.get_client().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "MCP server \"fake\" requires sign-in. Run /mcp to sign in."
    );
    assert_eq!(connection.state(), ServerState::NeedsAuth);
    connection.close().await.unwrap();
}

/// A loopback HTTP server answering every request with `respond`; records each request.
struct TestHttpServer {
    origin: String,
    requests: Arc<Mutex<Vec<test_http::Request>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

mod test_http {
    //! A tiny HTTP/1.1 server for tests: one request per connection.
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[derive(Clone, Debug)]
    pub struct Request {
        pub method: String,
        pub target: String,
        pub headers: HashMap<String, String>,
        pub body: Vec<u8>,
    }

    pub struct Response {
        pub status: u16,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Response {
        pub fn new(status: u16) -> Self {
            Self {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            }
        }

        pub fn json(status: u16, body: &serde_json::Value) -> Self {
            Self {
                status,
                headers: vec![("content-type".into(), "application/json".into())],
                body: body.to_string().into_bytes(),
            }
        }

        pub fn header(mut self, name: &str, value: &str) -> Self {
            self.headers.push((name.into(), value.into()));
            self
        }
    }

    pub async fn read_request(stream: &mut TcpStream) -> Option<Request> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break index;
            }
            let read = stream.read(&mut chunk).await.ok()?;
            if read == 0 {
                return None;
            }
            buffer.extend_from_slice(&chunk[..read]);
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
        let mut lines = head.split("\r\n");
        let mut first = lines.next()?.split_whitespace();
        let method = first.next()?.to_string();
        let target = first.next()?.to_string();
        let mut headers = HashMap::new();
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        let length: usize = headers
            .get("content-length")
            .and_then(|length| length.parse().ok())
            .unwrap_or(0);
        let mut body = buffer[head_end + 4..].to_vec();
        while body.len() < length {
            let read = stream.read(&mut chunk).await.ok()?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read]);
        }
        Some(Request {
            method,
            target,
            headers,
            body,
        })
    }

    pub async fn write_response(stream: &mut TcpStream, response: Response) {
        let mut head = format!("HTTP/1.1 {} X\r\n", response.status);
        for (name, value) in &response.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!(
            "content-length: {}\r\nconnection: close\r\n\r\n",
            response.body.len()
        ));
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(&response.body).await;
        let _ = stream.shutdown().await;
    }
}

type Responder =
    Arc<dyn Fn(test_http::Request, String) -> BoxFuture<'static, Option<test_http::Response>> + Send + Sync>;

async fn start_http_server(respond: Responder) -> TestHttpServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests: Arc<Mutex<Vec<test_http::Request>>> = Arc::default();
    let recorded = requests.clone();
    let server_origin = origin.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let respond = respond.clone();
            let recorded = recorded.clone();
            let origin = server_origin.clone();
            tokio::spawn(async move {
                let Some(request) = test_http::read_request(&mut stream).await else {
                    return;
                };
                recorded.lock().push(request.clone());
                // `None` stalls the request, like an unresponsive server.
                match respond(request, origin).await {
                    Some(response) => test_http::write_response(&mut stream, response).await,
                    None => {
                        tokio::time::sleep(Duration::from_secs(3600)).await;
                    }
                }
            });
        }
    });
    TestHttpServer {
        origin,
        requests,
        task,
    }
}

#[tokio::test]
async fn connections_send_the_provider_token_and_ask_for_the_provider_login_when_rejected() {
    let server = start_http_server(Arc::new(|_, _| {
        Box::pin(async { Some(test_http::Response::new(401)) })
    }))
    .await;
    let connection = McpServerConnection::new(&McpServerConnectionOptions {
        entry: entry_with(
            "radius",
            json!({ "url": format!("{}/mcp", server.origin), "auth": { "provider": "radius" } }),
        ),
        cwd: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        create_transport: default_transport_factory(),
        credentials: credentials(),
        provider_token: Some(Arc::new(|provider: String| {
            Box::pin(async move { Ok((provider == "radius").then(|| "tok".to_string())) })
        })),
        on_tools: Arc::new(|_| {}),
        on_change: None,
        log: None,
    });
    assert_eq!(connection.oauth_url(), None);
    let error = connection.get_client().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "MCP server \"radius\" requires sign-in. Run /login radius to sign in."
    );
    assert_eq!(connection.state(), ServerState::NeedsAuth);
    let authorizations: Vec<Option<String>> = server
        .requests
        .lock()
        .iter()
        .map(|request| request.headers.get("authorization").cloned())
        .collect();
    assert_eq!(authorizations, vec![Some("Bearer tok".to_string())]);
    connection.close().await.unwrap();
}

#[tokio::test]
async fn connections_append_server_log_messages_to_the_log_file() {
    let dir = TempDir::new("pi-mcp-log-");
    let path = dir.join("mcp.log");
    let servers: Arc<Mutex<Vec<InMemoryTransport>>> = Arc::default();
    let recorded = servers.clone();
    let (connection, _) = connect(
        stdio_entry("fake"),
        vec![Box::new(move || {
            let (client, server) = fake_server(Default::default());
            recorded.lock().push(server);
            plain(client)
        })],
        Some(Arc::new(McpServerLog::new(&path))),
    );
    connection.get_client().await.unwrap();
    let server = servers.lock().last().cloned().unwrap();
    let _ = server
        .send(
            JsonRpcMessage::notification(
                "notifications/message",
                Some(json!({ "level": "warning", "logger": "db", "data": "slow\nquery" })),
            ),
            Default::default(),
        )
        .await;
    let _ = server
        .send(
            JsonRpcMessage::notification(
                "notifications/message",
                Some(json!({ "level": "error", "data": { "code": 7 } })),
            ),
            Default::default(),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let text = std::fs::read_to_string(&path).unwrap();
    let lines = regex::Regex::new(r"(?m)^\S+ ")
        .unwrap()
        .replace_all(&text, "")
        .into_owned();
    assert_eq!(lines, "[fake] warning db: slow\n    query\n[fake] error {\"code\":7}\n");
    connection.close().await.unwrap();
}

#[tokio::test]
async fn connections_resolve_the_oauth_client_secret_lazily() {
    let (connection, _) = connect(
        entry_with(
            "fake",
            json!({ "url": "http://unused.invalid", "oauth": { "clientSecret": "!exit 1" } }),
        ),
        vec![Box::new(|| plain(fake_server(Default::default()).0))],
        None,
    );
    assert_eq!(
        connection
            .call_tool("echo", IndexMap::new(), Default::default())
            .await
            .unwrap(),
        ok_result()
    );
    let error = connection.oauth_settings().unwrap_err();
    assert!(error.to_string().contains("oauth.clientSecret"), "{error}");
    connection.close().await.unwrap();
}

// #10526: listing every resource can take long (a large drive), so connecting must not wait for it.
#[tokio::test]
async fn connections_connect_to_servers_with_resources_without_listing_them() {
    let methods: Arc<Mutex<Vec<String>>> = Arc::default();
    let server_methods = methods.clone();
    let (connection, _) = connect(
        stdio_entry("fake"),
        vec![Box::new(move || {
            plain(
                fake_server(FakeServerOptions {
                    resources: true,
                    methods: Some(server_methods.clone()),
                    ..Default::default()
                })
                .0,
            )
        })],
        None,
    );
    connection.get_client().await.unwrap();
    assert_eq!(connection.state(), ServerState::Connected);
    assert!(connection.has_resources());
    assert_eq!(
        *methods.lock(),
        vec!["initialize".to_string(), "tools/list".to_string()]
    );
    connection.close().await.unwrap();
}

// ------------------------------------------------------------------------------------------------
// MCP servers section
// ------------------------------------------------------------------------------------------------

fn listing(name: &str, description: Option<&str>, exposure: Option<&str>) -> McpServerListing {
    let mut config = json!({ "command": "x" });
    if let Some(description) = description {
        config["description"] = json!(description);
    }
    if let Some(exposure) = exposure {
        config["exposure"] = json!(exposure);
    }
    McpServerListing {
        entry: entry_with(name, config),
        connection: None,
    }
}

#[test]
fn section_lists_servers_with_how_their_tools_are_reached_and_their_description() {
    let mut plain_server = listing("plain", None, None);
    plain_server.connection = Some(McpServerListingConnection {
        instructions: Some("From instructions.".into()),
    });
    let section = render_servers_section(&[
        listing("docs", Some("Docs search.\nMore."), None),
        listing("later", None, Some("deferred")),
        listing("direct", Some("Declared."), Some("direct")),
        plain_server,
    ])
    .unwrap();
    assert_eq!(
        section.split('\n').skip(1).collect::<Vec<_>>(),
        vec![
            "- mcp__docs (codemode): Docs search.",
            "- mcp__later (tool_search)",
            "- mcp__plain (codemode): From instructions.",
        ]
    );
    assert_eq!(
        render_servers_section(&[listing("direct", Some("Declared."), Some("direct"))]),
        None
    );
}

#[test]
fn section_shortens_descriptions_to_fit_the_size_limit() {
    let description = "x".repeat(400);
    let servers: Vec<McpServerListing> = (0..40)
        .map(|index| listing(&format!("server{index}"), Some(&description), None))
        .collect();
    let section = render_servers_section(&servers).unwrap();
    assert!(section.chars().count() as i64 <= MAX_SERVERS_SECTION_CHARS);
    assert_eq!(section.split('\n').count(), 41);
    assert!(section.contains("- mcp__server39 (codemode): x"));
}

#[test]
fn section_leaves_out_the_last_servers_when_their_names_alone_do_not_fit() {
    let servers: Vec<McpServerListing> = (0..200)
        .map(|index| listing(&format!("server-with-a-long-name-{index}"), Some("desc"), None))
        .collect();
    let section = render_servers_section(&servers).unwrap();
    assert!(section.chars().count() as i64 <= MAX_SERVERS_SECTION_CHARS);
    let lines: Vec<&str> = section.split('\n').collect();
    let last = lines.last().unwrap();
    let pattern =
        regex::Regex::new(r"^- … (\d+) more servers; find their tools with searchTools\(\)$").unwrap();
    let omitted: usize = pattern.captures(last).expect(last)[1].parse().unwrap();
    assert_eq!(lines.len() - 2 + omitted, 200);
}

// ------------------------------------------------------------------------------------------------
// `pi mcp`
// ------------------------------------------------------------------------------------------------

fn fixture() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pi-mcp/tests/fixtures/stdio-server.sh")
        .to_string_lossy()
        .into_owned()
}

struct CommandRun {
    exit_code: i32,
    output: String,
    agent_dir: String,
}

async fn run_command(args: &[&str], servers: Option<Value>, dir: Option<&TempDir>) -> (CommandRun, Option<TempDir>) {
    let owned = if dir.is_none() {
        Some(TempDir::new("pi-mcp-command-"))
    } else {
        None
    };
    let agent_dir = dir.or(owned.as_ref()).unwrap().path();
    if let Some(servers) = servers {
        std::fs::write(
            Path::new(&agent_dir).join("mcp.json"),
            json!({ "mcpServers": servers }).to_string(),
        )
        .unwrap();
    }
    let output: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = output.clone();
    let error = output.clone();
    let exit_code = cli::run_mcp_command(
        &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
        &cli::McpCommandOptions {
            cwd: agent_dir.clone(),
            agent_dir: agent_dir.clone(),
            credentials: Some(credentials()),
            open_url: None,
            log: Some(Arc::new(move |line| log.lock().push(line))),
            error: Some(Arc::new(move |line| error.lock().push(line))),
        },
    )
    .await
    .unwrap();
    let output = output.lock().join("\n");
    (
        CommandRun {
            exit_code,
            output,
            agent_dir,
        },
        owned,
    )
}

fn command_servers() -> Value {
    json!({
        "fixture": { "command": "sh", "args": [fixture()] },
        "broken": { "command": "pi-test-missing-mcp-server" },
        "parked": { "command": "sh", "args": [fixture()], "enabled": false },
        "bad": { "args": ["no command"] },
    })
}

#[tokio::test]
async fn command_lists_servers_with_their_state_tools_and_errors() {
    let (run, _dir) = run_command(&["list"], Some(command_servers()), None).await;
    assert_eq!(run.exit_code, 1, "{}", run.output);
    assert!(run.output.contains("fixture: connected, 1 tool (codemode, global)\n"), "{}", run.output);
    assert!(run.output.contains("  tools: echo"));
    assert!(run.output.contains(
        "broken: failed (codemode, global)\n  pi-test-missing-mcp-server\n  spawn pi-test-missing-mcp-server ENOENT"
    ), "{}", run.output);
    assert!(run.output.contains("parked: disabled (codemode, global)"));
    assert!(run.output.contains("config error: "));
    assert!(run.output.contains("server \"bad\" needs either \"command\""));

    let (ok, _dir) = run_command(
        &["list"],
        Some(json!({ "fixture": command_servers()["fixture"] })),
        None,
    )
    .await;
    assert_eq!(ok.exit_code, 0, "{}", ok.output);
}

#[tokio::test]
async fn command_prints_json_for_scripts() {
    let servers = command_servers();
    let (run, _dir) = run_command(
        &["list", "--json"],
        Some(json!({ "fixture": servers["fixture"], "parked": servers["parked"] })),
        None,
    )
    .await;
    assert_eq!(run.exit_code, 0, "{}", run.output);
    let parsed: Value = serde_json::from_str(&run.output).unwrap();
    let summary: Vec<Value> = parsed["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|server| json!({ "name": server["name"], "state": server["state"], "tools": server["tools"] }))
        .collect();
    assert_eq!(
        summary,
        vec![
            json!({ "name": "fixture", "state": "connected", "tools": ["echo"] }),
            json!({ "name": "parked", "state": "disabled", "tools": [] }),
        ]
    );
}

#[tokio::test]
async fn command_rejects_unknown_servers_and_servers_without_oauth() {
    let (run, _dir) = run_command(&["login", "nope"], Some(command_servers()), None).await;
    assert_eq!(run.exit_code, 1);
    assert_eq!(
        run.output,
        "No MCP server named \"nope\". Configured: fixture, broken, parked."
    );
    let (run, _dir) = run_command(&["logout", "fixture"], Some(command_servers()), None).await;
    assert_eq!(run.exit_code, 1);
    assert_eq!(
        run.output,
        "MCP server \"fixture\" does not use OAuth. Only HTTP servers without an Authorization header do."
    );
    let (run, _dir) = run_command(&["frobnicate"], Some(command_servers()), None).await;
    assert_eq!(run.exit_code, 1);
}

#[tokio::test]
async fn command_adds_stdio_servers_and_passes_options_after_the_command_through() {
    let (added, dir) = run_command(
        &["add", "--env", "A=1", "--env", "B=x=y", "files", "--", "npx", "-y", "server", "--root", "."],
        None,
        None,
    )
    .await;
    let dir = dir.unwrap();
    assert_eq!(added.exit_code, 0, "{}", added.output);
    assert!(added.output.contains("Added global MCP server \"files\""));
    let path = Path::new(&added.agent_dir).join("mcp.json").to_string_lossy().into_owned();
    assert_eq!(
        read_json(&path),
        json!({ "mcpServers": { "files": { "command": "npx", "args": ["-y", "server", "--root", "."], "env": { "A": "1", "B": "x=y" } } } })
    );
    // Without `--`, options after the command belong to the command too.
    let (replaced, _) = run_command(
        &["add", "files", "node", "server.js", "--port", "1"],
        None,
        Some(&dir),
    )
    .await;
    assert!(replaced.output.contains("Replaced global MCP server \"files\""), "{}", replaced.output);
    assert_eq!(
        read_json(&path),
        json!({ "mcpServers": { "files": { "command": "node", "args": ["server.js", "--port", "1"] } } })
    );
}

#[tokio::test]
async fn command_adds_http_servers_and_keeps_other_content() {
    let fixture_server = command_servers()["fixture"].clone();
    let (run, dir) = run_command(
        &[
            "add",
            "docs",
            "--url",
            "https://example.com/mcp",
            "--bearer-token-env-var",
            "DOCS_TOKEN",
            "--header",
            "X-Team=core",
            "--exposure",
            "direct",
            "--description",
            "Product docs",
        ],
        Some(json!({ "fixture": fixture_server })),
        None,
    )
    .await;
    let dir = dir.unwrap();
    assert_eq!(run.exit_code, 0, "{}", run.output);
    assert!(!run.output.contains("mcp login"));
    let path = Path::new(&run.agent_dir).join("mcp.json").to_string_lossy().into_owned();
    assert_eq!(
        read_json(&path),
        json!({
            "mcpServers": {
                "fixture": fixture_server,
                "docs": {
                    "url": "https://example.com/mcp",
                    "headers": { "X-Team": "core", "Authorization": "Bearer ${DOCS_TOKEN}" },
                    "exposure": "direct",
                    "description": "Product docs",
                },
            }
        })
    );
    let (oauth, _) = run_command(
        &[
            "add",
            "sentry",
            "--url",
            "https://mcp.sentry.dev/mcp",
            "--oauth-client-id",
            "pi",
            "--oauth-client-name",
            "Claude Code",
        ],
        None,
        Some(&dir),
    )
    .await;
    assert!(oauth.output.contains("If it requires sign-in: pi mcp login sentry"), "{}", oauth.output);
    assert_eq!(
        read_json(&path)["mcpServers"]["sentry"],
        json!({ "url": "https://mcp.sentry.dev/mcp", "oauth": { "clientId": "pi", "clientName": "Claude Code" } })
    );
}

#[tokio::test]
async fn command_rejects_invalid_add_invocations_without_writing() {
    let cases: &[&[&str]] = &[
        &["add", "x"],
        &["add", "x", "--url", "https://example.com", "--", "cmd"],
        &["add", "bad name", "--", "cmd"],
        &["add", "x", "--url", "ftp://example.com"],
        &["add", "x", "--env", "A=1", "--url", "https://example.com"],
        &["add", "x", "--header", "A=1", "--", "cmd"],
        &["add", "x", "--env", "NOVALUE", "--", "cmd"],
        &["add", "x", "--exposure", "loud", "--", "cmd"],
    ];
    for args in cases {
        let (run, _dir) = run_command(args, None, None).await;
        assert_eq!(run.exit_code, 1, "{}", args.join(" "));
        assert!(!Path::new(&run.agent_dir).join("mcp.json").exists());
    }
}

#[tokio::test]
async fn command_adds_and_removes_project_servers() {
    let (added, dir) = run_command(&["add", "-l", "local", "--", "node", "server.js"], None, None).await;
    let dir = dir.unwrap();
    assert!(added.output.contains("The project is not trusted"), "{}", added.output);
    let project_config = Path::new(&added.agent_dir)
        .join(".pi/mcp.json")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        read_json(&project_config),
        json!({ "mcpServers": { "local": { "command": "node", "args": ["server.js"] } } })
    );
    let (wrong_scope, _) = run_command(&["remove", "local"], None, Some(&dir)).await;
    assert_eq!(wrong_scope.exit_code, 1);
    assert!(
        wrong_scope
            .output
            .contains(&format!("It is defined in {project_config}; use --local.")),
        "{}",
        wrong_scope.output
    );
    let (removed, _) = run_command(&["remove", "local", "--local"], None, Some(&dir)).await;
    assert_eq!(removed.exit_code, 0);
    assert!(removed.output.contains("Removed project MCP server \"local\""));
    assert_eq!(read_json(&project_config), json!({ "mcpServers": {} }));
}

#[tokio::test]
async fn command_removes_global_servers() {
    let (run, dir) = run_command(&["remove", "broken"], Some(command_servers()), None).await;
    let dir = dir.unwrap();
    assert_eq!(run.exit_code, 0, "{}", run.output);
    let path = Path::new(&run.agent_dir).join("mcp.json").to_string_lossy().into_owned();
    let names: Vec<String> = read_json(&path)["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(names, vec!["fixture", "parked", "bad"]);
    let (missing, _) = run_command(&["remove", "broken"], None, Some(&dir)).await;
    assert_eq!(missing.exit_code, 1);
    assert!(missing.output.contains("No global MCP server named \"broken\""));
}

// ------------------------------------------------------------------------------------------------
// OAuth refresh and sign-in, against an MCP server with its own authorization server
// ------------------------------------------------------------------------------------------------

#[derive(Default, Clone)]
struct OAuthServerOptions {
    iss: Option<String>,
    iss_parameter: bool,
    cimd: bool,
    redirect_path: Option<String>,
}

#[derive(Default)]
struct OAuthServerState {
    log: Vec<String>,
    registrations: Vec<Value>,
    authorizations: Vec<Vec<(String, String)>>,
    token_requests: Vec<Vec<(String, String)>>,
    valid_tokens: Vec<String>,
    refresh_tokens: Vec<String>,
    challenges: Vec<(String, String)>,
    issued: usize,
}

struct OAuthMcpServer {
    url: String,
    state: Arc<Mutex<OAuthServerState>>,
    _server: TestHttpServer,
}

impl OAuthMcpServer {
    fn expire_access_tokens(&self) {
        self.state.lock().valid_tokens.clear();
    }
}

fn param(params: &[(String, String)], key: &str) -> Option<String> {
    params
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

/// Port of `test/suite/mcp-oauth-server.ts`.
async fn start_oauth_mcp_server(options: OAuthServerOptions) -> OAuthMcpServer {
    use test_http::Response;
    let state: Arc<Mutex<OAuthServerState>> = Arc::default();
    let shared = state.clone();
    let server = start_http_server(Arc::new(move |request, origin| {
        let state = shared.clone();
        let options = options.clone();
        Box::pin(async move {
            let url = url::Url::parse(&format!("{origin}{}", request.target)).unwrap();
            let mut state = state.lock();
            let response = match url.path() {
                "/mcp" => {
                    let token = request
                        .headers
                        .get("authorization")
                        .map(|value| value.trim_start_matches("Bearer ").to_string());
                    if request.method != "POST" {
                        Response::new(if request.method == "GET" { 405 } else { 200 })
                    } else if !token
                        .as_ref()
                        .is_some_and(|token| state.valid_tokens.contains(token))
                    {
                        state.log.push(format!(
                            "401 {}",
                            token.clone().unwrap_or_else(|| "none".into())
                        ));
                        Response::new(401).header(
                            "www-authenticate",
                            &format!(
                                "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\""
                            ),
                        )
                    } else {
                        let message: Value = serde_json::from_slice(&request.body).unwrap();
                        match message.get("id") {
                            None => Response::new(202),
                            Some(id) => {
                                let token = token.unwrap();
                                let result = match message["method"].as_str() {
                                    Some("initialize") => json!({
                                        "protocolVersion": LATEST_PROTOCOL_VERSION,
                                        "capabilities": { "tools": {} },
                                        "serverInfo": { "name": "issues", "version": "1.0.0" },
                                    }),
                                    Some("tools/list") => json!({ "tools": [{ "name": "whoami", "inputSchema": { "type": "object", "properties": {} } }] }),
                                    Some("tools/call") => {
                                        state.log.push(format!("call {token}"));
                                        json!({ "content": [{ "type": "text", "text": format!("token {token}") }] })
                                    }
                                    _ => json!({}),
                                };
                                Response::json(200, &json!({ "jsonrpc": "2.0", "id": id, "result": result }))
                                    .header("mcp-session-id", "session-1")
                            }
                        }
                    }
                }
                "/.well-known/oauth-protected-resource/mcp" => Response::json(
                    200,
                    &json!({ "resource": format!("{origin}/mcp"), "authorization_servers": [origin] }),
                ),
                "/.well-known/oauth-authorization-server" => {
                    let mut metadata = json!({
                        "issuer": origin,
                        "authorization_endpoint": format!("{origin}/authorize"),
                        "token_endpoint": format!("{origin}/token"),
                        "registration_endpoint": format!("{origin}/register"),
                        "response_types_supported": ["code"],
                        "code_challenge_methods_supported": ["S256"],
                        "token_endpoint_auth_methods_supported": ["none"],
                    });
                    if options.cimd {
                        metadata["client_id_metadata_document_supported"] = json!(true);
                    }
                    if options.iss_parameter {
                        metadata["authorization_response_iss_parameter_supported"] = json!(true);
                    }
                    Response::json(200, &metadata)
                }
                "/register" => {
                    let mut metadata: Value = serde_json::from_slice(&request.body).unwrap();
                    state.log.push("register".into());
                    state.registrations.push(metadata.clone());
                    metadata["client_id"] = json!("client-1");
                    Response::json(201, &metadata)
                }
                "/authorize" => {
                    let params = parse_query(url.query().unwrap_or(""));
                    state.authorizations.push(params.clone());
                    let code = format!("code-{}", state.challenges.len() + 1);
                    state.challenges.push((
                        code.clone(),
                        param(&params, "code_challenge").unwrap_or_default(),
                    ));
                    let mut redirect =
                        url::Url::parse(&param(&params, "redirect_uri").unwrap_or_default()).unwrap();
                    if let Some(path) = &options.redirect_path {
                        redirect.set_path(path);
                    }
                    let iss = options
                        .iss
                        .clone()
                        .or_else(|| options.iss_parameter.then(|| origin.clone()));
                    {
                        let mut query = redirect.query_pairs_mut();
                        query.append_pair("code", &code);
                        query.append_pair("state", &param(&params, "state").unwrap_or_default());
                        if let Some(iss) = iss {
                            query.append_pair("iss", &iss);
                        }
                    }
                    Response::new(302).header("location", redirect.as_str())
                }
                "/token" => {
                    let params = parse_query(&String::from_utf8_lossy(&request.body));
                    state.token_requests.push(params.clone());
                    let issue = |state: &mut OAuthServerState| {
                        state.issued += 1;
                        let access = format!("access-{}", state.issued);
                        let refresh = format!("refresh-{}", state.issued);
                        state.valid_tokens.push(access.clone());
                        state.refresh_tokens.push(refresh.clone());
                        json!({ "access_token": access, "refresh_token": refresh, "token_type": "Bearer", "expires_in": 3600 })
                    };
                    if param(&params, "grant_type").as_deref() == Some("authorization_code") {
                        let code = param(&params, "code").unwrap_or_default();
                        let challenge = state
                            .challenges
                            .iter()
                            .find(|(candidate, _)| *candidate == code)
                            .map(|(_, challenge)| challenge.clone());
                        use base64::Engine;
                        let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                            sha2::Sha256::digest(
                                param(&params, "code_verifier").unwrap_or_default().as_bytes(),
                            ),
                        );
                        if challenge.as_deref() != Some(verifier.as_str()) {
                            Response::json(400, &json!({ "error": "invalid_grant" }))
                        } else {
                            state.challenges.retain(|(candidate, _)| *candidate != code);
                            state.log.push("token code".into());
                            let tokens = issue(&mut state);
                            Response::json(200, &tokens)
                        }
                    } else {
                        let refresh = param(&params, "refresh_token").unwrap_or_default();
                        if !state.refresh_tokens.contains(&refresh) {
                            Response::json(400, &json!({ "error": "invalid_grant" }))
                        } else {
                            state.refresh_tokens.retain(|candidate| *candidate != refresh);
                            state.log.push("token refresh".into());
                            let tokens = issue(&mut state);
                            Response::json(200, &tokens)
                        }
                    }
                }
                _ => Response::new(404),
            };
            Some(response)
        })
    }))
    .await;
    OAuthMcpServer {
        url: format!("{}/mcp", server.origin),
        state,
        _server: server,
    }
}

use sha2::Digest;

/// A browser that follows the authorization URL to the loopback callback.
struct BrowserPrompt {
    opened: Arc<Mutex<Vec<String>>>,
}

impl BrowserPrompt {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            opened: Arc::default(),
        })
    }
}

#[async_trait::async_trait]
impl McpSignInPrompt for BrowserPrompt {
    fn show_authorization_url(&self, url: &str) {
        self.opened.lock().push(url.to_string());
        let url = url.to_string();
        tokio::spawn(async move {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let _ = client.get(url).send().await;
        });
    }

    async fn prompt_for_redirect_url(
        &self,
        signal: CancellationToken,
    ) -> anyhow::Result<Option<String>> {
        signal.cancelled().await;
        Ok(None)
    }
}

async fn sign_in_to(
    server_url: &str,
    store: Arc<dyn pi_mcp::oauth::provider::McpOAuthStateStore>,
    settings: oauth::McpOAuthSettings,
    challenge: Option<pi_mcp::oauth::types::OAuthChallenge>,
    prompt: Arc<dyn McpSignInPrompt>,
) -> anyhow::Result<()> {
    sign_in_mcp_server(&oauth::SignInMcpServerOptions {
        server_url: server_url.to_string(),
        store,
        settings,
        challenge,
        prompt,
        signal: None,
    })
    .await
}

fn unauthorized(server_url: &str, token: &str) -> pi_mcp::UnauthorizedContext {
    pi_mcp::UnauthorizedContext {
        response: pi_mcp::HttpResponseHead {
            status: 401,
            headers: Default::default(),
        },
        server_url: url::Url::parse(server_url).unwrap(),
        fetch: pi_mcp::default_fetch(),
        token: Some(token.to_string()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_refreshes_once_when_several_processes_find_the_same_token_rejected() {
    let server = start_oauth_mcp_server(Default::default()).await;
    let lock_dir = TempDir::new("pi-mcp-refresh-");
    // Stores sharing the credential file and lock directory stand in for separate pi processes.
    let backend: Arc<dyn crate::core::auth_storage::AuthStorageBackend> =
        Arc::new(InMemoryAuthStorageBackend::new());
    let process = || {
        let store = McpOAuthCredentialStore::new(Some(backend.clone()), Some(&lock_dir.path()))
            .for_server("test", &server.url);
        let provider = oauth::create_mcp_auth_provider(&oauth::CreateMcpAuthProviderOptions {
            server_url: server.url.clone(),
            store: store.clone(),
            settings: Arc::new(|| Ok(Default::default())),
            on_challenge: Arc::new(|_| {}),
        })
        .unwrap();
        (store, provider)
    };
    let (store, _) = process();
    sign_in_to(&server.url, store, Default::default(), None, BrowserPrompt::new())
        .await
        .unwrap();
    server.expire_access_tokens();
    let processes = vec![process(), process(), process()];
    // The server rotates refresh tokens: a second refresh with refresh-1 would fail with invalid_grant.
    let refreshes = processes.iter().map(|(_, provider)| {
        let provider = provider.clone();
        let context = unauthorized(&server.url, "access-1");
        async move { provider.on_unauthorized(context).unwrap().await }
    });
    for result in futures::future::join_all(refreshes).await {
        result.unwrap();
    }
    assert_eq!(
        server
            .state
            .lock()
            .log
            .iter()
            .filter(|entry| *entry == "token refresh")
            .count(),
        1
    );
    for (_, provider) in &processes {
        assert_eq!(provider.token().await.unwrap().as_deref(), Some("access-2"));
    }
    // The lock is released.
    assert_eq!(std::fs::read_dir(&lock_dir.0).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_waits_for_a_running_refresh_to_save_the_new_tokens() {
    let server = start_oauth_mcp_server(Default::default()).await;
    let store = credentials().for_server("test", &server.url);
    sign_in_to(&server.url, store.clone(), Default::default(), None, BrowserPrompt::new())
        .await
        .unwrap();
    server.expire_access_tokens();
    let provider = oauth::create_mcp_auth_provider(&oauth::CreateMcpAuthProviderOptions {
        server_url: server.url.clone(),
        store: store.clone(),
        settings: Arc::new(|| Ok(Default::default())),
        on_challenge: Arc::new(|_| {}),
    })
    .unwrap();
    let refreshing = {
        let provider = provider.clone();
        let context = unauthorized(&server.url, "access-1");
        tokio::spawn(async move { provider.on_unauthorized(context).unwrap().await })
    };
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    provider.settled().await.unwrap();
    assert_eq!(
        store
            .load()
            .await
            .unwrap()
            .and_then(|state| state.tokens)
            .map(|tokens| tokens.access_token)
            .as_deref(),
        Some("access-2")
    );
    refreshing.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_rejects_an_authorization_response_from_another_issuer() {
    let server = start_oauth_mcp_server(OAuthServerOptions {
        iss: Some("https://attacker.example".into()),
        ..Default::default()
    })
    .await;
    let error = sign_in_to(
        &server.url,
        credentials().for_server("test", &server.url),
        Default::default(),
        None,
        BrowserPrompt::new(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<McpError>(),
            Some(McpError::OAuthIssuerMismatch { .. })
        ),
        "{error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_keeps_the_granted_scope_when_the_server_asks_for_more() {
    let server = start_oauth_mcp_server(Default::default()).await;
    let store = credentials().for_server("test", &server.url);
    let prompt = BrowserPrompt::new();
    let challenge = |error: Option<&str>, scope: &str| pi_mcp::oauth::types::OAuthChallenge {
        error: error.map(str::to_string),
        scope: Some(scope.to_string()),
        ..Default::default()
    };
    sign_in_to(
        &server.url,
        store.clone(),
        Default::default(),
        Some(challenge(None, "issues:read")),
        prompt.clone(),
    )
    .await
    .unwrap();
    // The token response names no scope, so the grant has the requested one.
    let scope = || async {
        store
            .load()
            .await
            .unwrap()
            .and_then(|state| state.tokens)
            .and_then(|tokens| tokens.scope)
    };
    assert_eq!(scope().await.as_deref(), Some("issues:read"));
    // The step-up challenge lists only the missing scope. Requesting just that would lose
    // issues:read, so the next request would ask for sign-in again.
    sign_in_to(
        &server.url,
        store.clone(),
        Default::default(),
        Some(challenge(Some("insufficient_scope"), "issues:write")),
        prompt.clone(),
    )
    .await
    .unwrap();
    let scopes: Vec<Option<String>> = prompt
        .opened
        .lock()
        .iter()
        .map(|url| {
            pi_mcp::oauth::flow::get_search_param(&url::Url::parse(url).unwrap(), "scope")
        })
        .collect();
    assert_eq!(
        scopes,
        vec![
            Some("issues:read".to_string()),
            Some("issues:read issues:write".to_string())
        ]
    );
    assert_eq!(scope().await.as_deref(), Some("issues:read issues:write"));
}

// #10172
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_uses_the_configured_authorization_server_metadata_url() {
    let server = start_oauth_mcp_server(Default::default()).await;
    let origin = server.url.trim_end_matches("/mcp").to_string();
    let error = sign_in_to(
        &server.url,
        credentials().for_server("test", &server.url),
        oauth::McpOAuthSettings {
            auth_server_metadata_url: Some(format!("{origin}/missing")),
            ..Default::default()
        },
        None,
        BrowserPrompt::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("HTTP 404 loading authorization server metadata"),
        "{error}"
    );
}

fn cimd() -> oauth::McpOAuthSettings {
    oauth::McpOAuthSettings {
        client_registration: Some(oauth::McpOAuthSettingsClientRegistration::Cimd),
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_registers_dynamically_by_default_even_when_documents_are_supported() {
    let server = start_oauth_mcp_server(OAuthServerOptions {
        cimd: true,
        iss_parameter: true,
        ..Default::default()
    })
    .await;
    sign_in_to(
        &server.url,
        credentials().for_server("test", &server.url),
        Default::default(),
        None,
        BrowserPrompt::new(),
    )
    .await
    .unwrap();
    let state = server.state.lock();
    assert_eq!(state.registrations.len(), 1);
    assert_eq!(
        param(&state.authorizations[0], "client_id").as_deref(),
        Some("client-1")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_uses_pis_document_when_authorization_responses_name_their_issuer() {
    let server = start_oauth_mcp_server(OAuthServerOptions {
        cimd: true,
        iss_parameter: true,
        ..Default::default()
    })
    .await;
    let store = credentials().for_server("test", &server.url);
    sign_in_to(&server.url, store.clone(), cimd(), None, BrowserPrompt::new())
        .await
        .unwrap();
    let redirect = {
        let state = server.state.lock();
        let authorization = &state.authorizations[0];
        assert_eq!(
            param(authorization, "client_id").as_deref(),
            Some("https://pi.dev/oauth/client.json")
        );
        let redirect = url::Url::parse(&param(authorization, "redirect_uri").unwrap()).unwrap();
        assert_eq!(
            format!("{}{}", redirect.host_str().unwrap(), redirect.path()),
            "127.0.0.1/callback"
        );
        assert!(redirect.port().is_some());
        assert!(state.registrations.is_empty());
        assert_eq!(
            param(&state.token_requests[0], "client_id").as_deref(),
            Some("https://pi.dev/oauth/client.json")
        );
        assert_eq!(
            param(&state.token_requests[0], "redirect_uri").as_deref(),
            Some(redirect.as_str())
        );
        redirect
    };
    let _ = redirect;
    // The document is not stored, so signing in again refreshes the tokens instead of discarding them.
    assert!(store.load().await.unwrap().unwrap().client_information.is_none());
    sign_in_to(&server.url, store, cimd(), None, BrowserPrompt::new())
        .await
        .unwrap();
    let state = server.state.lock();
    assert_eq!(state.authorizations.len(), 1);
    assert_eq!(
        param(&state.token_requests[1], "grant_type").as_deref(),
        Some("refresh_token")
    );
    assert_eq!(
        param(&state.token_requests[1], "client_id").as_deref(),
        Some("https://pi.dev/oauth/client.json")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_uses_a_server_specific_document_and_callback_path_without_the_iss_parameter() {
    use base64::Engine;
    let server = start_oauth_mcp_server(OAuthServerOptions {
        cimd: true,
        ..Default::default()
    })
    .await;
    sign_in_to(
        &server.url,
        credentials().for_server("test", &server.url),
        cimd(),
        None,
        BrowserPrompt::new(),
    )
    .await
    .unwrap();
    // Computed like Codex: the first 9 bytes of the SHA-256 of the MCP server URL.
    let digest = sha2::Sha256::digest(server.url.as_bytes());
    let id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..9]);
    let state = server.state.lock();
    let authorization = &state.authorizations[0];
    assert_eq!(
        param(authorization, "client_id"),
        Some(format!("https://pi.dev/oauth/{id}/client.json"))
    );
    let redirect = url::Url::parse(&param(authorization, "redirect_uri").unwrap()).unwrap();
    assert_eq!(redirect.path(), format!("/callback/{id}"));
    assert!(state.registrations.is_empty());
    assert_eq!(
        param(&state.token_requests[0], "redirect_uri").as_deref(),
        Some(redirect.as_str())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_replaces_a_registered_client_when_switching_to_the_document() {
    let server = start_oauth_mcp_server(OAuthServerOptions {
        cimd: true,
        iss_parameter: true,
        ..Default::default()
    })
    .await;
    let store = credentials().for_server("test", &server.url);
    sign_in_to(&server.url, store.clone(), Default::default(), None, BrowserPrompt::new())
        .await
        .unwrap();
    assert_eq!(
        store
            .load()
            .await
            .unwrap()
            .unwrap()
            .client_information
            .map(|client| client.client_id)
            .as_deref(),
        Some("client-1")
    );
    sign_in_to(&server.url, store.clone(), cimd(), None, BrowserPrompt::new())
        .await
        .unwrap();
    // The registered client's tokens are not refreshed with another client.
    let clients: Vec<Option<String>> = server
        .state
        .lock()
        .authorizations
        .iter()
        .map(|authorization| param(authorization, "client_id"))
        .collect();
    assert_eq!(
        clients,
        vec![
            Some("client-1".to_string()),
            Some("https://pi.dev/oauth/client.json".to_string())
        ]
    );
    assert!(store.load().await.unwrap().unwrap().client_information.is_none());
}

/// A prompt that pastes a redirect URL on the shared callback path instead of opening the browser.
struct PastePrompt {
    shown: Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl McpSignInPrompt for PastePrompt {
    fn show_authorization_url(&self, url: &str) {
        *self.shown.lock() = Some(url.to_string());
    }

    async fn prompt_for_redirect_url(
        &self,
        _signal: CancellationToken,
    ) -> anyhow::Result<Option<String>> {
        let shown = url::Url::parse(&self.shown.lock().clone().unwrap()).unwrap();
        let mut redirect = url::Url::parse(
            &pi_mcp::oauth::flow::get_search_param(&shown, "redirect_uri").unwrap(),
        )
        .unwrap();
        redirect.set_path("/callback");
        redirect.set_query(Some(&format!(
            "code=code-1&state={}",
            pi_mcp::oauth::flow::get_search_param(&shown, "state").unwrap()
        )));
        Ok(Some(redirect.to_string()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_accepts_the_authorization_response_only_on_the_server_specific_redirect_uri() {
    // A mixed-up authorization server redirects to the shared callback path.
    let server = start_oauth_mcp_server(OAuthServerOptions {
        cimd: true,
        redirect_path: Some("/callback".into()),
        ..Default::default()
    })
    .await;
    let error = sign_in_to(
        &server.url,
        credentials().for_server("test", &server.url),
        cimd(),
        None,
        BrowserPrompt::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("arrived on another redirect URI"), "{error}");

    // The same for a redirect URL pasted from the browser.
    let server = start_oauth_mcp_server(OAuthServerOptions {
        cimd: true,
        ..Default::default()
    })
    .await;
    let error = sign_in_to(
        &server.url,
        credentials().for_server("test", &server.url),
        cimd(),
        None,
        Arc::new(PastePrompt {
            shown: Mutex::new(None),
        }),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not match this sign-in's redirect URI"),
        "{error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_fails_instead_of_registering_when_documents_are_not_supported() {
    let server = start_oauth_mcp_server(OAuthServerOptions {
        iss_parameter: true,
        ..Default::default()
    })
    .await;
    let error = sign_in_to(
        &server.url,
        credentials().for_server("test", &server.url),
        cimd(),
        None,
        BrowserPrompt::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not support Client ID Metadata Documents"),
        "{error}"
    );
    assert!(server.state.lock().registrations.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_connection_signs_in_refreshes_and_signs_out() {
    let server = start_oauth_mcp_server(Default::default()).await;
    let store = credentials();
    let entry = entry_with("issues", json!({ "url": server.url }));
    let connection = McpServerConnection::new(&McpServerConnectionOptions {
        entry,
        cwd: std::env::temp_dir().to_string_lossy().into_owned(),
        create_transport: default_transport_factory(),
        credentials: store.clone(),
        provider_token: None,
        on_tools: Arc::new(|_| {}),
        on_change: None,
        log: None,
    });
    let error = connection.get_client().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "MCP server \"issues\" requires sign-in. Run /mcp to sign in."
    );
    assert_eq!(connection.state(), ServerState::NeedsAuth);
    sign_in_to(
        &server.url,
        store.for_server("issues", &server.url),
        connection.oauth_settings().unwrap(),
        connection.challenge(),
        BrowserPrompt::new(),
    )
    .await
    .unwrap();
    connection.reconnect().await.unwrap();
    assert_eq!(connection.state(), ServerState::Connected);
    let tools: Vec<String> = connection.tools().iter().map(|tool| tool.name.clone()).collect();
    assert_eq!(tools, vec!["whoami"]);
    let result = connection
        .call_tool("whoami", IndexMap::new(), Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap()["content"][0]["text"],
        json!("token access-1")
    );
    // An expired token is refreshed after the 401.
    server.expire_access_tokens();
    let result = connection
        .call_tool("whoami", IndexMap::new(), Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap()["content"][0]["text"],
        json!("token access-2")
    );
    assert!(store.remove("issues", &server.url));
    connection.sign_out().await.unwrap();
    assert_eq!(connection.state(), ServerState::NeedsAuth);
    connection.close().await.unwrap();
}

// ------------------------------------------------------------------------------------------------
// The extension against a fake `pi`
// ------------------------------------------------------------------------------------------------

struct FakeUi {
    notifications: Mutex<Vec<(String, NotifyType)>>,
    noop: crate::core::extensions::runner::NoOpUIContext,
}

#[async_trait::async_trait]
impl ExtensionUIContext for FakeUi {
    async fn select(&self, title: &str, options: Vec<String>, opts: ExtensionUIDialogOptions) -> Option<String> {
        self.noop.select(title, options, opts).await
    }
    async fn confirm(&self, title: &str, message: &str, opts: ExtensionUIDialogOptions) -> bool {
        self.noop.confirm(title, message, opts).await
    }
    async fn input(&self, title: &str, placeholder: Option<&str>, opts: ExtensionUIDialogOptions) -> Option<String> {
        self.noop.input(title, placeholder, opts).await
    }
    fn notify(&self, message: &str, kind: NotifyType) {
        self.notifications.lock().push((message.to_string(), kind));
    }
    fn on_terminal_input(&self, handler: crate::core::extensions::types::TerminalInputHandler) -> Unsubscribe {
        self.noop.on_terminal_input(handler)
    }
    fn set_status(&self, key: &str, text: Option<&str>) {
        self.noop.set_status(key, text)
    }
    fn set_working_message(&self, message: Option<&str>) {
        self.noop.set_working_message(message)
    }
    fn set_working_visible(&self, visible: bool) {
        self.noop.set_working_visible(visible)
    }
    fn set_working_indicator(&self, options: Option<crate::core::extensions::types::WorkingIndicatorOptions>) {
        self.noop.set_working_indicator(options)
    }
    fn set_hidden_thinking_label(&self, label: Option<&str>) {
        self.noop.set_hidden_thinking_label(label)
    }
    fn set_widget(
        &self,
        key: &str,
        content: Option<crate::core::extensions::types::WidgetContent>,
        options: crate::core::extensions::types::ExtensionWidgetOptions,
    ) {
        self.noop.set_widget(key, content, options)
    }
    fn set_footer(&self, factory: Option<crate::core::extensions::types::FooterFactory>) {
        self.noop.set_footer(factory)
    }
    fn set_header(&self, factory: Option<crate::core::extensions::types::ComponentFactory>) {
        self.noop.set_header(factory)
    }
    fn set_title(&self, title: &str) {
        self.noop.set_title(title)
    }
    async fn custom(
        &self,
        factory: crate::core::extensions::types::CustomComponentFactory,
        options: crate::core::extensions::types::CustomUiOptions,
    ) -> anyhow::Result<crate::core::extensions::types::CustomUiResult> {
        self.noop.custom(factory, options).await
    }
    fn paste_to_editor(&self, text: &str) {
        self.noop.paste_to_editor(text)
    }
    fn set_editor_text(&self, text: &str) {
        self.noop.set_editor_text(text)
    }
    fn get_editor_text(&self) -> String {
        self.noop.get_editor_text()
    }
    async fn editor(&self, title: &str, prefill: Option<&str>) -> Option<String> {
        self.noop.editor(title, prefill).await
    }
    fn add_autocomplete_provider(&self, factory: crate::core::extensions::types::AutocompleteProviderFactory) {
        self.noop.add_autocomplete_provider(factory)
    }
    fn set_editor_component(&self, factory: Option<crate::core::extensions::types::EditorFactory>) {
        self.noop.set_editor_component(factory)
    }
    fn get_editor_component(&self) -> Option<crate::core::extensions::types::EditorFactory> {
        self.noop.get_editor_component()
    }
    fn theme(&self) -> Arc<crate::modes::interactive::theme::theme::Theme> {
        self.noop.theme()
    }
    fn get_all_themes(&self) -> Vec<crate::core::extensions::types::ThemeInfo> {
        self.noop.get_all_themes()
    }
    fn get_theme(&self, name: &str) -> Option<Arc<crate::modes::interactive::theme::theme::Theme>> {
        self.noop.get_theme(name)
    }
    fn set_theme(
        &self,
        theme: crate::core::extensions::types::ThemeSelection,
    ) -> crate::core::extensions::types::SetThemeResult {
        self.noop.set_theme(theme)
    }
    fn get_tools_expanded(&self) -> bool {
        self.noop.get_tools_expanded()
    }
    fn set_tools_expanded(&self, expanded: bool) {
        self.noop.set_tools_expanded(expanded)
    }
}

struct FakeActions {
    project_trusted: bool,
}

impl ExtensionContextActions for FakeActions {
    fn get_model(&self) -> Option<pi_ai::types::Model> {
        None
    }
    fn get_scoped_models(&self) -> Vec<crate::core::model_resolver::ScopedModel> {
        Vec::new()
    }
    fn is_idle(&self) -> bool {
        true
    }
    fn is_project_trusted(&self) -> bool {
        self.project_trusted
    }
    fn get_signal(&self) -> Option<CancellationToken> {
        None
    }
    fn abort(&self) {}
    fn has_pending_messages(&self) -> bool {
        false
    }
    fn shutdown(&self) {}
    fn get_context_usage(&self) -> Option<crate::core::extensions::types::ContextUsage> {
        None
    }
    fn compact(&self, _options: crate::core::extensions::types::CompactOptions) {}
    fn get_system_prompt(&self) -> String {
        String::new()
    }
}

struct FakeHost {
    ui: Arc<FakeUi>,
    mode: ExtensionMode,
    cwd: String,
}

impl ExtensionContextHost for FakeHost {
    fn assert_active(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn ui(&self) -> Arc<dyn ExtensionUIContext> {
        self.ui.clone()
    }
    fn mode(&self) -> ExtensionMode {
        self.mode
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn cwd(&self) -> String {
        self.cwd.clone()
    }
    fn session_manager(&self) -> Option<crate::core::session_manager::ReadonlySessionManager> {
        None
    }
    fn model_registry(&self) -> Option<Arc<crate::core::model_registry::ModelRegistry>> {
        None
    }
    fn thinking_level(&self) -> Option<pi_agent::types::ThinkingLevel> {
        None
    }
    fn actions(&self) -> Arc<dyn ExtensionContextActions> {
        Arc::new(FakeActions {
            project_trusted: false,
        })
    }
}

#[derive(Default)]
struct FakePiState {
    handlers: HashMap<String, Vec<ExtensionHandler>>,
    tools: indexmap::IndexMap<String, ToolDefinition>,
    /// Tools other extensions registered (for example codemode), as `getAllTools()` lists them.
    other_tools: Vec<ToolInfo>,
    active: Vec<String>,
    commands: HashMap<String, CommandOptions>,
    renderers: Vec<crate::core::extensions::types::ToolRendererResolver>,
    mcp_servers: Vec<RegisteredMcpServer>,
}

/// A `pi` object that records registrations, for driving the extension without a session.
#[derive(Default)]
struct FakePi {
    state: Mutex<FakePiState>,
}

fn source_info() -> SourceInfo {
    SourceInfo {
        path: "builtin:mcp".into(),
        source: "builtin".into(),
        scope: SourceScope::Temporary,
        origin: SourceOrigin::TopLevel,
        base_dir: None,
    }
}

fn tool_info(definition: &ToolDefinition) -> ToolInfo {
    ToolInfo {
        name: definition.name.clone(),
        description: definition.description.clone(),
        parameters: definition.parameters.clone(),
        prompt_guidelines: definition.prompt_guidelines.clone(),
        reaches_tools: definition.reaches_tools.clone(),
        exposure: definition.effective_exposure(),
        namespace: definition.namespace.clone(),
        annotations: definition.annotations.clone(),
        source_info: source_info(),
    }
}

#[async_trait::async_trait]
impl ExtensionAPI for FakePi {
    fn on(&self, event: &str, handler: ExtensionHandler) -> anyhow::Result<Unsubscribe> {
        self.state
            .lock()
            .handlers
            .entry(event.to_string())
            .or_default()
            .push(handler);
        Ok(Arc::new(|| {}))
    }
    fn on_project_trust(
        &self,
        _handler: crate::core::extensions::types::ProjectTrustHandler,
    ) -> anyhow::Result<Unsubscribe> {
        anyhow::bail!("not supported by the test API")
    }
    fn register_tool(&self, tool: ToolDefinition) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        // Registering activates direct tools, like the runner.
        if tool.effective_exposure() == ToolExposure::Direct && !state.active.contains(&tool.name) {
            state.active.push(tool.name.clone());
        }
        state.tools.insert(tool.name.clone(), tool);
        Ok(())
    }
    fn register_command(&self, name: &str, options: CommandOptions) -> anyhow::Result<()> {
        self.state.lock().commands.insert(name.to_string(), options);
        Ok(())
    }
    fn register_shortcut(
        &self,
        _shortcut: &str,
        _options: crate::core::extensions::types::ShortcutOptions,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn register_flag(
        &self,
        _name: &str,
        _options: crate::core::extensions::types::FlagOptions,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn get_flag(&self, _name: &str) -> anyhow::Result<Option<crate::core::extensions::types::FlagValue>> {
        Ok(None)
    }
    fn register_message_renderer(
        &self,
        _custom_type: &str,
        _renderer: crate::core::extensions::types::MessageRenderer,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn register_markdown_transformer(
        &self,
        _transformer: crate::core::extensions::types::MarkdownTransformer,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn register_entry_renderer(
        &self,
        _custom_type: &str,
        _renderer: crate::core::extensions::types::EntryRenderer,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn register_tool_renderer(
        &self,
        resolver: crate::core::extensions::types::ToolRendererResolver,
    ) -> anyhow::Result<()> {
        self.state.lock().renderers.push(resolver);
        Ok(())
    }
    fn send_message(
        &self,
        _message: crate::core::extensions::types::CustomMessageInput,
        _options: crate::core::extensions::types::SendMessageOptions,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn send_user_message(
        &self,
        _content: pi_ai::types::UserContent,
        _options: crate::core::extensions::types::SendUserMessageOptions,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn append_entry(&self, _custom_type: &str, _data: Option<Value>) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn set_session_name(&self, _name: &str) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn get_session_name(&self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    fn set_label(&self, _entry_id: &str, _label: Option<&str>) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    async fn exec(
        &self,
        _command: &str,
        _args: &[String],
        _options: crate::core::extensions::types::ExecOptions,
    ) -> anyhow::Result<crate::core::extensions::types::ExecResult> {
        anyhow::bail!("not supported by the test API")
    }
    fn get_active_tools(&self) -> anyhow::Result<Vec<String>> {
        Ok(self.state.lock().active.clone())
    }
    fn get_all_tools(&self) -> anyhow::Result<Vec<ToolInfo>> {
        let state = self.state.lock();
        let mut tools: Vec<ToolInfo> = state.other_tools.clone();
        tools.extend(state.tools.values().map(tool_info));
        Ok(tools)
    }
    fn get_settings(&self) -> anyhow::Result<crate::core::settings_manager::Settings> {
        anyhow::bail!("not supported by the test API")
    }
    fn set_active_tools(&self, tool_names: Vec<String>) -> anyhow::Result<()> {
        self.state.lock().active = tool_names;
        Ok(())
    }
    fn get_commands(&self) -> anyhow::Result<Vec<crate::core::slash_commands::SlashCommandInfo>> {
        Ok(Vec::new())
    }
    async fn set_model(&self, _model: pi_ai::types::Model) -> anyhow::Result<bool> {
        Ok(false)
    }
    fn get_thinking_level(&self) -> anyhow::Result<pi_agent::types::ThinkingLevel> {
        anyhow::bail!("not supported by the test API")
    }
    fn set_thinking_level(&self, _level: pi_agent::types::ThinkingLevel) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn register_provider(
        &self,
        _registration: crate::core::extensions::types::ProviderRegistration,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn unregister_provider(&self, _name: &str) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn register_mcp_server(&self, name: &str, config: McpServerConfig) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        state.mcp_servers.retain(|server| server.name != name);
        state.mcp_servers.push(RegisteredMcpServer {
            name: name.to_string(),
            config,
            extension_path: "/ext/servers.ts".to_string(),
        });
        Ok(())
    }
    fn unregister_mcp_server(&self, name: &str) -> anyhow::Result<()> {
        self.state.lock().mcp_servers.retain(|server| server.name != name);
        Ok(())
    }
    fn get_mcp_servers(&self) -> anyhow::Result<Vec<RegisteredMcpServer>> {
        Ok(self.state.lock().mcp_servers.clone())
    }
    fn register_virtual_model(
        &self,
        _model: crate::core::extensions::types::ExtensionVirtualModel,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn unregister_virtual_model(&self, _provider: &str, _id: &str) -> anyhow::Result<()> {
        anyhow::bail!("not supported by the test API")
    }
    fn events(&self) -> Arc<dyn crate::core::event_bus::EventBus> {
        crate::core::event_bus::create_event_bus()
    }
}

/// The extension installed on a fake `pi` with in-memory servers.
struct Harness {
    pi: Arc<FakePi>,
    ui: Arc<FakeUi>,
    ctx: ExtensionContext,
}

impl Harness {
    async fn new(
        servers: Vec<McpServerEntry>,
        transports: HashMap<String, FakeServerOptions>,
        configure: impl FnOnce(&mut McpExtensionOptions),
    ) -> Self {
        let pi = Arc::new(FakePi::default());
        let ui = Arc::new(FakeUi {
            notifications: Mutex::default(),
            noop: crate::core::extensions::runner::NoOpUIContext,
        });
        let host = Arc::new(FakeHost {
            ui: ui.clone(),
            mode: ExtensionMode::Print,
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
        });
        let ctx = ExtensionContext::new(host);
        let transports = Arc::new(transports);
        let mut options = McpExtensionOptions {
            load_config: Some(Arc::new(move |_| LoadedMcpConfig {
                servers: servers.clone(),
                auto_enable_codemode: None,
                errors: Vec::new(),
                project_config: None,
            })),
            create_transport: Some(Arc::new(move |entry, _, _| {
                let options = transports.get(&entry.name).cloned().unwrap_or_default();
                Ok(McpTransportHandle::new(fake_server(options).0))
            })),
            credentials: Some(credentials()),
            log_path: Some(
                std::env::temp_dir()
                    .join(format!("pi-mcp-test-{}.log", std::process::id()))
                    .to_string_lossy()
                    .into_owned(),
            ),
            open_url: Some(Arc::new(|_| {})),
            update_config: Some(Arc::new(|_, _| Ok(()))),
            startup_wait_ms: Some(2_000),
        };
        configure(&mut options);
        let factory = create_mcp_extension(Some(&options)).unwrap();
        factory(pi.clone()).await.unwrap();
        Self { pi, ui, ctx }
    }

    async fn emit(&self, name: &str, event: ExtensionEvent) -> Vec<ExtensionEventResult> {
        let handlers = self
            .pi
            .state
            .lock()
            .handlers
            .get(name)
            .cloned()
            .unwrap_or_default();
        let mut results = Vec::new();
        for handler in handlers {
            if let Some(result) = handler(event.clone(), self.ctx.clone()).await.unwrap() {
                results.push(result);
            }
        }
        results
    }

    async fn start(&self) {
        self.emit(
            "session_start",
            ExtensionEvent::SessionStart(SessionStartEvent {
                reason: crate::core::extensions::types::SessionStartReason::Startup,
                previous_session_file: None,
            }),
        )
        .await;
    }

    async fn shutdown(&self) {
        self.emit(
            "session_shutdown",
            ExtensionEvent::SessionShutdown(SessionShutdownEvent {
                reason: crate::core::extensions::types::SessionShutdownReason::Quit,
                target_session_file: None,
            }),
        )
        .await;
    }

    /// Run a prompt's `before_agent_start` and return the `mcp_servers` section.
    async fn before_agent_start(&self) -> Option<String> {
        let options = serde_json::from_value(json!({ "cwd": "/" })).unwrap();
        let results = self
            .emit(
                "before_agent_start",
                ExtensionEvent::BeforeAgentStart(BeforeAgentStartEvent {
                    system_prompt: String::new(),
                    system_prompt_options: options,
                    kind: BeforeAgentStartKind::User {
                        prompt: "hi".into(),
                        images: None,
                    },
                }),
            )
            .await;
        results.into_iter().find_map(|result| match result {
            ExtensionEventResult::BeforeAgentStart(result) => result
                .system_prompt_options
                .and_then(|options| options.sections.get(MCP_SERVERS_SECTION).cloned()),
            _ => None,
        })
    }

    async fn tool_call(&self, tool: &str, input: Value) {
        let input = match input {
            Value::Object(object) => object,
            _ => Default::default(),
        };
        self.emit(
            "tool_call",
            ExtensionEvent::ToolCall(ToolCallEvent {
                tool_call_id: "call-1".into(),
                parent_tool_call_id: None,
                tool_name: tool.into(),
                input,
            }),
        )
        .await;
    }

    async fn command(&self, args: &str) {
        let handler = self.pi.state.lock().commands.get("mcp").unwrap().handler.clone();
        handler(
            args.to_string(),
            ExtensionCommandContext {
                ctx: self.ctx.clone(),
                command_actions: Arc::new(NoCommandActions),
            },
        )
        .await
        .unwrap();
    }

    fn tool(&self, name: &str) -> Option<ToolDefinition> {
        self.pi.state.lock().tools.get(name).cloned()
    }

    fn exposure(&self, name: &str) -> Option<ToolExposure> {
        self.tool(name).map(|tool| tool.effective_exposure())
    }

    fn active(&self) -> Vec<String> {
        self.pi.state.lock().active.clone()
    }

    fn notifications(&self) -> Vec<(String, NotifyType)> {
        self.ui.notifications.lock().clone()
    }

    /// Wait until a condition holds, polling.
    async fn until(&self, condition: impl Fn(&Harness) -> bool) {
        for _ in 0..200 {
            if condition(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("condition not reached");
    }
}

struct NoCommandActions;

#[async_trait::async_trait]
impl crate::core::extensions::types::ExtensionCommandContextActions for NoCommandActions {
    async fn wait_for_idle(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn new_session(
        &self,
        _options: crate::core::extensions::types::NewSessionOptions,
    ) -> anyhow::Result<crate::core::extensions::types::SessionReplaceOutcome> {
        anyhow::bail!("not supported by the test API")
    }
    async fn fork(
        &self,
        _entry_id: &str,
        _options: crate::core::extensions::types::ForkOptions,
    ) -> anyhow::Result<crate::core::extensions::types::SessionReplaceOutcome> {
        anyhow::bail!("not supported by the test API")
    }
    async fn navigate_tree(
        &self,
        _target_id: &str,
        _options: crate::core::extensions::types::NavigateTreeOptions,
    ) -> anyhow::Result<crate::core::extensions::types::SessionReplaceOutcome> {
        anyhow::bail!("not supported by the test API")
    }
    async fn switch_session(
        &self,
        _session_path: &str,
        _options: crate::core::extensions::types::SwitchSessionOptions,
    ) -> anyhow::Result<crate::core::extensions::types::SessionReplaceOutcome> {
        anyhow::bail!("not supported by the test API")
    }
    async fn reload(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn tool(name: &str, description: &str) -> Value {
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": { "q": { "type": "string" } } } })
}

fn codemode_tool(reaching: bool) -> ToolInfo {
    ToolInfo {
        name: CODEMODE_TOOL_NAME.into(),
        description: "Run a script".into(),
        parameters: json!({ "type": "object" }),
        prompt_guidelines: None,
        reaches_tools: reaching.then(|| {
            Arc::new(|input: &serde_json::Map<String, Value>| {
                let words = input
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .filter(|word| !word.is_empty())
                    .map(str::to_string)
                    .collect();
                ReachesTools::Words(words)
            }) as crate::core::extensions::types::ReachesToolsFn
        }),
        exposure: ToolExposure::Direct,
        namespace: None,
        annotations: None,
        source_info: source_info(),
    }
}

#[tokio::test]
async fn extension_registers_tools_with_their_exposure_and_namespace() {
    let mut transports = HashMap::new();
    transports.insert(
        "docs".to_string(),
        FakeServerOptions {
            tools: vec![tool("search", "Search docs."), tool("delete_all", "Delete.")],
            instructions: Some("Use search first.\nMore.".into()),
            ..Default::default()
        },
    );
    transports.insert(
        "issues".to_string(),
        FakeServerOptions {
            tools: vec![tool("list", "List issues.")],
            ..Default::default()
        },
    );
    let harness = Harness::new(
        vec![
            entry_with(
                "docs",
                json!({ "command": "x", "description": "Docs search", "toolExposure": { "delete_*": "hidden" } }),
            ),
            entry_with("issues", json!({ "command": "x", "exposure": "direct" })),
        ],
        transports,
        |_| {},
    )
    .await;
    harness.pi.state.lock().other_tools.push(codemode_tool(true));
    harness.start().await;
    // Codemode is activated from the config, before the servers connect.
    assert!(harness.active().contains(&CODEMODE_TOOL_NAME.to_string()));
    let section = harness.before_agent_start().await.unwrap();
    assert_eq!(
        section,
        "MCP servers whose tools are not declared to you. Call the tools of `codemode` servers from codemode scripts.\n- mcp__docs (codemode): Docs search"
    );
    harness
        .until(|harness| harness.tool("mcp__docs__search").is_some() && harness.tool("mcp__issues__list").is_some())
        .await;
    let search = harness.tool("mcp__docs__search").unwrap();
    assert_eq!(search.exposure, Some(ToolExposure::Deferred));
    assert_eq!(search.label, "docs/search");
    assert_eq!(search.description, "Search docs.");
    assert_eq!(
        search.namespace,
        Some(ToolNamespace {
            name: "mcp__docs".into(),
            aliases: Some(vec!["docs".into()]),
            description: Some("Docs search".into()),
            instructions: Some("Use search first.\nMore.".into()),
        })
    );
    assert_eq!(
        search.parameters,
        json!({ "type": "object", "properties": { "q": { "type": "string" } } })
    );
    assert_eq!(harness.exposure("mcp__docs__delete_all"), Some(ToolExposure::Hidden));
    assert_eq!(harness.exposure("mcp__issues__list"), Some(ToolExposure::Direct));
    assert!(harness.active().contains(&"mcp__issues__list".to_string()));

    // Calls run on the server.
    let result = (search.execute)("call-1".into(), json!({ "q": "pi" }), None, None, None)
        .await
        .unwrap();
    assert_eq!(result_json(&result)["content"], json!([{ "type": "text", "text": "ok" }]));
    assert_eq!(result.details, Some(json!({ "server": "docs", "tool": "search" })));

    // Calls to a hidden tool fail.
    let hidden = harness.tool("mcp__docs__delete_all").unwrap();
    let error = (hidden.execute)("call-2".into(), json!({}), None, None, None)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "MCP tool \"docs/delete_all\" is no longer available.");

    // The plain status lists both servers.
    harness.command("").await;
    let (status, _) = harness.notifications().last().cloned().unwrap();
    assert_eq!(
        status,
        "docs: connected, 2 tools (codemode)\nissues: connected, 1 tools (direct)"
    );
    harness.shutdown().await;
}

#[tokio::test]
async fn extension_renders_calls_to_mcp_tools_that_are_not_registered() {
    let harness = Harness::new(Vec::new(), HashMap::new(), |_| {}).await;
    let resolver = harness.pi.state.lock().renderers[0].clone();
    let rendered = resolver("mcp__my_docs__search", &|| None);
    assert!(rendered.is_some_and(|renderers| renderers.render_call.is_some()));
    assert!(resolver("not_mcp", &|| None).is_none());
    // Registered tools keep their own renderers.
    let own = ToolRenderers::default();
    assert!(resolver("mcp__a__b", &|| Some(own.clone())).is_some_and(|renderers| renderers.render_call.is_none()));
}

#[tokio::test]
async fn extension_does_not_activate_codemode_when_auto_enable_codemode_is_false() {
    let harness = Harness::new(
        vec![entry_with("docs", json!({ "command": "x" }))],
        HashMap::new(),
        |options| {
            options.load_config = Some(Arc::new(|_| LoadedMcpConfig {
                servers: vec![entry_with("docs", json!({ "command": "x" }))],
                auto_enable_codemode: Some(false),
                errors: Vec::new(),
                project_config: None,
            }));
        },
    )
    .await;
    harness.pi.state.lock().other_tools.push(codemode_tool(true));
    harness.start().await;
    assert!(!harness.active().contains(&CODEMODE_TOOL_NAME.to_string()));
    assert!(harness.notifications().iter().any(|(message, kind)| {
        *kind == NotifyType::Warning
            && message
                == "MCP tools are only reachable from the codemode or tool_search tool, but neither is active (autoEnableCodemode is false); they cannot be called."
    }));
    harness.shutdown().await;
}

#[tokio::test]
async fn extension_does_not_activate_another_extensions_tool_named_codemode() {
    let harness = Harness::new(
        vec![entry_with("docs", json!({ "command": "x" }))],
        HashMap::new(),
        |_| {},
    )
    .await;
    harness.pi.state.lock().other_tools.push(codemode_tool(false));
    harness.start().await;
    assert!(!harness.active().contains(&CODEMODE_TOOL_NAME.to_string()));
    assert!(harness
        .notifications()
        .iter()
        .any(|(message, _)| message.starts_with("MCP tools are only reachable")));
    harness.shutdown().await;
}

#[tokio::test]
async fn extension_registers_resource_tools_with_the_widest_exposure() {
    let mut transports = HashMap::new();
    transports.insert(
        "docs".to_string(),
        FakeServerOptions {
            resources: true,
            ..Default::default()
        },
    );
    let harness = Harness::new(
        vec![entry_with("docs", json!({ "command": "x", "exposure": "direct" }))],
        transports,
        |_| {},
    )
    .await;
    harness.start().await;
    harness
        .until(|harness| harness.tool(LIST_MCP_RESOURCES_TOOL).is_some())
        .await;
    assert_eq!(harness.exposure(LIST_MCP_RESOURCES_TOOL), Some(ToolExposure::Direct));
    assert_eq!(harness.exposure(READ_MCP_RESOURCE_TOOL), Some(ToolExposure::Direct));
    let list = harness.tool(LIST_MCP_RESOURCES_TOOL).unwrap();
    let result = (list.execute)("call".into(), json!({}), None, None, None)
        .await
        .unwrap();
    // MCP App resources, `_meta`, and icons are left out.
    assert_eq!(
        result.structured_content,
        Some(json!({ "resources": [{ "server": "docs", "uri": "docs://a", "name": "a" }] }))
    );
    let result = (list.execute)("call".into(), json!({ "server": "docs" }), None, None, None)
        .await
        .unwrap();
    assert_eq!(
        result.structured_content,
        Some(json!({ "server": "docs", "resources": [{ "server": "docs", "uri": "docs://a", "name": "a" }] }))
    );
    let error = (list.execute)("call".into(), json!({ "cursor": "x" }), None, None, None)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "cursor can only be used when a server is specified");
    let error = (list.execute)("call".into(), json!({ "server": "nope" }), None, None, None)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "MCP server \"nope\" has no resources. Servers with resources: docs"
    );
    let templates = harness.tool(LIST_MCP_RESOURCE_TEMPLATES_TOOL).unwrap();
    let result = (templates.execute)("call".into(), json!({}), None, None, None)
        .await
        .unwrap();
    assert_eq!(result.structured_content, Some(json!({ "resourceTemplates": [] })));
    let read = harness.tool(READ_MCP_RESOURCE_TOOL).unwrap();
    let result = (read.execute)(
        "call".into(),
        json!({ "server": "docs", "uri": "docs://a" }),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result_json(&result)["content"], json!([{ "type": "text", "text": "ok" }]));
    assert_eq!(
        result.structured_content,
        Some(json!({ "server": "docs", "uri": "docs://a", "contents": [{ "uri": "docs://a", "text": "ok" }] }))
    );
    harness.shutdown().await;
}

#[tokio::test]
async fn extension_waits_for_the_servers_a_codemode_script_names() {
    let mut transports = HashMap::new();
    for name in ["docs", "issues"] {
        transports.insert(
            name.to_string(),
            FakeServerOptions {
                tools: vec![tool("search", "Search.")],
                ..Default::default()
            },
        );
    }
    let harness = Harness::new(
        vec![
            entry_with("docs", json!({ "command": "x" })),
            entry_with("issues", json!({ "command": "x" })),
        ],
        transports,
        |_| {},
    )
    .await;
    harness.pi.state.lock().other_tools.push(codemode_tool(true));
    harness.start().await;
    harness
        .tool_call(CODEMODE_TOOL_NAME, json!({ "code": "await mcp__docs__search({})" }))
        .await;
    assert!(harness.tool("mcp__docs__search").is_some());
    harness.shutdown().await;
}

#[tokio::test]
async fn extension_connects_registered_servers_and_prefers_mcp_json() {
    let mut transports = HashMap::new();
    transports.insert(
        "my-docs".to_string(),
        FakeServerOptions {
            tools: vec![tool("search", "Search.")],
            ..Default::default()
        },
    );
    transports.insert(
        "later".to_string(),
        FakeServerOptions {
            tools: vec![tool("ping", "Ping.")],
            ..Default::default()
        },
    );
    let harness = Harness::new(
        vec![entry_with("my-docs", json!({ "command": "x", "exposure": "direct" }))],
        transports,
        |_| {},
    )
    .await;
    harness
        .pi
        .register_mcp_server(
            "my_docs",
            serde_json::from_value(json!({ "command": "y" })).unwrap(),
        )
        .unwrap();
    harness.start().await;
    harness.until(|harness| harness.tool("mcp__my_docs__search").is_some()).await;
    harness.command("").await;
    let (status, _) = harness.notifications().last().cloned().unwrap();
    assert_eq!(
        status,
        "my-docs: connected, 1 tools (direct)\noverridden: \"my_docs\" registered by /ext/servers.ts is overridden by \"my-docs\" in test"
    );

    // Servers registered during the session connect right away, and disconnect when unregistered.
    harness
        .pi
        .register_mcp_server(
            "later",
            serde_json::from_value(json!({ "command": "z", "exposure": "direct" })).unwrap(),
        )
        .unwrap();
    harness
        .emit(
            "mcp_servers_change",
            ExtensionEvent::McpServersChange(McpServersChangeEvent {
                servers: harness.pi.get_mcp_servers().unwrap(),
            }),
        )
        .await;
    assert_eq!(harness.exposure("mcp__later__ping"), Some(ToolExposure::Direct));
    harness.pi.unregister_mcp_server("later").unwrap();
    harness
        .emit(
            "mcp_servers_change",
            ExtensionEvent::McpServersChange(McpServersChangeEvent {
                servers: harness.pi.get_mcp_servers().unwrap(),
            }),
        )
        .await;
    assert_eq!(harness.exposure("mcp__later__ping"), Some(ToolExposure::Hidden));
    harness.shutdown().await;
}

#[tokio::test]
async fn extension_reports_servers_that_need_attention() {
    let harness = Harness::new(
        vec![entry_with("docs", json!({ "command": "x" }))],
        HashMap::new(),
        |options| {
            options.create_transport = Some(Arc::new(|_, _, _| anyhow::bail!("no transport")));
            options.load_config = Some(Arc::new(|_| LoadedMcpConfig {
                servers: vec![entry_with("docs", json!({ "command": "x" }))],
                auto_enable_codemode: None,
                errors: vec!["mcp.json: broken".into()],
                project_config: None,
            }));
        },
    )
    .await;
    harness.start().await;
    harness
        .until(|harness| {
            harness
                .notifications()
                .iter()
                .any(|(message, _)| message.starts_with("MCP servers need attention"))
        })
        .await;
    let (message, kind) = harness
        .notifications()
        .into_iter()
        .find(|(message, _)| message.starts_with("MCP servers need attention"))
        .unwrap();
    assert_eq!(kind, NotifyType::Warning);
    assert_eq!(
        message,
        "MCP servers need attention:\n  config: mcp.json: broken\n  docs: failed: no transport\nRun /mcp to fix.\n"
            .trim_end()
    );
    harness.shutdown().await;
}

#[tokio::test]
async fn extension_completes_mcp_subcommands() {
    let harness = Harness::new(Vec::new(), HashMap::new(), |_| {}).await;
    let completions = harness
        .pi
        .state
        .lock()
        .commands
        .get("mcp")
        .unwrap()
        .get_argument_completions
        .clone()
        .unwrap();
    let items = completions("lo".into()).await.unwrap();
    assert_eq!(
        items.iter().map(|item| item.value.as_str()).collect::<Vec<_>>(),
        vec!["login ", "logout "]
    );
    assert!(completions("login x y".into()).await.is_none());
    assert!(completions("login ".into()).await.is_none());
}

// ------------------------------------------------------------------------------------------------
// Manager view
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn manager_view_menu_rebuilds_on_changes_and_resolves_the_choice() {
    use crate::modes::interactive::theme::theme::{init_theme, theme};
    init_theme(Some("dark"), Some(false));
    let view = ui::McpManagerView::create(
        None,
        theme(),
        Arc::new(crate::core::keybindings::KeybindingsManager::new(None, None)),
    );
    let counter = Arc::new(AtomicUsize::new(0));
    let listeners: Arc<Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>> = Arc::default();
    let build_counter = counter.clone();
    let registered = listeners.clone();
    let menu_view = view.clone();
    let menu = tokio::spawn(async move {
        menu_view
            .menu(
                Arc::new(move || McpMenu {
                    title: "MCP servers".into(),
                    items: vec![
                        SelectItem::new("a", "a", Some(format!("build {}", build_counter.fetch_add(1, Ordering::SeqCst)))),
                        SelectItem::new("b", "b", None),
                    ],
                    confirm_label: "manage".into(),
                    cancel_label: "close".into(),
                    ..Default::default()
                }),
                Some(Arc::new(move |listener| {
                    registered.lock().push(listener);
                    Arc::new(|| {})
                })),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    let rendered = || crate::utils::ansi::strip_ansi(&view.render(80).join("\n")).unwrap();
    assert!(rendered().contains("build 0"), "{}", rendered());
    let listener = listeners.lock()[0].clone();
    listener();
    assert!(rendered().contains("build 1"), "{}", rendered());
    view.handle_input("\x1b[B");
    view.handle_input("\r");
    assert_eq!(menu.await.unwrap().unwrap().as_deref(), Some("b"));
}

// #10565
#[tokio::test]
async fn manager_status_screen_cancels_the_running_operation_with_the_cancel_key() {
    use crate::modes::interactive::theme::theme::{init_theme, theme};
    init_theme(Some("dark"), Some(false));
    let view = ui::McpManagerView::create(
        None,
        theme(),
        Arc::new(crate::core::keybindings::KeybindingsManager::new(None, None)),
    );
    let cancelled = Arc::new(AtomicUsize::new(0));
    let counter = cancelled.clone();
    view.status(
        "Sign in to issues",
        "Contacting the authorization server…",
        Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })),
    );
    let rendered = crate::utils::ansi::strip_ansi(&view.render(80).join("\n")).unwrap();
    assert!(rendered.contains("cancel"), "{rendered}");
    view.handle_input("\x1b");
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);

    view.status("Sign in to issues", "Connecting…", None);
    let rendered = crate::utils::ansi::strip_ansi(&view.render(80).join("\n")).unwrap();
    assert!(!rendered.contains("cancel"), "{rendered}");
    view.handle_input("\x1b");
}
