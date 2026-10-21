//! Port of `packages/coding-agent/src/core/tools/edit.ts`.

use crate::core::extensions::types::ToolDefinition;
use futures::future::BoxFuture;
use pi_agent::types::AgentToolRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub static EDIT_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/edit.ts:editToolSystemPromptContribution"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditToolInputValueEdits {
    /// Exact text for one targeted replacement. It must be unique in the original file and must not overlap with any other edits[].oldText in the same call.
    pub old_text: String,
    /// Replacement text for this targeted edit.
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditToolInputValue {
    /// Path to the file to edit (relative or absolute)
    pub path: String,
    /// One or more targeted replacements. Each edit is matched against the original file, not incrementally. Do not include overlapping or nested edits. If two changes touch the same block or nearby lines, merge them into one edit instead.
    pub edits: Vec<EditToolInputValueEdits>,
}

pub type EditToolInput = EditToolInputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyEditToolInputValue {
    #[serde(flatten)]
    pub base: EditToolInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_text: Option<Value>,
}

pub type LegacyEditToolInput = LegacyEditToolInputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleEditInput {
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditToolDetails {
    /// Display-oriented diff of the changes made
    pub diff: String,
    /// Standard unified patch of the changes made
    pub patch: String,
    /// Line number of the first change in the new file (for editor navigation)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_changed_line: Option<i64>,
}

/// Pluggable operations for the edit tool.
/// Override these to delegate file editing to remote systems (for example SSH).
#[derive(Clone)]
pub struct EditOperations {
    /// Read file contents as a Buffer
    pub read_file: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Vec<u8>>> + Send + Sync>,
    /// Write content to a file
    pub write_file:
        Arc<dyn Fn(String, String) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    /// Check if file is readable and writable (throw if not)
    pub access: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
}

#[derive(Clone)]
pub struct EditToolOptions {
    /// Custom operations for file editing. Default: local filesystem
    pub operations: Option<EditOperations>,
}

pub fn create_edit_tool_definition(
    cwd: &str,
    options: Option<&EditToolOptions>,
) -> anyhow::Result<Arc<dyn ToolDefinition>> {
    todo!("port core/tools/edit.ts:createEditToolDefinition")
}

pub fn create_edit_tool(cwd: &str, options: Option<&EditToolOptions>) -> AgentToolRef {
    todo!("port core/tools/edit.ts:createEditTool")
}
