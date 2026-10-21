//! Port of `test/session-context-edit.test.ts`.

use crate::common::*;
use pi_agent::types::AgentMessage;
use pi_coding_agent::core::compaction::{CompactionSettings, estimate_projected_context_tokens, prepare_compaction};
use pi_coding_agent::core::session_manager::{
    ContextEditEntryReplacement, ContextEditableContent, FileEntry, SessionEntry, SessionManager,
};
use serde_json::{Value, json};

fn assistant(text: &str) -> Value {
    json!({
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "api": "faux",
        "provider": "faux",
        "model": "faux",
        "usage": {
            "input": 10, "output": 1, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 11,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 }
        },
        "stopReason": "stop",
        "timestamp": now_ms()
    })
}

fn user(text: &str) -> AgentMessage {
    agent_message(json!({ "role": "user", "content": text, "timestamp": now_ms() }))
}

fn text(message: &AgentMessage) -> String {
    match &mjson(message)["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .map(|p| p["text"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn summary_or_text(message: &AgentMessage) -> String {
    match mjson(message).get("summary") {
        Some(Value::String(summary)) => summary.clone(),
        _ => text(message),
    }
}

fn replace(content: impl Into<ContextEditableContent>) -> ContextEditEntryReplacement {
    ContextEditEntryReplacement { content: content.into() }
}

fn blocks(text: &str) -> ContextEditableContent {
    serde_json::from_value(json!([{ "type": "text", "text": text }])).unwrap()
}

fn keep_one() -> CompactionSettings {
    CompactionSettings {
        keep_recent_tokens: 1,
        ..CompactionSettings::default()
    }
}

fn roles(messages: &[AgentMessage]) -> Vec<String> {
    messages.iter().map(|m| m.role().to_string()).collect()
}

#[test]
fn omits_a_target_only_from_model_projection() {
    let session = in_memory();
    session.append_message(user("request")).unwrap();
    let assistant_id = session.append_message(agent_message(assistant("partial"))).unwrap();
    let result = agent_message(json!({
        "role": "toolResult", "toolCallId": "call-1", "toolName": "read",
        "content": [{ "type": "text", "text": "raw output" }], "details": { "path": "large.txt" },
        "isError": true, "timestamp": now_ms()
    }));
    let result_id = session.append_message(result.clone()).unwrap();
    session.append_context_edit(&assistant_id, None).unwrap();
    session.append_context_edit(&result_id, None).unwrap();

    assert_eq!(
        session.get_branch(None).iter().filter(|e| e.type_name() == "message").count(),
        3
    );
    assert_eq!(roles(&session.build_session_projection().messages), ["user"]);
    assert_eq!(session.get_entry(&result_id).unwrap().message(), Some(&result));
}

#[test]
fn replaces_only_content_and_lets_the_latest_edit_win() {
    let session = in_memory();
    let target_id = session.append_message(agent_message(assistant("original"))).unwrap();
    session.append_context_edit(&target_id, Some(&replace(blocks("first")))).unwrap();
    session.append_context_edit(&target_id, None).unwrap();
    session.append_context_edit(&target_id, Some(&replace(blocks("restored")))).unwrap();

    let projected = &session.build_session_projection().messages[0];
    let AgentMessage::Assistant(projected_assistant) = projected else {
        panic!("expected assistant")
    };
    assert_eq!(text(projected), "restored");
    assert_eq!(projected_assistant.usage.total_tokens, 11);
    assert_eq!(text(session.get_entry(&target_id).unwrap().message().unwrap()), "original");
}

#[test]
fn normalizes_string_replacements_for_array_only_roles() {
    let session = in_memory();
    let assistant_id = session.append_message(agent_message(assistant("original"))).unwrap();
    let result_id = session
        .append_message(agent_message(json!({
            "role": "toolResult", "toolCallId": "call-1", "toolName": "read",
            "content": [{ "type": "text", "text": "original result" }], "isError": false, "timestamp": now_ms()
        })))
        .unwrap();
    let assistant_edit = session
        .append_context_edit(&assistant_id, Some(&replace("assistant replacement")))
        .unwrap();
    let result_edit = session.append_context_edit(&result_id, Some(&replace("result replacement"))).unwrap();

    assert_eq!(
        session.get_entry(&assistant_edit).unwrap().to_value()["replacement"],
        json!({ "content": [{ "type": "text", "text": "assistant replacement" }] })
    );
    assert_eq!(
        session.get_entry(&result_edit).unwrap().to_value()["replacement"],
        json!({ "content": [{ "type": "text", "text": "result replacement" }] })
    );
    let projected = session.build_session_projection().messages;
    assert_eq!(projected[0].role(), "assistant");
    assert_eq!(mjson(&projected[0])["content"], json!([{ "type": "text", "text": "assistant replacement" }]));
    assert_eq!(projected[1].role(), "toolResult");
    assert_eq!(mjson(&projected[1])["content"], json!([{ "type": "text", "text": "result replacement" }]));
}

#[test]
fn normalizes_imported_string_replacements_while_projecting_array_only_roles() {
    let entries: Vec<FileEntry> = [
        json!({ "type": "message", "id": "a1", "parentId": null, "timestamp": "2025-01-01T00:00:00Z", "message": assistant("original") }),
        json!({ "type": "context_edit", "id": "e1", "parentId": "a1", "timestamp": "2025-01-01T00:00:01Z", "targetId": "a1", "replacement": { "content": "imported replacement" } }),
    ]
    .into_iter()
    .map(|v| serde_json::from_value(v).unwrap())
    .collect();
    let session = SessionManager::in_memory(None, None, Some(entries)).unwrap();
    let projected = &session.build_session_projection().messages[0];
    assert_eq!(projected.role(), "assistant");
    assert_eq!(mjson(projected)["content"], json!([{ "type": "text", "text": "imported replacement" }]));
}

#[test]
fn keeps_edits_branch_relative() {
    let session = in_memory();
    let target_id = session.append_message(user("original")).unwrap();
    session.append_context_edit(&target_id, Some(&replace("edited"))).unwrap();
    assert_eq!(text(&session.build_session_projection().messages[0]), "edited");
    session.branch(&target_id).unwrap();
    assert_eq!(text(&session.build_session_projection().messages[0]), "original");
}

#[test]
fn context_edit_validation_errors() {
    let session = in_memory();
    let user_id = session.append_message(user("original")).unwrap();
    let model_id = session.append_model_change("a", "b").unwrap();
    assert_eq!(
        session.append_context_edit("missing", None).unwrap_err().to_string(),
        "Entry missing not found"
    );
    assert_eq!(
        session.append_context_edit(&model_id, None).unwrap_err().to_string(),
        format!("Entry {model_id} does not contribute editable model content")
    );
    session.branch(&user_id).unwrap();
    session.reset_leaf();
    assert_eq!(
        session.append_context_edit(&user_id, None).unwrap_err().to_string(),
        format!("Entry {user_id} is not on the active branch")
    );
}

#[test]
fn uses_a_self_referencing_compaction_to_retain_no_preceding_entries() {
    let session = in_memory();
    session.append_message(user("discarded")).unwrap();
    let compaction_id = session.append_compaction("exact handoff", None, 100, None, None, None).unwrap();
    session.append_message(user("after")).unwrap();

    let Some(SessionEntry::Compaction(compaction)) = session.get_entry(&compaction_id) else {
        panic!()
    };
    assert_eq!(compaction.first_kept_entry_id, compaction_id);
    let messages = session.build_session_projection().messages;
    assert_eq!(roles(&messages), ["compactionSummary", "user"]);
    assert_eq!(
        messages.iter().map(summary_or_text).collect::<Vec<_>>(),
        ["exact handoff", "after"]
    );
}

#[test]
fn applies_post_compaction_edits_to_retained_pre_compaction_entries() {
    let session = in_memory();
    session.append_message(user("summarized")).unwrap();
    let retained = session.append_message(user("original retained")).unwrap();
    session.append_compaction("summary", Some(&retained), 100, None, None, None).unwrap();
    session.append_context_edit(&retained, Some(&replace("edited retained"))).unwrap();
    assert_eq!(
        session
            .build_session_projection()
            .messages
            .iter()
            .map(summary_or_text)
            .collect::<Vec<_>>(),
        ["summary", "edited retained"]
    );
}

#[test]
fn uses_only_the_newest_summary_when_repeated_compaction_retains_older_entries() {
    let session = in_memory();
    session.append_message(user("summarized first")).unwrap();
    let retained = session.append_message(user("retained")).unwrap();
    session.append_compaction("first summary", Some(&retained), 100, None, None, None).unwrap();
    session.append_message(agent_message(assistant("after first compaction"))).unwrap();
    session.append_compaction("second summary", Some(&retained), 80, None, None, None).unwrap();
    session.append_message(user(&"new tail ".repeat(100))).unwrap();

    let summaries: Vec<String> = session
        .build_session_projection()
        .messages
        .iter()
        .filter_map(|m| mjson(m).get("summary").and_then(Value::as_str).map(str::to_string))
        .collect();
    assert_eq!(summaries, ["second summary"]);
    let preparation = prepare_compaction(&session.get_branch(None), &keep_one()).unwrap();
    assert_eq!(preparation.previous_summary.as_deref(), Some("second summary"));
}

#[test]
fn supports_repeated_retain_none_compactions() {
    let session = in_memory();
    session.append_message(user("discarded")).unwrap();
    session.append_compaction("first handoff", None, 100, None, None, None).unwrap();
    session.append_message(user("also discarded")).unwrap();
    let second = session.append_compaction("second handoff", None, 50, None, None, None).unwrap();
    let Some(SessionEntry::Compaction(c)) = session.get_entry(&second) else {
        panic!()
    };
    assert_eq!(c.first_kept_entry_id, second);
    assert_eq!(
        session
            .build_session_projection()
            .messages
            .iter()
            .map(|m| mjson(m).get("summary").and_then(Value::as_str).unwrap_or("").to_string())
            .collect::<Vec<_>>(),
        ["second handoff"]
    );
}

#[test]
fn does_not_trust_pre_edit_assistant_usage_for_projected_estimates() {
    let session = in_memory();
    let large_user = session.append_message(user(&"discarded input ".repeat(2_000))).unwrap();
    let mut response = assistant("small answer");
    response["usage"]["input"] = json!(10_000);
    response["usage"]["totalTokens"] = json!(10_001);
    let assistant_id = session.append_message(agent_message(response)).unwrap();
    session.append_context_edit(&large_user, None).unwrap();

    let edited = estimate_projected_context_tokens(&session.build_session_projection(), &session.get_branch(None));
    assert_eq!(edited.usage_tokens, 0);
    assert!(edited.tokens < 100);

    session.append_context_edit(&assistant_id, None).unwrap();
    assert_eq!(
        estimate_projected_context_tokens(&session.build_session_projection(), &session.get_branch(None)).tokens,
        0
    );
}

#[test]
fn uses_assistant_usage_captured_after_the_latest_context_edit() {
    let session = in_memory();
    let user_id = session.append_message(user("original")).unwrap();
    session.append_context_edit(&user_id, Some(&replace("edited"))).unwrap();
    let mut response = assistant("answer");
    response["usage"]["input"] = json!(4_000);
    response["usage"]["output"] = json!(100);
    response["usage"]["totalTokens"] = json!(4_100);
    session.append_message(agent_message(response)).unwrap();
    session.append_message(user("next")).unwrap();

    let estimate = estimate_projected_context_tokens(&session.build_session_projection(), &session.get_branch(None));
    assert_eq!(estimate.usage_tokens, 4_100);
    assert_eq!(estimate.trailing_tokens, 1);
    assert_eq!(estimate.tokens, 4_101);
}

#[test]
fn does_not_reuse_post_edit_assistant_usage_after_a_later_compaction() {
    let session = in_memory();
    let user_id = session.append_message(user("small input")).unwrap();
    session.append_context_edit(&user_id, Some(&replace("edited input"))).unwrap();
    let mut response = assistant("answer");
    response["usage"]["input"] = json!(50_000);
    response["usage"]["output"] = json!(1);
    response["usage"]["totalTokens"] = json!(50_001);
    session.append_message(agent_message(response)).unwrap();
    session.append_compaction("small summary", Some(&user_id), 50_001, None, None, None).unwrap();

    let estimate = estimate_projected_context_tokens(&session.build_session_projection(), &session.get_branch(None));
    assert_eq!(estimate.usage_tokens, 0);
    assert!(estimate.tokens < 100);
}

#[test]
fn includes_effective_system_and_tool_context_in_edited_estimates() {
    let session = in_memory();
    session
        .append_message(agent_message(json!({
            "role": "system",
            "content": "system prompt ".repeat(3_000),
            "toolsAdded": [{
                "name": "example",
                "description": "tool declaration ".repeat(100),
                "parameters": { "type": "object", "properties": {} }
            }],
            "timestamp": now_ms()
        })))
        .unwrap();
    let user_id = session.append_message(user("ask")).unwrap();
    session.append_message(agent_message(assistant("done"))).unwrap();
    session.append_context_edit(&user_id, Some(&replace("ask"))).unwrap();
    assert!(
        estimate_projected_context_tokens(&session.build_session_projection(), &session.get_branch(None)).tokens
            > 10_000
    );
}

#[test]
fn does_not_advance_past_a_boundary_replacement_of_the_candidate_input() {
    let session = in_memory();
    session.append_message(user("old request")).unwrap();
    session.append_message(agent_message(assistant("old answer"))).unwrap();
    let replaced = session.append_message(user("original input")).unwrap();
    let assistant_id = session.append_message(agent_message(assistant("answered original input"))).unwrap();
    session
        .append_context_edit(&replaced, Some(&replace("NEW-INSTRUCTION ".repeat(100))))
        .unwrap();
    session.append_context_edit(&assistant_id, None).unwrap();
    session.append_custom_entry("bookkeeping", Some(json!({ "source": "test" }))).unwrap();

    let preparation = prepare_compaction(&session.get_branch(None), &keep_one()).unwrap();
    assert_eq!(preparation.first_kept_entry_id, replaced);
    assert!(!serde_json::to_string(&preparation.messages_to_summarize).unwrap().contains("NEW-INSTRUCTION"));
    assert!(!serde_json::to_string(&preparation.turn_prefix_messages).unwrap().contains("NEW-INSTRUCTION"));
}

#[test]
fn does_not_let_metadata_move_the_cut_past_unsent_boundary_input() {
    let session = in_memory();
    session.append_message(user("old request")).unwrap();
    session.append_message(agent_message(assistant("old answer"))).unwrap();
    let instruction = session
        .append_custom_message_entry("next-work", "UNSENT-INSTRUCTION ".repeat(100).into(), false, None)
        .unwrap();
    session.append_custom_entry("bookkeeping", Some(json!({ "source": "test" }))).unwrap();

    let preparation = prepare_compaction(&session.get_branch(None), &keep_one()).unwrap();
    assert_eq!(preparation.first_kept_entry_id, instruction);
    assert!(!serde_json::to_string(&preparation.messages_to_summarize).unwrap().contains("UNSENT-INSTRUCTION"));
    assert!(!serde_json::to_string(&preparation.turn_prefix_messages).unwrap().contains("UNSENT-INSTRUCTION"));
}

#[test]
fn does_not_treat_an_omitted_custom_message_as_a_recovery_attempt() {
    let session = in_memory();
    session.append_message(user(&"unanswered input ".repeat(100))).unwrap();
    let custom_id = session
        .append_custom_message_entry("temporary", "temporary context".into(), false, None)
        .unwrap();
    session.append_context_edit(&custom_id, None).unwrap();
    assert!(prepare_compaction(&session.get_branch(None), &keep_one()).is_none());
}

#[test]
fn advances_past_input_for_an_omitted_assistant_recovery_suffix_with_metadata() {
    let session = in_memory();
    let user_id = session.append_message(user(&"recovery input ".repeat(100))).unwrap();
    let attempt_id = session.append_message(agent_message(assistant("failed attempt"))).unwrap();
    session.append_context_edit(&attempt_id, None).unwrap();
    session.append_custom_entry("bookkeeping", Some(json!({ "source": "test" }))).unwrap();

    let preparation = prepare_compaction(&session.get_branch(None), &keep_one()).unwrap();
    assert_eq!(preparation.first_kept_entry_id, attempt_id);
    assert_eq!(preparation.turn_prefix_messages.len(), 1);
    assert_eq!(preparation.turn_prefix_messages[0].role(), "user");
    assert!(text(&preparation.turn_prefix_messages[0]).contains("recovery input"));
    assert!(
        !preparation
            .messages_to_summarize
            .iter()
            .any(|m| m.role() == "user" && text(m).contains("recovery input"))
    );
    assert_ne!(user_id, attempt_id);
}

#[test]
fn prepares_compaction_from_edited_model_content() {
    let session = in_memory();
    let omitted = session.append_message(user(&"OMIT-ME ".repeat(100))).unwrap();
    session.append_message(agent_message(assistant(&"old answer ".repeat(100)))).unwrap();
    session.append_context_edit(&omitted, None).unwrap();
    session.append_message(user("keep")).unwrap();
    session.append_message(agent_message(assistant("suffix"))).unwrap();

    let preparation = prepare_compaction(&session.get_branch(None), &keep_one()).unwrap();
    assert!(!serde_json::to_string(&preparation.messages_to_summarize).unwrap().contains("OMIT-ME"));
    assert!(!serde_json::to_string(&preparation.turn_prefix_messages).unwrap().contains("OMIT-ME"));
}
