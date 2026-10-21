//! Shared helpers (port of the relevant parts of `test/utilities.ts`).

use pi_agent::types::AgentMessage;
use pi_coding_agent::core::session_manager::{SessionEntry, SessionManager};
use serde_json::{Value, json};
use std::sync::Arc;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn agent_message(value: Value) -> AgentMessage {
    AgentMessage::from_value(value).expect("valid message JSON")
}

pub fn usage_json(input: u64, output: u64) -> Value {
    json!({
        "input": input,
        "output": output,
        "cacheRead": 0,
        "cacheWrite": 0,
        "totalTokens": input + output,
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 }
    })
}

/// `userMsg(text)`.
pub fn user_msg(text: &str) -> AgentMessage {
    agent_message(json!({ "role": "user", "content": text, "timestamp": now_ms() }))
}

/// `assistantMsg(text)`.
pub fn assistant_msg(text: &str) -> AgentMessage {
    agent_message(json!({
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "api": "anthropic-messages",
        "provider": "anthropic",
        "model": "test",
        "usage": usage_json(1, 1),
        "stopReason": "stop",
        "timestamp": now_ms()
    }))
}

pub fn in_memory() -> Arc<SessionManager> {
    SessionManager::in_memory(None, None, None).unwrap()
}

/// Build a session entry from its JSON form.
pub fn entry(value: Value) -> SessionEntry {
    serde_json::from_value(value).expect("valid session entry JSON")
}

/// JSON of a message, for structural assertions.
pub fn mjson(message: &AgentMessage) -> Value {
    message.to_value()
}

/// `readSessionFileRoles(file)`.
pub fn read_session_file_roles(file: &str) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap()
        .trim()
        .split('\n')
        .map(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            record
                .pointer("/message/role")
                .and_then(Value::as_str)
                .or_else(|| record.get("type").and_then(Value::as_str))
                .unwrap()
                .to_string()
        })
        .collect()
}

pub fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

pub fn path_str(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn is_uuid_v7(id: &str) -> bool {
    regex::Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
        .unwrap()
        .is_match(id)
}
