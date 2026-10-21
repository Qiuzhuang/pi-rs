//! Port of `packages/coding-agent/src/core/prompt-templates.ts`.

use crate::core::diagnostics::ResourceDiagnostic;
use crate::core::source_info::SourceInfo;
use serde::{Deserialize, Serialize};

/// Represents a prompt template loaded from a markdown file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplate {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    pub content: String,
    pub source_info: SourceInfo,
    pub file_path: String,
}

/// Parse command arguments respecting quoted strings (bash-style)
/// Returns array of arguments
pub fn parse_command_args(args_string: &str) -> Vec<String> {
    todo!("port core/prompt-templates.ts:parseCommandArgs")
}

/// Substitute argument placeholders in template content
/// Supports:
/// - $1, $2, ... for positional args
/// - $@ and $ARGUMENTS for all args
/// - ${N:-default} for positional arg N with default when missing/empty
/// - ${@:-default} and ${ARGUMENTS:-default} for all args with a default when empty
/// - ${@:N} for args from Nth onwards (bash-style slicing)
/// - ${@:N:L} for L args starting from Nth
///
/// Note: Replacement happens on the template string only. Argument and default values
/// containing patterns like $1, $@, or $ARGUMENTS are NOT recursively substituted.
pub fn substitute_args(content: &str, args: &[String]) -> String {
    todo!("port core/prompt-templates.ts:substituteArgs")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadPromptTemplatesOptions {
    /// Working directory for project-local templates.
    pub cwd: String,
    /// Agent config directory for global templates.
    pub agent_dir: String,
    /// Explicit prompt template paths (files or directories).
    pub prompt_paths: Vec<String>,
    /// Include default prompt directories.
    pub include_defaults: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadPromptTemplatesResult {
    pub templates: Vec<PromptTemplate>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

/// Load all prompt templates from:
/// 1. Global: agentDir/prompts/
/// 2. Project: cwd/{CONFIG_DIR_NAME}/prompts/
/// 3. Explicit prompt paths
pub fn load_prompt_templates(options: &LoadPromptTemplatesOptions) -> LoadPromptTemplatesResult {
    todo!("port core/prompt-templates.ts:loadPromptTemplates")
}

/// Expand a prompt template if it matches a template name.
/// Returns the expanded content or the original text if not a template.
pub fn expand_prompt_template(text: &str, templates: &[PromptTemplate]) -> String {
    todo!("port core/prompt-templates.ts:expandPromptTemplate")
}
