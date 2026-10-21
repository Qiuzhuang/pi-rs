//! GitHub Copilot dynamic request headers (port of `api/github-copilot-headers.ts`).

use indexmap::IndexMap;

use crate::types::{Message, UserContent};

/// Whether the request is user-initiated or agent-initiated.
pub fn infer_copilot_initiator(messages: &[Message]) -> &'static str {
    match messages.last() {
        Some(last) if !matches!(last, Message::User(_)) => "agent",
        _ => "user",
    }
}

/// Whether the transcript contains image input (Copilot-Vision-Request).
pub fn has_copilot_vision_input(messages: &[Message]) -> bool {
    messages.iter().any(|msg| match msg {
        Message::User(user) => matches!(&user.content, UserContent::Blocks(b) if b.iter().any(|c| c.as_image().is_some())),
        Message::ToolResult(result) => result.content.iter().any(|c| c.as_image().is_some()),
        _ => false,
    })
}

pub fn build_copilot_dynamic_headers(messages: &[Message], has_images: bool) -> IndexMap<String, String> {
    let mut headers = IndexMap::new();
    headers.insert("X-Initiator".to_string(), infer_copilot_initiator(messages).to_string());
    headers.insert("Openai-Intent".to_string(), "conversation-edits".to_string());
    if has_images {
        headers.insert("Copilot-Vision-Request".to_string(), "true".to_string());
    }
    headers
}
