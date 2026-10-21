//! Port of `packages/coding-agent/src/core/resolve-config-value.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum TemplatePart {
    #[serde(rename = "literal")]
    Literal { value: String },
    #[serde(rename = "env")]
    Env { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum ConfigValueReference {
    #[serde(rename = "command")]
    Command { config: String },
    #[serde(rename = "template")]
    Template { parts: Vec<TemplatePart> },
}

pub fn get_config_value_env_var_name(config: &str) -> Option<String> {
    todo!("port core/resolve-config-value.ts:getConfigValueEnvVarName")
}

pub fn get_config_value_env_var_names(config: &str) -> Vec<String> {
    todo!("port core/resolve-config-value.ts:getConfigValueEnvVarNames")
}

pub fn get_missing_config_value_env_var_names(
    config: &str,
    env: Option<IndexMap<String, String>>,
) -> Vec<String> {
    todo!("port core/resolve-config-value.ts:getMissingConfigValueEnvVarNames")
}

pub fn is_command_config_value(config: &str) -> bool {
    todo!("port core/resolve-config-value.ts:isCommandConfigValue")
}

pub fn is_config_value_configured(config: &str, env: Option<IndexMap<String, String>>) -> bool {
    todo!("port core/resolve-config-value.ts:isConfigValueConfigured")
}

/// Resolve a config value (API key, header value, etc.) to an actual value.
/// - If starts with "!", executes the rest as a shell command and uses stdout (cached)
/// - Interpolates "$ENV_VAR" or "${ENV_VAR}" references with the named environment variable
/// - In non-command values, "$$" escapes a literal "$" and "$!" escapes a literal "!"
/// - Otherwise treats the value as a literal
pub fn resolve_config_value(config: &str, env: Option<IndexMap<String, String>>) -> Option<String> {
    todo!("port core/resolve-config-value.ts:resolveConfigValue")
}

/// Resolve all header values using the same resolution logic as API keys.
pub fn resolve_config_value_uncached(
    config: &str,
    env: Option<IndexMap<String, String>>,
) -> Option<String> {
    todo!("port core/resolve-config-value.ts:resolveConfigValueUncached")
}

pub fn resolve_config_value_or_throw(
    config: &str,
    description: &str,
    env: Option<IndexMap<String, String>>,
) -> anyhow::Result<String> {
    todo!("port core/resolve-config-value.ts:resolveConfigValueOrThrow")
}

/// Resolve all header values using the same resolution logic as API keys.
pub fn resolve_headers(
    headers: Option<IndexMap<String, String>>,
    env: Option<IndexMap<String, String>>,
) -> Option<IndexMap<String, String>> {
    todo!("port core/resolve-config-value.ts:resolveHeaders")
}

pub fn resolve_headers_or_throw(
    headers: Option<IndexMap<String, String>>,
    description: &str,
    env: Option<IndexMap<String, String>>,
) -> Option<IndexMap<String, String>> {
    todo!("port core/resolve-config-value.ts:resolveHeadersOrThrow")
}

/// Clear the config value command cache. Exported for testing.
pub fn clear_config_value_cache() {
    todo!("port core/resolve-config-value.ts:clearConfigValueCache")
}
