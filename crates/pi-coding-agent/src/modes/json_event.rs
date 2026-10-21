//! Port of `packages/coding-agent/src/modes/json-event.ts`.

use crate::core::agent_session::AgentSessionEvent;
use pi_ai::types::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type WithoutPartial = Value;

pub type ToJsonAssistantMessageEvent = Value;

pub type MessageUpdateEvent = AgentSessionEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    AgentSessionEvent(AgentSessionEvent),
    JsonMessageUpdateEvent(JsonMessageUpdateEvent),
}

pub fn to_json_event(event: &AgentSessionEvent) -> anyhow::Result<JsonAgentSessionEvent> {
    todo!("port modes/json-event.ts:toJsonEvent")
}
