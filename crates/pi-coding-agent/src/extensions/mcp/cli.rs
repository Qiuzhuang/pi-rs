//! Port of `packages/coding-agent/src/extensions/mcp/cli.ts`.
//!
//! `pi mcp`: add, remove, and check MCP servers and sign in to them outside a session. Agents run it
//! through bash to configure servers, verify an `mcp.json` they wrote, and start an OAuth sign-in;
//! the user only approves access in the browser. Running sessions pick up new credentials on their
//! next turn.

use crate::config::{APP_NAME, CONFIG_DIR_NAME};
use crate::core::mcp_servers::{McpServerConfig, validate_mcp_server_config};
use crate::core::trust_manager::ProjectTrustStore;
use crate::extensions::mcp::config::{
    LoadMcpConfigOptions, LoadedMcpConfig, McpServerEntry, McpServerEntryScope,
    add_mcp_server_config_value, get_mcp_tool_exposure, json_stringify_indented, load_mcp_config,
    try_remove_mcp_server_config,
};
use crate::extensions::mcp::oauth::{McpSignInPrompt, SignInMcpServerOptions};
use crate::extensions::mcp::runtime::{
    McpOAuthCredentialStore, McpServerConnection, McpServerConnectionOptions, McpServerLog,
    McpSignInCancelledError, default_transport_factory, sign_in_mcp_server,
};
use crate::utils::chalk;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn help() -> String {
    format!(
        r#"{}
  {APP_NAME} mcp add <server> [options] -- <command> [args...]
  {APP_NAME} mcp add <server> [options] --url <url>
  {APP_NAME} mcp remove <server> [-l]
  {APP_NAME} mcp list [--json]
  {APP_NAME} mcp login <server> [--timeout <seconds>]
  {APP_NAME} mcp logout <server>

Configure and check MCP servers and sign in to OAuth servers without starting a session.
Reads ~/{CONFIG_DIR_NAME}/agent/mcp.json and, in trusted projects, {CONFIG_DIR_NAME}/mcp.json.

Commands:
  add <server>            Add or replace a server in mcp.json
  remove <server>         Remove a server from mcp.json
  list                    Show state, tools, and errors (exits 1 on failure)
  login <server>          Sign in through the browser
  logout <server>         Delete the stored OAuth credentials

Options for add and remove:
  -l, --local             Use {CONFIG_DIR_NAME}/mcp.json in the current project instead of the global file

Options for add:
  --url <url>             Streamable HTTP server URL (instead of a command)
  --env <KEY=VALUE>       Environment variable for a stdio server (repeatable)
  --cwd <dir>             Working directory for a stdio server
  --header <KEY=VALUE>    HTTP header (repeatable)
  --bearer-token-env-var <NAME>
                          Send "Authorization: Bearer ${{NAME}}"
  --oauth-client-id <id>  Pre-registered OAuth client id
  --oauth-client-secret <secret>
                          OAuth client secret (may be ${{NAME}} or !command)
  --oauth-callback-port <port>
                          Fixed OAuth callback port
  --oauth-client-name <name>
                          Client name sent when registering with the OAuth server
  --exposure <mode>       codemode (default), deferred, direct, or hidden
  --description <text>    What the server offers, shown in the system prompt

Other options:
  --json                  Print the list as JSON
  --timeout <seconds>     Time limit for the whole sign-in (default: 300)"#,
        chalk::bold("Usage:")
    )
}

fn help_hint() -> String {
    chalk::dim(&format!("Use \"{APP_NAME} mcp --help\" for usage."))
}

const DEFAULT_LOGIN_TIMEOUT_SECONDS: f64 = 300.0;

type Output = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct McpCommandOptions {
    pub cwd: String,
    pub agent_dir: String,
    /// Defaults to `mcp-auth.json` in the agent directory.
    pub credentials: Option<Arc<McpOAuthCredentialStore>>,
    /// Defaults to the platform browser.
    pub open_url: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Defaults to console output.
    pub log: Option<Output>,
    pub error: Option<Output>,
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

impl ParsedOptions {
    fn value(&self, option: &str) -> Option<String> {
        match self.values.get(option) {
            Some(ParsedOptionsValues::String(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn has(&self, option: &str) -> bool {
        self.values.contains_key(option)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionKind {
    Flag,
    Value,
    List,
}

fn describe_transport(entry: &McpServerEntry) -> String {
    match &entry.config {
        McpServerConfig::McpHttpServerConfig(config) => config.url.clone(),
        McpServerConfig::McpStdioServerConfig(config) => {
            let mut parts = vec![config.command.clone()];
            parts.extend(config.args.iter().flatten().cloned());
            parts.join(" ")
        }
    }
}

fn create_connection(
    entry: &McpServerEntry,
    options: &McpCommandOptions,
    credentials: &Arc<McpOAuthCredentialStore>,
) -> Arc<McpServerConnection> {
    let log_path = Path::new(&options.agent_dir).join("mcp.log");
    McpServerConnection::new(&McpServerConnectionOptions {
        entry: entry.clone(),
        cwd: options.cwd.clone(),
        create_transport: default_transport_factory(),
        credentials: credentials.clone(),
        provider_token: None,
        on_tools: Arc::new(|_| {}),
        on_change: None,
        log: Some(Arc::new(McpServerLog::new(&log_path.to_string_lossy()))),
    })
}

/// Short spellings of options. `-l`/`--local` match `pi install`.
fn option_alias(arg: &str) -> &str {
    match arg {
        "-l" => "--local",
        other => other,
    }
}

/// Parse `--name value` options; returns undefined and reports unknown ones. `--` ends the options,
/// as does reaching `max_positionals` positional arguments: the remaining arguments are positional,
/// so a command's own options (`add <server> <command> --flag`) are passed through.
fn parse_options(
    args: &[String],
    known: &[(&str, OptionKind)],
    error: &Output,
    max_positionals: usize,
) -> Option<ParsedOptions> {
    let mut parsed = ParsedOptions {
        positional: Vec::new(),
        values: IndexMap::new(),
        lists: IndexMap::new(),
    };
    let mut index = 0;
    while index < args.len() {
        let arg = option_alias(&args[index]);
        if arg == "--" || parsed.positional.len() >= max_positionals {
            let rest = if arg == "--" { index + 1 } else { index };
            parsed.positional.extend(args[rest..].iter().cloned());
            break;
        }
        if !arg.starts_with("--") {
            parsed.positional.push(arg.to_string());
            index += 1;
            continue;
        }
        let name = &arg[2..];
        let Some((_, kind)) = known.iter().find(|(option, _)| *option == name) else {
            error(format!("Unknown option {arg}.\n{}", help_hint()));
            return None;
        };
        if *kind == OptionKind::Flag {
            parsed
                .values
                .insert(name.to_string(), ParsedOptionsValues::Bool(true));
            index += 1;
            continue;
        }
        index += 1;
        let Some(value) = args.get(index) else {
            error(format!("{arg} needs a value."));
            return None;
        };
        if *kind == OptionKind::List {
            parsed
                .lists
                .entry(name.to_string())
                .or_default()
                .push(value.clone());
        } else {
            parsed
                .values
                .insert(name.to_string(), ParsedOptionsValues::String(value.clone()));
        }
        index += 1;
    }
    Some(parsed)
}

fn project_trusted(options: &McpCommandOptions) -> bool {
    matches!(
        ProjectTrustStore::new(&options.agent_dir).get(&options.cwd),
        Ok(Some(true))
    )
}

/// Run `pi mcp <args>` and return the exit code.
pub async fn run_mcp_command(args: &[String], options: &McpCommandOptions) -> anyhow::Result<i32> {
    let log: Output = options
        .log
        .clone()
        .unwrap_or_else(|| Arc::new(|line: String| println!("{line}")));
    let error: Output = options
        .error
        .clone()
        .unwrap_or_else(|| Arc::new(|line: String| eprintln!("{line}")));
    let Some(command) = args.first().map(String::as_str) else {
        log(help());
        return Ok(0);
    };
    if command == "help" || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        log(help());
        return Ok(0);
    }
    let rest = &args[1..];

    let project_config = Path::new(&options.cwd)
        .join(CONFIG_DIR_NAME)
        .join("mcp.json")
        .to_string_lossy()
        .into_owned();
    if command == "add" {
        return Ok(add(rest, &project_config, options, &log, &error));
    }
    if command == "remove" {
        return Ok(remove(rest, &project_config, options, &log, &error));
    }
    let trusted = project_trusted(options);
    let loaded = load_mcp_config(&LoadMcpConfigOptions {
        agent_dir: options.agent_dir.clone(),
        cwd: options.cwd.clone(),
        project_trusted: trusted,
    });
    let untrusted_note = (!trusted && Path::new(&project_config).exists()).then(|| {
        format!(
            "{project_config} is ignored because the project is not trusted. Start {APP_NAME} in the project to trust it."
        )
    });
    let credentials = options
        .credentials
        .clone()
        .unwrap_or_else(|| Arc::new(McpOAuthCredentialStore::new(None, None)));

    match command {
        "list" => {
            let Some(parsed) =
                parse_options(rest, &[("json", OptionKind::Flag)], &error, usize::MAX)
            else {
                return Ok(1);
            };
            if !parsed.positional.is_empty() {
                error(format!(
                    "Usage: {APP_NAME} mcp list [--json]\n{}",
                    help_hint()
                ));
                return Ok(1);
            }
            Ok(list(
                &loaded,
                parsed.has("json"),
                untrusted_note.as_deref(),
                options,
                &credentials,
                &log,
            )
            .await)
        }
        "login" | "logout" => {
            let known: &[(&str, OptionKind)] = if command == "login" {
                &[("timeout", OptionKind::Value)]
            } else {
                &[]
            };
            let Some(parsed) = parse_options(rest, known, &error, usize::MAX) else {
                return Ok(1);
            };
            let name = parsed.positional.first().cloned().unwrap_or_default();
            if name.is_empty() || parsed.positional.len() > 1 {
                error(format!(
                    "Usage: {APP_NAME} mcp {command} <server>\n{}",
                    help_hint()
                ));
                return Ok(1);
            }
            let Some(entry) = loaded.servers.iter().find(|server| server.name == name) else {
                let configured = loaded
                    .servers
                    .iter()
                    .map(|server| server.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                error(format!(
                    "No MCP server named \"{name}\".{} Configured: {}.",
                    untrusted_note
                        .as_deref()
                        .map(|note| format!(" {note}"))
                        .unwrap_or_default(),
                    if configured.is_empty() {
                        "none"
                    } else {
                        &configured
                    }
                ));
                return Ok(1);
            };
            let connection = create_connection(entry, options, &credentials);
            let Some(url) = connection.oauth_url() else {
                error(format!(
                    "MCP server \"{name}\" does not use OAuth. Only HTTP servers without an Authorization header do."
                ));
                return Ok(1);
            };
            if command == "logout" {
                let removed = credentials.remove(&name, &url);
                log(if removed {
                    format!("Signed out of MCP server \"{name}\".")
                } else {
                    format!("No stored credentials for MCP server \"{name}\".")
                });
                return Ok(0);
            }
            let timeout = match parsed.value("timeout") {
                Some(value) => js_number(&value),
                None => DEFAULT_LOGIN_TIMEOUT_SECONDS,
            };
            if !timeout.is_finite() || timeout <= 0.0 {
                error("--timeout must be a positive number of seconds.".to_string());
                return Ok(1);
            }
            let code = login(
                entry,
                &connection,
                &url,
                timeout * 1000.0,
                options,
                &credentials,
                &log,
                &error,
            )
            .await;
            let _ = connection.close().await;
            Ok(code)
        }
        _ => {
            error(format!("Unknown mcp command \"{command}\".\n{}", help_hint()));
            Ok(1)
        }
    }
}

/// `Number(value)`: NaN for text that is not a number.
fn js_number(value: &str) -> f64 {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

/// Parse `KEY=VALUE` pairs of a repeatable option into a record.
fn parse_pairs(
    option: &str,
    pairs: Option<&Vec<String>>,
    error: &Output,
) -> Option<Map<String, Value>> {
    let mut record = Map::new();
    for pair in pairs.into_iter().flatten() {
        match pair.find('=') {
            Some(separator) if separator > 0 => {
                record.insert(
                    pair[..separator].to_string(),
                    Value::String(pair[separator + 1..].to_string()),
                );
            }
            _ => {
                error(format!("--{option} expects KEY=VALUE, got \"{pair}\"."));
                return None;
            }
        }
    }
    Some(record)
}

fn add(
    args: &[String],
    project_config: &str,
    options: &McpCommandOptions,
    log: &Output,
    error: &Output,
) -> i32 {
    let usage = format!(
        "Usage: {APP_NAME} mcp add <server> [options] (--url <url> | -- <command> [args...])\n{}",
        help_hint()
    );
    let Some(parsed) = parse_options(
        args,
        &[
            ("local", OptionKind::Flag),
            ("url", OptionKind::Value),
            ("env", OptionKind::List),
            ("cwd", OptionKind::Value),
            ("header", OptionKind::List),
            ("bearer-token-env-var", OptionKind::Value),
            ("oauth-client-id", OptionKind::Value),
            ("oauth-client-secret", OptionKind::Value),
            ("oauth-callback-port", OptionKind::Value),
            ("oauth-client-name", OptionKind::Value),
            ("exposure", OptionKind::Value),
            ("description", OptionKind::Value),
        ],
        error,
        2,
    ) else {
        return 1;
    };
    let name = parsed.positional.first().cloned().unwrap_or_default();
    let command: Vec<String> = parsed.positional.iter().skip(1).cloned().collect();
    let url = parsed.value("url");
    if name.is_empty() || url.is_none() == command.is_empty() {
        error(usage);
        return 1;
    }
    let http_only = [
        "header",
        "bearer-token-env-var",
        "oauth-client-id",
        "oauth-client-secret",
        "oauth-callback-port",
        "oauth-client-name",
    ];
    let stdio_only = ["env", "cwd"];
    let candidates: &[&str] = if url.is_none() { &http_only } else { &stdio_only };
    if let Some(misplaced) = candidates
        .iter()
        .find(|option| parsed.values.contains_key(**option) || parsed.lists.contains_key(**option))
    {
        error(format!(
            "--{misplaced} only applies to {}.",
            if url.is_none() {
                "HTTP servers (--url)"
            } else {
                "stdio servers"
            }
        ));
        return 1;
    }

    let mut config = Map::new();
    if let Some(url) = &url {
        let Some(mut headers) = parse_pairs("header", parsed.lists.get("header"), error) else {
            return 1;
        };
        if let Some(bearer) = parsed.value("bearer-token-env-var") {
            headers.insert(
                "Authorization".into(),
                Value::String(format!("Bearer ${{{bearer}}}")),
            );
        }
        let mut oauth = Map::new();
        if let Some(client_id) = parsed.value("oauth-client-id") {
            oauth.insert("clientId".into(), Value::String(client_id));
        }
        if let Some(secret) = parsed.value("oauth-client-secret") {
            oauth.insert("clientSecret".into(), Value::String(secret));
        }
        if let Some(port) = parsed.value("oauth-callback-port") {
            let port = js_number(&port);
            oauth.insert(
                "callbackPort".into(),
                serde_json::Number::from_f64(port)
                    .map(|number| {
                        if port.fract() == 0.0 && port.abs() < 1e15 {
                            Value::from(port as i64)
                        } else {
                            Value::Number(number)
                        }
                    })
                    .unwrap_or(Value::Null),
            );
        }
        if let Some(client_name) = parsed.value("oauth-client-name") {
            oauth.insert("clientName".into(), Value::String(client_name));
        }
        config.insert("url".into(), Value::String(url.clone()));
        if !headers.is_empty() {
            config.insert("headers".into(), Value::Object(headers));
        }
        if !oauth.is_empty() {
            config.insert("oauth".into(), Value::Object(oauth));
        }
    } else {
        let Some(env) = parse_pairs("env", parsed.lists.get("env"), error) else {
            return 1;
        };
        config.insert("command".into(), Value::String(command[0].clone()));
        if command.len() > 1 {
            config.insert(
                "args".into(),
                Value::Array(command[1..].iter().cloned().map(Value::String).collect()),
            );
        }
        if !env.is_empty() {
            config.insert("env".into(), Value::Object(env));
        }
        if let Some(cwd) = parsed.value("cwd") {
            config.insert("cwd".into(), Value::String(cwd));
        }
    }
    if let Some(exposure) = parsed.value("exposure") {
        config.insert("exposure".into(), Value::String(exposure));
    }
    if let Some(description) = parsed.value("description") {
        config.insert("description".into(), Value::String(description));
    }
    let validated = match validate_mcp_server_config(&name, Value::Object(config.clone()))
        .into_result()
    {
        Ok(validated) => validated,
        Err(message) => {
            error(message);
            return 1;
        }
    };
    // Write the config as given, with exposure aliases resolved.
    if let Some(exposure) = validated.exposure() {
        config.insert("exposure".into(), Value::String(exposure.as_str().into()));
    }

    let project = parsed.has("local");
    let path = if project {
        project_config.to_string()
    } else {
        Path::new(&options.agent_dir)
            .join("mcp.json")
            .to_string_lossy()
            .into_owned()
    };
    let scope = if project { "project" } else { "global" };
    let replaced = match add_mcp_server_config_value(&path, &name, Value::Object(config)) {
        Ok(replaced) => replaced,
        Err(add_error) => {
            error(format!("Could not update {path}: {add_error}"));
            return 1;
        }
    };
    log(format!(
        "{} {scope} MCP server \"{name}\" in {path}.",
        if replaced { "Replaced" } else { "Added" }
    ));
    if project && !project_trusted(options) {
        log(format!(
            "The project is not trusted, so {path} is ignored until you start {APP_NAME} in the project and trust it."
        ));
    }
    // HTTP servers without an Authorization header may use OAuth.
    let may_need_sign_in = match &validated {
        McpServerConfig::McpHttpServerConfig(http) => !http.headers.as_ref().is_some_and(|headers| {
            headers
                .keys()
                .any(|header| header.eq_ignore_ascii_case("authorization"))
        }),
        McpServerConfig::McpStdioServerConfig(_) => false,
    };
    log(format!(
        "Check it with: {APP_NAME} mcp list{}",
        if may_need_sign_in {
            format!(". If it requires sign-in: {APP_NAME} mcp login {name}")
        } else {
            String::new()
        }
    ));
    0
}

fn remove(
    args: &[String],
    project_config: &str,
    options: &McpCommandOptions,
    log: &Output,
    error: &Output,
) -> i32 {
    let Some(parsed) = parse_options(args, &[("local", OptionKind::Flag)], error, usize::MAX)
    else {
        return 1;
    };
    let name = parsed.positional.first().cloned().unwrap_or_default();
    if name.is_empty() || parsed.positional.len() > 1 {
        error(format!(
            "Usage: {APP_NAME} mcp remove <server> [-l]\n{}",
            help_hint()
        ));
        return 1;
    }
    let project = parsed.has("local");
    let global_config = Path::new(&options.agent_dir)
        .join("mcp.json")
        .to_string_lossy()
        .into_owned();
    let path = if project {
        project_config.to_string()
    } else {
        global_config
    };
    let (scope, scope_name) = if project {
        (McpServerEntryScope::Project, "project")
    } else {
        (McpServerEntryScope::Global, "global")
    };
    let removed = match try_remove_mcp_server_config(&path, &name) {
        Ok(removed) => removed,
        Err(remove_error) => {
            error(format!("Could not update {path}: {remove_error}"));
            return 1;
        }
    };
    if removed {
        log(format!(
            "Removed {scope_name} MCP server \"{name}\" from {path}."
        ));
        return 0;
    }
    let other = load_mcp_config(&LoadMcpConfigOptions {
        agent_dir: options.agent_dir.clone(),
        cwd: options.cwd.clone(),
        project_trusted: true,
    })
    .servers
    .into_iter()
    .find(|server| server.name == name && server.scope != Some(scope));
    let hint = match other {
        Some(other) => format!(
            " It is defined in {}{}.",
            other.source,
            if other.scope == Some(McpServerEntryScope::Project) {
                "; use --local"
            } else {
                "; omit --local"
            }
        ),
        None => String::new(),
    };
    error(format!(
        "No {scope_name} MCP server named \"{name}\" in {path}.{hint}"
    ));
    1
}

async fn report_server(
    entry: &McpServerEntry,
    options: &McpCommandOptions,
    credentials: &Arc<McpOAuthCredentialStore>,
) -> ServerReport {
    let exposure = entry
        .config
        .exposure()
        .unwrap_or(crate::core::mcp_servers::McpExposure::Codemode);
    let enabled = match &entry.config {
        McpServerConfig::McpStdioServerConfig(config) => config.enabled,
        McpServerConfig::McpHttpServerConfig(config) => config.enabled,
    } != Some(false);
    let mut report = ServerReport {
        name: entry.name.clone(),
        scope: entry
            .scope
            .map(|scope| scope.as_str().to_string())
            .unwrap_or_else(|| "global".to_string()),
        source: entry.source.clone(),
        r#override: entry.r#override.clone(),
        enabled,
        exposure: exposure.as_str().to_string(),
        transport: describe_transport(entry),
        state: "disabled".to_string(),
        tools: Vec::new(),
        tool_exposure: None,
        resources: None,
        error: None,
    };
    if !report.enabled {
        return report;
    }
    let connection = create_connection(entry, options, credentials);
    // The connection records the state and error.
    let _ = connection.get_client().await;
    report.state = connection.state().as_str().to_string();
    let tools = connection.tools();
    report.tools = tools.iter().map(|tool| tool.name.clone()).collect();
    let overrides: IndexMap<String, String> = tools
        .iter()
        .filter_map(|tool| {
            let tool_exposure = get_mcp_tool_exposure(&entry.config, &tool.name);
            (tool_exposure != exposure)
                .then(|| (tool.name.clone(), tool_exposure.as_str().to_string()))
        })
        .collect();
    if !overrides.is_empty() {
        report.tool_exposure = Some(overrides);
    }
    if connection.has_resources() {
        report.resources = Some(true);
    }
    if report.state != "connected" {
        report.error = connection.error().filter(|error| !error.is_empty());
    }
    let _ = connection.close().await;
    report
}

async fn list(
    loaded: &LoadedMcpConfig,
    json: bool,
    untrusted_note: Option<&str>,
    options: &McpCommandOptions,
    credentials: &Arc<McpOAuthCredentialStore>,
    log: &Output,
) -> i32 {
    let reports: Vec<ServerReport> = futures::future::join_all(
        loaded
            .servers
            .iter()
            .map(|entry| report_server(entry, options, credentials)),
    )
    .await;
    let failed = !loaded.errors.is_empty()
        || reports
            .iter()
            .any(|report| report.enabled && report.state != "connected");

    if json {
        let mut output = Map::new();
        output.insert(
            "servers".into(),
            serde_json::to_value(&reports).unwrap_or(Value::Null),
        );
        output.insert(
            "errors".into(),
            serde_json::to_value(&loaded.errors).unwrap_or(Value::Null),
        );
        if let Some(note) = untrusted_note {
            output.insert("note".into(), Value::String(note.to_string()));
        }
        log(json_stringify_indented(&Value::Object(output), "  "));
        return if failed { 1 } else { 0 };
    }
    if reports.is_empty() && loaded.errors.is_empty() {
        log(format!(
            "No MCP servers configured. Add them to {} or .pi/mcp.json.",
            Path::new(&options.agent_dir).join("mcp.json").to_string_lossy()
        ));
    }
    for report in &reports {
        let state = match report.state.as_str() {
            "connected" => format!(
                "connected, {} tool{}",
                report.tools.len(),
                if report.tools.len() == 1 { "" } else { "s" }
            ),
            "needs-auth" => "needs sign-in".to_string(),
            other => other.to_string(),
        };
        log(format!(
            "{}: {state} ({}, {})",
            report.name, report.exposure, report.scope
        ));
        log(format!("  {}", report.transport));
        if let Some(r#override) = &report.r#override {
            log(format!("  project override: {override}"));
        }
        if report.state == "needs-auth" {
            log(format!(
                "  sign in with: {APP_NAME} mcp login {}",
                report.name
            ));
        }
        if !report.tools.is_empty() {
            let tools: Vec<String> = report
                .tools
                .iter()
                .map(|tool| {
                    match report
                        .tool_exposure
                        .as_ref()
                        .and_then(|exposures| exposures.get(tool))
                    {
                        Some(exposure) => format!("{tool} [{exposure}]"),
                        None => tool.clone(),
                    }
                })
                .collect();
            log(format!("  tools: {}", tools.join(", ")));
        }
        if report.resources == Some(true) {
            log("  offers resources".to_string());
        }
        if let Some(error) = &report.error {
            log(format!("  {}", error.split('\n').collect::<Vec<_>>().join("\n  ")));
        }
    }
    for config_error in &loaded.errors {
        log(format!("config error: {config_error}"));
    }
    if let Some(note) = untrusted_note {
        log(note.to_string());
    }
    if failed { 1 } else { 0 }
}

struct CliSignInPrompt {
    name: String,
    log: Output,
    open_url: Arc<dyn Fn(String) + Send + Sync>,
    interactive: bool,
}

#[async_trait::async_trait]
impl McpSignInPrompt for CliSignInPrompt {
    fn show_authorization_url(&self, url: &str) {
        (self.log)(format!(
            "Sign in to MCP server \"{}\" in your browser:\n{url}",
            self.name
        ));
        (self.open_url)(url.to_string());
    }

    async fn prompt_for_redirect_url(
        &self,
        signal: CancellationToken,
    ) -> anyhow::Result<Option<String>> {
        Ok(wait_for_redirect_url(signal, self.interactive).await)
    }
}

#[allow(clippy::too_many_arguments)]
async fn login(
    entry: &McpServerEntry,
    connection: &Arc<McpServerConnection>,
    url: &str,
    timeout_ms: f64,
    options: &McpCommandOptions,
    credentials: &Arc<McpOAuthCredentialStore>,
    log: &Output,
    error: &Output,
) -> i32 {
    let name = &entry.name;
    // Connecting first answers whether a sign-in is needed and records the server's challenge.
    match connection.get_client().await {
        Ok(_) => {
            log(format!(
                "Already signed in to MCP server \"{name}\" ({} tools).",
                connection.tools().len()
            ));
            return 0;
        }
        Err(_) => {
            if connection.state() != crate::extensions::mcp::runtime::ServerState::NeedsAuth {
                error(format!(
                    "MCP server \"{name}\" failed to connect: {}",
                    connection
                        .error()
                        .unwrap_or_else(|| "unknown error".to_string())
                ));
                return 1;
            }
        }
    }

    let open_url: Arc<dyn Fn(String) + Send + Sync> = options
        .open_url
        .clone()
        .unwrap_or_else(|| Arc::new(|url: String| crate::utils::open_browser::open_browser(&url)));
    let interactive = {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal() && options.open_url.is_none()
    };
    let settings = match connection.oauth_settings() {
        Ok(settings) => settings,
        Err(settings_error) => {
            error(format!(
                "Sign-in to MCP server \"{name}\" failed: {settings_error}"
            ));
            return 1;
        }
    };
    let signal = CancellationToken::new();
    let timer = {
        let signal = signal.clone();
        let duration = Duration::from_millis(timeout_ms.max(0.0) as u64);
        crate::utils::uncaught::spawn(async move {
            tokio::time::sleep(duration).await;
            signal.cancel();
        })
    };
    let result = sign_in_mcp_server(&SignInMcpServerOptions {
        server_url: url.to_string(),
        store: credentials.for_server(name, url),
        settings,
        challenge: connection.challenge(),
        prompt: Arc::new(CliSignInPrompt {
            name: name.clone(),
            log: log.clone(),
            open_url,
            interactive,
        }),
        signal: Some(signal),
    })
    .await;
    timer.abort();
    if let Err(sign_in_error) = result {
        error(
            if sign_in_error
                .downcast_ref::<McpSignInCancelledError>()
                .is_some()
            {
                format!(
                    "Sign-in to MCP server \"{name}\" was cancelled or not completed within {} seconds.",
                    (timeout_ms / 1000.0).round()
                )
            } else {
                format!("Sign-in to MCP server \"{name}\" failed: {sign_in_error}")
            },
        );
        return 1;
    }
    connection.set_challenge(None);
    if let Err(connect_error) = connection.reconnect().await {
        error(format!("Signed in, but {connect_error}"));
        return 1;
    }
    log(format!(
        "Signed in to MCP server \"{name}\" ({} tools).",
        connection.tools().len()
    ));
    0
}

/// The pasted redirect URL in a terminal; otherwise only the browser callback can finish the sign-in.
/// Resolves to undefined when `signal` aborts: the callback arrived, or the sign-in timed out.
async fn wait_for_redirect_url(signal: CancellationToken, interactive: bool) -> Option<String> {
    if !interactive {
        signal.cancelled().await;
        return None;
    }
    eprint!("If the browser cannot reach this machine, paste the URL it was redirected to: ");
    let read = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .ok()
            .filter(|read| *read > 0)
            .map(|_| line.trim_end_matches(['\r', '\n']).to_string())
    });
    tokio::select! {
        line = read => line.ok().flatten(),
        _ = signal.cancelled() => None,
    }
}
