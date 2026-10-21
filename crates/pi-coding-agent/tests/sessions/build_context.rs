//! Port of `test/session-manager/build-context.test.ts` and `save-entry.test.ts`.

use crate::common::*;
use pi_coding_agent::core::session_manager::{
    SessionEntry, SessionProjectionModel, build_context_entries, build_session_context,
};
use serde_json::{Value, json};

const TS: &str = "2025-01-01T00:00:00Z";

fn msg(id: &str, parent_id: Option<&str>, role: &str, text: &str) -> SessionEntry {
    let message = if role == "user" {
        json!({ "role": "user", "content": text, "timestamp": 1 })
    } else {
        json!({
            "role": "assistant",
            "content": [{ "type": "text", "text": text }],
            "api": "anthropic-messages",
            "provider": "anthropic",
            "model": "claude-test",
            "usage": usage_json(1, 1),
            "stopReason": "stop",
            "timestamp": 1
        })
    };
    entry(json!({ "type": "message", "id": id, "parentId": parent_id, "timestamp": TS, "message": message }))
}

fn compaction(id: &str, parent_id: Option<&str>, summary: &str, first_kept_entry_id: &str) -> SessionEntry {
    entry(json!({
        "type": "compaction", "id": id, "parentId": parent_id, "timestamp": TS,
        "summary": summary, "firstKeptEntryId": first_kept_entry_id, "tokensBefore": 1000
    }))
}

fn branch_summary(id: &str, parent_id: Option<&str>, summary: &str, from_id: &str) -> SessionEntry {
    entry(json!({
        "type": "branch_summary", "id": id, "parentId": parent_id, "timestamp": TS,
        "summary": summary, "fromId": from_id
    }))
}

fn custom(id: &str, parent_id: Option<&str>, custom_type: &str, data: Value) -> SessionEntry {
    entry(json!({
        "type": "custom", "id": id, "parentId": parent_id, "timestamp": TS,
        "customType": custom_type, "data": data
    }))
}

fn thinking_level(id: &str, parent_id: Option<&str>, level: &str) -> SessionEntry {
    entry(json!({
        "type": "thinking_level_change", "id": id, "parentId": parent_id, "timestamp": TS, "thinkingLevel": level
    }))
}

fn model_change(id: &str, parent_id: Option<&str>, provider: &str, model_id: &str) -> SessionEntry {
    entry(json!({
        "type": "model_change", "id": id, "parentId": parent_id, "timestamp": TS,
        "provider": provider, "modelId": model_id
    }))
}

fn roles(messages: &[pi_agent::types::AgentMessage]) -> Vec<String> {
    messages.iter().map(|m| m.role().to_string()).collect()
}

fn field(message: &pi_agent::types::AgentMessage, pointer: &str) -> Value {
    mjson(message).pointer(pointer).cloned().unwrap_or(Value::Null)
}

#[test]
fn empty_entries_returns_empty_context() {
    let ctx = build_session_context(&[], None, None);
    assert!(ctx.messages.is_empty());
    assert_eq!(ctx.thinking_level, "off");
    assert!(ctx.model.is_none());
}

#[test]
fn single_user_message() {
    let ctx = build_session_context(&[msg("1", None, "user", "hello")], None, None);
    assert_eq!(ctx.messages.len(), 1);
    assert_eq!(ctx.messages[0].role(), "user");
}

#[test]
fn simple_conversation() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        msg("2", Some("1"), "assistant", "hi there"),
        msg("3", Some("2"), "user", "how are you"),
        msg("4", Some("3"), "assistant", "great"),
    ];
    let ctx = build_session_context(&entries, None, None);
    assert_eq!(roles(&ctx.messages), ["user", "assistant", "user", "assistant"]);
}

#[test]
fn tracks_thinking_level_changes() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        thinking_level("2", Some("1"), "high"),
        msg("3", Some("2"), "assistant", "thinking hard"),
    ];
    let ctx = build_session_context(&entries, None, None);
    assert_eq!(ctx.thinking_level, "high");
    assert_eq!(ctx.messages.len(), 2);
}

#[test]
fn tracks_model_from_assistant_message_and_model_change() {
    let entries = vec![msg("1", None, "user", "hello"), msg("2", Some("1"), "assistant", "hi")];
    let ctx = build_session_context(&entries, None, None);
    let expected = SessionProjectionModel {
        provider: "anthropic".into(),
        model_id: "claude-test".into(),
    };
    assert_eq!(ctx.model.as_ref(), Some(&expected));

    let entries = vec![
        msg("1", None, "user", "hello"),
        model_change("2", Some("1"), "openai", "gpt-4"),
        msg("3", Some("2"), "assistant", "hi"),
    ];
    // Assistant message overwrites model change
    assert_eq!(build_session_context(&entries, None, None).model, Some(expected));

    let entries = vec![msg("1", None, "user", "hello"), model_change("2", Some("1"), "openai", "gpt-4")];
    assert_eq!(
        build_session_context(&entries, None, None).model,
        Some(SessionProjectionModel {
            provider: "openai".into(),
            model_id: "gpt-4".into()
        })
    );
}

#[test]
fn compaction_includes_summary_before_kept_messages() {
    let entries = vec![
        msg("1", None, "user", "first"),
        msg("2", Some("1"), "assistant", "response1"),
        msg("3", Some("2"), "user", "second"),
        msg("4", Some("3"), "assistant", "response2"),
        compaction("5", Some("4"), "Summary of first two turns", "3"),
        msg("6", Some("5"), "user", "third"),
        msg("7", Some("6"), "assistant", "response3"),
    ];
    let ctx = build_session_context(&entries, None, None);
    assert_eq!(ctx.messages.len(), 5);
    assert!(field(&ctx.messages[0], "/summary").as_str().unwrap().contains("Summary of first two turns"));
    assert_eq!(field(&ctx.messages[1], "/content"), "second");
    assert_eq!(field(&ctx.messages[2], "/content/0/text"), "response2");
    assert_eq!(field(&ctx.messages[3], "/content"), "third");
    assert_eq!(field(&ctx.messages[4], "/content/0/text"), "response3");
}

#[test]
fn compaction_keeping_from_first_message() {
    let entries = vec![
        msg("1", None, "user", "first"),
        msg("2", Some("1"), "assistant", "response"),
        compaction("3", Some("2"), "Empty summary", "1"),
        msg("4", Some("3"), "user", "second"),
    ];
    let ctx = build_session_context(&entries, None, None);
    assert_eq!(ctx.messages.len(), 4);
    assert!(field(&ctx.messages[0], "/summary").as_str().unwrap().contains("Empty summary"));
}

#[test]
fn multiple_compactions_uses_latest() {
    let entries = vec![
        msg("1", None, "user", "a"),
        msg("2", Some("1"), "assistant", "b"),
        compaction("3", Some("2"), "First summary", "1"),
        msg("4", Some("3"), "user", "c"),
        msg("5", Some("4"), "assistant", "d"),
        compaction("6", Some("5"), "Second summary", "4"),
        msg("7", Some("6"), "user", "e"),
    ];
    let ctx = build_session_context(&entries, None, None);
    assert_eq!(ctx.messages.len(), 4);
    assert!(field(&ctx.messages[0], "/summary").as_str().unwrap().contains("Second summary"));
}

#[test]
fn build_context_entries_returns_compaction_aware_entries_including_custom_entries() {
    let entries = vec![
        msg("1", None, "user", "first"),
        custom("2", Some("1"), "old-state", json!({ "hidden": true })),
        msg("3", Some("2"), "assistant", "response1"),
        custom("4", Some("3"), "kept-card", json!({ "title": "Kept" })),
        msg("5", Some("4"), "user", "second"),
        compaction("6", Some("5"), "Summary", "4"),
        custom("7", Some("6"), "after-card", json!({ "title": "After" })),
        msg("8", Some("7"), "assistant", "response2"),
    ];
    let ids: Vec<String> = build_context_entries(&entries, None, None)
        .iter()
        .map(|e| e.id().to_string())
        .collect();
    assert_eq!(ids, ["6", "4", "5", "7", "8"]);
    let ctx = build_session_context(&entries, None, None);
    assert_eq!(roles(&ctx.messages), ["compactionSummary", "user", "assistant"]);
}

#[test]
fn keeps_settings_from_the_full_path_after_compaction() {
    let entries = vec![
        msg("1", None, "user", "first"),
        thinking_level("2", Some("1"), "high"),
        msg("3", Some("2"), "assistant", "response1"),
        msg("4", Some("3"), "user", "second"),
        compaction("5", Some("4"), "Summary", "4"),
    ];
    let ctx = build_session_context(&entries, None, None);
    assert_eq!(ctx.thinking_level, "high");
    assert_eq!(roles(&ctx.messages), ["compactionSummary", "user"]);
}

#[test]
fn follows_path_to_specified_leaf() {
    let entries = vec![
        msg("1", None, "user", "start"),
        msg("2", Some("1"), "assistant", "response"),
        msg("3", Some("2"), "user", "branch A"),
        msg("4", Some("2"), "user", "branch B"),
    ];
    let ctx_a = build_session_context(&entries, Some(Some("3")), None);
    assert_eq!(ctx_a.messages.len(), 3);
    assert_eq!(field(&ctx_a.messages[2], "/content"), "branch A");
    let ctx_b = build_session_context(&entries, Some(Some("4")), None);
    assert_eq!(ctx_b.messages.len(), 3);
    assert_eq!(field(&ctx_b.messages[2], "/content"), "branch B");
    // Explicit null leaf: nothing selected.
    assert!(build_session_context(&entries, Some(None), None).messages.is_empty());
}

#[test]
fn includes_branch_summary_in_path() {
    let entries = vec![
        msg("1", None, "user", "start"),
        msg("2", Some("1"), "assistant", "response"),
        msg("3", Some("2"), "user", "abandoned path"),
        branch_summary("4", Some("2"), "Summary of abandoned work", "3"),
        msg("5", Some("4"), "user", "new direction"),
    ];
    let ctx = build_session_context(&entries, Some(Some("5")), None);
    assert_eq!(ctx.messages.len(), 4);
    assert!(field(&ctx.messages[2], "/summary").as_str().unwrap().contains("Summary of abandoned work"));
    assert_eq!(field(&ctx.messages[3], "/content"), "new direction");
}

#[test]
fn complex_tree_with_multiple_branches_and_compaction() {
    let entries = vec![
        msg("1", None, "user", "start"),
        msg("2", Some("1"), "assistant", "r1"),
        msg("3", Some("2"), "user", "q2"),
        msg("4", Some("3"), "assistant", "r2"),
        compaction("5", Some("4"), "Compacted history", "3"),
        msg("6", Some("5"), "user", "q3"),
        msg("7", Some("6"), "assistant", "r3"),
        msg("8", Some("3"), "user", "wrong path"),
        msg("9", Some("8"), "assistant", "wrong response"),
        branch_summary("10", Some("3"), "Tried wrong approach", "9"),
        msg("11", Some("10"), "user", "better approach"),
    ];
    let main = build_session_context(&entries, Some(Some("7")), None);
    assert_eq!(main.messages.len(), 5);
    assert!(field(&main.messages[0], "/summary").as_str().unwrap().contains("Compacted history"));
    assert_eq!(field(&main.messages[1], "/content"), "q2");
    assert_eq!(field(&main.messages[2], "/content/0/text"), "r2");
    assert_eq!(field(&main.messages[3], "/content"), "q3");
    assert_eq!(field(&main.messages[4], "/content/0/text"), "r3");

    let branch = build_session_context(&entries, Some(Some("11")), None);
    assert_eq!(branch.messages.len(), 5);
    assert_eq!(field(&branch.messages[0], "/content"), "start");
    assert_eq!(field(&branch.messages[1], "/content/0/text"), "r1");
    assert_eq!(field(&branch.messages[2], "/content"), "q2");
    assert!(field(&branch.messages[3], "/summary").as_str().unwrap().contains("Tried wrong approach"));
    assert_eq!(field(&branch.messages[4], "/content"), "better approach");
}

#[test]
fn uses_last_entry_when_leaf_id_not_found() {
    let entries = vec![msg("1", None, "user", "hello"), msg("2", Some("1"), "assistant", "hi")];
    assert_eq!(build_session_context(&entries, Some(Some("nonexistent")), None).messages.len(), 2);
}

#[test]
fn handles_orphaned_entries_gracefully() {
    let entries = vec![msg("1", None, "user", "hello"), msg("2", Some("missing"), "assistant", "orphan")];
    assert_eq!(build_session_context(&entries, Some(Some("2")), None).messages.len(), 1);
}

#[test]
fn save_custom_entries_and_include_them_in_tree_traversal() {
    let session = in_memory();
    let msg_id = session
        .append_message(agent_message(json!({ "role": "user", "content": "hello", "timestamp": 1 })))
        .unwrap();
    let custom_id = session.append_custom_entry("my_data", Some(json!({ "foo": "bar" }))).unwrap();
    let msg2_id = session.append_message(assistant_msg("hi")).unwrap();

    let entries = session.get_entries();
    assert_eq!(entries.len(), 3);
    let Some(SessionEntry::Custom(custom_entry)) = entries.iter().find(|e| e.type_name() == "custom") else {
        panic!("custom entry missing");
    };
    assert_eq!(custom_entry.custom_type, "my_data");
    assert_eq!(custom_entry.data, Some(json!({ "foo": "bar" })));
    assert_eq!(custom_entry.id, custom_id);
    assert_eq!(custom_entry.parent_id.as_deref(), Some(msg_id.as_str()));

    let path: Vec<String> = session.get_branch(None).iter().map(|e| e.id().to_string()).collect();
    assert_eq!(path, [msg_id, custom_id, msg2_id]);
    assert_eq!(session.build_session_context().messages.len(), 2);
}
