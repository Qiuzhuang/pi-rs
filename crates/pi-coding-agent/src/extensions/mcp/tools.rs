//! Port of `packages/coding-agent/src/extensions/mcp/tools.ts`.
//!
//! Adapts MCP tools to pi tool definitions.
//!
//! Results map onto pi's model-facing content (text and images). Text over 20KB keeps its start and
//! end with the middle cut out, like Codex does, and the full text is saved to a temp file the model
//! can read. Binary resources other than images are saved to temp files too, and resource links name
//! the `read_mcp_resource` tool. Codemode scripts receive the whole `CallToolResult` without `_meta`
//! (`content` blocks as sent by the server, `structuredContent`, `isError`), never truncated: it is
//! the tool's `structuredContent`, and every MCP tool declares a `CallToolResult` output schema. MCP
//! errors (`isError`) are error results for the model, but scripts still resolve to the result.

use crate::core::extensions::types::ToolAnnotations;
use crate::core::extensions::types::ToolDefinition;
use crate::core::extensions::types::ToolExposure;
use crate::core::extensions::types::ToolNamespace;
use crate::core::extensions::types::ToolRenderers;
use crate::core::mcp_servers::McpExposure;
pub use crate::core::mcp_servers::READ_MCP_RESOURCE_TOOL;
use crate::core::tools::render_utils::{
    GetTextOutputResult, format_tool_call_with_args, get_text_output, replace_tabs,
};
use crate::core::tools::truncate::{format_size, truncate_middle};
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::components::visual_truncate::{
    TruncateToVisualLinesKeep, VisualLinePreview, VisualLinePreviewOptions,
};
use crate::modes::interactive::theme::theme::ThemeColor;
use crate::utils::output_files::{WriteOutputFileData, write_output_file};
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_agent::types::AgentToolResult;
use pi_ai::types::{ImageContent, UserContentBlock};
use pi_mcp::client::McpRequestOptions;
use pi_mcp::protocol::content::CallToolResult;
use pi_mcp::protocol::content::ContentBlock;
use pi_mcp::protocol::content::{LlmContent, ResourceContents, to_llm_content};
use pi_mcp::protocol::types::Tool;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::{ComponentRef, Container, downcast_component};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Tool exposure of an MCP exposure. `codemode` and `deferred` both leave tools out of the codemode
/// description; they differ only in which tool the MCP extension activates to reach them.
pub fn to_tool_exposure(exposure: &McpExposure) -> ToolExposure {
    match exposure {
        McpExposure::Codemode | McpExposure::Deferred => ToolExposure::Deferred,
        McpExposure::Direct => ToolExposure::Direct,
        McpExposure::Hidden => ToolExposure::Hidden,
    }
}

/// Provider tool names are limited to 64 characters of `[A-Za-z0-9_-]`.
const MAX_TOOL_NAME_LENGTH: usize = 64;
/// Model-facing text of an MCP result beyond this is cut in the middle.
pub const MCP_OUTPUT_MAX_BYTES: i64 = 20480;
/// Visual (wrapped) result lines shown before the output is expanded.
const OUTPUT_PREVIEW_LINES: i64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDetails {
    pub server: String,
    pub tool: String,
    /// Temp file with the full text output, when the model-facing text was truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

impl McpToolDetails {
    pub(crate) fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

pub type SaveToTempFileData = McpOutputSaverData;

pub async fn save_to_temp_file(
    data: &SaveToTempFileData,
    extension: &str,
) -> anyhow::Result<String> {
    let data = match data {
        McpOutputSaverData::String(text) => WriteOutputFileData::String(text.clone()),
        McpOutputSaverData::List(bytes) => WriteOutputFileData::List(bytes.clone()),
    };
    write_output_file("pi-mcp", extension, &data).await
}

/// The default [`McpOutputSaver`]: a temp file.
fn default_saver() -> McpOutputSaver {
    Arc::new(|data, extension| Box::pin(async move { save_to_temp_file(&data, &extension).await }))
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
    create_mcp_tool_name_with(server, tool, |name| {
        is_taken.as_ref().is_some_and(|taken| taken(name.to_string()))
    })
}

/// [`create_mcp_tool_name`] with a borrowed predicate.
pub(crate) fn create_mcp_tool_name_with(
    server: &str,
    tool: &str,
    is_taken: impl Fn(&str) -> bool,
) -> String {
    // Like the JavaScript regex, a character outside the BMP is two UTF-16 units, each replaced.
    let mut name = String::new();
    for c in format!("mcp__{server}__{tool}").chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                name.push('_');
            }
        }
    }
    if name.len() <= MAX_TOOL_NAME_LENGTH && !is_taken(&name) {
        return name;
    }
    let digest = Sha256::digest(format!("{server}\0{tool}").as_bytes());
    let hash: String = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()[..8]
        .to_string();
    let keep = (MAX_TOOL_NAME_LENGTH - hash.len() - 1).min(name.len());
    format!("{}_{hash}", &name[..keep])
}

fn text_of(content: &[UserContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// MCP content blocks as one compact type with the fields of text and image blocks, which scripts
/// read most (`image(block)` takes an image block as is). Rendered into every MCP tool's declaration.
fn content_block_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": { "enum": ["text", "image", "audio", "resource_link", "resource"] },
            "text": { "type": "string" },
            "data": { "type": "string" },
            "mimeType": { "type": "string" },
        },
        "required": ["type"],
        "additionalProperties": true,
    })
}

/// Keywords whose values are data, so a `$ref` key in them is not a reference.
const SCHEMA_DATA_KEYWORDS: [&str; 4] = ["const", "enum", "default", "examples"];
/// Keywords whose values map names to schemas, so their keys are names, not keywords.
const SCHEMA_MAP_KEYWORDS: [&str; 6] = [
    "properties",
    "patternProperties",
    "dependentSchemas",
    "dependencies",
    "$defs",
    "definitions",
];

/// `schema` as a subschema at the JSON pointer `pointer` (`#/properties/structuredContent`) of
/// another schema: its local references (`#`, `#/$defs/Item`) are rewritten to point below
/// `pointer`, so they resolve to the same subschemas as before.
fn relocate_schema(schema: &Value, pointer: &str) -> Value {
    match schema {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| relocate_schema(item, pointer))
                .collect(),
        ),
        Value::Object(object) => {
            let mut relocated = Map::new();
            for (key, value) in object {
                let next = match value {
                    Value::String(reference)
                        if key == "$ref" && (reference == "#" || reference.starts_with("#/")) =>
                    {
                        Value::String(format!("{pointer}{}", &reference[1..]))
                    }
                    _ if SCHEMA_DATA_KEYWORDS.contains(&key.as_str()) => value.clone(),
                    Value::Object(map) if SCHEMA_MAP_KEYWORDS.contains(&key.as_str()) => {
                        Value::Object(
                            map.iter()
                                .map(|(name, subschema)| {
                                    (name.clone(), relocate_schema(subschema, pointer))
                                })
                                .collect(),
                        )
                    }
                    _ => relocate_schema(value, pointer),
                };
                relocated.insert(key.clone(), next);
            }
            Value::Object(relocated)
        }
        other => other.clone(),
    }
}

/// Output schema of every MCP tool: the `CallToolResult` scripts receive (without `_meta`, which
/// {@link convertMcpResult} drops), with the tool's own output schema as `structuredContent`.
pub fn create_mcp_result_schema(
    structured_content_schema: Option<IndexMap<String, Value>>,
) -> Value {
    let structured = match structured_content_schema {
        Some(schema) => relocate_schema(
            &Value::Object(schema.into_iter().collect()),
            "#/properties/structuredContent",
        ),
        // Servers may send structured content without declaring a schema for it, and it may be any JSON value.
        None => json!({}),
    };
    json!({
        "type": "object",
        "properties": {
            "content": { "type": "array", "items": content_block_schema() },
            "structuredContent": structured,
            "isError": { "type": "boolean" },
        },
        "required": ["content"],
        // Fields newer protocol versions add.
        "additionalProperties": true,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitMcpContentResult {
    pub content: Vec<UserContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

/// Keep model-facing text within {@link MCP_OUTPUT_MAX_BYTES}. Longer text becomes one text block in
/// Codex's truncation format, followed by the path of the file with the full text; images follow it.
pub async fn limit_mcp_content(
    content: &[UserContentBlock],
    save_output: Option<&McpOutputSaver>,
) -> anyhow::Result<LimitMcpContentResult> {
    let combined = text_of(content);
    let truncation = truncate_middle(&combined, MCP_OUTPUT_MAX_BYTES);
    if !truncation.truncated {
        return Ok(LimitMcpContentResult {
            content: content.to_vec(),
            full_output_path: None,
        });
    }
    let saver = save_output.cloned().unwrap_or_else(default_saver);
    let mut full_output_path = None;
    let where_ = match saver(McpOutputSaverData::String(combined), ".txt".to_string()).await {
        Ok(path) => {
            let where_ = format!("[Full output: {path} (read it with offset/limit)]");
            full_output_path = Some(path);
            where_
        }
        Err(error) => format!("[Could not save the full output: {error}]"),
    };
    let tokens = (truncation.total_bytes as f64 / 4.0).ceil() as i64;
    let text = format!(
        "Warning: truncated output (original token count: {tokens})\nTotal output lines: {}\n\n{}\n\n{where_}",
        truncation.total_lines, truncation.content
    );
    let mut limited = vec![UserContentBlock::text(text)];
    limited.extend(
        content
            .iter()
            .filter(|block| matches!(block, UserContentBlock::Image(_)))
            .cloned(),
    );
    Ok(LimitMcpContentResult {
        content: limited,
        full_output_path: full_output_path.filter(|path| !path.is_empty()),
    })
}

#[derive(Clone, Default)]
pub struct ConvertMcpResultOptions {
    /// Saves truncated text and binary resources. Default: a temp file.
    pub save_output: Option<McpOutputSaver>,
    /// Whether the server's resources can be read with `read_mcp_resource`, which resource links then name.
    pub readable_resources: Option<bool>,
}

/// File extension for a saved binary resource: the one its URI ends in, else `.bin`.
fn extension_of(uri: &str) -> String {
    let path = match url::Url::parse(uri) {
        Ok(url) => url.path().to_string(),
        Err(_) => uri.to_string(),
    };
    let bytes = path.as_bytes();
    let mut start = bytes.len();
    while start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        start -= 1;
    }
    let length = bytes.len() - start;
    if (1..=8).contains(&length) && start > 0 && bytes[start - 1] == b'.' {
        return path[start - 1..].to_string();
    }
    ".bin".to_string()
}

/// Blobs of these types are shown as text.
fn is_text_mime_type(mime_type: Option<&str>) -> bool {
    let Some(mime_type) = mime_type.filter(|mime_type| !mime_type.is_empty()) else {
        return false;
    };
    let kind = mime_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    kind.starts_with("text/")
        || kind == "application/json"
        || kind.ends_with("+json")
        || kind.ends_with("+xml")
}

/// `Buffer.from(value, "base64")`: lenient about padding, whitespace, and URL-safe characters.
pub(crate) fn decode_base64_lenient(value: &str) -> Vec<u8> {
    use base64::Engine;
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    let cleaned: String = value
        .chars()
        .filter_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' => Some(c),
            '-' => Some('+'),
            '_' => Some('/'),
            _ => None,
        })
        .collect();
    // A single leftover character carries no full byte.
    let usable = if cleaned.len() % 4 == 1 {
        &cleaned[..cleaned.len() - 1]
    } else {
        &cleaned[..]
    };
    let engine = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    engine.decode(usable).unwrap_or_default()
}

pub(crate) fn llm_to_user_content(content: Vec<LlmContent>) -> Vec<UserContentBlock> {
    content
        .into_iter()
        .map(|block| match block {
            LlmContent::Text { text } => UserContentBlock::text(text),
            LlmContent::Image { data, mime_type } => {
                UserContentBlock::Image(ImageContent { data, mime_type })
            }
        })
        .collect()
}

/// `String(number)` for the numbers MCP sends (sizes, progress).
pub(crate) fn js_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Model-facing content of one block of `server`'s result.
async fn block_to_content(
    server: &str,
    block: &ContentBlock,
    options: &ConvertMcpResultOptions,
) -> Vec<UserContentBlock> {
    if let ContentBlock::ResourceLink(link) = block {
        let mut details: Vec<String> = Vec::new();
        if let Some(mime_type) = link.mime_type.as_deref().filter(|m| !m.is_empty()) {
            details.push(mime_type.to_string());
        }
        if let Some(size) = link.size {
            details.push(format_size(size as i64));
        }
        let read = if options.readable_resources.unwrap_or(false) {
            format!(". Read it with {READ_MCP_RESOURCE_TOOL} (server \"{server}\")")
        } else {
            String::new()
        };
        let description = match link.description.as_deref() {
            Some(description) if !description.is_empty() => format!(": {description}"),
            _ => String::new(),
        };
        let details = if details.is_empty() {
            String::new()
        } else {
            format!(" ({})", details.join(", "))
        };
        return vec![UserContentBlock::text(format!(
            "[Resource {} \"{}\"{details}{description}{read}]",
            link.uri,
            link.title.as_deref().unwrap_or(&link.name)
        ))];
    }
    if let ContentBlock::Resource(embedded) = block
        && let ResourceContents::Blob(resource) = &embedded.resource
        && !resource
            .mime_type
            .as_deref()
            .is_some_and(|mime_type| mime_type.starts_with("image/"))
    {
        let data = decode_base64_lenient(&resource.blob);
        if is_text_mime_type(resource.mime_type.as_deref()) {
            return vec![UserContentBlock::text(
                String::from_utf8_lossy(&data).into_owned(),
            )];
        }
        let kind = format!(
            "{}, {}",
            resource.mime_type.as_deref().unwrap_or("unknown type"),
            format_size(data.len() as i64)
        );
        let saver = options.save_output.clone().unwrap_or_else(default_saver);
        return match saver(McpOutputSaverData::List(data), extension_of(&resource.uri)).await {
            Ok(path) => vec![UserContentBlock::text(format!(
                "[Binary resource {} ({kind}) saved to {path}]",
                resource.uri
            ))],
            Err(error) => vec![UserContentBlock::text(format!(
                "[Binary resource {} ({kind}) could not be saved: {error}]",
                resource.uri
            ))],
        };
    }
    llm_to_user_content(to_llm_content(std::slice::from_ref(block), None))
}

/// Model-facing content of `server`'s content blocks, before the output limit.
pub async fn to_model_content(
    server: &str,
    blocks: &[ContentBlock],
    options: Option<&ConvertMcpResultOptions>,
) -> anyhow::Result<Vec<UserContentBlock>> {
    let default = ConvertMcpResultOptions::default();
    let options = options.unwrap_or(&default);
    let converted = futures::future::join_all(
        blocks
            .iter()
            .map(|block| block_to_content(server, block, options)),
    )
    .await;
    Ok(converted.into_iter().flatten().collect())
}

/// The result scripts receive: the `CallToolResult` without `_meta`.
fn script_result(result: &CallToolResult) -> Value {
    let mut value = serde_json::to_value(result).unwrap_or(Value::Null);
    if let Value::Object(object) = &mut value {
        object.shift_remove("_meta");
    }
    value
}

/// Convert an MCP result. `isError` results become error results that keep the structured result.
pub async fn convert_mcp_result(
    server: &str,
    tool: &str,
    result: CallToolResult,
    options: Option<&ConvertMcpResultOptions>,
) -> anyhow::Result<AgentToolResult> {
    // Without content blocks, toLlmContent falls back to the structured content as JSON.
    let mut converted = if !result.content.is_empty() {
        to_model_content(server, &result.content, options).await?
    } else {
        llm_to_user_content(result.to_llm_content())
    };
    let is_error = result.is_error == Some(true);
    if is_error && text_of(&converted).is_empty() {
        converted.push(UserContentBlock::text(format!(
            "MCP tool {server}/{tool} returned an error"
        )));
    }
    let limited = limit_mcp_content(
        &converted,
        options.and_then(|options| options.save_output.as_ref()),
    )
    .await?;
    let details = McpToolDetails {
        server: server.to_string(),
        tool: tool.to_string(),
        full_output_path: limited.full_output_path,
    };
    Ok(AgentToolResult {
        content: limited.content,
        details: Some(details.to_value()),
        structured_content: Some(script_result(&result)),
        is_error: is_error.then_some(true),
        ..Default::default()
    })
}

/// Tool input schemas must be objects. MCP servers may omit `type`, and some providers reject object
/// schemas without `properties`.
fn to_parameters(schema: &Map<String, Value>) -> Value {
    let mut parameters = schema.clone();
    if parameters.get("type").is_none_or(Value::is_null) {
        parameters.insert("type".into(), Value::String("object".into()));
    }
    if !parameters.contains_key("properties") {
        parameters.insert("properties".into(), Value::Object(Map::new()));
    }
    Value::Object(parameters)
}

/// The boolean hints of an MCP tool's annotations, or undefined when it has none.
fn to_tool_annotations(tool: &Tool) -> Option<ToolAnnotations> {
    let hints = tool.annotations.as_ref()?;
    let annotations = ToolAnnotations {
        read_only_hint: hints.read_only_hint,
        destructive_hint: hints.destructive_hint,
        idempotent_hint: hints.idempotent_hint,
        open_world_hint: hints.open_world_hint,
    };
    (annotations != ToolAnnotations::default()).then_some(annotations)
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

pub fn create_mcp_tool_definition(options: &CreateMcpToolDefinitionOptions) -> ToolDefinition {
    let server = options.server.clone();
    let tool = options.tool.clone();
    let title = tool.title.clone().or_else(|| {
        tool.annotations
            .as_ref()
            .and_then(|annotations| annotations.title.clone())
    });
    let annotations = to_tool_annotations(&tool);
    let label = format!("{server}/{}", tool.name);
    let description = match tool.description.as_deref().map(str::trim) {
        Some(description) if !description.is_empty() => description.to_string(),
        _ => match title.as_deref() {
            Some(title) if !title.is_empty() => title.to_string(),
            _ => format!("MCP tool {} from server {server}", tool.name),
        },
    };
    let parameters = to_parameters(&tool.input_schema);
    let output_schema = create_mcp_result_schema(
        tool.output_schema
            .clone()
            .map(|schema| schema.into_iter().collect()),
    );
    let get_client = options.get_client.clone();
    let readable_resources = options.readable_resources.clone();
    let timeout_ms = options.timeout_ms;
    let execute_server = server.clone();
    let tool_name = tool.name.clone();
    let mut definition = ToolDefinition::new(
        options.name.clone(),
        label.clone(),
        description,
        parameters,
        move |_tool_call_id, params, signal, on_update, _ctx| {
            let get_client = get_client.clone();
            let readable_resources = readable_resources.clone();
            let server = execute_server.clone();
            let tool_name = tool_name.clone();
            async move {
                let client = get_client().await?;
                let args: IndexMap<String, Value> = match params {
                    Value::Object(object) => object.into_iter().collect(),
                    _ => IndexMap::new(),
                };
                let progress_server = server.clone();
                let progress_tool = tool_name.clone();
                let mut request = McpRequestOptions {
                    signal: signal.map(pi_mcp::AbortSignal::from),
                    timeout_ms: Some(timeout_ms.max(0) as u64),
                    on_progress: None,
                };
                if let Some(on_update) = on_update {
                    request = request.on_progress(move |progress| {
                        let total = progress
                            .total
                            .map(|total| format!("/{}", js_number(total)))
                            .unwrap_or_default();
                        let text = progress.message.clone().unwrap_or_else(|| {
                            format!("Progress {}{total}", js_number(progress.progress))
                        });
                        on_update(AgentToolResult {
                            content: vec![UserContentBlock::text(text)],
                            details: Some(json!({
                                "server": progress_server,
                                "tool": progress_tool,
                            })),
                            ..Default::default()
                        });
                    });
                }
                let result = client.call_tool(&tool_name, args, request).await?;
                convert_mcp_result(
                    &server,
                    &tool_name,
                    result,
                    Some(&ConvertMcpResultOptions {
                        save_output: None,
                        readable_resources: readable_resources.map(|readable| readable()),
                    }),
                )
                .await
            }
        },
    );
    definition.output_schema = Some(output_schema);
    definition.exposure = Some(to_tool_exposure(&options.exposure));
    definition.namespace = Some(options.namespace.clone());
    definition.annotations = annotations;
    let renderers = create_mcp_tool_renderers(&label);
    definition.render_shell = renderers.render_shell;
    definition.render_call = renderers.render_call;
    definition.render_result = renderers.render_result;
    definition
}

/// Renderers of calls to an MCP tool, labeled `server/tool`, also used before the tool is registered.
pub fn create_mcp_tool_renderers(label: &str) -> ToolRenderers {
    let call_label = label.to_string();
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(move |args, theme, context| {
            let component = context
                .last_component
                .as_ref()
                .and_then(downcast_component::<Text>)
                .unwrap_or_else(|| Arc::new(Text::new("", 0, 0, None)));
            component.set_text(format_tool_call_with_args(
                &call_label,
                args.clone(),
                theme.clone(),
                context.expanded,
            ));
            component as ComponentRef
        })),
        render_result: Some(Arc::new(move |result, options, theme, context| {
            let component = context
                .last_component
                .as_ref()
                .and_then(downcast_component::<Container>)
                .unwrap_or_else(|| Arc::new(Container::new()));
            component.clear();
            let text_result: Option<GetTextOutputResult> = serde_json::to_value(result)
                .ok()
                .and_then(|value| serde_json::from_value(value).ok());
            let output = get_text_output(text_result.as_ref(), context.show_images)
                .trim()
                .to_string();
            if output.is_empty() {
                return component as ComponentRef;
            }
            let color = if context.is_error {
                ThemeColor::Error
            } else {
                ThemeColor::ToolOutput
            };
            let styled = replace_tabs(&output)
                .split('\n')
                .map(|line| theme.fg(&color, line))
                .collect::<Vec<_>>()
                .join("\n");
            component.add_child(Arc::new(Spacer::new(1)));
            if options.expanded {
                component.add_child(Arc::new(Text::new(styled, 0, 0, None)));
            } else {
                // Limit wrapped lines, not logical ones: MCP results are often one long JSON line.
                let hint_theme = theme.clone();
                component.add_child(Arc::new(VisualLinePreview::new(
                    &VisualLinePreviewOptions {
                        text: styled,
                        max_visual_lines: OUTPUT_PREVIEW_LINES,
                        keep: TruncateToVisualLinesKeep::Start,
                        format_hint: Arc::new(move |hidden| {
                            format!(
                                "{} {}{}",
                                hint_theme.fg(
                                    &ThemeColor::Muted,
                                    &format!("... ({} more lines,", js_number(hidden))
                                ),
                                key_hint("app.tools.expand".to_string(), "to expand"),
                                hint_theme.fg(&ThemeColor::Muted, ")")
                            )
                        }),
                    },
                )));
                let full_output_path = result
                    .details
                    .as_ref()
                    .and_then(|details| details.get("fullOutputPath"))
                    .and_then(Value::as_str);
                if let Some(path) = full_output_path.filter(|path| !path.is_empty()) {
                    component.add_child(Arc::new(Text::new(
                        theme.fg(&ThemeColor::Muted, &format!("Full output: {path}")),
                        0,
                        0,
                        None,
                    )));
                }
            }
            component as ComponentRef
        })),
    }
}
