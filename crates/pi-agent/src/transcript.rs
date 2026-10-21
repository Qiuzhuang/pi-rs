//! Transcript replay helpers over [`AgentMessage`]s.
//!
//! pi-ai's helpers only read system messages, so agent transcripts that carry custom
//! roles are reduced to their system messages before delegating.

use pi_ai::utils::transcript as ai;
use pi_ai::{Message, SystemMessage, Tool};

use crate::types::AgentMessage;

fn system_messages(messages: &[AgentMessage]) -> Vec<Message> {
    messages
        .iter()
        .filter_map(|m| m.as_system().cloned().map(Message::System))
        .collect()
}

/// Resolve the tools available after applying every transcript delta in order.
pub fn get_current_tools(messages: &[AgentMessage]) -> Vec<Tool> {
    ai::get_current_tools(&system_messages(messages))
}

/// Replay every system message into one system message holding the current prompt and tools.
pub fn get_current_system_message(messages: &[AgentMessage]) -> Option<SystemMessage> {
    ai::get_current_system_message(&system_messages(messages))
}

/// Render the current system prompt after replaying every system message.
pub fn get_current_system_prompt(messages: &[AgentMessage]) -> String {
    ai::get_current_system_prompt(&system_messages(messages))
}
