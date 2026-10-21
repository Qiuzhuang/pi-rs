//! Port of `packages/coding-agent/src/core/tools/render-utils.ts`.

use crate::core::extensions::types::ToolRenderContext;
use crate::modes::interactive::theme::theme::Theme;
use pi_ai::types::UserContentBlock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub fn shorten_path(path: Value) -> String {
    todo!("port core/tools/render-utils.ts:shortenPath")
}

pub fn link_path(styled_text: &str, raw_path: &str, cwd: &str) -> String {
    todo!("port core/tools/render-utils.ts:linkPath")
}

pub fn str(value: Value) -> Option<String> {
    todo!("port core/tools/render-utils.ts:str")
}

/// Format finite numbers or numeric strings as a read line range, omitting malformed ranges.
pub fn format_read_line_range(offset: Value, limit: Value) -> String {
    todo!("port core/tools/render-utils.ts:formatReadLineRange")
}

pub fn replace_tabs(text: &str) -> String {
    todo!("port core/tools/render-utils.ts:replaceTabs")
}

pub fn normalize_display_text(text: &str) -> String {
    todo!("port core/tools/render-utils.ts:normalizeDisplayText")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTextOutputResultContent {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTextOutputResult {
    pub content: Vec<GetTextOutputResultContent>,
}

pub fn get_text_output(result: Option<&GetTextOutputResult>, show_images: bool) -> String {
    todo!("port core/tools/render-utils.ts:getTextOutput")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRenderResultLike {
    pub content: Vec<UserContentBlock>,
    pub details: Value,
}

/// Generic tool call header: the title followed by the arguments. Collapsed, they are `key=value`
/// pairs on the title line, cut to {@link COLLAPSED_ARGS_CHARS}. Expanded, each is a `key: value`
/// line below the title, with strings shown raw and continuation lines indented.
pub fn format_tool_call_with_args(
    title: &str,
    args: Value,
    theme: Arc<Theme>,
    expanded: bool,
) -> String {
    todo!("port core/tools/render-utils.ts:formatToolCallWithArgs")
}

/// Duration as `4.2s` below a minute, then `1m 30s` and `2h 3m 4s`.
pub fn format_tool_duration(ms: f64) -> String {
    todo!("port core/tools/render-utils.ts:formatToolDuration")
}

/// Renderer state of {@link startToolTimer} and {@link toolTimerText}, kept in `ToolRenderContext.state`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTimerState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Value>,
}

/// Start the row's clock once execution starts. Call from `renderCall`.
pub fn start_tool_timer(context: &ToolRenderContext) {
    todo!("port core/tools/render-utils.ts:startToolTimer")
}

/// The row's timing line, from `renderResult`: `Elapsed 4.2s` while the result is partial, redrawn
/// every second, and `Took 4.2s` once it is final. Undefined when the row has no start time and no
/// recorded duration.
pub fn tool_timer_text(context: &ToolRenderContext, is_partial: bool) -> Option<String> {
    todo!("port core/tools/render-utils.ts:toolTimerText")
}

pub fn invalid_arg_text(theme: Arc<Theme>) -> String {
    todo!("port core/tools/render-utils.ts:invalidArgText")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderToolPathOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_fallback: Option<String>,
}

pub fn render_tool_path(
    raw_path: Option<&str>,
    theme: Arc<Theme>,
    cwd: &str,
    options: Option<&RenderToolPathOptions>,
) -> String {
    todo!("port core/tools/render-utils.ts:renderToolPath")
}
