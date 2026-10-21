//! Port of `packages/coding-agent/src/core/tools/read.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::tools::truncate::TruncationResult;
use futures::future::BoxFuture;
use pi_agent::types::AgentToolRef;
use pi_ai::providers::model_schema::ModelImageResizeOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub static READ_TOOL_SYSTEM_PROMPT_CONTRIBUTION: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/tools/read.ts:readToolSystemPromptContribution"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadToolInputValue {
    /// Path to the file to read (relative or absolute)
    pub path: String,
    /// Line number to start reading from (1-indexed)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Maximum number of lines to read
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

pub type ReadToolInput = ReadToolInputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadToolOutputVariant2 {
    /// Always "image".
    pub r#type: String,
    pub data: String,
    pub mime_type: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReadToolOutputValue {
    String(String),
    ReadToolOutputVariant2(ReadToolOutputVariant2),
}

pub type ReadToolOutput = ReadToolOutputValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadToolDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<TruncationResult>,
}

/// Pluggable operations for the read tool.
/// Override these to delegate file reading to remote systems (for example SSH).
#[derive(Clone)]
pub struct ReadOperations {
    /// Read file contents as a Buffer
    pub read_file: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Vec<u8>>> + Send + Sync>,
    /// Check if file is readable (throw if not)
    pub access: Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    /// Detect image MIME type, return null or undefined for non-images
    pub detect_image_mime_type: Option<
        Arc<dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Option<String>>> + Send + Sync>,
    >,
}

#[derive(Clone)]
pub struct ReadToolOptions {
    /// Whether to auto-resize images. Default: true
    pub auto_resize_images: Option<bool>,
    /// Fallback resize profile when the execution context has no model metadata.
    pub resize_options: Option<ModelImageResizeOptions>,
    /// Custom operations for file reading. Default: local filesystem
    pub operations: Option<ReadOperations>,
}

pub fn create_read_tool_definition(
    cwd: &str,
    options: Option<&ReadToolOptions>,
) -> anyhow::Result<Arc<dyn ToolDefinition>> {
    todo!("port core/tools/read.ts:createReadToolDefinition")
}

pub fn create_read_tool(cwd: &str, options: Option<&ReadToolOptions>) -> AgentToolRef {
    todo!("port core/tools/read.ts:createReadTool")
}
