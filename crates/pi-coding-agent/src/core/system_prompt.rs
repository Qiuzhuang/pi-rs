//! Port of `packages/coding-agent/src/core/system-prompt.ts`.

use crate::config::{get_docs_path, get_examples_path, get_readme_path};
use crate::core::skills::{FormatSkillsForPromptFileReadTool, Skill, format_skills_for_prompt};
use pi_ai::types::{SystemContent, SystemMessage};
use pi_ai::utils::text::get_system_message_text;
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

/// Context file entry of [`NormalizedBuildSystemPromptOptions`].
pub type NormalizedBuildSystemPromptOptionsContextFiles = BuildSystemPromptOptionsContextFiles;

/// `BuildSystemPromptOptions` with every collection present (TS `NormalizedBuildSystemPromptOptions`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedBuildSystemPromptOptions {
    /// Custom system prompt (replaces the default prefix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    /// Exact full prompt replacement set by a before_agent_start handler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_system_prompt: Option<String>,
    /// Tools to include in prompt.
    #[serde(default)]
    pub selected_tools: Vec<String>,
    /// Selected tools whose declarations requests leave out (`prepareLoadout` hidden declarations).
    #[serde(default)]
    pub hidden_tools: Vec<String>,
    /// One-line tool snippets keyed by tool name.
    #[serde(default)]
    pub tool_snippets: IndexMap<String, String>,
    /// Guideline bullets contributed by each tool, keyed by tool name.
    #[serde(default)]
    pub tool_guidelines: IndexMap<String, Vec<String>>,
    /// Additional guideline bullets appended to the default system prompt rules.
    #[serde(default)]
    pub prompt_guidelines: Vec<String>,
    /// Text appended from user configuration before project context, skills, and cwd.
    #[serde(default)]
    pub append_system_prompt: String,
    /// Additional XML-wrapped prompt sections keyed by tag name.
    #[serde(default)]
    pub sections: IndexMap<String, String>,
    /// Working directory.
    pub cwd: String,
    /// Pre-loaded context files.
    #[serde(default)]
    pub context_files: Vec<NormalizedBuildSystemPromptOptionsContextFiles>,
    /// Pre-loaded skills.
    #[serde(default)]
    pub skills: Vec<Skill>,
}

impl From<NormalizedBuildSystemPromptOptions> for BuildSystemPromptOptions {
    fn from(o: NormalizedBuildSystemPromptOptions) -> Self {
        BuildSystemPromptOptions {
            custom_prompt: o.custom_prompt,
            force_system_prompt: o.force_system_prompt,
            selected_tools: Some(o.selected_tools),
            hidden_tools: Some(o.hidden_tools),
            tool_snippets: Some(o.tool_snippets),
            tool_guidelines: Some(o.tool_guidelines),
            prompt_guidelines: Some(o.prompt_guidelines),
            append_system_prompt: Some(o.append_system_prompt),
            sections: Some(o.sections),
            cwd: o.cwd,
            context_files: Some(o.context_files),
            skills: Some(o.skills),
        }
    }
}

impl From<&NormalizedBuildSystemPromptOptions> for BuildSystemPromptOptions {
    fn from(o: &NormalizedBuildSystemPromptOptions) -> Self {
        o.clone().into()
    }
}

impl BuildSystemPromptOptions {
    /// Options with only `cwd` set.
    pub fn new(cwd: impl Into<String>) -> Self {
        BuildSystemPromptOptions {
            custom_prompt: None,
            force_system_prompt: None,
            selected_tools: None,
            hidden_tools: None,
            tool_snippets: None,
            tool_guidelines: None,
            prompt_guidelines: None,
            append_system_prompt: None,
            sections: None,
            cwd: cwd.into(),
            context_files: None,
            skills: None,
        }
    }
}

/// Ordered system prompt sections, keyed by name. `preamble` is untagged text; every other
/// section is wrapped in a tag of the same name so the model can match later updates to it.
/// These become `SystemMessage.sections` in the transcript.
pub type SystemPromptSections = IndexMap<String, String>;

fn is_valid_section_name(name: &str) -> bool {
    // /^[a-z][a-z0-9_-]*$/
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Normalize prompt input into the mutable, collection-complete shape exposed to extensions.
pub fn normalize_build_system_prompt_options(
    input: &BuildSystemPromptOptions,
) -> NormalizedBuildSystemPromptOptions {
    NormalizedBuildSystemPromptOptions {
        custom_prompt: input.custom_prompt.clone(),
        force_system_prompt: input.force_system_prompt.clone(),
        selected_tools: input.selected_tools.clone().unwrap_or_else(|| {
            ["read", "bash", "edit", "write"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        }),
        hidden_tools: input.hidden_tools.clone().unwrap_or_default(),
        tool_snippets: input.tool_snippets.clone().unwrap_or_default(),
        tool_guidelines: input.tool_guidelines.clone().unwrap_or_default(),
        prompt_guidelines: input.prompt_guidelines.clone().unwrap_or_default(),
        append_system_prompt: input.append_system_prompt.clone().unwrap_or_default(),
        sections: input.sections.clone().unwrap_or_default(),
        cwd: input.cwd.clone(),
        context_files: input.context_files.clone().unwrap_or_default(),
        skills: input.skills.clone().unwrap_or_default(),
    }
}

fn render_project_context(context_files: &[BuildSystemPromptOptionsContextFiles]) -> String {
    let mut parts = vec!["Project-specific instructions and guidelines:".to_string()];
    for file in context_files {
        parts.push(format!(
            "<project_instructions path=\"{}\">\n{}\n</project_instructions>",
            file.path, file.content
        ));
    }
    parts.join("\n\n")
}

fn build_rules(
    selected_tools: &[String],
    tool_guidelines: &IndexMap<String, Vec<String>>,
    prompt_guidelines: &[String],
) -> String {
    let mut rules: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add_rule = |rule: &str| {
        let normalized = rule.trim();
        if normalized.is_empty() || seen.contains(normalized) {
            return;
        }
        seen.insert(normalized.to_string());
        rules.push(normalized.to_string());
    };

    let has = |name: &str| selected_tools.iter().any(|t| t == name);
    let has_bash = has("bash");
    let has_power_shell = has("powershell");
    let has_grep = has("grep");
    let has_find = has("find");
    let has_ls = has("ls");

    if (has_bash || has_power_shell) && !has_grep && !has_find && !has_ls {
        if has_bash && has_power_shell {
            add_rule(
                "Use bash or PowerShell for file operations like listing, searching, and finding files",
            );
        } else if has_power_shell {
            add_rule("Use PowerShell for file operations like listing, searching, and finding files");
        } else {
            add_rule("Use bash for file operations like ls, rg, find");
        }
    }

    for name in selected_tools {
        if let Some(guidelines) = tool_guidelines.get(name) {
            for rule in guidelines {
                add_rule(rule);
            }
        }
    }
    for rule in prompt_guidelines {
        add_rule(rule);
    }
    add_rule("Be concise in your responses");
    add_rule("Show file paths clearly when working with files");
    rules
        .iter()
        .map(|rule| format!("- {rule}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the ordered, independently replaceable sections of the structured system prompt.
pub fn build_system_prompt_sections(
    input: &BuildSystemPromptOptions,
) -> anyhow::Result<SystemPromptSections> {
    build_normalized_system_prompt_sections(&normalize_build_system_prompt_options(input))
}

/// [`build_system_prompt_sections`] for already-normalized options.
pub fn build_normalized_system_prompt_sections(
    options: &NormalizedBuildSystemPromptOptions,
) -> anyhow::Result<SystemPromptSections> {
    for name in options.sections.keys() {
        if !is_valid_section_name(name) || name == "preamble" {
            anyhow::bail!("Invalid system prompt section name: {name}");
        }
    }

    let declared_tools: Vec<String> = options
        .selected_tools
        .iter()
        .filter(|name| !options.hidden_tools.contains(name))
        .cloned()
        .collect();
    let mut prompt_sections: IndexMap<String, String> = IndexMap::new();
    match options.custom_prompt.as_deref() {
        Some(custom) if !custom.is_empty() => {
            prompt_sections.insert("preamble".into(), custom.to_string());
        }
        _ => {
            prompt_sections.insert(
                "preamble".into(),
                "You are an expert coding assistant operating inside pi, a coding agent harness. You help users by reading files, executing commands, editing code, and writing new files.".into(),
            );
            let visible_tools: Vec<&String> = declared_tools
                .iter()
                .filter(|name| {
                    options
                        .tool_snippets
                        .get(name.as_str())
                        .is_some_and(|s| !s.is_empty())
                })
                .collect();
            let tools = if visible_tools.is_empty() {
                "(none)".to_string()
            } else {
                visible_tools
                    .iter()
                    .map(|name| format!("- {}: {}", name, options.tool_snippets[name.as_str()]))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            prompt_sections.insert(
                "tools".into(),
                format!(
                    "{tools}\n\nIn addition to the tools above, you may have access to other custom tools depending on the project."
                ),
            );
            prompt_sections.insert(
                "rules".into(),
                build_rules(
                    &declared_tools,
                    &options.tool_guidelines,
                    &options.prompt_guidelines,
                ),
            );
            prompt_sections.insert(
                "docs".into(),
                format!(
                    "Pi documentation (read only when the user asks about pi itself, its SDK, extensions, themes, skills, or TUI):
- Main documentation: {}
- Additional docs: {}
- Examples: {} (extensions, custom tools, SDK)
- When reading pi docs or examples, resolve docs/... under Additional docs and examples/... under Examples, not the current working directory
- When asked about: extensions (docs/extensions.md, examples/extensions/), themes (docs/themes.md), skills (docs/skills.md), prompt templates (docs/prompt-templates.md), TUI components (docs/tui.md), keybindings (docs/keybindings.md), SDK integrations (docs/sdk.md), custom providers (docs/custom-provider.md), adding models (docs/models.md), pi packages (docs/packages.md), environment variables (docs/environment-variables.md), MCP servers (docs/mcp.md), codemode scripts and non-LLM models such as classifiers and image models (docs/codemode.md)
- When working on pi topics, read the docs and examples, and follow .md cross-references before implementing
- Always read pi .md files completely and follow links to related docs (e.g., tui.md for TUI API details)",
                    get_readme_path(),
                    get_docs_path(),
                    get_examples_path()
                ),
            );
        }
    }

    if !options.append_system_prompt.is_empty() {
        prompt_sections.insert("addendum".into(), options.append_system_prompt.clone());
    }
    if !options.context_files.is_empty() {
        prompt_sections.insert(
            "project_context".into(),
            render_project_context(&options.context_files),
        );
    }
    // A hidden reader is still reachable through another tool, so skills stay but the hint names no tool.
    let readers = [
        ("read", FormatSkillsForPromptFileReadTool::Read),
        ("bash", FormatSkillsForPromptFileReadTool::Bash),
    ];
    let skill_file_read_tool = readers
        .iter()
        .find(|(name, _)| declared_tools.iter().any(|t| t == name))
        .map(|(_, tool)| *tool)
        .or_else(|| {
            readers
                .iter()
                .any(|(name, _)| options.selected_tools.iter().any(|t| t == name))
                .then_some(FormatSkillsForPromptFileReadTool::Indirect)
        });
    if let Some(tool) = skill_file_read_tool
        && !options.skills.is_empty()
    {
        let skills_prompt = format_skills_for_prompt(&options.skills, Some(tool))
            .trim()
            .to_string();
        if !skills_prompt.is_empty() {
            prompt_sections.insert("skills".into(), skills_prompt);
        }
    }
    prompt_sections.insert("cwd".into(), options.cwd.replace('\\', "/"));
    for (name, content) in &options.sections {
        if !content.is_empty() {
            prompt_sections.insert(name.clone(), content.clone());
        }
    }

    let mut sections = SystemPromptSections::new();
    sections.insert("preamble".into(), prompt_sections["preamble"].clone());
    for (name, content) in &prompt_sections {
        if name != "preamble" {
            sections.insert(name.clone(), format!("<{name}>\n{content}\n</{name}>"));
        }
    }
    Ok(sections)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSystemPromptStateResult {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<SystemPromptSections>,
}

/// The complete prompt state for `input`. A forced prompt is opaque and lives in `content`
/// with no sections; otherwise `content` is empty and the structured sections carry the prompt.
pub fn build_system_prompt_state(
    input: &BuildSystemPromptOptions,
) -> anyhow::Result<BuildSystemPromptStateResult> {
    if let Some(forced) = &input.force_system_prompt {
        return Ok(BuildSystemPromptStateResult {
            content: forced.clone(),
            sections: None,
        });
    }
    Ok(BuildSystemPromptStateResult {
        content: String::new(),
        sections: Some(build_system_prompt_sections(input)?),
    })
}

/// Build the system prompt text, rendered exactly as the transcript's system message replays it.
pub fn build_system_prompt(input: &BuildSystemPromptOptions) -> anyhow::Result<String> {
    let state = build_system_prompt_state(input)?;
    let message = SystemMessage {
        content: SystemContent::Text(state.content),
        sections: state
            .sections
            .map(|s| s.into_iter().map(|(k, v)| (k, Some(v))).collect()),
        tools_added: None,
        tools_removed: None,
        timestamp: 0,
    };
    Ok(get_system_message_text(&message))
}

/// Diff the sections the model currently has (replayed from the transcript, so never null)
/// against the desired ones. Returns a `SystemMessage.sections` patch, or undefined when
/// nothing changed.
pub fn diff_system_prompt_sections(
    previous: IndexMap<String, Option<String>>,
    current: &SystemPromptSections,
) -> Option<IndexMap<String, Option<String>>> {
    let mut patch: IndexMap<String, Option<String>> = IndexMap::new();
    for (name, text) in current {
        if previous.get(name).map(|v| v.as_deref()) != Some(Some(text.as_str())) {
            patch.insert(name.clone(), Some(text.clone()));
        }
    }
    for name in previous.keys() {
        if !current.contains_key(name) {
            patch.insert(name.clone(), None);
        }
    }
    if patch.is_empty() { None } else { Some(patch) }
}
