//! Port of `test/compaction.test.ts`, `compaction-serialization.test.ts`,
//! `compaction-nested-calls.test.ts`, `compaction-summary-reasoning.test.ts` and
//! `branch-summarization.test.ts`. Mocked `completeSimple` calls are replaced by a recording
//! stream function, which exercises the same request-building path.

use crate::common::*;
use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, StreamFn, ThinkingLevel};
use pi_ai::types::{AssistantMessage, AssistantMessageEvent, Message, Model, SimpleStreamOptions, StopReason};
use pi_ai::utils::event_stream::AssistantMessageEventStream;
use pi_coding_agent::core::compaction::*;
use pi_coding_agent::core::session_manager::{
    FileEntry, SessionEntry, build_session_context, migrate_session_entries, parse_session_entries,
};
use serde_json::{Value, json};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_large_session_entries() -> Vec<SessionEntry> {
    let content = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/large-session.jsonl"))
        .unwrap();
    let mut entries = parse_session_entries(&content);
    migrate_session_entries(&mut entries); // Add id/parentId for v1 fixtures
    entries
        .into_iter()
        .filter_map(|e| match e {
            FileEntry::Entry(entry) => Some(entry),
            _ => None,
        })
        .collect()
}

fn mock_usage(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Value {
    json!({
        "input": input, "output": output, "cacheRead": cache_read, "cacheWrite": cache_write,
        "totalTokens": input + output + cache_read + cache_write,
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 }
    })
}

fn user_message(text: &str) -> Value {
    json!({ "role": "user", "content": text, "timestamp": now_ms() })
}

fn assistant_message(text: &str, usage: Option<Value>) -> Value {
    json!({
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "usage": usage.unwrap_or_else(|| mock_usage(100, 50, 0, 0)),
        "stopReason": "stop",
        "timestamp": now_ms(),
        "api": "anthropic-messages",
        "provider": "anthropic",
        "model": "claude-sonnet-4-5"
    })
}

/// Sequential entry builder (`createMessageEntry` & co.).
struct Builder {
    counter: usize,
    last_id: Option<String>,
}

impl Builder {
    fn new() -> Self {
        Self { counter: 0, last_id: None }
    }

    fn next(&mut self, mut value: Value) -> SessionEntry {
        let id = format!("test-id-{}", self.counter);
        self.counter += 1;
        value["id"] = json!(id);
        value["parentId"] = json!(self.last_id);
        value["timestamp"] = json!("2025-01-01T00:00:00.000Z");
        self.last_id = Some(id);
        entry(value)
    }

    fn message(&mut self, message: Value) -> SessionEntry {
        self.next(json!({ "type": "message", "message": message }))
    }

    fn compaction(&mut self, summary: &str, first_kept_entry_id: &str) -> SessionEntry {
        self.next(json!({
            "type": "compaction", "summary": summary, "firstKeptEntryId": first_kept_entry_id, "tokensBefore": 10000
        }))
    }

    fn model_change(&mut self, provider: &str, model_id: &str) -> SessionEntry {
        self.next(json!({ "type": "model_change", "provider": provider, "modelId": model_id }))
    }

    fn thinking_level(&mut self, level: &str) -> SessionEntry {
        self.next(json!({ "type": "thinking_level_change", "thinkingLevel": level }))
    }

    fn custom_message(&mut self, content: &str) -> SessionEntry {
        self.next(json!({ "type": "custom_message", "customType": "test", "content": content, "display": true }))
    }
}

fn message_of(entry: &SessionEntry) -> &AgentMessage {
    entry.message().expect("message entry")
}

fn extract_text(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let value = mjson(message);
            match message.role() {
                "branchSummary" | "compactionSummary" => value["summary"].as_str().unwrap_or("").to_string(),
                "bashExecution" => format!(
                    "{}\n{}",
                    value["command"].as_str().unwrap_or(""),
                    value["output"].as_str().unwrap_or("")
                ),
                _ => match &value["content"] {
                    Value::String(s) => s.clone(),
                    Value::Array(blocks) => blocks
                        .iter()
                        .filter(|b| b["type"] == "text")
                        .map(|b| b["text"].as_str().unwrap_or("").to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                    _ => String::new(),
                },
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn usage(value: Value) -> pi_ai::types::Usage {
    serde_json::from_value(value).unwrap()
}

// ---------------------------------------------------------------------------
// Token calculation
// ---------------------------------------------------------------------------

#[test]
fn calculates_total_context_tokens_from_usage() {
    assert_eq!(calculate_context_tokens(&usage(mock_usage(1000, 500, 200, 100))), 1800);
    assert_eq!(calculate_context_tokens(&usage(mock_usage(0, 0, 0, 0))), 0);
    let mut no_total = usage(mock_usage(1, 2, 3, 4));
    no_total.total_tokens = 0;
    assert_eq!(calculate_context_tokens(&no_total), 10);
}

#[test]
fn get_last_assistant_usage_cases() {
    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("Hello")),
        b.message(assistant_message("Hi", Some(mock_usage(100, 50, 0, 0)))),
        b.message(user_message("How are you?")),
        b.message(assistant_message("Good", Some(mock_usage(200, 100, 0, 0)))),
    ];
    assert_eq!(get_last_assistant_usage(&entries).unwrap().input, 200);

    let mut aborted = assistant_message("Aborted", Some(mock_usage(300, 150, 0, 0)));
    aborted["stopReason"] = json!("aborted");
    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("Hello")),
        b.message(assistant_message("Hi", Some(mock_usage(100, 50, 0, 0)))),
        b.message(user_message("How are you?")),
        b.message(aborted),
    ];
    assert_eq!(get_last_assistant_usage(&entries).unwrap().input, 100);

    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("Hello")),
        b.message(assistant_message("Hi", Some(mock_usage(100, 50, 0, 0)))),
        b.message(user_message("continue")),
        b.message(assistant_message("Partial", Some(mock_usage(0, 0, 0, 0)))),
    ];
    assert_eq!(get_last_assistant_usage(&entries).unwrap().input, 100);

    let mut b = Builder::new();
    assert!(get_last_assistant_usage(&[b.message(user_message("Hello"))]).is_none());
}

#[test]
fn estimate_context_tokens_uses_last_non_zero_usage_as_anchor() {
    let messages: Vec<AgentMessage> = [
        user_message("Hello"),
        assistant_message("Hi", Some(mock_usage(100, 50, 0, 0))),
        user_message("continue"),
        assistant_message("Partial thinking", Some(mock_usage(0, 0, 0, 0))),
    ]
    .into_iter()
    .map(agent_message)
    .collect();
    let estimate = estimate_context_tokens(&messages);
    assert_eq!(estimate.usage_tokens, 150);
    assert_eq!(estimate.last_usage_index, Some(1));
    assert!(estimate.trailing_tokens > 0);
    assert_eq!(estimate.tokens, 150 + estimate.trailing_tokens);
}

#[test]
fn estimate_tokens_uses_chars_over_four() {
    assert_eq!(estimate_tokens(&agent_message(user_message("abcde"))), 2);
    assert_eq!(
        estimate_tokens(&agent_message(json!({
            "role": "user", "timestamp": 1,
            "content": [{ "type": "text", "text": "abcd" }, { "type": "image", "data": "x", "mimeType": "image/png" }]
        }))),
        1201
    );
    let bash: AgentMessage = agent_message(json!({
        "role": "bashExecution", "command": "ls", "output": "abcdef", "cancelled": false, "truncated": false, "timestamp": 1
    }));
    assert_eq!(estimate_tokens(&bash), 2);
    let summary = agent_message(json!({ "role": "compactionSummary", "summary": "x".repeat(9), "tokensBefore": 1, "timestamp": 1 }));
    assert_eq!(estimate_tokens(&summary), 3);
    // UTF-16 lengths like JS: one astral char counts as two units.
    assert_eq!(estimate_tokens(&agent_message(user_message("😀😀"))), 1);
    let unknown = agent_message(json!({ "role": "somethingElse", "text": "abc" }));
    assert_eq!(estimate_tokens(&unknown), 0);
}

#[test]
fn should_compact_cases() {
    let settings = CompactionSettings {
        enabled: true,
        reserve_tokens: 10000,
        keep_recent_tokens: 20000,
    };
    assert!(should_compact(95000, 100000, &settings));
    assert!(!should_compact(89000, 100000, &settings));
    let disabled = CompactionSettings {
        enabled: false,
        ..settings
    };
    assert!(!should_compact(95000, 100000, &disabled));
    assert_eq!(*DEFAULT_COMPACTION_SETTINGS, CompactionSettings {
        enabled: true,
        reserve_tokens: 16384,
        keep_recent_tokens: 20000
    });
}

// ---------------------------------------------------------------------------
// findCutPoint
// ---------------------------------------------------------------------------

#[test]
fn finds_cut_point_at_a_valid_message() {
    let mut b = Builder::new();
    let mut entries = Vec::new();
    for i in 0..10u64 {
        entries.push(b.message(user_message(&format!("User {i}"))));
        entries.push(b.message(assistant_message(
            &format!("Assistant {i}"),
            Some(mock_usage(0, 100, (i + 1) * 1000, 0)),
        )));
    }
    let result = find_cut_point(&entries, 0, entries.len() as i64, 2500);
    let cut = &entries[result.first_kept_entry_index as usize];
    assert_eq!(cut.type_name(), "message");
    assert!(matches!(message_of(cut).role(), "user" | "assistant"));
}

#[test]
fn find_cut_point_edge_cases() {
    let mut b = Builder::new();
    let entries = vec![b.message(assistant_message("a", None))];
    assert_eq!(find_cut_point(&entries, 0, 1, 1000).first_kept_entry_index, 0);

    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("1")),
        b.message(assistant_message("a", Some(mock_usage(0, 50, 500, 0)))),
        b.message(user_message("2")),
        b.message(assistant_message("b", Some(mock_usage(0, 50, 1000, 0)))),
    ];
    assert_eq!(find_cut_point(&entries, 0, 4, 50000).first_kept_entry_index, 0);
}

#[test]
fn indicates_split_turn_when_cutting_at_assistant_message() {
    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("Turn 1")),
        b.message(assistant_message("A1", Some(mock_usage(0, 100, 1000, 0)))),
        b.message(user_message("Turn 2")),
        b.message(assistant_message("A2-1", Some(mock_usage(0, 100, 5000, 0)))),
        b.message(assistant_message("A2-2", Some(mock_usage(0, 100, 8000, 0)))),
        b.message(assistant_message("A2-3", Some(mock_usage(0, 100, 10000, 0)))),
    ];
    let result = find_cut_point(&entries, 0, entries.len() as i64, 3000);
    if message_of(&entries[result.first_kept_entry_index as usize]).role() == "assistant" {
        assert!(result.is_split_turn);
        assert_eq!(result.turn_start_index, 2);
    }
}

#[test]
fn budgets_context_visible_custom_message_entries() {
    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("hi")),
        b.message(assistant_message("hello", None)),
        b.custom_message(&"x".repeat(4000)),
        b.message(assistant_message("ok", None)),
    ];
    let tiny = find_cut_point(&entries, 0, 4, 1);
    assert_eq!(tiny, CutPointResult {
        first_kept_entry_index: 3,
        turn_start_index: 2,
        is_split_turn: true
    });
    let fits = find_cut_point(&entries, 0, 4, 2);
    assert_eq!(fits, CutPointResult {
        first_kept_entry_index: 2,
        turn_start_index: -1,
        is_split_turn: false
    });
}

#[test]
fn falls_back_to_latest_cut_point_before_oversized_trailing_tool_results() {
    let mut b = Builder::new();
    let old_user = b.message(user_message("old history"));
    let old_assistant = b.message(assistant_message("old answer", None));
    let current_user = b.message(user_message("read the large file"));
    let mut call = assistant_message("", None);
    call["content"] = json!([{ "type": "toolCall", "id": "call-1", "name": "read", "arguments": { "path": "big.txt" } }]);
    call["stopReason"] = json!("toolUse");
    let tool_call = b.message(call);
    let tool_result = b.message(json!({
        "role": "toolResult", "toolCallId": "call-1", "toolName": "read",
        "content": [{ "type": "text", "text": "x".repeat(8000) }], "isError": false, "timestamp": now_ms()
    }));
    let entries = vec![
        old_user.clone(),
        old_assistant.clone(),
        current_user.clone(),
        tool_call.clone(),
        tool_result,
    ];

    assert_eq!(find_cut_point(&entries, 0, 5, 1000), CutPointResult {
        first_kept_entry_index: 3,
        turn_start_index: 2,
        is_split_turn: true
    });

    let preparation = prepare_compaction(&entries, &CompactionSettings {
        keep_recent_tokens: 1000,
        ..CompactionSettings::default()
    })
    .unwrap();
    assert_eq!(preparation.first_kept_entry_id, tool_call.id());
    assert_eq!(preparation.messages_to_summarize, vec![
        message_of(&old_user).clone(),
        message_of(&old_assistant).clone()
    ]);
    assert_eq!(preparation.turn_prefix_messages, vec![message_of(&current_user).clone()]);
    assert_eq!(
        compute_file_lists(&preparation.file_ops).read_files,
        Vec::<String>::new(),
        "the tool call itself is kept, not summarized"
    );
}

// ---------------------------------------------------------------------------
// buildSessionContext
// ---------------------------------------------------------------------------

#[test]
fn build_session_context_with_compactions() {
    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("1")),
        b.message(assistant_message("a", None)),
        b.message(user_message("2")),
        b.message(assistant_message("b", None)),
    ];
    let loaded = build_session_context(&entries, None, None);
    assert_eq!(loaded.messages.len(), 4);
    assert_eq!(loaded.thinking_level, "off");
    assert_eq!(loaded.model.unwrap().model_id, "claude-sonnet-4-5");

    let mut b = Builder::new();
    let u1 = b.message(user_message("1"));
    let a1 = b.message(assistant_message("a", None));
    let u2 = b.message(user_message("2"));
    let a2 = b.message(assistant_message("b", None));
    let compaction = b.compaction("Summary of 1,a,2,b", u2.id());
    let u3 = b.message(user_message("3"));
    let a3 = b.message(assistant_message("c", None));
    let loaded = build_session_context(&[u1, a1, u2, a2, compaction, u3, a3], None, None);
    assert_eq!(loaded.messages.len(), 5);
    assert_eq!(loaded.messages[0].role(), "compactionSummary");
    assert!(mjson(&loaded.messages[0])["summary"].as_str().unwrap().contains("Summary of 1,a,2,b"));

    let mut b = Builder::new();
    let u1 = b.message(user_message("1"));
    let a1 = b.message(assistant_message("a", None));
    let c1 = b.compaction("First summary", u1.id());
    let u2 = b.message(user_message("2"));
    let bb = b.message(assistant_message("b", None));
    let u3 = b.message(user_message("3"));
    let c = b.message(assistant_message("c", None));
    let c2 = b.compaction("Second summary", u3.id());
    let u4 = b.message(user_message("4"));
    let d = b.message(assistant_message("d", None));
    let loaded = build_session_context(&[u1.clone(), a1.clone(), c1.clone(), u2.clone(), bb.clone(), u3, c, c2, u4, d], None, None);
    assert_eq!(loaded.messages.len(), 5);
    assert!(mjson(&loaded.messages[0])["summary"].as_str().unwrap().contains("Second summary"));

    let loaded = build_session_context(&[u1, a1, c1, u2, bb], None, None);
    assert_eq!(loaded.messages.len(), 5);
}

#[test]
fn build_session_context_tracks_model_and_thinking_level() {
    let mut b = Builder::new();
    let entries = vec![
        b.message(user_message("1")),
        b.model_change("openai", "gpt-4"),
        b.message(assistant_message("a", None)),
        b.thinking_level("high"),
    ];
    let loaded = build_session_context(&entries, None, None);
    let model = loaded.model.unwrap();
    assert_eq!((model.provider.as_str(), model.model_id.as_str()), ("anthropic", "claude-sonnet-4-5"));
    assert_eq!(loaded.thinking_level, "high");
}

// ---------------------------------------------------------------------------
// prepareCompaction
// ---------------------------------------------------------------------------

#[test]
fn prepare_compaction_does_not_treat_system_messages_as_history() {
    let mut b = Builder::new();
    let system = b.message(json!({ "role": "system", "content": "", "sections": { "preamble": "current prompt" }, "timestamp": now_ms() }));
    let user = b.message(user_message("one long turn"));
    let assistant = b.message(assistant_message("assistant suffix", None));
    let preparation = prepare_compaction(&[system, user.clone(), assistant.clone()], &CompactionSettings {
        keep_recent_tokens: 1,
        ..CompactionSettings::default()
    })
    .unwrap();
    assert_eq!(preparation.first_kept_entry_id, assistant.id());
    assert!(preparation.is_split_turn);
    assert!(preparation.messages_to_summarize.is_empty());
    assert_eq!(preparation.turn_prefix_messages, vec![message_of(&user).clone()]);
}

#[test]
fn prepare_compaction_returns_none_after_a_compaction_entry() {
    let mut b = Builder::new();
    let u = b.message(user_message("x"));
    let c = b.compaction("s", u.id());
    assert!(prepare_compaction(&[u, c], &CompactionSettings::default()).is_none());
}

#[test]
fn skips_repeated_compactions_when_kept_messages_still_fit() {
    let mut b = Builder::new();
    let u1 = b.message(user_message("user msg 1 (summarized by compaction1)"));
    let a1 = b.message(assistant_message("assistant msg 1", None));
    let u2 = b.message(user_message("user msg 2 - kept by compaction1"));
    let a2 = b.message(assistant_message("assistant msg 2", None));
    let u3 = b.message(user_message("user msg 3 - kept by compaction1"));
    let a3 = b.message(assistant_message("assistant msg 3", Some(mock_usage(5000, 1000, 0, 0))));
    let compaction1 = b.compaction("First summary", u2.id());
    let u4 = b.message(user_message("user msg 4 (new after compaction1)"));
    let a4 = b.message(assistant_message("assistant msg 4", Some(mock_usage(8000, 2000, 0, 0))));
    assert!(prepare_compaction(&[u1, a1, u2, a2, u3, a3, compaction1, u4, a4], &DEFAULT_COMPACTION_SETTINGS).is_none());
}

#[test]
fn re_summarizes_previously_kept_messages_when_the_window_moves_past_them() {
    let mut b = Builder::new();
    let u1 = b.message(user_message(&"user msg 1 (summarized by compaction1)".repeat(4)));
    let a1 = b.message(assistant_message(&"assistant msg 1".repeat(4), None));
    let u2 = b.message(user_message(&"user msg 2 - kept by compaction1 ".repeat(12)));
    let a2 = b.message(assistant_message(&"assistant msg 2 ".repeat(12), None));
    let u3 = b.message(user_message(&"user msg 3 - kept by compaction1 ".repeat(12)));
    let a3 = b.message(assistant_message(&"assistant msg 3 ".repeat(12), Some(mock_usage(5000, 1000, 0, 0))));
    let compaction1 = b.compaction("First summary", u2.id());
    let u4 = b.message(user_message(&"user msg 4 (new after compaction1) ".repeat(12)));
    let a4 = b.message(assistant_message(&"assistant msg 4 ".repeat(12), Some(mock_usage(8000, 2000, 0, 0))));
    let settings = CompactionSettings {
        keep_recent_tokens: 100,
        ..CompactionSettings::default()
    };
    let preparation = prepare_compaction(&[u1, a1, u2, a2, u3, a3, compaction1, u4, a4], &settings).unwrap();
    let summarized = extract_text(&preparation.messages_to_summarize);
    assert!(summarized.contains("user msg 2 - kept by compaction1"));
    assert!(summarized.contains("user msg 3 - kept by compaction1"));
    assert!(!summarized.contains("First summary"));
    assert_eq!(preparation.previous_summary.as_deref(), Some("First summary"));
}

#[test]
fn prepare_compaction_carries_file_operations_from_previous_compaction() {
    let mut b = Builder::new();
    let u1 = b.message(user_message(&"old ".repeat(100)));
    let mut previous = b.compaction("Earlier", u1.id());
    if let SessionEntry::Compaction(c) = &mut previous {
        c.details = Some(json!({ "readFiles": ["old-read.ts"], "modifiedFiles": ["old-mod.ts"] }));
    }
    let mut call = assistant_message("", None);
    call["content"] = json!([
        { "type": "toolCall", "id": "c1", "name": "read", "arguments": { "path": "new-read.ts" } },
        { "type": "toolCall", "id": "c2", "name": "write", "arguments": { "path": "old-read.ts" } }
    ]);
    let a1 = b.message(call);
    let u2 = b.message(user_message(&"new ".repeat(100)));
    let a2 = b.message(assistant_message("done", None));
    let preparation = prepare_compaction(&[u1, previous, a1, u2, a2], &CompactionSettings {
        keep_recent_tokens: 1,
        ..CompactionSettings::default()
    })
    .unwrap();
    let lists = compute_file_lists(&preparation.file_ops);
    assert_eq!(lists.read_files, ["new-read.ts"]);
    assert_eq!(lists.modified_files, ["old-mod.ts", "old-read.ts"]);
}

// ---------------------------------------------------------------------------
// Large session fixture
// ---------------------------------------------------------------------------

#[test]
fn large_session_fixture() {
    let entries = load_large_session_entries();
    assert!(entries.len() > 100);
    assert!(entries.iter().filter(|e| e.type_name() == "message").count() > 100);
    // Every v1 entry received an id and is chained to the previous one.
    assert!(entries.iter().all(|e| e.id().len() == 8));
    assert!(entries.windows(2).all(|w| w[1].parent_id() == Some(w[0].id())));

    let result = find_cut_point(
        &entries,
        0,
        entries.len() as i64,
        DEFAULT_COMPACTION_SETTINGS.keep_recent_tokens,
    );
    let cut = &entries[result.first_kept_entry_index as usize];
    assert_eq!(cut.type_name(), "message");
    assert!(matches!(message_of(cut).role(), "user" | "assistant"));

    let loaded = build_session_context(&entries, None, None);
    assert!(loaded.messages.len() > 100);
    assert!(loaded.model.is_some());

    let preparation = prepare_compaction(&entries, &DEFAULT_COMPACTION_SETTINGS).unwrap();
    assert!(preparation.tokens_before > 0);
    assert!(!preparation.messages_to_summarize.is_empty());
}

// ---------------------------------------------------------------------------
// serializeConversation / file operations
// ---------------------------------------------------------------------------

fn llm(value: Value) -> Message {
    serde_json::from_value(value).unwrap()
}

#[test]
fn serialize_conversation_truncates_long_tool_results_only() {
    let long = "x".repeat(5000);
    let result = serialize_conversation(&[llm(json!({
        "role": "toolResult", "toolCallId": "tc1", "toolName": "read",
        "content": [{ "type": "text", "text": long }], "isError": false, "timestamp": 1
    }))]);
    assert!(result.contains("[Tool result]:"));
    assert!(result.contains("[... 3000 more characters truncated]"));
    assert!(!result.contains(&"x".repeat(3000)));
    assert!(result.contains(&"x".repeat(2000)));

    let short = "x".repeat(1500);
    let result = serialize_conversation(&[llm(json!({
        "role": "toolResult", "toolCallId": "tc1", "toolName": "read",
        "content": [{ "type": "text", "text": short }], "isError": false, "timestamp": 1
    }))]);
    assert_eq!(result, format!("[Tool result]: {short}"));

    let long_text = "y".repeat(5000);
    let result = serialize_conversation(&[
        llm(json!({ "role": "user", "content": [{ "type": "text", "text": long_text }], "timestamp": 1 })),
        llm(json!({
            "role": "assistant", "content": [{ "type": "text", "text": long_text }], "api": "anthropic",
            "provider": "anthropic", "model": "test", "usage": usage_json(0, 0), "stopReason": "stop", "timestamp": 1
        })),
    ]);
    assert!(!result.contains("truncated"));
    assert!(result.contains(&long_text));
}

#[test]
fn serialize_conversation_omits_thinking() {
    let result = serialize_conversation(&[llm(json!({
        "role": "assistant",
        "content": [
            { "type": "thinking", "thinking": "private reasoning", "thinkingSignature": "sig" },
            { "type": "text", "text": "visible reply" },
            { "type": "toolCall", "id": "tc1", "name": "read", "arguments": { "path": "a.ts" } }
        ],
        "api": "anthropic", "provider": "anthropic", "model": "test", "usage": usage_json(0, 0),
        "stopReason": "toolUse", "timestamp": 1
    }))]);
    assert_eq!(result, "[Assistant]: visible reply\n\n[Assistant tool calls]: read(path=\"a.ts\")");
}

#[test]
fn file_operations_include_nested_calls_recorded_on_tool_results() {
    let result = agent_message(json!({
        "role": "toolResult", "toolCallId": "codemode-1", "toolName": "codemode", "content": [],
        "isError": false, "timestamp": 0,
        "nestedCalls": {
            "calls": [
                { "id": "codemode-1/1", "name": "read", "arguments": { "path": "a.ts" }, "status": "ok" },
                { "id": "codemode-1/2", "name": "edit", "arguments": { "path": "b.ts", "edits": [] }, "status": "ok" },
                { "id": "codemode-1/3", "name": "write", "argumentsBytes": 40000, "status": "ok" }
            ],
            "complete": false
        }
    }));
    let mut file_ops = create_file_ops();
    extract_file_ops_from_message(&result, &mut file_ops);
    assert_eq!(compute_file_lists(&file_ops), ComputeFileListsResult {
        read_files: vec!["a.ts".into()],
        modified_files: vec!["b.ts".into()]
    });
    assert_eq!(
        format_file_operations(&["a.ts".into()], &["b.ts".into()]),
        "\n\n<read-files>\na.ts\n</read-files>\n\n<modified-files>\nb.ts\n</modified-files>"
    );
    assert_eq!(format_file_operations(&[], &[]), "");
}

// ---------------------------------------------------------------------------
// Summarization requests (recording stream function)
// ---------------------------------------------------------------------------

struct Call {
    context: Value,
    options: SimpleStreamOptions,
}

fn create_model(reasoning: bool) -> Model {
    serde_json::from_value(json!({
        "id": if reasoning { "reasoning-model" } else { "non-reasoning-model" },
        "name": "Model",
        "api": "anthropic-messages",
        "provider": "anthropic",
        "baseUrl": "https://api.anthropic.com",
        "reasoning": reasoning,
        "input": ["text"],
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
        "contextWindow": 200000,
        "maxTokens": 8192
    }))
    .unwrap()
}

fn summary_response() -> AssistantMessage {
    serde_json::from_value(json!({
        "content": [{ "type": "text", "text": "## Goal\nTest summary" }],
        "api": "anthropic-messages",
        "provider": "anthropic",
        "model": "claude-sonnet-4-5",
        "usage": {
            "input": 10, "output": 10, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 20,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 }
        },
        "stopReason": "stop",
        "timestamp": 1
    }))
    .unwrap()
}

fn tool_call_response() -> AssistantMessage {
    let mut response = summary_response();
    response.content = serde_json::from_value(json!([
        { "type": "toolCall", "id": "tool-call-1", "name": "read", "arguments": { "path": "README.md" } }
    ]))
    .unwrap();
    response.stop_reason = StopReason::ToolUse;
    response
}

fn length_response() -> AssistantMessage {
    let mut response = summary_response();
    response.content = serde_json::from_value(json!([{ "type": "text", "text": "partial" }])).unwrap();
    response.stop_reason = StopReason::Length;
    response
}

/// A stream function that records requests and answers with the queued responses (then the default).
fn recording_stream(responses: Vec<AssistantMessage>) -> (StreamFn, Arc<Mutex<Vec<Call>>>) {
    let calls: Arc<Mutex<Vec<Call>>> = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(Mutex::new(responses));
    let recorded = calls.clone();
    let stream_fn = pi_agent::types::stream_fn(move |_model, context, options| {
        recorded.lock().push(Call {
            context: serde_json::to_value(&context).unwrap(),
            options,
        });
        let response = {
            let mut queue = queue.lock();
            if queue.is_empty() {
                summary_response()
            } else {
                queue.remove(0)
            }
        };
        let (sender, stream) = AssistantMessageEventStream::new();
        let reason = response.stop_reason;
        sender.push(AssistantMessageEvent::Done {
            reason,
            message: response,
        });
        stream
    });
    (stream_fn, calls)
}

fn messages() -> Vec<AgentMessage> {
    vec![agent_message(json!({ "role": "user", "content": "Summarize this.", "timestamp": 1 }))]
}

fn split_preparation(previous_summary: Option<&str>, messages_to_summarize: Vec<AgentMessage>) -> CompactionPreparation {
    CompactionPreparation {
        first_kept_entry_id: "entry-keep".into(),
        messages_to_summarize,
        turn_prefix_messages: messages(),
        is_split_turn: true,
        tokens_before: 100,
        previous_summary: previous_summary.map(str::to_string),
        file_ops: create_file_ops(),
        settings: CompactionSettings {
            enabled: true,
            reserve_tokens: 2000,
            keep_recent_tokens: 20,
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn summarize(
    model: &Model,
    thinking_level: Option<ThinkingLevel>,
    stream_fn: StreamFn,
) -> anyhow::Result<GenerateSummaryWithUsageResult> {
    generate_summary_with_usage(
        &messages(),
        model,
        None,
        Some("test-key"),
        None,
        None,
        None,
        None,
        thinking_level,
        Some(stream_fn),
        None,
        None,
        None,
        None,
    )
    .await
}

#[tokio::test]
async fn uses_the_provided_thinking_level_for_reasoning_capable_models() {
    let (stream_fn, calls) = recording_stream(vec![]);
    let result = summarize(&create_model(true), Some(ThinkingLevel::Medium), stream_fn).await.unwrap();
    assert_eq!(result.text, "## Goal\nTest summary");
    assert_eq!(result.usage, summary_response().usage);
    let calls = calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].options.reasoning, Some(pi_ai::providers::model_schema::ThinkingLevel::Medium));
    assert_eq!(calls[0].options.api_key.as_deref(), Some("test-key"));
}

#[tokio::test]
async fn does_not_set_reasoning_when_off_or_unsupported() {
    let (stream_fn, calls) = recording_stream(vec![]);
    summarize(&create_model(true), Some(ThinkingLevel::Off), stream_fn.clone()).await.unwrap();
    summarize(&create_model(false), Some(ThinkingLevel::Medium), stream_fn).await.unwrap();
    let calls = calls.lock();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.options.reasoning.is_none()));
    assert!(calls.iter().all(|c| c.options.api_key.as_deref() == Some("test-key")));
}

#[tokio::test]
async fn uses_fresh_routing_sessions_without_prompt_caching() {
    let (stream_fn, calls) = recording_stream(vec![]);
    let text = generate_summary(
        &messages(), &create_model(false), None, Some("test-key"), None, None, None, None, None,
        Some(stream_fn.clone()), None, None, None, None,
    )
    .await
    .unwrap();
    assert_eq!(text, "## Goal\nTest summary");
    summarize(&create_model(false), None, stream_fn).await.unwrap();
    let calls = calls.lock();
    assert!(calls
        .iter()
        .all(|c| c.options.cache_retention == Some(pi_ai::providers::model_schema::CacheRetention::None)));
    assert_ne!(calls[0].options.session_id, calls[1].options.session_id);
    assert!(calls[0].options.session_id.is_some());
    // The request is a system prompt plus one user prompt.
    let context = &calls[0].context["messages"];
    assert_eq!(context[0]["role"], "system");
    assert_eq!(context[0]["content"], SUMMARIZATION_SYSTEM_PROMPT);
    assert_eq!(context[1]["role"], "user");
    let prompt = context[1]["content"][0]["text"].as_str().unwrap();
    assert!(prompt.starts_with("<conversation>\n[User]: Summarize this.\n</conversation>\n\n"));
}

#[tokio::test]
async fn honors_caller_supplied_routing_session_and_tool_choice_without_prompt_caching() {
    let (stream_fn, calls) = recording_stream(vec![]);
    let context = pi_ai::normalize_context(&pi_ai::types::Context {
        system_prompt: Some("Summarize".into()),
        messages: vec![],
        tools: None,
    });
    let options = SimpleStreamOptions {
        session_id: Some("current-routing-session".into()),
        cache_retention: Some(pi_ai::providers::model_schema::CacheRetention::Long),
        tool_choice: Some(serde_json::from_value(json!("auto")).unwrap()),
        ..Default::default()
    };
    complete_summarization(&create_model(false), context, options, Some(stream_fn), None, None)
        .await
        .unwrap();
    let calls = calls.lock();
    assert_eq!(calls[0].options.session_id.as_deref(), Some("current-routing-session"));
    assert_eq!(calls[0].options.cache_retention, Some(pi_ai::providers::model_schema::CacheRetention::None));
    assert!(calls[0].options.tool_choice.is_some());
}

#[tokio::test]
async fn preserves_previous_summary_without_an_empty_history_request_for_a_split_turn() {
    let (stream_fn, calls) = recording_stream(vec![]);
    let result = compact(
        &split_preparation(Some("previous checkpoint"), vec![]),
        &create_model(false),
        Some("test-key"),
        None,
        None,
        None,
        None,
        Some(stream_fn),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let calls = calls.lock();
    assert_eq!(calls.len(), 1);
    assert!(result.summary.contains("previous checkpoint"));
    assert!(result.summary.contains("**Turn Context (split turn):**"));
    let prompt = serde_json::to_string(&calls[0].context["messages"]).unwrap();
    assert!(prompt.contains("# Conversation\\n[User]: Summarize this."));
    assert!(prompt.contains("# Instructions\\nThe messages above are earlier context from an ongoing conversation."));
    assert_eq!(result.details, Some(json!({ "readFiles": [], "modifiedFiles": [] })));
}

#[tokio::test]
async fn applies_custom_instructions_to_the_turn_prefix_summary() {
    let (stream_fn, calls) = recording_stream(vec![]);
    compact(
        &split_preparation(Some("previous checkpoint"), vec![]),
        &create_model(false),
        Some("test-key"),
        None,
        Some("keep migration decisions"),
        None,
        None,
        Some(stream_fn),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let calls = calls.lock();
    assert_eq!(calls.len(), 1);
    assert!(serde_json::to_string(&calls[0].context["messages"])
        .unwrap()
        .contains("Additional focus: keep migration decisions"));
}

#[tokio::test]
async fn rejects_tool_calls_and_length_limited_summaries() {
    let (stream_fn, _) = recording_stream(vec![tool_call_response()]);
    let error = summarize(&create_model(false), None, stream_fn).await.unwrap_err();
    assert_eq!(error.to_string(), "Summarization attempted to call a tool");

    let (stream_fn, _) = recording_stream(vec![tool_call_response()]);
    let error = compact(
        &split_preparation(None, vec![]), &create_model(false), Some("test-key"), None, None, None, None,
        Some(stream_fn), None, None, None, None,
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Turn prefix summarization attempted to call a tool");

    let (stream_fn, _) = recording_stream(vec![length_response()]);
    let error = summarize(&create_model(false), None, stream_fn).await.unwrap_err();
    assert!(error.to_string().contains("generation hit the token cap"));

    let (stream_fn, _) = recording_stream(vec![length_response()]);
    let error = compact(
        &split_preparation(None, vec![]), &create_model(false), Some("test-key"), None, None, None, None,
        Some(stream_fn), None, None, None, None,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("generation hit the token cap"));
}

#[tokio::test]
async fn combines_usage_from_split_turn_summaries() {
    let (stream_fn, calls) = recording_stream(vec![]);
    let mut preparation = split_preparation(None, messages());
    preparation.tokens_before = 600000;
    let result = compact(
        &preparation, &create_model(false), Some("test-key"), None, None, None, None,
        Some(stream_fn), None, None, None, None,
    )
    .await
    .unwrap();
    assert_eq!(calls.lock().len(), 2);
    let usage = result.usage.unwrap();
    assert_eq!((usage.input, usage.output, usage.total_tokens), (20, 20, 40));
    assert_eq!(result.tokens_before, 600000);
    assert_eq!(result.first_kept_entry_id, "entry-keep");
}

#[tokio::test]
async fn stream_function_errors_propagate() {
    let failing: StreamFn = Arc::new(|_, _, _| Box::pin(async { Err(anyhow::anyhow!("boom")) }));
    let error = summarize(&create_model(false), None, failing).await.unwrap_err();
    assert_eq!(error.to_string(), "boom");
}

// ---------------------------------------------------------------------------
// Branch summarization
// ---------------------------------------------------------------------------

fn branch_entries() -> Vec<SessionEntry> {
    vec![entry(json!({
        "type": "message", "id": "branch-user", "parentId": null, "timestamp": "1970-01-01T00:00:00.001Z",
        "message": { "role": "user", "content": "Abandoned request", "timestamp": 1 }
    }))]
}

fn branch_options(stream_fn: StreamFn) -> GenerateBranchSummaryOptions {
    let mut model = create_model(false);
    model.id = "test-model".into();
    GenerateBranchSummaryOptions {
        model,
        api_key: None,
        headers: None,
        env: None,
        signal: tokio_util::sync::CancellationToken::new(),
        custom_instructions: None,
        replace_instructions: None,
        reserve_tokens: None,
        stream_fn: Some(stream_fn),
        retry: None,
        callbacks: None,
    }
}

#[tokio::test]
async fn branch_summary_does_not_override_tool_choice() {
    let (stream_fn, calls) = recording_stream(vec![]);
    let result = generate_branch_summary(&branch_entries(), &branch_options(stream_fn)).await.unwrap();
    let calls = calls.lock();
    assert!(calls[0].options.tool_choice.is_none());
    let summary = result.summary.unwrap();
    assert!(summary.starts_with("The user explored a different conversation branch before returning here.\nSummary of that exploration:\n\n## Goal"));
    assert_eq!(result.read_files, Some(vec![]));
}

#[tokio::test]
async fn branch_summary_rejects_tool_calls_and_length_stops() {
    let (stream_fn, _) = recording_stream(vec![tool_call_response()]);
    let result = generate_branch_summary(&branch_entries(), &branch_options(stream_fn)).await.unwrap();
    assert_eq!(result.error.as_deref(), Some("Branch summarization attempted to call a tool"));

    let (stream_fn, _) = recording_stream(vec![length_response()]);
    let result = generate_branch_summary(&branch_entries(), &branch_options(stream_fn)).await.unwrap();
    assert_eq!(
        result.error.as_deref(),
        Some("Branch summarization failed: generation hit the token cap and the summary is incomplete")
    );

    let mut aborted = summary_response();
    aborted.stop_reason = StopReason::Aborted;
    let (stream_fn, _) = recording_stream(vec![aborted]);
    let result = generate_branch_summary(&branch_entries(), &branch_options(stream_fn)).await.unwrap();
    assert_eq!(result.aborted, Some(true));
}

#[tokio::test]
async fn branch_summary_with_no_content() {
    let (stream_fn, calls) = recording_stream(vec![]);
    let entries = vec![entry(json!({
        "type": "model_change", "id": "m", "parentId": null, "timestamp": "2025-01-01T00:00:00Z",
        "provider": "a", "modelId": "b"
    }))];
    let result = generate_branch_summary(&entries, &branch_options(stream_fn)).await.unwrap();
    assert_eq!(result.summary.as_deref(), Some("No content to summarize"));
    assert!(calls.lock().is_empty());
}

#[test]
fn prepare_branch_entries_collects_file_ops_and_respects_budget() {
    let entries = vec![
        entry(json!({
            "type": "branch_summary", "id": "b1", "parentId": null, "timestamp": "2025-01-01T00:00:00Z",
            "fromId": "x", "summary": "earlier", "details": { "readFiles": ["r.ts"], "modifiedFiles": ["m.ts"] }
        })),
        entry(json!({
            "type": "branch_summary", "id": "b2", "parentId": "b1", "timestamp": "2025-01-01T00:00:00Z",
            "fromId": "x", "summary": "hook", "fromHook": true, "details": { "readFiles": ["hook.ts"] }
        })),
        entry(json!({
            "type": "message", "id": "u", "parentId": "b2", "timestamp": "2025-01-01T00:00:00Z",
            "message": { "role": "user", "content": "x".repeat(400), "timestamp": 1 }
        })),
        entry(json!({
            "type": "message", "id": "t", "parentId": "u", "timestamp": "2025-01-01T00:00:00Z",
            "message": { "role": "toolResult", "toolCallId": "c", "toolName": "read", "content": [], "isError": false, "timestamp": 1 }
        })),
    ];
    let all = prepare_branch_entries(&entries, None);
    assert_eq!(
        all.messages.iter().map(|m| m.role().to_string()).collect::<Vec<_>>(),
        ["branchSummary", "branchSummary", "user"]
    );
    let lists = compute_file_lists(&all.file_ops);
    assert_eq!(lists.read_files, ["r.ts"]);
    assert_eq!(lists.modified_files, ["m.ts"]);

    // Budget of 100 tokens: the 100-token user message fits, the summaries do not.
    let limited = prepare_branch_entries(&entries, Some(100));
    assert_eq!(limited.messages.len(), 1);
    assert_eq!(limited.total_tokens, 100);
}

#[test]
fn collect_entries_for_branch_summary_walks_to_common_ancestor() {
    let session = in_memory();
    let root = session.append_message(user_msg("root")).unwrap();
    let a1 = session.append_message(assistant_msg("a1")).unwrap();
    let a2 = session.append_message(user_msg("a2")).unwrap();
    session.branch(&root).unwrap();
    let b1 = session.append_message(assistant_msg("b1")).unwrap();

    let result = collect_entries_for_branch_summary(&session, Some(&a2), &b1);
    assert_eq!(result.common_ancestor_id.as_deref(), Some(root.as_str()));
    assert_eq!(
        result.entries.iter().map(|e| e.id().to_string()).collect::<Vec<_>>(),
        [a1, a2]
    );
    let none = collect_entries_for_branch_summary(&session, None, &b1);
    assert!(none.entries.is_empty());
    assert!(none.common_ancestor_id.is_none());
}
