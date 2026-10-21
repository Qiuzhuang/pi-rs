//! Port of `packages/coding-agent/src/core/tools/renderers/ls.ts`.
//!
//! Presentation for the ls tool.

use super::find::display_arg;
use super::{arg, as_component, format_list_output, reuse_text};
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderers};
use crate::core::tools::js_util::{js_number_string, js_trim};
use crate::core::tools::ls::LsToolDetails;
use crate::core::tools::render_utils::{RenderToolPathOptions, get_content_text_output, render_tool_path_ref, str_ref};
use crate::core::tools::truncate::format_size;
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use pi_agent::types::AgentToolResult;
use serde_json::Value;
use std::sync::Arc;

fn format_ls_call(args: &Value, theme: &Theme, cwd: &str) -> String {
    let path_display = render_tool_path_ref(
        str_ref(arg(args, "path")).as_deref(),
        theme,
        cwd,
        Some(&RenderToolPathOptions {
            empty_fallback: Some(".".to_string()),
        }),
    );
    let mut text = format!("{} {path_display}", theme.fg(&ThemeColor::ToolTitle, &theme.bold("ls")));
    if let Some(limit) = arg(args, "limit") {
        text.push_str(&theme.fg(&ThemeColor::ToolOutput, &format!(" (limit {})", display_arg(limit))));
    }
    text
}

fn format_ls_result(result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Theme, show_images: bool) -> String {
    let output = js_trim(&get_content_text_output(Some(&result.content), show_images)).to_string();
    let mut text = format_list_output(&output, options.expanded, 20, theme);
    let details: Option<LsToolDetails> = result.details.as_ref().and_then(|d| serde_json::from_value(d.clone()).ok());
    let entry_limit = details.as_ref().and_then(|d| d.entry_limit_reached).filter(|l| *l != 0.0 && !l.is_nan());
    let truncation = details.as_ref().and_then(|d| d.truncation.clone()).filter(|t| t.truncated);
    if entry_limit.is_some() || truncation.is_some() {
        let mut warnings: Vec<String> = Vec::new();
        if let Some(limit) = entry_limit {
            warnings.push(format!("{} entries limit", js_number_string(limit)));
        }
        if let Some(truncation) = truncation {
            warnings.push(format!("{} limit", format_size(truncation.max_bytes)));
        }
        text.push_str(&format!(
            "\n{}",
            theme.fg(&ThemeColor::Warning, &format!("[Truncated: {}]", warnings.join(", ")))
        ));
    }
    text
}

pub fn ls_renderers() -> ToolRenderers {
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(|args: &Value, theme: &Arc<Theme>, context: &ToolRenderContext| {
            let text = reuse_text(context);
            text.set_text(format_ls_call(args, theme, &context.cwd));
            as_component(text)
        })),
        render_result: Some(Arc::new(
            |result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Arc<Theme>, context: &ToolRenderContext| {
                let text = reuse_text(context);
                text.set_text(format_ls_result(result, options, theme, context.show_images));
                as_component(text)
            },
        )),
    }
}
