//! Port of `packages/coding-agent/src/cli/args.ts`.

use crate::core::extensions::types::ExtensionFlag;
use crate::core::settings_schema::TuiMode;
use indexmap::IndexMap;
use pi_agent::types::ThinkingLevel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "rpc")]
    Rpc,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Rpc => "rpc",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgsListModels {
    String(String),
    Bool(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgsUnknownFlags {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArgsDiagnosticsType {
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

impl ArgsDiagnosticsType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArgsDiagnostics {
    pub r#type: ArgsDiagnosticsType,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Args {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#continue: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_session: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_builtin_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_extensions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_mcp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_skills: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_templates: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_prompt_templates: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub themes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_themes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_context_files: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_models: Option<ArgsListModels>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tui_mode: Option<TuiMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_trust_override: Option<bool>,
    pub messages: Vec<String>,
    pub file_args: Vec<String>,
    /// Unknown flags (potentially extension flags) - map of flag name to value
    pub unknown_flags: IndexMap<String, ArgsUnknownFlags>,
    pub diagnostics: Vec<ArgsDiagnostics>,
}

pub fn is_valid_thinking_level(level: &str) -> bool {
    todo!("port cli/args.ts:isValidThinkingLevel")
}

pub fn normalize_session_name(value: &str) -> Option<String> {
    todo!("port cli/args.ts:normalizeSessionName")
}

pub fn parse_args(args: &[String]) -> Args {
    todo!("port cli/args.ts:parseArgs")
}

pub fn print_help(extension_flags: Option<&[ExtensionFlag]>) {
    todo!("port cli/args.ts:printHelp")
}
