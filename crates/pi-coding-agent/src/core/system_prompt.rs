//! Port of `packages/coding-agent/src/core/system-prompt.ts`.

use crate::core::skills::Skill;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSystemPromptOptionsContextFiles {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSystemPromptOptions {
    /// Custom system prompt (replaces the default prefix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    /// Exact full prompt replacement set by a before_agent_start handler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_system_prompt: Option<String>,
    /// Tools to include in prompt. Default: [read, bash, edit, write].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_tools: Option<Vec<String>>,
    /// Selected tools whose declarations requests leave out (`prepareLoadout` hidden declarations).
    /// They are reachable only through another tool, so the tool list and rules leave them out too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_tools: Option<Vec<String>>,
    /// Optional one-line tool snippets keyed by tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_snippets: Option<IndexMap<String, String>>,
    /// Guideline bullets contributed by each tool, keyed by tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_guidelines: Option<IndexMap<String, Vec<String>>>,
    /// Additional guideline bullets appended to the default system prompt rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_guidelines: Option<Vec<String>>,
    /// Text appended from user configuration before project context, skills, and cwd.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,
    /// Additional XML-wrapped prompt sections keyed by tag name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<IndexMap<String, String>>,
    /// Working directory.
    pub cwd: String,
    /// Pre-loaded context files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_files: Option<Vec<BuildSystemPromptOptionsContextFiles>>,
    /// Pre-loaded skills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<Skill>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedBuildSystemPromptOptionsContextFiles {
    pub path: String,
    pub content: String,
}

// TS: BuildSystemPromptOptions & { selectedTools: string[]; hiddenTools: string[]; toolSnippets: Record<string, string>; toolGuidelines: Record<string, string[]>; promptGuidelines: string[]; appendSystemPro
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedBuildSystemPromptOptions {
    /// Custom system prompt (replaces the default prefix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    /// Exact full prompt replacement set by a before_agent_start handler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_system_prompt: Option<String>,
    /// Tools to include in prompt. Default: [read, bash, edit, write].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_tools: Option<Vec<String>>,
    /// Selected tools whose declarations requests leave out (`prepareLoadout` hidden declarations).
    /// They are reachable only through another tool, so the tool list and rules leave them out too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_tools: Option<Vec<String>>,
    /// Optional one-line tool snippets keyed by tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_snippets: Option<IndexMap<String, String>>,
    /// Guideline bullets contributed by each tool, keyed by tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_guidelines: Option<IndexMap<String, Vec<String>>>,
    /// Additional guideline bullets appended to the default system prompt rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_guidelines: Option<Vec<String>>,
    /// Text appended from user configuration before project context, skills, and cwd.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,
    /// Additional XML-wrapped prompt sections keyed by tag name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<IndexMap<String, String>>,
    /// Working directory.
    pub cwd: String,
    /// Pre-loaded context files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_files: Option<Vec<NormalizedBuildSystemPromptOptionsContextFiles>>,
    /// Pre-loaded skills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<Skill>>,
}

/// Ordered system prompt sections, keyed by name. `preamble` is untagged text; every other
/// section is wrapped in a tag of the same name so the model can match later updates to it.
/// These become `SystemMessage.sections` in the transcript.
pub type SystemPromptSections = IndexMap<String, String>;

/// Normalize prompt input into the mutable, collection-complete shape exposed to extensions.
pub fn normalize_build_system_prompt_options(
    input: &BuildSystemPromptOptions,
) -> NormalizedBuildSystemPromptOptions {
    todo!("port core/system-prompt.ts:normalizeBuildSystemPromptOptions")
}

/// Build the ordered, independently replaceable sections of the structured system prompt.
pub fn build_system_prompt_sections(
    input: &BuildSystemPromptOptions,
) -> anyhow::Result<SystemPromptSections> {
    todo!("port core/system-prompt.ts:buildSystemPromptSections")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSystemPromptStateBuildSystemPromptStateResult {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<SystemPromptSections>,
}

/// The complete prompt state for `input`. A forced prompt is opaque and lives in `content`
/// with no sections; otherwise `content` is empty and the structured sections carry the prompt.
pub fn build_system_prompt_state(
    input: &BuildSystemPromptOptions,
) -> BuildSystemPromptStateBuildSystemPromptStateResult {
    todo!("port core/system-prompt.ts:buildSystemPromptState")
}

/// Build the system prompt text, rendered exactly as the transcript's system message replays it.
pub fn build_system_prompt(input: &BuildSystemPromptOptions) -> String {
    todo!("port core/system-prompt.ts:buildSystemPrompt")
}

/// Diff the sections the model currently has (replayed from the transcript, so never null)
/// against the desired ones. Returns a `SystemMessage.sections` patch, or undefined when
/// nothing changed.
pub fn diff_system_prompt_sections(
    previous: IndexMap<String, Option<String>>,
    current: &SystemPromptSections,
) -> Option<IndexMap<String, Option<String>>> {
    todo!("port core/system-prompt.ts:diffSystemPromptSections")
}
