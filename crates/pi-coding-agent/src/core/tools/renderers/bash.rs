//! Port of `packages/coding-agent/src/core/tools/renderers/bash.ts`.
//!
//! Presentation for the shell tools.

use super::{arg, as_component, reuse_container, reuse_text};
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderers};
use crate::core::tools::bash::BashToolDetails;
use crate::core::tools::js_util::{js_number_string, js_trim, js_trim_end};
use crate::core::tools::render_utils::{get_content_text_output, invalid_arg_text_ref, start_tool_timer, str_ref, tool_timer_text};
use crate::core::tools::truncate::{TruncationResultTruncatedBy, format_size};
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::components::visual_truncate::{
    TruncateToVisualLinesKeep, VisualLinePreview, VisualLinePreviewOptions,
};
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use pi_agent::types::AgentToolResult;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, Container};
use serde_json::Value;
use std::sync::Arc;

const BASH_PREVIEW_LINES: i64 = 5;
pub const BASH_UPDATE_THROTTLE_MS: i64 = 100;

fn format_shell_call(args: &Value, prompt: &str, theme: &Theme) -> String {
    let command = str_ref(arg(args, "command"));
    let timeout_suffix = match arg(args, "timeout") {
        Some(Value::Number(n)) if n.as_f64().is_some_and(|t| t != 0.0 && !t.is_nan()) => {
            theme.fg(&ThemeColor::Muted, &format!(" (timeout {}s)", js_number_string(n.as_f64().unwrap_or_default())))
        }
        Some(Value::String(s)) if !s.is_empty() => theme.fg(&ThemeColor::Muted, &format!(" (timeout {s}s)")),
        Some(Value::Bool(true)) => theme.fg(&ThemeColor::Muted, " (timeout trues)"),
        _ => String::new(),
    };
    let command_display = match command {
        None => invalid_arg_text_ref(theme),
        Some(command) if command.is_empty() => theme.fg(&ThemeColor::ToolOutput, "..."),
        Some(command) => command,
    };
    theme.fg(&ThemeColor::ToolTitle, &theme.bold(&format!("{prompt} {command_display}"))) + &timeout_suffix
}

fn rebuild_bash_result_render_component(
    component: &Container,
    result: &AgentToolResult,
    options: ToolRenderResultOptions,
    show_images: bool,
    timer_text: Option<String>,
    theme: &Arc<Theme>,
) {
    component.clear();

    let mut output = js_trim(&get_content_text_output(Some(&result.content), show_images)).to_string();
    let details: Option<BashToolDetails> = result
        .details
        .as_ref()
        .and_then(|d| serde_json::from_value(d.clone()).ok());
    let truncation = details.as_ref().and_then(|d| d.truncation.clone());
    let full_output_path = details.as_ref().and_then(|d| d.full_output_path.clone()).filter(|p| !p.is_empty());
    if !options.is_partial
        && truncation.as_ref().is_some_and(|t| t.truncated)
        && let Some(path) = &full_output_path
        && output.ends_with(']')
        && let Some(footer_start) = output.rfind("\n\n[")
        && output[footer_start..].contains(path.as_str())
    {
        output = js_trim_end(&output[..footer_start]).to_string();
    }

    if !output.is_empty() {
        let styled_output = output
            .split('\n')
            .map(|line| theme.fg(&ThemeColor::ToolOutput, line))
            .collect::<Vec<_>>()
            .join("\n");
        if options.expanded {
            component.add_child(Arc::new(Text::new(format!("\n{styled_output}"), 0, 0, None)));
        } else {
            component.add_child(Arc::new(Spacer::new(1)));
            let hint_theme = theme.clone();
            component.add_child(Arc::new(VisualLinePreview::new(&VisualLinePreviewOptions {
                text: styled_output,
                max_visual_lines: BASH_PREVIEW_LINES,
                keep: TruncateToVisualLinesKeep::End,
                format_hint: Arc::new(move |hidden: f64| {
                    format!(
                        "{} {}{}",
                        hint_theme.fg(&ThemeColor::Muted, &format!("... ({} earlier lines,", js_number_string(hidden))),
                        key_hint("app.tools.expand".to_string(), "to expand"),
                        hint_theme.fg(&ThemeColor::Muted, ")")
                    )
                }),
            })));
        }
    }

    let truncated = truncation.as_ref().is_some_and(|t| t.truncated);
    if truncated || full_output_path.is_some() {
        let mut warnings: Vec<String> = Vec::new();
        if let Some(path) = &full_output_path {
            warnings.push(format!("Full output: {path}"));
        }
        if let Some(truncation) = truncation.as_ref().filter(|t| t.truncated) {
            if truncation.truncated_by == Some(TruncationResultTruncatedBy::Lines) {
                warnings.push(format!(
                    "Truncated: showing {} of {} lines",
                    truncation.output_lines, truncation.total_lines
                ));
            } else {
                warnings.push(format!(
                    "Truncated: {} lines shown ({} limit)",
                    truncation.output_lines,
                    format_size(truncation.max_bytes)
                ));
            }
        }
        component.add_child(Arc::new(Text::new(
            format!("\n{}", theme.fg(&ThemeColor::Warning, &format!("[{}]", warnings.join(". ")))),
            0,
            0,
            None,
        )));
    }

    if let Some(timer_text) = timer_text {
        component.add_child(Arc::new(Text::new(
            format!("\n{}", theme.fg(&ThemeColor::Muted, &timer_text)),
            0,
            0,
            None,
        )));
    }
}

/// Shell renderers are shared by bash and powershell, which differ only in the prompt they display.
pub fn create_shell_renderers(prompt: &str) -> ToolRenderers {
    let call_prompt = prompt.to_string();
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(move |args: &Value, theme: &Arc<Theme>, context: &ToolRenderContext| {
            start_tool_timer(context);
            let text = reuse_text(context);
            text.set_text(format_shell_call(args, &call_prompt, theme));
            as_component(text)
        })),
        render_result: Some(Arc::new(
            |result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Arc<Theme>, context: &ToolRenderContext| {
                let timer_text = tool_timer_text(context, options.is_partial);
                let component = reuse_container(context);
                rebuild_bash_result_render_component(&component, result, options, context.show_images, timer_text, theme);
                component.invalidate();
                as_component(component)
            },
        )),
    }
}
