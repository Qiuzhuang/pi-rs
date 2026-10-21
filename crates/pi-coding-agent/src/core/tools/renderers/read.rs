//! Port of `packages/coding-agent/src/core/tools/renderers/read.ts`.
//!
//! Presentation for the read tool.

use super::{arg, as_component, path_arg, reuse_text};
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderers};
use crate::core::tools::path_utils::{node_path_relative, node_path_resolve, resolve_to_cwd};
use crate::core::tools::read::ReadToolDetails;
use crate::core::tools::render_utils::{format_read_line_range_ref, get_content_text_output, render_tool_path_ref, replace_tabs, str_ref};
use crate::core::tools::truncate::{TruncationResultTruncatedBy, format_size};
use crate::modes::interactive::components::keybinding_hints::{key_hint, key_text};
use crate::modes::interactive::theme::theme::{Theme, get_language_from_path, highlight_code};
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use pi_agent::types::AgentToolResult;
use serde_json::Value;
use std::path::{MAIN_SEPARATOR_STR, Path};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompactReadKind {
    Docs,
    Resource,
    Skill,
}

impl CompactReadKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Docs => "docs",
            Self::Resource => "resource",
            Self::Skill => "skill",
        }
    }
}

struct CompactReadClassification {
    kind: CompactReadKind,
    label: String,
}

const COMPACT_RESOURCE_FILE_NAMES: &[&str] = &["AGENTS.override.md", "AGENTS.md", "AGENTS.MD", "CLAUDE.md", "CLAUDE.MD"];

fn styled_read_line_range(args: &Value, theme: &Theme) -> String {
    let range = format_read_line_range_ref(arg(args, "offset"), arg(args, "limit"));
    if range.is_empty() {
        String::new()
    } else {
        theme.fg(&ThemeColor::Warning, &range)
    }
}

fn format_read_call(args: &Value, theme: &Theme, cwd: &str) -> String {
    let path_display = render_tool_path_ref(str_ref(path_arg(args)).as_deref(), theme, cwd, None);
    format!(
        "{} {path_display}{}",
        theme.fg(&ThemeColor::ToolTitle, &theme.bold("read")),
        styled_read_line_range(args, theme)
    )
}

fn trim_trailing_empty_lines(lines: Vec<String>) -> Vec<String> {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].is_empty() {
        end -= 1;
    }
    lines.into_iter().take(end).collect()
}

fn to_posix_path(file_path: &str) -> String {
    file_path.split(MAIN_SEPARATOR_STR).collect::<Vec<_>>().join("/")
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn dirname(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn is_outside(relative_path: &str) -> bool {
    relative_path.is_empty()
        || relative_path == ".."
        || relative_path.starts_with(&format!("..{MAIN_SEPARATOR_STR}"))
        || Path::new(relative_path).is_absolute()
}

fn get_pi_docs_classification(absolute_path: &str) -> Option<CompactReadClassification> {
    let package_root = dirname(&crate::config::get_readme_path());
    let relative_path = node_path_relative(&node_path_resolve(&[&package_root]), &node_path_resolve(&[absolute_path]));
    if is_outside(&relative_path) {
        return None;
    }
    let label = to_posix_path(&relative_path);
    if label == "README.md" || label.starts_with("docs/") || label.starts_with("examples/") {
        return Some(CompactReadClassification {
            kind: CompactReadKind::Docs,
            label,
        });
    }
    None
}

/// `formatPathRelativeToCwdOrAbsolute(filePath, cwd)` from `utils/paths.ts`.
fn format_path_relative_to_cwd_or_absolute(file_path: &str, cwd: &str) -> String {
    let resolve = |input: &str, base: &str| {
        crate::core::tools::path_utils::resolve_path_with_options(input, base, &Default::default())
    };
    let process_cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let absolute_path = resolve(file_path, cwd);
    let resolved_cwd = resolve(cwd, &process_cwd);
    let resolved_path = resolve(&absolute_path, &resolved_cwd);
    let relative_path = node_path_relative(&resolved_cwd, &resolved_path);
    let inside = relative_path.is_empty()
        || (relative_path != ".."
            && !relative_path.starts_with(&format!("..{MAIN_SEPARATOR_STR}"))
            && !Path::new(&relative_path).is_absolute());
    let display = if inside {
        if relative_path.is_empty() { ".".to_string() } else { relative_path }
    } else {
        absolute_path
    };
    to_posix_path(&display)
}

fn get_compact_read_classification(args: &Value, cwd: &str) -> Option<CompactReadClassification> {
    let raw_path = str_ref(path_arg(args)).filter(|p| !p.is_empty())?;
    let absolute_path = resolve_to_cwd(&raw_path, cwd);
    let file_name = basename(&absolute_path);
    if file_name == "SKILL.md" {
        let parent = basename(&dirname(&absolute_path));
        return Some(CompactReadClassification {
            kind: CompactReadKind::Skill,
            label: if parent.is_empty() { file_name } else { parent },
        });
    }
    if let Some(docs) = get_pi_docs_classification(&absolute_path) {
        return Some(docs);
    }
    if COMPACT_RESOURCE_FILE_NAMES.contains(&file_name.as_str()) {
        return Some(CompactReadClassification {
            kind: CompactReadKind::Resource,
            label: format_path_relative_to_cwd_or_absolute(&absolute_path, cwd),
        });
    }
    None
}

fn format_compact_read_call(classification: &CompactReadClassification, args: &Value, theme: &Theme) -> String {
    let expand_hint = theme.fg(
        &ThemeColor::Dim,
        &format!(" ({} to expand)", key_text("app.tools.expand".to_string())),
    );
    if classification.kind == CompactReadKind::Skill {
        return format!(
            "{}{}{}{expand_hint}",
            theme.fg(&ThemeColor::CustomMessageLabel, "\x1b[1m[skill]\x1b[22m "),
            theme.fg(&ThemeColor::CustomMessageText, &classification.label),
            styled_read_line_range(args, theme)
        );
    }
    format!(
        "{} {}{}{expand_hint}",
        theme.fg(
            &ThemeColor::ToolTitle,
            &theme.bold(&format!("read {}", classification.kind.as_str()))
        ),
        theme.fg(&ThemeColor::Accent, &classification.label),
        styled_read_line_range(args, theme)
    )
}

fn format_read_result(
    args: &Value,
    result: &AgentToolResult,
    options: ToolRenderResultOptions,
    theme: &Theme,
    show_images: bool,
    is_error: bool,
) -> String {
    if !options.expanded && !is_error {
        return String::new();
    }
    let raw_path = str_ref(path_arg(args)).filter(|p| !p.is_empty());
    let output = get_content_text_output(Some(&result.content), show_images);
    let lang = if !is_error { raw_path.as_deref().and_then(get_language_from_path) } else { None };
    let rendered_lines: Vec<String> = match &lang {
        Some(lang) => highlight_code(&replace_tabs(&output), Some(lang)),
        None => output.split('\n').map(str::to_string).collect(),
    };
    let lines = trim_trailing_empty_lines(rendered_lines);
    let max_lines = if options.expanded { lines.len() } else { 10 };
    let display_lines: Vec<String> = lines
        .iter()
        .take(max_lines)
        .map(|line| {
            if lang.is_some() {
                replace_tabs(line)
            } else {
                theme.fg(&ThemeColor::ToolOutput, &replace_tabs(line))
            }
        })
        .collect();
    let mut text = format!("\n{}", display_lines.join("\n"));
    if lines.len() > max_lines {
        let remaining = lines.len() - max_lines;
        text.push_str(&format!(
            "{} {}{}",
            theme.fg(&ThemeColor::Muted, &format!("\n... ({remaining} more lines,")),
            key_hint("app.tools.expand".to_string(), "to expand"),
            theme.fg(&ThemeColor::Muted, ")")
        ));
    }

    let details: Option<ReadToolDetails> = result.details.as_ref().and_then(|d| serde_json::from_value(d.clone()).ok());
    if let Some(truncation) = details.and_then(|d| d.truncation).filter(|t| t.truncated) {
        let warning = if truncation.first_line_exceeds_limit {
            format!("[First line exceeds {} limit]", format_size(truncation.max_bytes))
        } else if truncation.truncated_by == Some(TruncationResultTruncatedBy::Lines) {
            format!(
                "[Truncated: showing {} of {} lines ({} line limit)]",
                truncation.output_lines, truncation.total_lines, truncation.max_lines
            )
        } else {
            format!(
                "[Truncated: {} lines shown ({} limit)]",
                truncation.output_lines,
                format_size(truncation.max_bytes)
            )
        };
        text.push_str(&format!("\n{}", theme.fg(&ThemeColor::Warning, &warning)));
    }
    text
}

pub fn read_renderers() -> ToolRenderers {
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(|args: &Value, theme: &Arc<Theme>, context: &ToolRenderContext| {
            let text = reuse_text(context);
            let classification = if !context.expanded {
                get_compact_read_classification(args, &context.cwd)
            } else {
                None
            };
            text.set_text(match classification {
                Some(classification) => format_compact_read_call(&classification, args, theme),
                None => format_read_call(args, theme, &context.cwd),
            });
            as_component(text)
        })),
        render_result: Some(Arc::new(
            |result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Arc<Theme>, context: &ToolRenderContext| {
                let text = reuse_text(context);
                text.set_text(format_read_result(
                    &context.args,
                    result,
                    options,
                    theme,
                    context.show_images,
                    context.is_error,
                ));
                as_component(text)
            },
        )),
    }
}
