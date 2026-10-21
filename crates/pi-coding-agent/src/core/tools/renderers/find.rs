//! Port of `packages/coding-agent/src/core/tools/renderers/find.ts`.
//!
//! Presentation for the find tool.

use super::{arg, as_component, format_list_output, reuse_text};
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderers};
use crate::core::tools::find::FindToolDetails;
use crate::core::tools::js_util::{js_number_string, js_trim};
use crate::core::tools::render_utils::{get_content_text_output, invalid_arg_text_ref, shorten_path_str, str_ref};
use crate::core::tools::truncate::format_size;
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use pi_agent::types::AgentToolResult;
use serde_json::Value;
use std::sync::Arc;

/// `${value}` for an optional `limit` argument.
pub(crate) fn display_arg(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => js_number_string(n.as_f64().unwrap_or_default()),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn format_find_call(args: &Value, theme: &Theme) -> String {
    let pattern = str_ref(arg(args, "pattern"));
    let path = str_ref(arg(args, "path")).map(|p| shorten_path_str(if p.is_empty() { "." } else { &p }));
    let invalid_arg = invalid_arg_text_ref(theme);
    let mut text = format!(
        "{} {}{}",
        theme.fg(&ThemeColor::ToolTitle, &theme.bold("find")),
        match &pattern {
            None => invalid_arg.clone(),
            Some(pattern) => theme.fg(&ThemeColor::Accent, pattern),
        },
        theme.fg(
            &ThemeColor::ToolOutput,
            &format!(" in {}", path.unwrap_or_else(|| invalid_arg.clone()))
        )
    );
    if let Some(limit) = arg(args, "limit") {
        text.push_str(&theme.fg(&ThemeColor::ToolOutput, &format!(" (limit {})", display_arg(limit))));
    }
    text
}

fn format_find_result(result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Theme, show_images: bool) -> String {
    let output = js_trim(&get_content_text_output(Some(&result.content), show_images)).to_string();
    let mut text = format_list_output(&output, options.expanded, 20, theme);
    let details: Option<FindToolDetails> = result.details.as_ref().and_then(|d| serde_json::from_value(d.clone()).ok());
    let result_limit = details.as_ref().and_then(|d| d.result_limit_reached).filter(|l| *l != 0.0 && !l.is_nan());
    let truncation = details.as_ref().and_then(|d| d.truncation.clone()).filter(|t| t.truncated);
    if result_limit.is_some() || truncation.is_some() {
        let mut warnings: Vec<String> = Vec::new();
        if let Some(limit) = result_limit {
            warnings.push(format!("{} results limit", js_number_string(limit)));
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

pub fn find_renderers() -> ToolRenderers {
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(|args: &Value, theme: &Arc<Theme>, context: &ToolRenderContext| {
            let text = reuse_text(context);
            text.set_text(format_find_call(args, theme));
            as_component(text)
        })),
        render_result: Some(Arc::new(
            |result: &AgentToolResult, options: ToolRenderResultOptions, theme: &Arc<Theme>, context: &ToolRenderContext| {
                let text = reuse_text(context);
                text.set_text(format_find_result(result, options, theme, context.show_images));
                as_component(text)
            },
        )),
    }
}
