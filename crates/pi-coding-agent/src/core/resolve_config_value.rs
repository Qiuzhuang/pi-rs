//! Port of `packages/coding-agent/src/core/resolve-config-value.ts`.
//!
//! Resolve configuration values that may be shell commands, environment variables, or literals.
//! Used by auth-storage and model-registry.

use indexmap::IndexMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Cache for shell command results (persists for process lifetime)
static COMMAND_RESULT_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const COMMAND_TIMEOUT: Duration = Duration::from_millis(10_000);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum TemplatePart {
    #[serde(rename = "literal")]
    Literal { value: String },
    #[serde(rename = "env")]
    Env { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum ConfigValueReference {
    #[serde(rename = "command")]
    Command { config: String },
    #[serde(rename = "template")]
    Template { parts: Vec<TemplatePart> },
}

fn is_env_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_env_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_env_name_start) && chars.all(is_env_name_char)
}

fn append_literal(parts: &mut Vec<TemplatePart>, value: &str) {
    if value.is_empty() {
        return;
    }
    if let Some(TemplatePart::Literal { value: previous }) = parts.last_mut() {
        previous.push_str(value);
        return;
    }
    parts.push(TemplatePart::Literal {
        value: value.to_string(),
    });
}

fn parse_config_value_template(config: &str) -> Vec<TemplatePart> {
    let mut parts = Vec::new();
    let mut index = 0;

    while index < config.len() {
        let Some(dollar_offset) = config[index..].find('$') else {
            append_literal(&mut parts, &config[index..]);
            break;
        };
        let dollar_index = index + dollar_offset;
        append_literal(&mut parts, &config[index..dollar_index]);
        let rest = &config[dollar_index + 1..];
        let next_char = rest.chars().next();

        if next_char == Some('$') || next_char == Some('!') {
            append_literal(&mut parts, &rest[..1]);
            index = dollar_index + 2;
            continue;
        }

        if next_char == Some('{') {
            let Some(end_offset) = config[dollar_index + 2..].find('}') else {
                append_literal(&mut parts, "$");
                index = dollar_index + 1;
                continue;
            };
            let end_index = dollar_index + 2 + end_offset;
            let name = &config[dollar_index + 2..end_index];
            if is_env_var_name(name) {
                parts.push(TemplatePart::Env {
                    name: name.to_string(),
                });
            } else {
                append_literal(&mut parts, &config[dollar_index..=end_index]);
            }
            index = end_index + 1;
            continue;
        }

        if next_char.is_some_and(is_env_name_start) {
            let len = rest
                .char_indices()
                .find(|(_, c)| !is_env_name_char(*c))
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            parts.push(TemplatePart::Env {
                name: rest[..len].to_string(),
            });
            index = dollar_index + 1 + len;
            continue;
        }

        append_literal(&mut parts, "$");
        index = dollar_index + 1;
    }

    parts
}

fn parse_config_value_reference(config: &str) -> ConfigValueReference {
    if config.starts_with('!') {
        return ConfigValueReference::Command {
            config: config.to_string(),
        };
    }
    ConfigValueReference::Template {
        parts: parse_config_value_template(config),
    }
}

fn resolve_env_config_value(name: &str, env: Option<&IndexMap<String, String>>) -> Option<String> {
    env.and_then(|env| env.get(name))
        .filter(|v| !v.is_empty())
        .cloned()
        .or_else(|| std::env::var(name).ok().filter(|v| !v.is_empty()))
}

fn get_template_env_var_names(parts: &[TemplatePart]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for part in parts {
        if let TemplatePart::Env { name } = part
            && !names.contains(name)
        {
            names.push(name.clone());
        }
    }
    names
}

fn resolve_template(parts: &[TemplatePart], env: Option<&IndexMap<String, String>>) -> Option<String> {
    let mut resolved = String::new();
    for part in parts {
        match part {
            TemplatePart::Literal { value } => resolved.push_str(value),
            TemplatePart::Env { name } => resolved.push_str(&resolve_env_config_value(name, env)?),
        }
    }
    Some(resolved)
}

pub fn get_config_value_env_var_name(config: &str) -> Option<String> {
    match parse_config_value_reference(config) {
        ConfigValueReference::Template { parts } if parts.len() == 1 => match &parts[0] {
            TemplatePart::Env { name } => Some(name.clone()),
            TemplatePart::Literal { .. } => None,
        },
        _ => None,
    }
}

pub fn get_config_value_env_var_names(config: &str) -> Vec<String> {
    match parse_config_value_reference(config) {
        ConfigValueReference::Template { parts } => get_template_env_var_names(&parts),
        ConfigValueReference::Command { .. } => Vec::new(),
    }
}

pub fn get_missing_config_value_env_var_names(
    config: &str,
    env: Option<IndexMap<String, String>>,
) -> Vec<String> {
    get_config_value_env_var_names(config)
        .into_iter()
        .filter(|name| resolve_env_config_value(name, env.as_ref()).is_none())
        .collect()
}

pub fn is_command_config_value(config: &str) -> bool {
    matches!(
        parse_config_value_reference(config),
        ConfigValueReference::Command { .. }
    )
}

pub fn is_config_value_configured(config: &str, env: Option<IndexMap<String, String>>) -> bool {
    get_missing_config_value_env_var_names(config, env).is_empty()
}

/// Resolve a config value (API key, header value, etc.) to an actual value.
/// - If starts with "!", executes the rest as a shell command and uses stdout (cached)
/// - Interpolates "$ENV_VAR" or "${ENV_VAR}" references with the named environment variable
/// - In non-command values, "$$" escapes a literal "$" and "$!" escapes a literal "!"
/// - Otherwise treats the value as a literal
pub fn resolve_config_value(config: &str, env: Option<IndexMap<String, String>>) -> Option<String> {
    match parse_config_value_reference(config) {
        ConfigValueReference::Command { config } => execute_command(&config),
        ConfigValueReference::Template { parts } => resolve_template(&parts, env.as_ref()),
    }
}

/// Run a process with a timeout; returns `(success, stdout)` or an error when it cannot start.
fn run_with_timeout(mut command: Command, input: Option<&str>) -> std::io::Result<(bool, String)> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    if let Some(input) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = stdin.write_all(input.as_bytes());
    }
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.read_to_end(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() >= COMMAND_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let output = reader.join().unwrap_or_default();
    match status {
        Some(status) => Ok((
            status.success(),
            String::from_utf8_lossy(&output).into_owned(),
        )),
        None => Ok((false, String::new())),
    }
}

struct ConfiguredShellResult {
    executed: bool,
    value: Option<String>,
}

fn execute_with_configured_shell(command: &str) -> ConfiguredShellResult {
    use crate::utils::shell::{ShellConfigCommandTransport, get_shell_config};
    let Ok(config) = get_shell_config(None) else {
        return ConfiguredShellResult {
            executed: false,
            value: None,
        };
    };
    let command_from_stdin = config.command_transport == Some(ShellConfigCommandTransport::Stdin);
    let mut process = Command::new(&config.shell);
    process.args(&config.args);
    if !command_from_stdin {
        process.arg(command);
    }
    match run_with_timeout(process, command_from_stdin.then_some(command)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ConfiguredShellResult {
            executed: false,
            value: None,
        },
        Err(_) => ConfiguredShellResult {
            executed: true,
            value: None,
        },
        Ok((false, _)) => ConfiguredShellResult {
            executed: true,
            value: None,
        },
        Ok((true, stdout)) => {
            let value = stdout.trim().to_string();
            ConfiguredShellResult {
                executed: true,
                value: (!value.is_empty()).then_some(value),
            }
        }
    }
}

/// `execSync(command)`: runs through `/bin/sh -c` (or `cmd.exe /d /s /c` on Windows).
fn execute_with_default_shell(command: &str) -> Option<String> {
    let process = if cfg!(windows) {
        let mut c = Command::new(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()));
        c.args(["/d", "/s", "/c", command]);
        c
    } else {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", command]);
        c
    };
    match run_with_timeout(process, None) {
        Ok((true, stdout)) => {
            let value = stdout.trim().to_string();
            (!value.is_empty()).then_some(value)
        }
        _ => None,
    }
}

fn execute_command_uncached(command_config: &str) -> Option<String> {
    let command = &command_config[1..];
    if cfg!(windows) {
        let configured = execute_with_configured_shell(command);
        if configured.executed {
            configured.value
        } else {
            execute_with_default_shell(command)
        }
    } else {
        execute_with_default_shell(command)
    }
}

fn execute_command(command_config: &str) -> Option<String> {
    if let Some(cached) = COMMAND_RESULT_CACHE.lock().get(command_config) {
        return cached.clone();
    }
    let result = execute_command_uncached(command_config);
    COMMAND_RESULT_CACHE
        .lock()
        .insert(command_config.to_string(), result.clone());
    result
}

/// Resolve all header values using the same resolution logic as API keys.
pub fn resolve_config_value_uncached(
    config: &str,
    env: Option<IndexMap<String, String>>,
) -> Option<String> {
    match parse_config_value_reference(config) {
        ConfigValueReference::Command { config } => execute_command_uncached(&config),
        ConfigValueReference::Template { parts } => resolve_template(&parts, env.as_ref()),
    }
}

pub fn resolve_config_value_or_throw(
    config: &str,
    description: &str,
    env: Option<IndexMap<String, String>>,
) -> anyhow::Result<String> {
    if let Some(value) = resolve_config_value_uncached(config, env.clone()) {
        return Ok(value);
    }

    match parse_config_value_reference(config) {
        ConfigValueReference::Command { config } => {
            anyhow::bail!(
                "Failed to resolve {description} from shell command: {}",
                &config[1..]
            )
        }
        ConfigValueReference::Template { .. } => {
            let missing = get_missing_config_value_env_var_names(config, env);
            if missing.len() == 1 {
                anyhow::bail!(
                    "Failed to resolve {description} from environment variable: {}",
                    missing[0]
                );
            }
            if missing.len() > 1 {
                anyhow::bail!(
                    "Failed to resolve {description} from environment variables: {}",
                    missing.join(", ")
                );
            }
        }
    }
    anyhow::bail!("Failed to resolve {description}")
}

/// Resolve all header values using the same resolution logic as API keys.
pub fn resolve_headers(
    headers: Option<IndexMap<String, String>>,
    env: Option<IndexMap<String, String>>,
) -> Option<IndexMap<String, String>> {
    let headers = headers?;
    let mut resolved = IndexMap::new();
    for (key, value) in headers {
        if let Some(resolved_value) = resolve_config_value(&value, env.clone())
            && !resolved_value.is_empty()
        {
            resolved.insert(key, resolved_value);
        }
    }
    (!resolved.is_empty()).then_some(resolved)
}

pub fn resolve_headers_or_throw(
    headers: Option<IndexMap<String, String>>,
    description: &str,
    env: Option<IndexMap<String, String>>,
) -> anyhow::Result<Option<IndexMap<String, String>>> {
    let Some(headers) = headers else {
        return Ok(None);
    };
    let mut resolved = IndexMap::new();
    for (key, value) in headers {
        let resolved_value = resolve_config_value_or_throw(
            &value,
            &format!("{description} header \"{key}\""),
            env.clone(),
        )?;
        resolved.insert(key, resolved_value);
    }
    Ok((!resolved.is_empty()).then_some(resolved))
}

/// Clear the config value command cache. Exported for testing.
pub fn clear_config_value_cache() {
    COMMAND_RESULT_CACHE.lock().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn set_env(name: &str, value: Option<&str>) {
        unsafe {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
    }

    #[test]
    fn resolves_literals_environment_templates_and_escapes() {
        let _g = ENV_LOCK.lock().unwrap();
        set_env("TEST_CONFIG_LEFT", Some("left"));
        set_env("TEST_CONFIG_RIGHT", Some("right"));
        assert_eq!(resolve_config_value("literal-key", None).as_deref(), Some("literal-key"));
        assert_eq!(resolve_config_value("$TEST_CONFIG_LEFT", None).as_deref(), Some("left"));
        assert_eq!(
            resolve_config_value("${TEST_CONFIG_LEFT}_$TEST_CONFIG_RIGHT", None).as_deref(),
            Some("left_right")
        );
        assert_eq!(
            resolve_config_value("$$TEST_CONFIG_LEFT", None).as_deref(),
            Some("$TEST_CONFIG_LEFT")
        );
        assert_eq!(
            resolve_config_value("$!literal-$TEST_CONFIG_RIGHT", None).as_deref(),
            Some("!literal-right")
        );
        assert_eq!(resolve_config_value("cost $5", None).as_deref(), Some("cost $5"));
        assert_eq!(resolve_config_value("${not valid}", None).as_deref(), Some("${not valid}"));
        assert_eq!(resolve_config_value("${UNCLOSED", None).as_deref(), Some("${UNCLOSED"));
        set_env("TEST_CONFIG_LEFT", None);
        set_env("TEST_CONFIG_RIGHT", None);
    }

    #[test]
    fn uses_credential_scoped_environment_before_process_env() {
        let _g = ENV_LOCK.lock().unwrap();
        set_env("TEST_CONFIG_SCOPED", Some("process"));
        let env: IndexMap<String, String> =
            [("TEST_CONFIG_SCOPED".to_string(), "credential".to_string())].into();
        assert_eq!(
            resolve_config_value("$TEST_CONFIG_SCOPED", Some(env)).as_deref(),
            Some("credential")
        );
        set_env("TEST_CONFIG_SCOPED", None);
    }

    #[cfg(unix)]
    #[test]
    fn executes_shell_commands_and_trims_output() {
        assert_eq!(
            resolve_config_value("!echo '  spaced-key  '", None).as_deref(),
            Some("spaced-key")
        );
        assert_eq!(
            resolve_config_value("!printf 'line1\\nline2'", None).as_deref(),
            Some("line1\nline2")
        );
        assert_eq!(
            resolve_config_value("!echo 'hello world' | tr ' ' '-'", None).as_deref(),
            Some("hello-world")
        );
        for command in ["!exit 1", "!nonexistent-command-12345", "!printf ''"] {
            assert_eq!(resolve_config_value(command, None), None, "{command}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn caches_commands_until_cleared_and_uncached_runs_every_time() {
        let temp = tempfile::tempdir().unwrap();
        let counter = temp.path().join("counter");
        std::fs::write(&counter, "0").unwrap();
        let path = counter.to_str().unwrap();
        let success = format!(
            "!sh -c 'count=$(cat \"{path}\"); echo $((count + 1)) > \"{path}\"; echo cached-value'"
        );
        let read = || std::fs::read_to_string(&counter).unwrap().trim().to_string();
        assert_eq!(resolve_config_value(&success, None).as_deref(), Some("cached-value"));
        assert_eq!(resolve_config_value(&success, None).as_deref(), Some("cached-value"));
        assert_eq!(read(), "1");
        COMMAND_RESULT_CACHE.lock().remove(&success);
        assert_eq!(resolve_config_value(&success, None).as_deref(), Some("cached-value"));
        assert_eq!(read(), "2");

        let failure = format!(
            "!sh -c 'count=$(cat \"{path}\"); echo $((count + 1)) > \"{path}\"; exit 1'"
        );
        assert_eq!(resolve_config_value(&failure, None), None);
        assert_eq!(resolve_config_value(&failure, None), None);
        assert_eq!(read(), "3");

        assert_eq!(
            resolve_config_value_uncached(&success, None).as_deref(),
            Some("cached-value")
        );
        assert_eq!(
            resolve_config_value_uncached(&success, None).as_deref(),
            Some("cached-value")
        );
        assert_eq!(read(), "5");
    }

    #[test]
    fn does_not_cache_environment_values() {
        let _g = ENV_LOCK.lock().unwrap();
        set_env("TEST_CONFIG_DYNAMIC", Some("first"));
        assert_eq!(resolve_config_value("$TEST_CONFIG_DYNAMIC", None).as_deref(), Some("first"));
        set_env("TEST_CONFIG_DYNAMIC", Some("second"));
        assert_eq!(resolve_config_value("$TEST_CONFIG_DYNAMIC", None).as_deref(), Some("second"));
        set_env("TEST_CONFIG_DYNAMIC", None);
    }

    #[test]
    fn reports_env_var_names_and_errors() {
        let _g = ENV_LOCK.lock().unwrap();
        assert_eq!(get_config_value_env_var_name("$FOO_TEST_X").as_deref(), Some("FOO_TEST_X"));
        assert_eq!(get_config_value_env_var_name("x$FOO_TEST_X"), None);
        assert_eq!(
            get_config_value_env_var_names("$A_TEST_1-${B_TEST_2}-$A_TEST_1"),
            vec!["A_TEST_1", "B_TEST_2"]
        );
        assert!(is_command_config_value("!cmd"));
        assert!(!is_config_value_configured("$MISSING_TEST_VAR_123", None));
        assert_eq!(
            resolve_config_value_or_throw("$MISSING_TEST_VAR_123", "API key", None)
                .unwrap_err()
                .to_string(),
            "Failed to resolve API key from environment variable: MISSING_TEST_VAR_123"
        );
        assert_eq!(
            resolve_config_value_or_throw("$MISSING_A_1$MISSING_B_2", "API key", None)
                .unwrap_err()
                .to_string(),
            "Failed to resolve API key from environment variables: MISSING_A_1, MISSING_B_2"
        );
        let headers: IndexMap<String, String> =
            [("x-key".to_string(), "$MISSING_TEST_VAR_123".to_string())].into();
        assert_eq!(
            resolve_headers_or_throw(Some(headers.clone()), "provider", None)
                .unwrap_err()
                .to_string(),
            "Failed to resolve provider header \"x-key\" from environment variable: MISSING_TEST_VAR_123"
        );
        assert_eq!(resolve_headers(Some(headers), None), None);
    }
}
