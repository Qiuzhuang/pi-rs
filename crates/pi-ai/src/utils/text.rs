//! Text helpers (port of `utils/text.ts`).

use crate::types::{AssistantContent, SystemContent, SystemMessage, UserContent, UserContentBlock};

/// Extract and join text from user/tool-result content.
pub fn content_text(content: &UserContent, separator: &str) -> String {
    match content {
        UserContent::Text(t) => t.clone(),
        UserContent::Blocks(blocks) => blocks_text(blocks, separator),
    }
}

/// Join the text blocks of user/tool-result content blocks.
pub fn blocks_text(blocks: &[UserContentBlock], separator: &str) -> String {
    blocks.iter().filter_map(UserContentBlock::as_text).map(|t| t.text.as_str()).collect::<Vec<_>>().join(separator)
}

/// Join the text blocks of assistant content.
pub fn assistant_content_text(blocks: &[AssistantContent], separator: &str) -> String {
    blocks.iter().filter_map(AssistantContent::as_text).map(|t| t.text.as_str()).collect::<Vec<_>>().join(separator)
}

/// Text of a system message's content (blocks joined by "\n").
pub fn system_content_text(content: &SystemContent) -> String {
    match content {
        SystemContent::Text(t) => t.clone(),
        SystemContent::Blocks(blocks) => blocks.iter().map(|b| b.text()).collect::<Vec<_>>().join("\n"),
    }
}

/// Render a system message as a complete prompt: its content followed by its sections.
pub fn get_system_message_text(message: &SystemMessage) -> String {
    let mut parts = vec![system_content_text(&message.content)];
    if let Some(sections) = &message.sections {
        for text in sections.values().flatten() {
            parts.push(text.clone());
        }
    }
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// Render a later system message for APIs that accept system messages mid-conversation.
pub fn render_system_message_update(message: &SystemMessage) -> String {
    let mut parts = Vec::new();
    let text = system_content_text(&message.content);
    if !text.is_empty() {
        parts.push(text);
    }
    if let Some(sections) = &message.sections {
        for (name, value) in sections {
            parts.push(match value {
                None => format!("Removed system prompt section \"{name}\"."),
                Some(value) => format!("Updated system prompt section \"{name}\":\n\n{value}"),
            });
        }
    }
    parts.join("\n\n")
}
