//! Context token estimation (port of `utils/estimate.ts`).

use super::text::get_system_message_text;
use super::js_len;
use crate::types::{AssistantContent, Message, Tool, ToolReference, UserContent, UserContentBlock, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsageEstimate {
    /// Estimated total context tokens.
    pub tokens: u64,
    /// Tokens reported by the most recent applicable assistant usage block.
    pub usage_tokens: u64,
    /// Estimated tokens after the most recent applicable assistant usage block.
    pub trailing_tokens: u64,
    /// Index of the applicable message that provided usage.
    pub last_usage_index: Option<usize>,
}

const CHARS_PER_TOKEN: f64 = 3.5;
const ESTIMATED_IMAGE_CHARS: usize = 4800;

pub fn calculate_context_tokens(usage: &Usage) -> u64 {
    if usage.total_tokens != 0 {
        usage.total_tokens
    } else {
        usage.input + usage.output + usage.cache_read + usage.cache_write
    }
}

fn chars_to_tokens(chars: usize) -> u64 {
    (chars as f64 / CHARS_PER_TOKEN).ceil() as u64
}

fn blocks_chars(blocks: &[UserContentBlock]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            UserContentBlock::Text(t) => js_len(&t.text),
            UserContentBlock::Image(_) => ESTIMATED_IMAGE_CHARS,
        })
        .sum()
}

pub fn estimate_text_tokens(text: &str) -> u64 {
    chars_to_tokens(js_len(text))
}

pub fn estimate_text_and_image_content_tokens(content: &UserContent) -> u64 {
    match content {
        UserContent::Text(t) => chars_to_tokens(js_len(t)),
        UserContent::Blocks(blocks) => chars_to_tokens(blocks_chars(blocks)),
    }
}

pub fn estimate_blocks_tokens(blocks: &[UserContentBlock]) -> u64 {
    chars_to_tokens(blocks_chars(blocks))
}

fn estimate_tools_tokens(tools: Option<&Vec<Tool>>) -> u64 {
    match tools {
        Some(tools) if !tools.is_empty() => estimate_text_tokens(&serde_json::to_string(tools).unwrap_or_default()),
        _ => 0,
    }
}

fn estimate_tool_refs_tokens(tools: Option<&Vec<ToolReference>>) -> u64 {
    match tools {
        Some(tools) if !tools.is_empty() => estimate_text_tokens(&serde_json::to_string(tools).unwrap_or_default()),
        _ => 0,
    }
}

pub fn estimate_message_tokens(message: &Message) -> u64 {
    match message {
        Message::System(m) => {
            estimate_text_tokens(&get_system_message_text(m))
                + estimate_tools_tokens(m.tools_added.as_ref())
                + estimate_tool_refs_tokens(m.tools_removed.as_ref())
        }
        Message::User(m) => estimate_text_and_image_content_tokens(&m.content),
        Message::ToolResult(m) => estimate_blocks_tokens(&m.content),
        Message::Assistant(m) => {
            let mut chars = 0usize;
            for block in &m.content {
                chars += match block {
                    AssistantContent::Text(t) => js_len(&t.text),
                    AssistantContent::Thinking(t) => js_len(&t.thinking),
                    AssistantContent::ToolCall(c) => {
                        js_len(&c.name) + js_len(&serde_json::to_string(&c.arguments).unwrap_or_default())
                    }
                };
            }
            chars_to_tokens(chars)
        }
    }
}

fn get_last_assistant_usage_info(messages: &[Message]) -> Option<(&Usage, usize)> {
    let mut latest_prefix_timestamp = i64::MIN;
    let mut info = None;
    for (i, message) in messages.iter().enumerate() {
        if let Message::Assistant(assistant) = message {
            let applies = assistant.timestamp >= latest_prefix_timestamp;
            if applies
                && !matches!(assistant.stop_reason, crate::types::StopReason::Aborted | crate::types::StopReason::Error)
                && calculate_context_tokens(&assistant.usage) > 0
            {
                info = Some((&assistant.usage, i));
            }
        }
        latest_prefix_timestamp = latest_prefix_timestamp.max(message.timestamp());
    }
    info
}

pub fn estimate_context_tokens(messages: &[Message]) -> ContextUsageEstimate {
    if let Some((usage, index)) = get_last_assistant_usage_info(messages) {
        let usage_tokens = calculate_context_tokens(usage);
        let trailing_tokens: u64 = messages[index + 1..].iter().map(estimate_message_tokens).sum();
        return ContextUsageEstimate {
            tokens: usage_tokens + trailing_tokens,
            usage_tokens,
            trailing_tokens,
            last_usage_index: Some(index),
        };
    }
    let tokens: u64 = messages.iter().map(estimate_message_tokens).sum();
    ContextUsageEstimate { tokens, usage_tokens: 0, trailing_tokens: tokens, last_usage_index: None }
}
