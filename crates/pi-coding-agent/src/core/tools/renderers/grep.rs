//! Port of `packages/coding-agent/src/core/tools/renderers/grep.ts`.
//!
//! Presentation for the grep tool.

use super::find::display_arg;
use super::{arg, as_component, format_list_output, reuse_text};
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderers};
use crate::core::tools::grep::GrepToolDetails;
use crate::core::tools::js_util::{js_number_string, js_trim};
use crate::core::tools::render_utils::{get_content_text_output, invalid_arg_text_ref, shorten_path_str, str_ref};
use crate::core::tools::truncate::format_size;
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use pi_agent::types::AgentToolResult;
use serde_json::Value;
use std::sync::Arc;

fn format_grep_call(args: &Value, theme: &Theme) -> String {
    let pattern = str_ref(arg(args, "pattern"));
    let path = str_ref(arg(args, "path")).map(|p| shorten_path_str(if p.is_empty() { "." } else { &p }));
    let glob = str_ref(arg(args, "glob"));
    let invalid_arg = invalid_arg_text_ref(theme);
    let mut text = format!(
        "{} {}{}",
        theme.fg(&ThemeColor::ToolTitle, &theme.bold("grep")),
        match &pattern {
            None => invalid_arg.clone(),
            Some(pattern) => theme.fg(&ThemeColor::Accent, &format!("/{pattern}/")),
        },
        theme.fg(
            &ThemeColor::ToolOutput,
            &format!(" in {}", path.unwrap_or_else(|| invalid_arg.clone()))
        )
    );
    if let Some(glob) = glob.filter(|g| !g.is_empty()) {
        text.push_str(&theme.fg(&ThemeColor::ToolOutput, &format!(" ({glob})")));
    }
    if let Some(limit) = arg(args, "limit") {
        text.push_str(&theme.fg(&ThemeColor::ToolOutput, &format!(" limit {}", display_arg(limit))));
    }
    text
}

fn format_grep_result(result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Theme, show_images: bool) -> String {
    let output = js_trim(&get_content_text_output(Some(&result.content), show_images)).to_string();
    let mut text = format_list_output(&output, options.expanded, 15, theme);
    let details: Option<GrepToolDetails> = result.details.as_ref().and_then(|d| serde_json::from_value(d.clone()).ok());
    let match_limit = details.as_ref().and_then(|d| d.match_limit_reached).filter(|l| *l != 0.0 && !l.is_nan());
    let truncation = details.as_ref().and_then(|d| d.truncation.clone()).filter(|t| t.truncated);
    let lines_truncated = details.as_ref().and_then(|d| d.lines_truncated).unwrap_or(false);
    if match_limit.is_some() || truncation.is_some() || lines_truncated {
        let mut warnings: Vec<String> = Vec::new();
        if let Some(limit) = match_limit {
            warnings.push(format!("{} matches limit", js_number_string(limit)));
        }
        if let Some(truncation) = truncation {
            warnings.push(format!("{} limit", format_size(truncation.max_bytes)));
        }
        if lines_truncated {
            warnings.push("some lines truncated".to_string());
        }
        text.push_str(&format!(
            "\n{}",
            theme.fg(&ThemeColor::Warning, &format!("[Truncated: {}]", warnings.join(", ")))
        ));
    }
    text
}

pub fn grep_renderers() -> ToolRenderers {
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(|args: &Value, theme: &Arc<Theme>, context: &ToolRenderContext| {
            let text = reuse_text(context);
            text.set_text(format_grep_call(args, theme));
            as_component(text)
        })),
        render_result: Some(Arc::new(
            |result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Arc<Theme>, context: &ToolRenderContext| {
                let text = reuse_text(context);
                text.set_text(format_grep_result(result, options, theme, context.show_images));
                as_component(text)
            },
        )),
    }
}
