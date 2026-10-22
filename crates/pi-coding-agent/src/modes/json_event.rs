//! Port of `packages/coding-agent/src/modes/json-event.ts`.

use crate::core::agent_session::AgentSessionEvent;
use pi_agent::types::AgentMessage;
use pi_ai::types::{AssistantContent, AssistantMessageEvent, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An assistant message event without its cumulative `partial` snapshot (JSON object).
pub type WithoutPartial = Value;

/// [`WithoutPartial`], plus `id` and `toolName` for `toolcall_start`.
pub type ToJsonAssistantMessageEvent = Value;

pub type MessageUpdateEvent = AgentSessionEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonMessageUpdateEvent {
    /// Always "message_update".
    pub r#type: String,
    pub usage: Usage,
    pub assistant_message_event: ToJsonAssistantMessageEvent,
}

/// Session event shape emitted by the JSON and RPC stdout protocols.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonAgentSessionEvent {
    JsonMessageUpdateEvent(JsonMessageUpdateEvent),
    AgentSessionEvent(AgentSessionEvent),
    /// A wire event this build cannot type (only produced when deserializing, e.g. by `RpcClient`).
    Other(Value),
}

impl JsonAgentSessionEvent {
    /// The `type` discriminant.
    pub fn type_name(&self) -> String {
        match self {
            Self::JsonMessageUpdateEvent(_) => "message_update".to_string(),
            Self::AgentSessionEvent(event) => serde_json::to_value(event)
                .ok()
                .and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default(),
            Self::Other(value) => value.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
        }
    }
}

fn to_json_assistant_message_event(event: &AssistantMessageEvent) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(event)?;
    let Some(object) = value.as_object_mut() else {
        return Ok(value);
    };
    if let AssistantMessageEvent::ToolCallStart { content_index, partial } = event {
        let Some(AssistantContent::ToolCall(tool_call)) = partial.content.get(*content_index) else {
            anyhow::bail!("toolcall_start content at index {content_index} is not a tool call");
        };
        object.shift_remove("partial");
        object.insert("id".to_string(), Value::String(tool_call.id.clone()));
        object.insert("toolName".to_string(), Value::String(tool_call.name.clone()));
        return Ok(value);
    }
    object.shift_remove("partial");
    Ok(value)
}

/// Remove cumulative assistant snapshots from streaming wire events.
/// `message_start` provides the initial message, deltas build it, and
/// `message_end` provides the final authoritative message. Cumulative usage,
/// tool-call ids, and tool names remain available because their size is constant.
pub fn to_json_event(event: &AgentSessionEvent) -> anyhow::Result<JsonAgentSessionEvent> {
    let AgentSessionEvent::MessageUpdate { message, assistant_message_event } = event else {
        return Ok(JsonAgentSessionEvent::AgentSessionEvent(event.clone()));
    };
    let AgentMessage::Assistant(assistant) = message else {
        anyhow::bail!("message_update message is not an assistant message");
    };
    Ok(JsonAgentSessionEvent::JsonMessageUpdateEvent(JsonMessageUpdateEvent {
        r#type: "message_update".to_string(),
        usage: assistant.usage.clone(),
        assistant_message_event: to_json_assistant_message_event(assistant_message_event)?,
    }))
}

/// Serialize the wire form of an event to JSON.
pub fn to_json_event_value(event: &AgentSessionEvent) -> anyhow::Result<Value> {
    Ok(serde_json::to_value(to_json_event(event)?)?)
}
