use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::serde_util::{lenient, present};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentAnnotations {
    /// `"user"` and/or `"assistant"`.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub audience: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::serde_util::opt_number")]
    pub priority: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextContent {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub annotations: Option<ContentAnnotations>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    pub data: String,
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub annotations: Option<ContentAnnotations>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
}

pub type AudioContent = ImageContent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLinkContent {
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextResourceContents {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub mime_type: Option<String>,
    pub text: String,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobResourceContents {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub mime_type: Option<String>,
    pub blob: String,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
}

/// `TextResourceContents | BlobResourceContents`; text wins when both are present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResourceContents {
    Text(TextResourceContents),
    Blob(BlobResourceContents),
}

impl ResourceContents {
    pub fn uri(&self) -> &str {
        match self {
            ResourceContents::Text(contents) => &contents.uri,
            ResourceContents::Blob(contents) => &contents.uri,
        }
    }

    pub fn mime_type(&self) -> Option<&str> {
        match self {
            ResourceContents::Text(contents) => contents.mime_type.as_deref(),
            ResourceContents::Blob(contents) => contents.mime_type.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddedResourceContent {
    pub resource: ResourceContents,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub annotations: Option<ContentAnnotations>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
}

/// A content block of a tool result or prompt message. Blocks of unknown types, or malformed
/// known ones, are kept as [`ContentBlock::Unknown`] with their JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(TextContent),
    Image(ImageContent),
    Audio(AudioContent),
    ResourceLink(ResourceLinkContent),
    Resource(EmbeddedResourceContent),
    Unknown(Value),
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text(TextContent { text: text.into(), annotations: None, meta: None })
    }

    /// The `type` discriminant.
    pub fn type_name(&self) -> String {
        match self {
            ContentBlock::Text(_) => "text".into(),
            ContentBlock::Image(_) => "image".into(),
            ContentBlock::Audio(_) => "audio".into(),
            ContentBlock::ResourceLink(_) => "resource_link".into(),
            ContentBlock::Resource(_) => "resource".into(),
            ContentBlock::Unknown(value) => match value.get("type") {
                Some(Value::String(kind)) => kind.clone(),
                Some(other) => other.to_string(),
                None => "undefined".into(),
            },
        }
    }

    pub fn from_value(value: Value) -> Self {
        fn parse<T: serde::de::DeserializeOwned>(value: &Value, wrap: fn(T) -> ContentBlock) -> Option<ContentBlock> {
            serde_json::from_value::<T>(value.clone()).ok().map(wrap)
        }
        let parsed = match value.get("type").and_then(Value::as_str) {
            Some("text") => parse(&value, ContentBlock::Text),
            Some("image") => parse(&value, ContentBlock::Image),
            Some("audio") => parse(&value, ContentBlock::Audio),
            Some("resource_link") => parse(&value, ContentBlock::ResourceLink),
            Some("resource") => parse(&value, ContentBlock::Resource),
            _ => None,
        };
        parsed.unwrap_or(ContentBlock::Unknown(value))
    }

    pub fn to_value(&self) -> Value {
        fn tagged<T: Serialize>(kind: &str, block: &T) -> Value {
            let mut object = Map::new();
            object.insert("type".into(), Value::String(kind.into()));
            if let Ok(Value::Object(fields)) = serde_json::to_value(block) {
                object.extend(fields);
            }
            Value::Object(object)
        }
        match self {
            ContentBlock::Text(block) => tagged("text", block),
            ContentBlock::Image(block) => tagged("image", block),
            ContentBlock::Audio(block) => tagged("audio", block),
            ContentBlock::ResourceLink(block) => tagged("resource_link", block),
            ContentBlock::Resource(block) => tagged("resource", block),
            ContentBlock::Unknown(value) => value.clone(),
        }
    }
}

impl Serialize for ContentBlock {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Value::deserialize(deserializer).map(ContentBlock::from_value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    /// Any JSON value, including arrays and `null` (`Some(Value::Null)`); `None` when absent
    /// (MCP 2026-07-28; earlier revisions allowed only objects).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub structured_content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub is_error: Option<bool>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub meta: Option<Map<String, Value>>,
    /// Other fields of the result, passed through.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Tool result content in the shape LLM APIs accept: text and base64 images. Matches the
/// `TextContent` and `ImageContent` types of pi-ai.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LlmContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image", rename_all = "camelCase")]
    Image { data: String, mime_type: String },
}

fn block_to_llm_content(block: &ContentBlock) -> LlmContent {
    match block {
        ContentBlock::Text(block) => LlmContent::Text { text: block.text.clone() },
        ContentBlock::Image(block) => LlmContent::Image { data: block.data.clone(), mime_type: block.mime_type.clone() },
        ContentBlock::Audio(block) => LlmContent::Text { text: format!("[audio {} omitted]", block.mime_type) },
        ContentBlock::ResourceLink(block) => LlmContent::Text { text: format!("{}: {}", block.name, block.uri) },
        ContentBlock::Resource(block) => match &block.resource {
            ResourceContents::Text(resource) => LlmContent::Text { text: resource.text.clone() },
            ResourceContents::Blob(resource) => match &resource.mime_type {
                Some(mime_type) if mime_type.starts_with("image/") => {
                    LlmContent::Image { data: resource.blob.clone(), mime_type: mime_type.clone() }
                }
                mime_type => LlmContent::Text {
                    text: format!(
                        "[binary resource {} ({}) omitted]",
                        resource.uri,
                        mime_type.as_deref().unwrap_or("unknown type")
                    ),
                },
            },
        },
        ContentBlock::Unknown(_) => {
            LlmContent::Text { text: format!("[unsupported MCP content {}]", block.type_name()) }
        }
    }
}

/// `JSON.stringify(value, null, 2)`.
pub(crate) fn json_pretty(value: &Value) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    if value.serialize(&mut serializer).is_err() {
        return "null".into();
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Convert a tool result to text and image content for a model. Text and images pass through,
/// embedded text resources become text, embedded image resources become images, and other blocks
/// (audio, resource links, binary resources) become a short text placeholder. A result without
/// content blocks but with `structured_content` becomes its JSON, since servers should, but do not
/// always, mirror structured results as text.
pub fn to_llm_content(content: &[ContentBlock], structured_content: Option<&Value>) -> Vec<LlmContent> {
    let mut result: Vec<LlmContent> = content.iter().map(block_to_llm_content).collect();
    if result.is_empty()
        && let Some(structured) = structured_content
    {
        result.push(LlmContent::Text { text: json_pretty(structured) });
    }
    result
}

impl CallToolResult {
    /// [`to_llm_content`] of this result.
    pub fn to_llm_content(&self) -> Vec<LlmContent> {
        to_llm_content(&self.content, self.structured_content.as_ref())
    }
}
