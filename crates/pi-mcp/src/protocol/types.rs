use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::content::{ContentAnnotations, ContentBlock, ResourceContents};
use super::jsonrpc::JsonRpcId;
use crate::serde_util::lenient;

pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// Versions the client accepts from a server. Servers that do not support the requested version
/// answer with their own latest one, so older versions stay accepted for servers built on older SDKs.
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] = [LATEST_PROTOCOL_VERSION, "2025-06-18", "2025-03-26", "2024-11-05"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Implementation {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub title: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Implementation {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self { name: name.into(), version: version.into(), title: None, extra: Map::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Root {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Root {
    pub fn new(uri: impl Into<String>, name: Option<String>) -> Self {
        Self { uri: uri.into(), name }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChangedCapability {
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub list_changed: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesCapability {
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub subscribe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub list_changed: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<ListChangedCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub experimental: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub logging: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub prompts: Option<ListChangedCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub resources: Option<ResourcesCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub tools: Option<ListChangedCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub completions: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: Implementation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressNotification {
    pub progress_token: JsonRpcId,
    #[serde(with = "crate::serde_util::number")]
    pub progress: f64,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::serde_util::opt_number")]
    pub total: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelledNotification {
    pub request_id: JsonRpcId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub open_world_hint: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecution {
    /// `"forbidden" | "optional" | "required"`.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub task_support: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A tool definition as the server lists it; unknown fields are preserved in `extra`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub description: Option<String>,
    pub input_schema: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub output_schema: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub annotations: Option<ToolAnnotations>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub execution: Option<ToolExecution>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

/// A resource a server lists in `resources/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub uri: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::serde_util::opt_number")]
    pub size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub annotations: Option<ContentAnnotations>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A family of resources, addressed by an RFC 6570 URI template, from `resources/templates/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTemplate {
    pub uri_template: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub annotations: Option<ContentAnnotations>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    pub resources: Vec<Resource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourceTemplatesResult {
    pub resource_templates: Vec<ResourceTemplate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An argument a prompt accepts (`prompts/list`). Not part of the TypeScript package's typed API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub required: Option<bool>,
}

/// A prompt a server lists in `prompts/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub arguments: Option<Vec<PromptArgument>>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsResult {
    pub prompts: Vec<Prompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptMessage {
    /// `"user"` or `"assistant"`.
    pub role: String,
    pub content: ContentBlock,
}

/// Result of `prompts/get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetPromptResult {
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
