//! Port of `packages/coding-agent/src/core/prompt-templates.ts`.

use crate::config::CONFIG_DIR_NAME;
use crate::core::diagnostics::{ResourceDiagnostic, ResourceDiagnosticType};
use crate::core::source_info::{
    CreateSyntheticSourceInfoOptions, SourceInfo, SourceScope, create_synthetic_source_info,
};
use crate::utils::frontmatter::parse_frontmatter;
use crate::utils::paths::{
    PathInputOptions, node_basename, node_dirname, node_join, node_resolve, resolve_path,
};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::LazyLock;

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
    let mut args = Vec::new();
    let mut current = String::new();
    // Tracks whether a token has started, so quoted empty strings ("" or '') are kept as arguments
    let mut has_token = false;
    let mut in_quote: Option<char> = None;

    for c in args_string.chars() {
        if let Some(quote) = in_quote {
            if c == quote {
                in_quote = None;
            } else {
                current.push(c);
            }
        } else if c == '"' || c == '\'' {
            in_quote = Some(c);
            has_token = true;
        } else if is_js_whitespace(c) {
            if has_token {
                args.push(std::mem::take(&mut current));
                has_token = false;
            }
        } else {
            current.push(c);
            has_token = true;
        }
    }

    if has_token {
        args.push(current);
    }

    args
}

/// JS `\s`.
fn is_js_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

static SUBSTITUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{([0-9]+|ARGUMENTS|@):-([^}]*)\}|\$\{@:([0-9]+)(?::([0-9]+))?\}|\$(ARGUMENTS|@|[0-9]+)")
        .unwrap()
});

/// `parseInt` of a digit string (saturating).
fn parse_index(digits: &str) -> i64 {
    digits.parse::<i64>().unwrap_or(i64::MAX)
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
    let all_args = args.join(" ");
    let arg_at = |index: i64| -> Option<&String> {
        if index < 0 {
            None
        } else {
            args.get(index as usize)
        }
    };

    SUBSTITUTE_RE
        .replace_all(content, |caps: &Captures| -> String {
            if let Some(default_target) = caps.get(1) {
                let default_value = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let value = match default_target.as_str() {
                    "@" | "ARGUMENTS" => Some(all_args.as_str()),
                    digits => arg_at(parse_index(digits) - 1).map(String::as_str),
                };
                return match value {
                    Some(v) if !v.is_empty() => v.to_string(),
                    _ => default_value.to_string(),
                };
            }

            if let Some(slice_start) = caps.get(3) {
                // Convert to 0-indexed (user provides 1-indexed); treat 0 as 1 (bash convention)
                let start = (parse_index(slice_start.as_str()) - 1).max(0) as usize;
                let start = start.min(args.len());
                let end = match caps.get(4) {
                    Some(length) => start.saturating_add(parse_index(length.as_str()) as usize).min(args.len()),
                    None => args.len(),
                };
                return args[start..end.max(start)].join(" ");
            }

            let simple = caps.get(5).map(|m| m.as_str()).unwrap_or("");
            if simple == "ARGUMENTS" || simple == "@" {
                return all_args.clone();
            }
            arg_at(parse_index(simple) - 1).cloned().unwrap_or_default()
        })
        .into_owned()
}

fn warning(message: impl Into<String>, path: &str) -> ResourceDiagnostic {
    ResourceDiagnostic {
        r#type: ResourceDiagnosticType::Warning,
        message: message.into(),
        path: Some(path.to_string()),
        collision: None,
    }
}

/// `str.slice(0, n)` counting UTF-16 code units.
fn js_slice_prefix(s: &str, n: usize) -> String {
    let mut units = 0;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        let len = c.len_utf16();
        if units + len > n {
            return s[..i].to_string();
        }
        units += len;
        end = i + c.len_utf8();
    }
    s[..end].to_string()
}

fn load_template_from_file(
    file_path: &str,
    source_info: SourceInfo,
) -> (Option<PromptTemplate>, Vec<ResourceDiagnostic>) {
    let mut diagnostics = Vec::new();
    let raw_content = match std::fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(error) => {
            diagnostics.push(warning(error.to_string(), file_path));
            return (None, diagnostics);
        }
    };

    let parsed = match parse_frontmatter(&raw_content) {
        Ok(parsed) => parsed,
        Err(error) => {
            diagnostics.push(warning(error.to_string(), file_path));
            return (None, diagnostics);
        }
    };
    let frontmatter = parsed.frontmatter;
    let body = parsed.body;

    let base_name = node_basename(file_path);
    let name = base_name
        .strip_suffix(".md")
        .unwrap_or(&base_name)
        .to_string();

    // Get description from frontmatter or first non-empty line
    let mut description = frontmatter
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if description.is_empty()
        && let Some(first_line) = body.split('\n').find(|line| !line.trim().is_empty())
    {
        // Truncate if too long
        description = js_slice_prefix(first_line, 60);
        if first_line.encode_utf16().count() > 60 {
            description.push_str("...");
        }
    }

    let argument_hint = frontmatter
        .get("argument-hint")
        .and_then(Value::as_str)
        .filter(|h| !h.is_empty())
        .map(str::to_string);
    (
        Some(PromptTemplate {
            name,
            description,
            argument_hint,
            content: body,
            source_info,
            file_path: file_path.to_string(),
        }),
        diagnostics,
    )
}

/// Scan a directory for .md files (non-recursive) and load them as prompt templates.
fn load_templates_from_dir(
    dir: &str,
    get_source_info: &dyn Fn(&str) -> anyhow::Result<SourceInfo>,
) -> LoadPromptTemplatesResult {
    let mut result = LoadPromptTemplatesResult::default();
    if !Path::new(dir).exists() {
        return result;
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return result;
    };
    let mut entries: Vec<_> = read_dir.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let full_path = node_join(&[dir, &name]);
        let Ok(file_type) = entry.file_type() else {
            return result;
        };

        // For symlinks, check if they point to a file
        let mut is_file = file_type.is_file();
        if file_type.is_symlink() {
            match std::fs::metadata(&full_path) {
                Ok(meta) => is_file = meta.is_file(),
                // Broken symlink, skip it
                Err(_) => continue,
            }
        }

        if is_file && name.ends_with(".md") {
            let Ok(source_info) = get_source_info(&full_path) else {
                return result;
            };
            let (template, diagnostics) = load_template_from_file(&full_path, source_info);
            if let Some(template) = template {
                result.templates.push(template);
            }
            result.diagnostics.extend(diagnostics);
        }
    }

    result
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    let resolved_cwd = resolve_path(&options.cwd, None, None);
    let resolved_agent_dir = resolve_path(&options.agent_dir, None, None);

    let mut templates: Vec<PromptTemplate> = Vec::new();
    let mut diagnostics: Vec<ResourceDiagnostic> = Vec::new();

    let global_prompts_dir = node_join(&[&resolved_agent_dir, "prompts"]);
    let project_prompts_dir = node_resolve(&[&resolved_cwd, CONFIG_DIR_NAME, "prompts"]);

    let is_under_path = |target: &str, root: &str| -> bool {
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
    };

    let get_source_info = |resolved_path: &str| -> anyhow::Result<SourceInfo> {
        if is_under_path(resolved_path, &global_prompts_dir) {
            return Ok(create_synthetic_source_info(
                resolved_path,
                &CreateSyntheticSourceInfoOptions {
                    source: "local".to_string(),
                    scope: Some(SourceScope::User),
                    origin: None,
                    base_dir: Some(global_prompts_dir.clone()),
                },
            ));
        }
        if is_under_path(resolved_path, &project_prompts_dir) {
            return Ok(create_synthetic_source_info(
                resolved_path,
                &CreateSyntheticSourceInfoOptions {
                    source: "local".to_string(),
                    scope: Some(SourceScope::Project),
                    origin: None,
                    base_dir: Some(project_prompts_dir.clone()),
                },
            ));
        }
        let base_dir = if std::fs::metadata(resolved_path)?.is_dir() {
            resolved_path.to_string()
        } else {
            node_dirname(resolved_path)
        };
        Ok(create_synthetic_source_info(
            resolved_path,
            &CreateSyntheticSourceInfoOptions {
                source: "local".to_string(),
                scope: None,
                origin: None,
                base_dir: Some(base_dir),
            },
        ))
    };

    let add_result = |result: LoadPromptTemplatesResult,
                          templates: &mut Vec<PromptTemplate>,
                          diagnostics: &mut Vec<ResourceDiagnostic>| {
        templates.extend(result.templates);
        diagnostics.extend(result.diagnostics);
    };

    if options.include_defaults {
        add_result(
            load_templates_from_dir(&global_prompts_dir, &get_source_info),
            &mut templates,
            &mut diagnostics,
        );
        add_result(
            load_templates_from_dir(&project_prompts_dir, &get_source_info),
            &mut templates,
            &mut diagnostics,
        );
    }

    // 3. Load explicit prompt paths
    let trim_options = PathInputOptions {
        trim: Some(true),
        ..Default::default()
    };
    for raw_path in &options.prompt_paths {
        let resolved_path = resolve_path(raw_path, Some(&resolved_cwd), Some(&trim_options));
        if !Path::new(&resolved_path).exists() {
            continue;
        }

        let outcome = (|| -> anyhow::Result<()> {
            let stats = std::fs::metadata(&resolved_path)?;
            if stats.is_dir() {
                add_result(
                    load_templates_from_dir(&resolved_path, &get_source_info),
                    &mut templates,
                    &mut diagnostics,
                );
            } else if stats.is_file() && resolved_path.ends_with(".md") {
                let (template, file_diagnostics) =
                    load_template_from_file(&resolved_path, get_source_info(&resolved_path)?);
                if let Some(template) = template {
                    templates.push(template);
                }
                diagnostics.extend(file_diagnostics);
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            diagnostics.push(warning(error.to_string(), &resolved_path));
        }
    }

    LoadPromptTemplatesResult {
        templates,
        diagnostics,
    }
}

/// Expand a prompt template if it matches a template name.
/// Returns the expanded content or the original text if not a template.
pub fn expand_prompt_template(text: &str, templates: &[PromptTemplate]) -> String {
    static COMMAND_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^/(\S+)(?:\s+([\s\S]*))?$").unwrap());
    if !text.starts_with('/') {
        return text.to_string();
    }
    let Some(caps) = COMMAND_RE.captures(text) else {
        return text.to_string();
    };
    let template_name = &caps[1];
    let args_string = caps.get(2).map(|m| m.as_str()).unwrap_or("");

    if let Some(template) = templates.iter().find(|t| t.name == template_name) {
        let args = parse_command_args(args_string);
        return substitute_args(&template.content, &args);
    }
    text.to_string()
}

#[cfg(test)]
#[path = "prompt_templates_tests.rs"]
mod tests;
