//! Port of `packages/coding-agent/src/core/export-html/tool-renderer.ts`.

use crate::core::extensions::types::ToolRenderers;
use crate::modes::interactive::theme::theme::Theme;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolHtmlRendererResult {
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
pub struct ToolHtmlRendererRenderResultResult {
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
        result: &[ToolHtmlRendererResult],
        details: Value,
        is_error: bool,
    ) -> Option<ToolHtmlRendererRenderResultResult>;
}

pub fn create_tool_html_renderer(deps: &ToolHtmlRendererDeps) -> Arc<dyn ToolHtmlRenderer> {
    todo!("port core/export-html/tool-renderer.ts:createToolHtmlRenderer")
}
