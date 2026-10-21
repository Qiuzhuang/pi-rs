//! Port of `packages/coding-agent/src/core/skills.ts`.

use crate::core::diagnostics::ResourceDiagnostic;
use crate::core::source_info::SourceInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type IgnoreMatcher = Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFrontmatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        rename = "disable-model-invocation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub disable_model_invocation: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: String,
    pub base_dir: String,
    pub source_info: SourceInfo,
    pub disable_model_invocation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSkillsResult {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSkillsFromDirOptions {
    /// Directory to scan for skills
    pub dir: String,
    /// Source identifier for these skills
    pub source: String,
}

/// Load skills from a directory.
///
/// Discovery rules:
/// - if a directory contains SKILL.md, treat it as a skill root and do not recurse further
/// - otherwise, load direct .md children in the root
/// - recurse into subdirectories to find SKILL.md
pub fn load_skills_from_dir(options: &LoadSkillsFromDirOptions) -> LoadSkillsResult {
    todo!("port core/skills.ts:loadSkillsFromDir")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormatSkillsForPromptFileReadTool {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "bash")]
    Bash,
    #[serde(rename = "indirect")]
    Indirect,
}

impl FormatSkillsForPromptFileReadTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Bash => "bash",
            Self::Indirect => "indirect",
        }
    }
}

/// Format skills for inclusion in a system prompt.
/// Uses XML format per Agent Skills standard.
/// See: https://agentskills.io/integrate-skills
///
/// Skills with disableModelInvocation=true are excluded from the prompt
/// (they can only be invoked explicitly via /skill:name commands).
///
/// `fileReadTool` names the tool that loads skill files; `indirect` names none, for a reader that
/// is reachable only through another tool.
pub fn format_skills_for_prompt(
    skills: &[Skill],
    file_read_tool: Option<FormatSkillsForPromptFileReadTool>,
) -> String {
    todo!("port core/skills.ts:formatSkillsForPrompt")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSkillsOptions {
    /// Working directory for project-local skills.
    pub cwd: String,
    /// Agent config directory for global skills.
    pub agent_dir: String,
    /// Explicit skill paths (files or directories)
    pub skill_paths: Vec<String>,
    /// Include default skills directories.
    pub include_defaults: bool,
}

/// Load skills from all configured locations.
/// Returns skills and any validation diagnostics.
pub fn load_skills(options: &LoadSkillsOptions) -> LoadSkillsResult {
    todo!("port core/skills.ts:loadSkills")
}
