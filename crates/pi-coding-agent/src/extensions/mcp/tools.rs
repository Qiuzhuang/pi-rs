//! Port of `packages/coding-agent/src/extensions/mcp/tools.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::extensions::types::ToolExposure;
use crate::core::extensions::types::ToolNamespace;
use crate::core::extensions::types::ToolRenderers;
use crate::core::mcp_servers::McpExposure;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_agent::types::AgentToolResult;
use pi_ai::types::UserContentBlock;
use pi_mcp::client::McpRequestOptions;
use pi_mcp::protocol::content::CallToolResult;
use pi_mcp::protocol::content::ContentBlock;
use pi_mcp::protocol::types::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Tool exposure of an MCP exposure. `codemode` and `deferred` both leave tools out of the codemode
/// description; they differ only in which tool the MCP extension activates to reach them.
pub fn to_tool_exposure(exposure: &McpExposure) -> ToolExposure {
    todo!("port extensions/mcp/tools.ts:toToolExposure")
}

/// Model-facing text of an MCP result beyond this is cut in the middle.
pub const MCP_OUTPUT_MAX_BYTES: u64 = 20480;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDetails {
    pub server: String,
    pub tool: String,
    /// Temp file with the full text output, when the model-facing text was truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpOutputSaverData {
    String(String),
    List(Vec<u8>),
}

/// Saves the full text of a truncated result, or a binary resource, and returns the file path.
/// `extension` includes the dot, for example `.txt`.
pub type McpOutputSaver = Arc<
    dyn Fn(McpOutputSaverData, String) -> BoxFuture<'static, anyhow::Result<String>> + Send + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SaveToTempFileData {
    String(String),
    List(Vec<u8>),
}

pub async fn save_to_temp_file(
    data: &SaveToTempFileData,
    extension: &str,
) -> anyhow::Result<String> {
    todo!("port extensions/mcp/tools.ts:saveToTempFile")
}

#[async_trait::async_trait]
pub trait McpToolCaller: Send + Sync {
    async fn call_tool(
        &self,
        name: &str,
        args: IndexMap<String, Value>,
        options: McpRequestOptions,
    ) -> anyhow::Result<CallToolResult>;
}

/// `mcp__<server>__<tool>`, sanitized and shortened with a hash suffix when too long. Like Codex,
/// everything but `[A-Za-z0-9_]` becomes `_`, so the name is also the identifier codemode scripts
/// call it by. `isTaken` reports names used by a different MCP tool: sanitizing can map two tools to
/// one name (`a-b` and `a_b`), which then get the hash suffix.
pub fn create_mcp_tool_name(
    server: &str,
    tool: &str,
    is_taken: Option<Arc<dyn Fn(String) -> bool + Send + Sync>>,
) -> String {
    todo!("port extensions/mcp/tools.ts:createMcpToolName")
}

/// Output schema of every MCP tool: the `CallToolResult` scripts receive (without `_meta`, which
/// {@link convertMcpResult} drops), with the tool's own output schema as `structuredContent`.
pub fn create_mcp_result_schema(
    structured_content_schema: Option<IndexMap<String, Value>>,
) -> Value {
    todo!("port extensions/mcp/tools.ts:createMcpResultSchema")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitMcpContentLimitMcpContentResult {
    pub content: Vec<UserContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

/// Keep model-facing text within {@link MCP_OUTPUT_MAX_BYTES}. Longer text becomes one text block in
/// Codex's truncation format, followed by the path of the file with the full text; images follow it.
pub async fn limit_mcp_content(
    content: &[UserContentBlock],
    save_output: Option<&McpOutputSaver>,
) -> anyhow::Result<LimitMcpContentLimitMcpContentResult> {
    todo!("port extensions/mcp/tools.ts:limitMcpContent")
}

#[derive(Clone)]
pub struct ConvertMcpResultOptions {
    /// Saves truncated text and binary resources. Default: a temp file.
    pub save_output: Option<McpOutputSaver>,
    /// Whether the server's resources can be read with `read_mcp_resource`, which resource links then name.
    pub readable_resources: Option<bool>,
}

/// Model-facing content of `server`'s content blocks, before the output limit.
pub async fn to_model_content(
    server: &str,
    blocks: &[ContentBlock],
    options: Option<&ConvertMcpResultOptions>,
) -> anyhow::Result<Vec<UserContentBlock>> {
    todo!("port extensions/mcp/tools.ts:toModelContent")
}

/// Convert an MCP result. `isError` results become error results that keep the structured result.
pub async fn convert_mcp_result(
    server: &str,
    tool: &str,
    result: CallToolResult,
    options: Option<&ConvertMcpResultOptions>,
) -> anyhow::Result<AgentToolResult> {
    todo!("port extensions/mcp/tools.ts:convertMcpResult")
}

#[derive(Clone)]
pub struct CreateMcpToolDefinitionOptions {
    pub server: String,
    pub tool: Tool,
    pub name: String,
    pub exposure: McpExposure,
    pub namespace: ToolNamespace,
    pub timeout_ms: i64,
    pub get_client:
        Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<Arc<dyn McpToolCaller>>> + Send + Sync>,
    /// Whether `read_mcp_resource` can read the server's resources.
    pub readable_resources: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

pub fn create_mcp_tool_definition(
    options: &CreateMcpToolDefinitionOptions,
) -> Arc<dyn ToolDefinition> {
    todo!("port extensions/mcp/tools.ts:createMcpToolDefinition")
}

/// Renderers of calls to an MCP tool, labeled `server/tool`, also used before the tool is registered.
pub fn create_mcp_tool_renderers(label: &str) -> ToolRenderers {
    todo!("port extensions/mcp/tools.ts:createMcpToolRenderers")
}
