//! Port of `packages/coding-agent/src/core/tools/renderers/write.ts`.
//!
//! Presentation for the write tool.

use super::{arg, as_component, path_arg, reuse_container, reuse_text};
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderers};
use crate::core::tools::render_utils::{normalize_display_text, render_tool_path_ref, replace_tabs, str_ref};
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::theme::theme::{Theme, get_language_from_path, highlight_code};
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use pi_agent::types::AgentToolResult;
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, MouseEvent, MouseResult, downcast_component};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone)]
struct WriteHighlightCache {
    raw_path: Option<String>,
    lang: String,
    raw_content: String,
    normalized_lines: Vec<String>,
    highlighted_lines: Vec<String>,
}

/// `class WriteCallRenderComponent extends Text { cache?: WriteHighlightCache }`.
pub struct WriteCallRenderComponent {
    text: Text,
    cache: parking_lot::Mutex<Option<WriteHighlightCache>>,
}

impl WriteCallRenderComponent {
    fn new() -> Self {
        Self {
            text: Text::new("", 0, 0, None),
            cache: parking_lot::Mutex::new(None),
        }
    }

    pub fn set_text(&self, text: impl Into<String>) {
        self.text.set_text(text);
    }
}

impl Component for WriteCallRenderComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.text.render(width)
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.text.handle_mouse(event)
    }

    fn invalidate(&self) {
        self.text.invalidate();
    }
}

const WRITE_PARTIAL_FULL_HIGHLIGHT_LINES: usize = 50;

fn highlight_single_line(line: &str, lang: &str) -> String {
    highlight_code(line, Some(lang)).into_iter().next().unwrap_or_default()
}

fn refresh_write_highlight_prefix(cache: &mut WriteHighlightCache) {
    let prefix_count = WRITE_PARTIAL_FULL_HIGHLIGHT_LINES.min(cache.normalized_lines.len());
    if prefix_count == 0 {
        return;
    }
    let prefix_source = cache.normalized_lines[..prefix_count].join("\n");
    let prefix_highlighted = highlight_code(&prefix_source, Some(&cache.lang));
    for i in 0..prefix_count {
        let line = match prefix_highlighted.get(i) {
            Some(line) => line.clone(),
            None => highlight_single_line(&cache.normalized_lines[i], &cache.lang),
        };
        if i < cache.highlighted_lines.len() {
            cache.highlighted_lines[i] = line;
        } else {
            cache.highlighted_lines.push(line);
        }
    }
}

fn rebuild_write_highlight_cache_full(raw_path: Option<&str>, file_content: &str) -> Option<WriteHighlightCache> {
    let lang = raw_path.filter(|p| !p.is_empty()).and_then(get_language_from_path)?;
    let normalized = replace_tabs(&normalize_display_text(file_content));
    Some(WriteHighlightCache {
        raw_path: raw_path.map(str::to_string),
        highlighted_lines: highlight_code(&normalized, Some(&lang)),
        lang,
        raw_content: file_content.to_string(),
        normalized_lines: normalized.split('\n').map(str::to_string).collect(),
    })
}

fn update_write_highlight_cache_incremental(
    cache: Option<WriteHighlightCache>,
    raw_path: Option<&str>,
    file_content: &str,
) -> Option<WriteHighlightCache> {
    let lang = raw_path.filter(|p| !p.is_empty()).and_then(get_language_from_path)?;
    let Some(mut cache) = cache else {
        return rebuild_write_highlight_cache_full(raw_path, file_content);
    };
    if cache.lang != lang || cache.raw_path.as_deref() != raw_path {
        return rebuild_write_highlight_cache_full(raw_path, file_content);
    }
    if !file_content.starts_with(&cache.raw_content) {
        return rebuild_write_highlight_cache_full(raw_path, file_content);
    }
    if file_content.len() == cache.raw_content.len() {
        return Some(cache);
    }

    let delta_raw = &file_content[cache.raw_content.len()..];
    let delta_normalized = replace_tabs(&normalize_display_text(delta_raw));
    cache.raw_content = file_content.to_string();
    if cache.normalized_lines.is_empty() {
        cache.normalized_lines.push(String::new());
        cache.highlighted_lines.push(String::new());
    }

    let segments: Vec<&str> = delta_normalized.split('\n').collect();
    let last_index = cache.normalized_lines.len() - 1;
    cache.normalized_lines[last_index].push_str(segments[0]);
    let highlighted = highlight_single_line(&cache.normalized_lines[last_index], &cache.lang);
    if last_index < cache.highlighted_lines.len() {
        cache.highlighted_lines[last_index] = highlighted;
    } else {
        cache.highlighted_lines.push(highlighted);
    }
    for segment in &segments[1..] {
        cache.normalized_lines.push(segment.to_string());
        cache.highlighted_lines.push(highlight_single_line(segment, &cache.lang));
    }
    refresh_write_highlight_prefix(&mut cache);
    Some(cache)
}

fn trim_trailing_empty_lines(lines: Vec<String>) -> Vec<String> {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].is_empty() {
        end -= 1;
    }
    lines.into_iter().take(end).collect()
}

fn format_write_call(
    args: &Value,
    options: ToolRenderResultOptions,
    theme: &Theme,
    cache: Option<&WriteHighlightCache>,
    cwd: &str,
) -> String {
    let raw_path = str_ref(path_arg(args));
    let file_content = str_ref(arg(args, "content"));
    let path_display = render_tool_path_ref(raw_path.as_deref(), theme, cwd, None);
    let mut text = format!("{} {path_display}", theme.fg(&ThemeColor::ToolTitle, &theme.bold("write")));

    match file_content {
        None => {
            text.push_str(&format!(
                "\n\n{}",
                theme.fg(&ThemeColor::Error, "[invalid content arg - expected string]")
            ));
        }
        Some(file_content) if !file_content.is_empty() => {
            let lang = raw_path.as_deref().filter(|p| !p.is_empty()).and_then(get_language_from_path);
            let rendered_lines: Vec<String> = match &lang {
                Some(lang) => match cache {
                    Some(cache) => cache.highlighted_lines.clone(),
                    None => highlight_code(&replace_tabs(&normalize_display_text(&file_content)), Some(lang)),
                },
                None => normalize_display_text(&file_content).split('\n').map(str::to_string).collect(),
            };
            let lines = trim_trailing_empty_lines(rendered_lines);
            let total_lines = lines.len();
            let max_lines = if options.expanded { lines.len() } else { 10 };
            let display: Vec<String> = lines
                .iter()
                .take(max_lines)
                .map(|line| {
                    if lang.is_some() {
                        line.clone()
                    } else {
                        theme.fg(&ThemeColor::ToolOutput, &replace_tabs(line))
                    }
                })
                .collect();
            text.push_str(&format!("\n\n{}", display.join("\n")));
            if lines.len() > max_lines {
                let remaining = lines.len() - max_lines;
                text.push_str(&format!(
                    "{} {}{}",
                    theme.fg(
                        &ThemeColor::Muted,
                        &format!("\n... ({remaining} more lines, {total_lines} total,")
                    ),
                    key_hint("app.tools.expand".to_string(), "to expand"),
                    theme.fg(&ThemeColor::Muted, ")")
                ));
            }
        }
        Some(_) => {}
    }
    text
}

fn format_write_result(result: &AgentToolResult, is_error: bool, theme: &Theme) -> Option<String> {
    if !is_error {
        return None;
    }
    let output = result
        .content
        .iter()
        .filter_map(|c| c.as_text())
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if output.is_empty() {
        return None;
    }
    Some(format!("\n{}", theme.fg(&ThemeColor::Error, &output)))
}

pub fn write_renderers() -> ToolRenderers {
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(|args: &Value, theme: &Arc<Theme>, context: &ToolRenderContext| {
            let raw_path = str_ref(path_arg(args));
            let file_content = str_ref(arg(args, "content"));
            let component = context
                .last_component
                .as_ref()
                .and_then(downcast_component::<WriteCallRenderComponent>)
                .unwrap_or_else(|| Arc::new(WriteCallRenderComponent::new()));
            let cache = {
                let mut cache_slot = component.cache.lock();
                *cache_slot = match &file_content {
                    Some(file_content) => {
                        if context.args_complete {
                            rebuild_write_highlight_cache_full(raw_path.as_deref(), file_content)
                        } else {
                            update_write_highlight_cache_incremental(cache_slot.take(), raw_path.as_deref(), file_content)
                        }
                    }
                    None => None,
                };
                cache_slot.clone()
            };
            component.set_text(format_write_call(
                args,
                ToolRenderResultOptions {
                    expanded: context.expanded,
                    is_partial: context.is_partial,
                },
                theme,
                cache.as_ref(),
                &context.cwd,
            ));
            as_component(component)
        })),
        render_result: Some(Arc::new(
            |result: &AgentToolResult, _options: ToolRenderResultOptions, theme: &Arc<Theme>, context: &ToolRenderContext| {
                match format_write_result(result, context.is_error, theme) {
                    None => {
                        let component = reuse_container(context);
                        component.clear();
                        as_component(component)
                    }
                    Some(output) => {
                        let text = reuse_text(context);
                        text.set_text(output);
                        as_component(text)
                    }
                }
            },
        )),
    }
}
