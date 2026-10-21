//! Port of `packages/coding-agent/src/core/export-html/tool-renderer.ts`.
//!
//! Tool HTML renderer for custom tools in HTML export.
//!
//! Renders custom tool calls and results to HTML by invoking their TUI renderers
//! and converting the ANSI output to HTML.

use crate::core::export_html::ansi_to_html::ansi_lines_to_html;
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderState, ToolRenderers};
use crate::modes::interactive::theme::theme::Theme;
use parking_lot::Mutex;
use pi_agent::types::AgentToolResult;
use pi_ai::types::UserContentBlock;
use pi_tui::tui::ComponentRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, LazyLock};

#[derive(Clone)]
pub struct ToolHtmlRendererDeps {
    /// Renderers of calls to a tool, as resolved by extensions and the registered tool
    pub get_tool_renderers: Arc<dyn Fn(String) -> Option<ToolRenderers> + Send + Sync>,
    /// Theme for styling
    pub theme: Arc<Theme>,
    /// Working directory for render context
    pub cwd: String,
    /// Terminal width for rendering (default: 100)
    pub width: Option<i64>,
}

/// A content block of a stored tool result (`{ type, text?, data?, mimeType? }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolHtmlRendererRenderResultResult {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl From<&UserContentBlock> for ToolHtmlRendererRenderResultResult {
    fn from(block: &UserContentBlock) -> Self {
        match block {
            UserContentBlock::Text(t) => Self {
                r#type: "text".into(),
                text: Some(t.text.clone()),
                data: None,
                mime_type: None,
            },
            UserContentBlock::Image(i) => Self {
                r#type: "image".into(),
                text: None,
                data: Some(i.data.clone()),
                mime_type: Some(i.mime_type.clone()),
            },
        }
    }
}

/// Collapsed/expanded HTML of a rendered tool result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolHtmlRendererRenderResultResult2 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<String>,
}

pub trait ToolHtmlRenderer: Send + Sync {
    /// Render a tool call to HTML. Returns undefined if tool has no custom renderer.
    fn render_call(&self, tool_call_id: &str, tool_name: &str, args: Value) -> Option<String>;
    /// Render a tool result to collapsed/expanded HTML. Returns undefined if tool has no custom renderer.
    fn render_result(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        result: &[ToolHtmlRendererRenderResultResult],
        details: Value,
        is_error: bool,
    ) -> Option<ToolHtmlRendererRenderResultResult2>;
}

static ANSI_ESCAPE_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\x1b\[[\d;]*m").expect("valid regex"));

fn is_blank_rendered_line(line: &str) -> bool {
    ANSI_ESCAPE_REGEX.replace_all(line, "").trim().is_empty()
}

fn trim_rendered_result_lines(lines: &[String]) -> &[String] {
    let mut start = 0;
    let mut end = lines.len();
    while start < end && is_blank_rendered_line(&lines[start]) {
        start += 1;
    }
    while end > start && is_blank_rendered_line(&lines[end - 1]) {
        end -= 1;
    }
    &lines[start..end]
}

#[derive(Default)]
struct RendererState {
    rendered_call_components: HashMap<String, ComponentRef>,
    rendered_result_components: HashMap<String, ComponentRef>,
    rendered_states: HashMap<String, ToolRenderState>,
    rendered_args: HashMap<String, Value>,
}

struct DefaultToolHtmlRenderer {
    deps: ToolHtmlRendererDeps,
    width: usize,
    state: Mutex<RendererState>,
}

impl DefaultToolHtmlRenderer {
    fn create_render_context(
        &self,
        tool_call_id: &str,
        last_component: Option<ComponentRef>,
        expanded: bool,
        is_partial: bool,
        is_error: bool,
    ) -> ToolRenderContext {
        let mut state = self.state.lock();
        let render_state = state
            .rendered_states
            .entry(tool_call_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .clone();
        ToolRenderContext {
            args: state.rendered_args.get(tool_call_id).cloned().unwrap_or(Value::Null),
            tool_call_id: tool_call_id.to_string(),
            invalidate: Arc::new(|| {}),
            last_component,
            state: render_state,
            cwd: self.deps.cwd.clone(),
            execution_started: true,
            args_complete: true,
            is_partial,
            expanded,
            show_images: false,
            is_error,
            duration_ms: None,
            output_pad: 1,
        }
    }
}

impl ToolHtmlRenderer for DefaultToolHtmlRenderer {
    fn render_call(&self, tool_call_id: &str, tool_name: &str, args: Value) -> Option<String> {
        // On error (panic), return None so HTML export can fall back to structured result rendering
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.state
                .lock()
                .rendered_args
                .insert(tool_call_id.to_string(), args.clone());
            let renderers = (self.deps.get_tool_renderers)(tool_name.to_string())?;
            let render_call = renderers.render_call?;

            let last = self.state.lock().rendered_call_components.get(tool_call_id).cloned();
            let context = self.create_render_context(tool_call_id, last, false, true, false);
            let component = render_call(&args, &self.deps.theme, &context);
            self.state
                .lock()
                .rendered_call_components
                .insert(tool_call_id.to_string(), component.clone());
            let lines = component.render(self.width);
            Some(ansi_lines_to_html(&lines))
        }))
        .ok()
        .flatten()
    }

    fn render_result(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        result: &[ToolHtmlRendererRenderResultResult],
        details: Value,
        is_error: bool,
    ) -> Option<ToolHtmlRendererRenderResultResult2> {
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let renderers = (self.deps.get_tool_renderers)(tool_name.to_string())?;
            let render_result = renderers.render_result?;

            // Build AgentToolResult from content array
            let content: Vec<UserContentBlock> = result
                .iter()
                .filter_map(|block| serde_json::to_value(block).ok())
                .filter_map(|value| serde_json::from_value(value).ok())
                .collect();
            let agent_tool_result = AgentToolResult {
                content,
                details: (!details.is_null()).then(|| details.clone()),
                is_error: Some(is_error),
                ..Default::default()
            };

            // Render collapsed
            let last = self.state.lock().rendered_result_components.get(tool_call_id).cloned();
            let context = self.create_render_context(tool_call_id, last, false, false, is_error);
            let collapsed_component = render_result(
                &agent_tool_result,
                ToolRenderResultOptions {
                    expanded: false,
                    is_partial: false,
                },
                &self.deps.theme,
                &context,
            );
            self.state
                .lock()
                .rendered_result_components
                .insert(tool_call_id.to_string(), collapsed_component.clone());
            let collapsed = ansi_lines_to_html(trim_rendered_result_lines(&collapsed_component.render(self.width)));

            // Render expanded
            let last = self.state.lock().rendered_result_components.get(tool_call_id).cloned();
            let context = self.create_render_context(tool_call_id, last, true, false, is_error);
            let expanded_component = render_result(
                &agent_tool_result,
                ToolRenderResultOptions {
                    expanded: true,
                    is_partial: false,
                },
                &self.deps.theme,
                &context,
            );
            self.state
                .lock()
                .rendered_result_components
                .insert(tool_call_id.to_string(), expanded_component.clone());
            let expanded = ansi_lines_to_html(trim_rendered_result_lines(&expanded_component.render(self.width)));

            Some(ToolHtmlRendererRenderResultResult2 {
                collapsed: (!collapsed.is_empty() && collapsed != expanded).then_some(collapsed),
                expanded: Some(expanded),
            })
        }))
        .ok()
        .flatten()
    }
}

/// Create a tool HTML renderer.
///
/// The renderer looks up tool definitions and invokes their renderCall/renderResult
/// methods, converting the resulting TUI Component output (ANSI) to HTML.
pub fn create_tool_html_renderer(deps: &ToolHtmlRendererDeps) -> Arc<dyn ToolHtmlRenderer> {
    let width = deps.width.filter(|w| *w > 0).unwrap_or(100) as usize;
    Arc::new(DefaultToolHtmlRenderer {
        deps: deps.clone(),
        width,
        state: Mutex::new(RendererState::default()),
    })
}
