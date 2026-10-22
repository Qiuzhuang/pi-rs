//! Port of `packages/coding-agent/src/core/skills.ts`.

use crate::config::{CONFIG_DIR_NAME, get_agent_dir};
use crate::core::diagnostics::{
    ResourceCollision, ResourceCollisionResourceType, ResourceDiagnostic, ResourceDiagnosticType,
};
use crate::core::source_info::{
    CreateSyntheticSourceInfoOptions, SourceInfo, SourceScope, create_synthetic_source_info,
};
use crate::utils::frontmatter::parse_frontmatter;
use crate::utils::paths::{
    canonicalize_path, node_basename, node_dirname, node_join, node_relative, node_resolve,
    resolve_path, PathInputOptions,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

/// Max name length per spec
const MAX_NAME_LENGTH: usize = 64;

/// Max description length per spec
const MAX_DESCRIPTION_LENGTH: usize = 1024;

const IGNORE_FILE_NAMES: &[&str] = &[".gitignore", ".ignore", ".fdignore"];

/// Gitignore-style matcher over paths relative to the scan root (the `ignore` npm package).
#[derive(Clone, Default)]
pub struct IgnoreMatcher {
    patterns: Vec<String>,
    matcher: Option<ignore::gitignore::Gitignore>,
}

impl IgnoreMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add gitignore patterns (already prefixed with their directory).
    pub fn add(&mut self, patterns: &[String]) {
        self.patterns.extend(patterns.iter().cloned());
        let mut builder = ignore::gitignore::GitignoreBuilder::new("");
        for pattern in &self.patterns {
            let _ = builder.add_line(None, pattern);
        }
        self.matcher = builder.build().ok();
    }

    /// Whether a relative path (directories end with `/`) is ignored.
    pub fn ignores(&self, path: &str) -> bool {
        let Some(matcher) = &self.matcher else {
            return false;
        };
        let is_dir = path.ends_with('/');
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() {
            return false;
        }
        matcher
            .matched_path_or_any_parents(trimmed, is_dir)
            .is_ignore()
    }
}

fn to_posix_path(p: &str) -> String {
    p.split(std::path::MAIN_SEPARATOR)
        .collect::<Vec<_>>()
        .join("/")
}

fn prefix_ignore_pattern(line: &str, prefix: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('#') && !trimmed.starts_with("\\#") {
        return None;
    }

    let mut pattern = line;
    let mut negated = false;

    if let Some(rest) = pattern.strip_prefix('!') {
        negated = true;
        pattern = rest;
    } else if pattern.starts_with("\\!") {
        pattern = &pattern[1..];
    }

    if let Some(rest) = pattern.strip_prefix('/') {
        pattern = rest;
    }

    let prefixed = if prefix.is_empty() {
        pattern.to_string()
    } else {
        format!("{prefix}{pattern}")
    };
    Some(if negated {
        format!("!{prefixed}")
    } else {
        prefixed
    })
}

fn add_ignore_rules(ig: &mut IgnoreMatcher, dir: &str, root_dir: &str) {
    let relative_dir = node_relative(root_dir, dir);
    let prefix = if relative_dir.is_empty() {
        String::new()
    } else {
        format!("{}/", to_posix_path(&relative_dir))
    };

    for filename in IGNORE_FILE_NAMES {
        let ignore_path = node_join(&[dir, filename]);
        if !Path::new(&ignore_path).exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&ignore_path) else {
            continue;
        };
        let patterns: Vec<String> = content
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .filter_map(|line| prefix_ignore_pattern(line, &prefix))
            .collect();
        if !patterns.is_empty() {
            ig.add(&patterns);
        }
    }
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSkillsResult {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

fn warning(message: impl Into<String>, path: &str) -> ResourceDiagnostic {
    ResourceDiagnostic {
        r#type: ResourceDiagnosticType::Warning,
        message: message.into(),
        path: Some(path.to_string()),
        collision: None,
    }
}

/// Validate skill name per Agent Skills spec.
/// Returns array of validation error messages (empty if valid).
fn validate_name(name: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let length = name.encode_utf16().count();

    if length > MAX_NAME_LENGTH {
        errors.push(format!(
            "name exceeds {MAX_NAME_LENGTH} characters ({length})"
        ));
    }

    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        errors.push(
            "name contains invalid characters (must be lowercase a-z, 0-9, hyphens only)"
                .to_string(),
        );
    }

    if name.starts_with('-') || name.ends_with('-') {
        errors.push("name must not start or end with a hyphen".to_string());
    }

    if name.contains("--") {
        errors.push("name must not contain consecutive hyphens".to_string());
    }

    errors
}

/// Validate description per Agent Skills spec.
fn validate_description(description: Option<&Value>) -> Vec<String> {
    let mut errors = Vec::new();
    match description.and_then(Value::as_str) {
        Some(d) if !d.trim().is_empty() => {
            let length = d.encode_utf16().count();
            if length > MAX_DESCRIPTION_LENGTH {
                errors.push(format!(
                    "description exceeds {MAX_DESCRIPTION_LENGTH} characters ({length})"
                ));
            }
        }
        _ => errors.push("description is required".to_string()),
    }
    errors
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSkillsFromDirOptions {
    /// Directory to scan for skills
    pub dir: String,
    /// Source identifier for these skills
    pub source: String,
}

fn create_skill_source_info(file_path: &str, base_dir: &str, source: &str) -> SourceInfo {
    let (source, scope) = match source {
        "user" => ("local", Some(SourceScope::User)),
        "project" => ("local", Some(SourceScope::Project)),
        "path" => ("local", None),
        other => (other, None),
    };
    create_synthetic_source_info(
        file_path,
        &CreateSyntheticSourceInfoOptions {
            source: source.to_string(),
            scope,
            origin: None,
            base_dir: Some(base_dir.to_string()),
        },
    )
}

/// Load skills from a directory.
///
/// Discovery rules:
/// - if a directory contains SKILL.md, treat it as a skill root and do not recurse further
/// - otherwise, load direct .md children in the root
/// - recurse into subdirectories to find SKILL.md
pub fn load_skills_from_dir(options: &LoadSkillsFromDirOptions) -> LoadSkillsResult {
    load_skills_from_dir_internal(&options.dir, &options.source, true, None, None)
}

struct DirEntryInfo {
    name: String,
    full_path: String,
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
}

/// `readdirSync(dir, { withFileTypes: true })` (entry order as returned by the OS, sorted by name
/// for determinism).
fn read_dir_entries(dir: &str) -> std::io::Result<Vec<DirEntryInfo>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        entries.push(DirEntryInfo {
            full_path: node_join(&[dir, &name]),
            name,
            is_file: file_type.is_file(),
            is_dir: file_type.is_dir(),
            is_symlink: file_type.is_symlink(),
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn load_skills_from_dir_internal(
    dir: &str,
    source: &str,
    include_root_files: bool,
    ignore_matcher: Option<&mut IgnoreMatcher>,
    root_dir: Option<&str>,
) -> LoadSkillsResult {
    let mut result = LoadSkillsResult::default();

    if !Path::new(dir).exists() {
        return result;
    }

    let root = root_dir.unwrap_or(dir).to_string();
    let mut own_matcher = IgnoreMatcher::new();
    let ig: &mut IgnoreMatcher = match ignore_matcher {
        Some(m) => m,
        None => &mut own_matcher,
    };
    add_ignore_rules(ig, dir, &root);

    let Ok(entries) = read_dir_entries(dir) else {
        return result;
    };

    for entry in &entries {
        if entry.name != "SKILL.md" {
            continue;
        }

        let mut is_file = entry.is_file;
        if entry.is_symlink {
            match std::fs::metadata(&entry.full_path) {
                Ok(meta) => is_file = meta.is_file(),
                Err(_) => continue,
            }
        }

        let rel_path = to_posix_path(&node_relative(&root, &entry.full_path));
        if !is_file || ig.ignores(&rel_path) {
            continue;
        }

        let loaded = load_skill_from_file(&entry.full_path, source);
        if let Some(skill) = loaded.skill {
            result.skills.push(skill);
        }
        result.diagnostics.extend(loaded.diagnostics);
        return result;
    }

    for entry in &entries {
        if entry.name.starts_with('.') {
            continue;
        }

        // Skip node_modules to avoid scanning dependencies
        if entry.name == "node_modules" {
            continue;
        }

        // For symlinks, check if they point to a directory and follow them
        let mut is_directory = entry.is_dir;
        let mut is_file = entry.is_file;
        if entry.is_symlink {
            match std::fs::metadata(&entry.full_path) {
                Ok(meta) => {
                    is_directory = meta.is_dir();
                    is_file = meta.is_file();
                }
                // Broken symlink, skip it
                Err(_) => continue,
            }
        }

        let rel_path = to_posix_path(&node_relative(&root, &entry.full_path));
        let ignore_path = if is_directory {
            format!("{rel_path}/")
        } else {
            rel_path
        };
        if ig.ignores(&ignore_path) {
            continue;
        }

        if is_directory {
            let sub = load_skills_from_dir_internal(
                &entry.full_path,
                source,
                false,
                Some(&mut *ig),
                Some(&root),
            );
            result.skills.extend(sub.skills);
            result.diagnostics.extend(sub.diagnostics);
            continue;
        }

        if !is_file || !include_root_files || !entry.name.ends_with(".md") {
            continue;
        }

        let loaded = load_skill_from_file(&entry.full_path, source);
        if let Some(skill) = loaded.skill {
            result.skills.push(skill);
        }
        result.diagnostics.extend(loaded.diagnostics);
    }

    result
}

struct LoadSkillFromFileResult {
    skill: Option<Skill>,
    diagnostics: Vec<ResourceDiagnostic>,
}

fn load_skill_from_file(file_path: &str, source: &str) -> LoadSkillFromFileResult {
    let mut diagnostics = Vec::new();
    let is_declared_skill = node_basename(file_path) == "SKILL.md";

    let raw_content = match std::fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(error) => {
            diagnostics.push(warning(error.to_string(), file_path));
            return LoadSkillFromFileResult {
                skill: None,
                diagnostics,
            };
        }
    };

    let frontmatter = match parse_frontmatter(&raw_content) {
        Ok(parsed) => parsed.frontmatter,
        Err(error) => {
            if is_declared_skill {
                diagnostics.push(warning(error.to_string(), file_path));
            }
            return LoadSkillFromFileResult {
                skill: None,
                diagnostics,
            };
        }
    };

    let description = frontmatter.get("description");
    let description_text = description
        .and_then(Value::as_str)
        .filter(|d| !d.trim().is_empty());
    let has_description = description_text.is_some();
    if !is_declared_skill && !has_description {
        return LoadSkillFromFileResult {
            skill: None,
            diagnostics,
        };
    }

    let skill_dir = node_dirname(file_path);
    let parent_dir_name = node_basename(&skill_dir);

    // Validate description
    for error in validate_description(description) {
        diagnostics.push(warning(error, file_path));
    }

    // Use name from frontmatter, or fall back to parent directory name
    let frontmatter_name = frontmatter
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty());
    let name = frontmatter_name
        .map(str::to_string)
        .unwrap_or(parent_dir_name);

    // Validate name
    for error in validate_name(&name) {
        diagnostics.push(warning(error, file_path));
    }

    // Still load the skill even with warnings, unless description is missing or empty.
    let Some(description) = description_text else {
        return LoadSkillFromFileResult {
            skill: None,
            diagnostics,
        };
    };

    LoadSkillFromFileResult {
        skill: Some(Skill {
            name,
            description: description.to_string(),
            file_path: file_path.to_string(),
            base_dir: skill_dir.clone(),
            source_info: create_skill_source_info(file_path, &skill_dir, source),
            disable_model_invocation: frontmatter.get("disable-model-invocation")
                == Some(&Value::Bool(true)),
        }),
        diagnostics,
    }
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
    let visible_skills: Vec<&Skill> = skills
        .iter()
        .filter(|s| !s.disable_model_invocation)
        .collect();

    if visible_skills.is_empty() {
        return String::new();
    }

    let mut lines: Vec<String> = vec![
        "\n\nThe following skills provide specialized instructions for specific tasks.".to_string(),
        match file_read_tool.unwrap_or(FormatSkillsForPromptFileReadTool::Read) {
            FormatSkillsForPromptFileReadTool::Read => {
                "Use the read tool to load a skill's file when the task matches its description."
            }
            FormatSkillsForPromptFileReadTool::Bash => {
                "Use bash to load a skill's file when the task matches its description."
            }
            FormatSkillsForPromptFileReadTool::Indirect => {
                "Load a skill's file when the task matches its description."
            }
        }
        .to_string(),
        "When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.".to_string(),
        String::new(),
        "<available_skills>".to_string(),
    ];

    for skill in visible_skills {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml(&skill.name)));
        lines.push(format!(
            "    <description>{}</description>",
            escape_xml(&skill.description)
        ));
        lines.push(format!(
            "    <location>{}</location>",
            escape_xml(&skill.file_path)
        ));
        lines.push("  </skill>".to_string());
    }

    lines.push("</available_skills>".to_string());

    lines.join("\n")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

fn is_under_path(target: &str, root: &str) -> bool {
    let normalized_root = node_resolve(&[root]);
    if target == normalized_root {
        return true;
    }
    let sep = std::path::MAIN_SEPARATOR;
    let prefix = if normalized_root.ends_with(sep) {
        normalized_root
    } else {
        format!("{normalized_root}{sep}")
    };
    target.starts_with(&prefix)
}

/// Load skills from all configured locations.
/// Returns skills and any validation diagnostics.
pub fn load_skills(options: &LoadSkillsOptions) -> LoadSkillsResult {
    let include_defaults = options.include_defaults;

    // Resolve agentDir - if not provided, use default from config
    let resolved_cwd = resolve_path(&options.cwd, None, None);
    let agent_dir = if options.agent_dir.is_empty() {
        get_agent_dir()
    } else {
        options.agent_dir.clone()
    };
    let resolved_agent_dir = resolve_path(&agent_dir, None, None);

    let mut skill_map: IndexMap<String, Skill> = IndexMap::new();
    let mut real_path_set: HashSet<String> = HashSet::new();
    let mut all_diagnostics: Vec<ResourceDiagnostic> = Vec::new();
    let mut collision_diagnostics: Vec<ResourceDiagnostic> = Vec::new();

    let mut add_skills = |result: LoadSkillsResult,
                          all_diagnostics: &mut Vec<ResourceDiagnostic>| {
        all_diagnostics.extend(result.diagnostics);
        for skill in result.skills {
            // Resolve symlinks to detect duplicate files
            let real_path = canonicalize_path(&skill.file_path);

            // Skip silently if we've already loaded this exact file (via symlink)
            if real_path_set.contains(&real_path) {
                continue;
            }

            if let Some(existing) = skill_map.get(&skill.name) {
                collision_diagnostics.push(ResourceDiagnostic {
                    r#type: ResourceDiagnosticType::Collision,
                    message: format!("name \"{}\" collision", skill.name),
                    path: Some(skill.file_path.clone()),
                    collision: Some(ResourceCollision {
                        resource_type: ResourceCollisionResourceType::Skill,
                        name: skill.name.clone(),
                        winner_path: existing.file_path.clone(),
                        loser_path: skill.file_path.clone(),
                        winner_source: None,
                        loser_source: None,
                    }),
                });
            } else {
                real_path_set.insert(real_path);
                skill_map.insert(skill.name.clone(), skill);
            }
        }
    };

    let user_skills_dir = node_join(&[&resolved_agent_dir, "skills"]);
    let project_skills_dir = node_resolve(&[&resolved_cwd, CONFIG_DIR_NAME, "skills"]);

    if include_defaults {
        add_skills(
            load_skills_from_dir_internal(&user_skills_dir, "user", true, None, None),
            &mut all_diagnostics,
        );
        add_skills(
            load_skills_from_dir_internal(&project_skills_dir, "project", true, None, None),
            &mut all_diagnostics,
        );
    }

    let get_source = |resolved_path: &str| -> &'static str {
        if !include_defaults {
            if is_under_path(resolved_path, &user_skills_dir) {
                return "user";
            }
            if is_under_path(resolved_path, &project_skills_dir) {
                return "project";
            }
        }
        "path"
    };

    let trim_options = PathInputOptions {
        trim: Some(true),
        ..Default::default()
    };
    for raw_path in &options.skill_paths {
        let resolved_path = resolve_path(raw_path, Some(&resolved_cwd), Some(&trim_options));
        if !Path::new(&resolved_path).exists() {
            all_diagnostics.push(warning("skill path does not exist", &resolved_path));
            continue;
        }

        match std::fs::metadata(&resolved_path) {
            Ok(stats) => {
                let source = get_source(&resolved_path);
                if stats.is_dir() {
                    add_skills(
                        load_skills_from_dir_internal(&resolved_path, source, true, None, None),
                        &mut all_diagnostics,
                    );
                } else if stats.is_file() && resolved_path.ends_with(".md") {
                    let loaded = load_skill_from_file(&resolved_path, source);
                    if let Some(skill) = loaded.skill {
                        add_skills(
                            LoadSkillsResult {
                                skills: vec![skill],
                                diagnostics: loaded.diagnostics,
                            },
                            &mut all_diagnostics,
                        );
                    } else {
                        all_diagnostics.extend(loaded.diagnostics);
                    }
                } else {
                    all_diagnostics.push(warning(
                        "skill path is not a markdown file",
                        &resolved_path,
                    ));
                }
            }
            Err(error) => {
                all_diagnostics.push(warning(error.to_string(), &resolved_path));
            }
        }
    }

    let mut diagnostics = all_diagnostics;
    diagnostics.extend(collision_diagnostics);
    LoadSkillsResult {
        skills: skill_map.into_values().collect(),
        diagnostics,
    }
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
