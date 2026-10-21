//! Port of `packages/coding-agent/src/modes/interactive/components/markdown-transform.ts`.

use crate::core::extensions::types::MarkdownTransformer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CreateMarkdownTransformMessageType {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "assistant-thinking")]
    AssistantThinking,
}

impl CreateMarkdownTransformMessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::AssistantThinking => "assistant-thinking",
        }
    }
}

pub fn create_markdown_transform(
    message_type: CreateMarkdownTransformMessageType,
    is_streaming: bool,
    transformers: &[MarkdownTransformer],
) -> Arc<dyn Fn(String, i64) -> String + Send + Sync> {
    todo!("port modes/interactive/components/markdown-transform.ts:createMarkdownTransform")
}
