//! Port of `packages/coding-agent/src/extensions/mcp/config.ts`.

use crate::config::CONFIG_DIR_NAME;
use crate::core::mcp_servers::{mcp_namespace, validate_mcp_server_config};
use crate::utils::text::strip_bom;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;

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
    let mut state = McpConfigState {
        servers: IndexMap::new(),
        auto_enable_codemode: None,
        errors: Vec::new(),
    };
    let global = Path::new(&options.agent_dir).join("mcp.json");
    read_config_file(&global.to_string_lossy(), McpServerEntryScope::Global, &mut state);
    let project_config = options.project_trusted.then(|| {
        Path::new(&options.cwd)
            .join(CONFIG_DIR_NAME)
            .join("mcp.json")
            .to_string_lossy()
            .into_owned()
    });
    if let Some(project) = &project_config {
        read_config_file(project, McpServerEntryScope::Project, &mut state);
    }
    LoadedMcpConfig {
        servers: state.servers.into_values().collect(),
        auto_enable_codemode: state.auto_enable_codemode,
        errors: state.errors,
        project_config,
    }
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
    let add_override = options.and_then(|options| options.r#override).unwrap_or(false);
    edit_mcp_servers(path, |parsed| {
        let servers = parsed
            .entry("mcpServers")
            .or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(servers) = servers else {
            unreachable!("checked by edit_mcp_servers")
        };
        if !servers.contains_key(name) && add_override {
            servers.insert(name.to_string(), Value::Object(Map::new()));
        }
        let Some(Value::Object(server)) = servers.get_mut(name) else {
            anyhow::bail!("{path} does not define MCP server \"{name}\"");
        };
        let keep_defaults = is_override(server);
        if let Some(enabled) = patch.enabled {
            if enabled && !keep_defaults {
                server.shift_remove("enabled");
            } else {
                server.insert("enabled".into(), Value::Bool(enabled));
            }
        }
        if let Some(exposure) = patch.exposure {
            if exposure == McpExposure::Codemode && !keep_defaults {
                server.shift_remove("exposure");
            } else {
                server.insert("exposure".into(), Value::String(exposure.as_str().into()));
            }
        }
        Ok(true)
    })
}

/// Add a server to an `mcp.json`, creating the file when missing. An existing entry with the same
/// name is replaced. Returns true when an entry was replaced.
pub fn add_mcp_server_config(path: &str, name: &str, config: &McpServerConfig) -> bool {
    add_mcp_server_config_value(
        path,
        name,
        serde_json::to_value(config).unwrap_or(Value::Null),
    )
    .unwrap_or(false)
}

/// [`add_mcp_server_config`] with the config as JSON, keeping its key order, and reporting
/// failures to read or write the file.
pub fn add_mcp_server_config_value(path: &str, name: &str, config: Value) -> anyhow::Result<bool> {
    let mut replaced = false;
    edit_mcp_servers(path, |parsed| {
        let servers = parsed
            .entry("mcpServers")
            .or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(servers) = servers else {
            unreachable!("checked by edit_mcp_servers")
        };
        replaced = servers.contains_key(name);
        servers.insert(name.to_string(), config.clone());
        Ok(true)
    })?;
    Ok(replaced)
}

/// Remove a server from an `mcp.json`. Returns false when the file does not define it.
pub fn remove_mcp_server_config(path: &str, name: &str) -> bool {
    try_remove_mcp_server_config(path, name).unwrap_or(false)
}

/// [`remove_mcp_server_config`], reporting failures to read or write the file.
pub fn try_remove_mcp_server_config(path: &str, name: &str) -> anyhow::Result<bool> {
    if !Path::new(path).exists() {
        return Ok(false);
    }
    let mut removed = false;
    edit_mcp_servers(path, |parsed| {
        let Some(Value::Object(servers)) = parsed.get_mut("mcpServers") else {
            return Ok(false);
        };
        if servers.shift_remove(name).is_none() {
            return Ok(false);
        }
        removed = true;
        Ok(true)
    })?;
    Ok(removed)
}

const OVERRIDE_KEYS: [&str; 3] = ["enabled", "exposure", "toolExposure"];

/// Whether an entry overrides a server defined elsewhere instead of defining one.
fn is_override(value: &Map<String, Value>) -> bool {
    !value.contains_key("command") && !value.contains_key("url") && !value.contains_key("type")
}

fn error_message(error: &serde_json::Error) -> String {
    error.to_string()
}

fn read_config_file(path: &str, scope: McpServerEntryScope, state: &mut McpConfigState) {
    if !Path::new(path).exists() {
        return;
    }
    let parsed: Value = match std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            serde_json::from_str(&strip_bom(&text)).map_err(|error| error_message(&error))
        }) {
        Ok(parsed) => parsed,
        Err(error) => {
            state.errors.push(format!("{path}: {error}"));
            return;
        }
    };
    let Value::Object(parsed) = parsed else {
        state
            .errors
            .push(format!("{path}: expected an object with an \"mcpServers\" object"));
        return;
    };
    let servers = match parsed.get("mcpServers") {
        None => Map::new(),
        Some(Value::Object(servers)) => servers.clone(),
        Some(_) => {
            state
                .errors
                .push(format!("{path}: expected an object with an \"mcpServers\" object"));
            return;
        }
    };
    match parsed.get("autoEnableCodemode") {
        Some(Value::Bool(value)) => state.auto_enable_codemode = Some(*value),
        Some(_) => state
            .errors
            .push(format!("{path}: autoEnableCodemode must be a boolean")),
        None => {}
    }
    for (name, value) in servers {
        if scope == McpServerEntryScope::Project
            && let Value::Object(object) = &value
            && is_override(object)
        {
            let Some(base) = state.servers.get(&name).cloned() else {
                state.errors.push(format!(
                    "{path}: server \"{name}\" needs \"command\" or \"url\", or a global server to override"
                ));
                continue;
            };
            if object.keys().any(|key| !OVERRIDE_KEYS.contains(&key.as_str())) {
                state.errors.push(format!(
                    "{path}: server \"{name}\": an override can only set {}",
                    OVERRIDE_KEYS.join(", ")
                ));
                continue;
            }
            let mut merged = match serde_json::to_value(&base.config) {
                Ok(Value::Object(merged)) => merged,
                _ => Map::new(),
            };
            for (key, value) in object {
                merged.insert(key.clone(), value.clone());
            }
            match validate_mcp_server_config(&name, Value::Object(merged)).into_result() {
                Err(error) => state.errors.push(format!("{path}: {error}")),
                Ok(config) => {
                    state.servers.insert(
                        name.clone(),
                        McpServerEntry {
                            config,
                            r#override: Some(path.to_string()),
                            ..base
                        },
                    );
                }
            }
            continue;
        }
        let config = match validate_mcp_server_config(&name, value).into_result() {
            Ok(config) => config,
            Err(error) => {
                state.errors.push(format!("{path}: {error}"));
                continue;
            }
        };
        // Names that differ only in `-` and `_` would share a namespace.
        let namespace = mcp_namespace(&name);
        if let Some(clash) = state
            .servers
            .keys()
            .find(|other| **other != name && mcp_namespace(other) == namespace)
        {
            state
                .errors
                .push(format!("{path}: server \"{name}\" conflicts with \"{clash}\""));
            continue;
        }
        if scope == McpServerEntryScope::Project
            && let McpServerConfig::McpHttpServerConfig(http) = &config
            && http.auth.is_some()
        {
            state.errors.push(format!(
                "{path}: server \"{name}\": auth is only allowed in the global mcp.json"
            ));
            continue;
        }
        state.servers.insert(
            name.clone(),
            McpServerEntry {
                name,
                config,
                source: path.to_string(),
                scope: Some(scope),
                r#override: None,
            },
        );
    }
}

/// `JSON.stringify(value, null, indent)`.
pub(crate) fn json_stringify_indented(value: &Value, indent: &str) -> String {
    use serde::Serialize;
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    if value.serialize(&mut serializer).is_err() {
        return "null".into();
    }
    String::from_utf8(out).unwrap_or_default()
}

/// The indentation of the first indented line, like `/^([ \t]+)\S/m`.
fn detect_indent(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let rest = &line[indent.len()..];
        (!indent.is_empty() && rest.chars().next().is_some_and(|c| !c.is_whitespace()))
            .then_some(indent)
    })
}

/// Read an `mcp.json` (an empty config when missing), let `edit` change it, and write it back with
/// its indentation when `edit` returns true. Other content is kept.
fn edit_mcp_servers(
    path: &str,
    edit: impl FnOnce(&mut Map<String, Value>) -> anyhow::Result<bool>,
) -> anyhow::Result<()> {
    let text = if Path::new(path).exists() {
        Some(strip_bom(&std::fs::read_to_string(path)?))
    } else {
        None
    };
    let parsed: Value = match &text {
        None => Value::Object(Map::new()),
        Some(text) => serde_json::from_str(text).map_err(|error| anyhow::anyhow!("{error}"))?,
    };
    let mut parsed = match parsed {
        Value::Object(parsed)
            if parsed
                .get("mcpServers")
                .is_none_or(|servers| servers.is_object()) =>
        {
            parsed
        }
        _ => anyhow::bail!("{path}: expected an object with an \"mcpServers\" object"),
    };
    if !edit(&mut parsed)? {
        return Ok(());
    }
    let indent = text
        .as_deref()
        .and_then(detect_indent)
        .unwrap_or_else(|| "  ".to_string());
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        format!("{}\n", json_stringify_indented(&Value::Object(parsed), &indent)),
    )?;
    Ok(())
}
