//! Port of `packages/coding-agent/src/core/export-html/index.ts`.

pub mod ansi_to_html;
pub mod tool_renderer;

use crate::core::session_manager::SessionManager;
use indexmap::IndexMap;
use pi_agent::types::AgentState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolHtmlRendererRenderResultResult2 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<String>,
}

/// Interface for rendering custom tools to HTML.
/// Used by agent-session to pre-render extension tool output.
pub trait ToolHtmlRenderer: Send + Sync {
    /// Render a tool call to HTML. Returns undefined if tool has no custom renderer.
    fn render_call(&self, tool_call_id: &str, tool_name: &str, args: Value) -> Option<String>;
    /// Render a tool result to HTML. Returns collapsed/expanded or undefined if tool has no custom renderer.
    fn render_result(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        result: &[ToolHtmlRendererRenderResultResult],
        details: Value,
        is_error: bool,
    ) -> Option<ToolHtmlRendererRenderResultResult2>;
}

/// Pre-rendered HTML for a custom tool call and result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedToolHtml {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_html_collapsed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_html_expanded: Option<String>,
}

#[derive(Clone)]
pub struct ExportOptions {
    pub output_path: Option<String>,
    pub theme_name: Option<String>,
    /// Optional tool renderer for custom tools
    pub tool_renderer: Option<Arc<dyn ToolHtmlRenderer>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDataTools {
    /// Tool name (used in LLM tool calls)
    pub name: String,
    /// Description for LLM
    pub description: String,
    /// Parameter schema (TypeBox)
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionData {
    pub header: Value,
    pub entries: Value,
    #[serde(default)]
    pub leaf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<SessionDataTools>>,
    /// Pre-rendered HTML for custom tool calls/results, keyed by tool call ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_tools: Option<IndexMap<String, RenderedToolHtml>>,
}

#[derive(Clone)]
pub enum ExportSessionToHtmlOptions {
    ExportOptions(ExportOptions),
    String(String),
}

/// Export session to HTML using SessionManager and AgentState.
/// Used by TUI's /export command.
pub async fn export_session_to_html(
    sm: Arc<SessionManager>,
    state: Option<AgentState>,
    options: Option<&ExportSessionToHtmlOptions>,
) -> anyhow::Result<String> {
    todo!("port core/export-html/index.ts:exportSessionToHtml")
}

#[derive(Clone)]
pub enum ExportFromFileOptions {
    ExportOptions(ExportOptions),
    String(String),
}

/// Export session file to HTML (standalone, without AgentState).
/// Used by CLI for exporting arbitrary session files.
pub async fn export_from_file(
    input_path: &str,
    options: Option<&ExportFromFileOptions>,
) -> anyhow::Result<String> {
    todo!("port core/export-html/index.ts:exportFromFile")
}
