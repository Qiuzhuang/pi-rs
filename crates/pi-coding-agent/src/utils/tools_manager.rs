//! Port of `packages/coding-agent/src/utils/tools-manager.ts`.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct ToolConfig {
    pub name: String,
    pub repo: String,
    pub binary_name: String,
    pub system_binary_names: Option<Vec<String>>,
    pub tag_prefix: String,
    pub get_asset_name: Arc<dyn Fn(String, String, String) -> Option<String> + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GetToolPathTool {
    #[serde(rename = "fd")]
    Fd,
    #[serde(rename = "rg")]
    Rg,
}

impl GetToolPathTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fd => "fd",
            Self::Rg => "rg",
        }
    }
}

pub fn get_tool_path(tool: GetToolPathTool) -> Option<String> {
    todo!("port utils/tools-manager.ts:getToolPath")
}

pub async fn get_latest_version(repo: &str) -> anyhow::Result<String> {
    todo!("port utils/tools-manager.ts:getLatestVersion")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolStatusType {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
}

impl ToolStatusType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub r#type: ToolStatusType,
    pub message: String,
}

/// Ensure a tool is available, downloading if necessary.
/// Reports progress through `onStatus`; status messages are otherwise silent.
/// Returns the tool path, or undefined if unavailable.
pub async fn ensure_tool(
    tool: GetToolPathTool,
    on_status: Option<Arc<dyn Fn(ToolStatus) + Send + Sync>>,
) -> anyhow::Result<Option<String>> {
    todo!("port utils/tools-manager.ts:ensureTool")
}
