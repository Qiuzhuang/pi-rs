//! Session files written by the TypeScript implementation load in Rust, and entries written by
//! Rust serialize exactly like the TypeScript writer (key order, optional fields, nulls).

use crate::common::*;
use pi_coding_agent::core::session_manager::{FileEntry, SessionEntry, SessionManager};
use serde_json::{Value, json};

/// One line per entry type, in the exact shape (and key order) `JSON.stringify` produces for
/// entries created by the TS `SessionManager`.
const TS_SESSION: &[&str] = &[
    r###"{"type":"session","version":3,"id":"019a1b2c-3d4e-7f60-8a9b-0c1d2e3f4a5b","timestamp":"2025-06-01T10:00:00.000Z","cwd":"/work/project"}"###,
    r###"{"type":"model_change","id":"a1b2c3d4","parentId":null,"timestamp":"2025-06-01T10:00:00.001Z","provider":"anthropic","modelId":"claude-sonnet-4-5"}"###,
    r###"{"type":"thinking_level_change","id":"b2c3d4e5","parentId":"a1b2c3d4","timestamp":"2025-06-01T10:00:00.002Z","thinkingLevel":"high"}"###,
    r###"{"type":"message","id":"c3d4e5f6","parentId":"b2c3d4e5","timestamp":"2025-06-01T10:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"Read the README"},{"type":"image","data":"aGVsbG8=","mimeType":"image/png"}],"timestamp":1748772001000}}"###,
    r###"{"type":"message","id":"d4e5f6a7","parentId":"c3d4e5f6","timestamp":"2025-06-01T10:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Let me read it.","thinkingSignature":"sig"},{"type":"text","text":"Reading."},{"type":"toolCall","id":"toolu_1","name":"read","arguments":{"path":"README.md"}}],"api":"anthropic-messages","provider":"anthropic","model":"claude-sonnet-4-5","usage":{"input":100,"output":20,"cacheRead":0,"cacheWrite":0,"totalTokens":120,"cost":{"input":0.0003,"output":0.0003,"cacheRead":0,"cacheWrite":0,"total":0.0006}},"stopReason":"toolUse","timestamp":1748772002000}}"###,
    r###"{"type":"message","id":"e5f6a7b8","parentId":"d4e5f6a7","timestamp":"2025-06-01T10:00:03.000Z","message":{"role":"toolResult","toolCallId":"toolu_1","toolName":"read","content":[{"type":"text","text":"# Project"}],"details":{"lines":1},"isError":false,"timestamp":1748772003000}}"###,
    r###"{"type":"custom","customType":"preset-state","data":{"name":"plan"},"id":"f6a7b8c9","parentId":"e5f6a7b8","timestamp":"2025-06-01T10:00:03.500Z"}"###,
    r###"{"type":"custom_message","customType":"reminder","content":"Remember the tests","display":true,"details":{"source":"ext"},"id":"a7b8c9d0","parentId":"f6a7b8c9","timestamp":"2025-06-01T10:00:04.000Z"}"###,
    r###"{"type":"message","id":"b8c9d0e1","parentId":"a7b8c9d0","timestamp":"2025-06-01T10:00:05.000Z","message":{"role":"bashExecution","command":"ls","output":"README.md","exitCode":0,"cancelled":false,"truncated":false,"timestamp":1748772005000}}"###,
    r###"{"type":"message","id":"c9d0e1f2","parentId":"b8c9d0e1","timestamp":"2025-06-01T10:00:06.000Z","message":{"role":"custom","customType":"note","content":"from extension","display":false,"timestamp":1748772006000}}"###,
    r###"{"type":"label","id":"d0e1f2a3","parentId":"c9d0e1f2","timestamp":"2025-06-01T10:00:07.000Z","targetId":"c3d4e5f6","label":"start"}"###,
    r###"{"type":"session_info","id":"e1f2a3b4","parentId":"d0e1f2a3","timestamp":"2025-06-01T10:00:08.000Z","name":"README work"}"###,
    r###"{"type":"context_edit","id":"f2a3b4c5","parentId":"e1f2a3b4","timestamp":"2025-06-01T10:00:09.000Z","targetId":"e5f6a7b8","replacement":{"content":[{"type":"text","text":"(elided)"}]}}"###,
    r###"{"type":"context_edit","id":"a3b4c5d6","parentId":"f2a3b4c5","timestamp":"2025-06-01T10:00:09.500Z","targetId":"a7b8c9d0","replacement":null}"###,
    r###"{"type":"usage","id":"b4c5d6e7","parentId":"a3b4c5d6","timestamp":"2025-06-01T10:00:10.000Z","kind":"cache_warm","provider":"anthropic","model":"claude-sonnet-4-5","usage":{"input":5,"output":0,"cacheRead":0,"cacheWrite":5,"totalTokens":10,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"note":"warm"}"###,
    r###"{"type":"compaction","id":"c5d6e7f8","parentId":"b4c5d6e7","timestamp":"2025-06-01T10:00:11.000Z","summary":"## Goal\nRead README","firstKeptEntryId":"e5f6a7b8","tokensBefore":1234,"details":{"readFiles":["README.md"],"modifiedFiles":[]},"usage":{"input":50,"output":10,"cacheRead":0,"cacheWrite":0,"totalTokens":60,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"fromHook":false}"###,
    r###"{"type":"label","id":"d6e7f8a9","parentId":"c5d6e7f8","timestamp":"2025-06-01T10:00:11.500Z","targetId":"c3d4e5f6"}"###,
    r###"{"type":"branch_summary","id":"e7f8a9b0","parentId":"c3d4e5f6","timestamp":"2025-06-01T10:00:12.000Z","fromId":"d6e7f8a9","summary":"Explored README","details":{"readFiles":[],"modifiedFiles":[]},"fromHook":false}"###,
    r###"{"type":"message","id":"f8a9b0c1","parentId":"e7f8a9b0","timestamp":"2025-06-01T10:00:13.000Z","message":{"role":"user","content":"New direction","timestamp":1748772013000}}"###,
];

fn write_ts_session(dir: &std::path::Path) -> String {
    let path = dir.join("2025-06-01T10-00-00-000Z_019a1b2c-3d4e-7f60-8a9b-0c1d2e3f4a5b.jsonl");
    std::fs::write(&path, TS_SESSION.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    path_str(&path)
}

#[test]
fn every_ts_entry_round_trips_byte_for_byte() {
    for line in TS_SESSION {
        let entry: FileEntry = serde_json::from_str(line).unwrap();
        assert!(!matches!(entry, FileEntry::Unknown(_)), "parsed as unknown: {line}");
        assert_eq!(serde_json::to_string(&entry).unwrap(), *line);
    }
}

#[test]
fn ts_session_loads_with_tree_labels_names_and_context() {
    let dir = tmp();
    let path = write_ts_session(dir.path());
    let original = std::fs::read_to_string(&path).unwrap();
    let sm = SessionManager::open(&path, None, None).unwrap();
    // Loading a current-version file never rewrites it.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

    assert_eq!(sm.get_session_id(), "019a1b2c-3d4e-7f60-8a9b-0c1d2e3f4a5b");
    assert_eq!(sm.get_cwd(), "/work/project");
    assert_eq!(sm.get_entries().len(), TS_SESSION.len() - 1);
    assert_eq!(sm.get_leaf_id().as_deref(), Some("f8a9b0c1"));
    assert_eq!(sm.get_session_name().as_deref(), Some("README work"));
    // The second label entry cleared the label.
    assert_eq!(sm.get_label("c3d4e5f6"), None);

    // Tree: the branch summary forks from the first user message.
    let tree = sm.get_tree();
    assert_eq!(tree.len(), 1);
    let mut node = &tree[0];
    while node.entry.id() != "c3d4e5f6" {
        node = &node.children[0];
    }
    assert_eq!(node.children.len(), 2);

    // Active branch: model/thinking from the path, user + branch summary + user.
    let ctx = sm.build_session_context();
    assert_eq!(ctx.thinking_level, "high");
    assert_eq!(ctx.model.as_ref().unwrap().model_id, "claude-sonnet-4-5");
    let roles: Vec<&str> = ctx.messages.iter().map(|m| m.role()).collect();
    assert_eq!(roles, ["user", "branchSummary", "user"]);

    // The other branch, through the compaction and context edits.
    sm.branch("d6e7f8a9").unwrap();
    let projection = sm.build_session_projection();
    let roles: Vec<&str> = projection.messages.iter().map(|m| m.role()).collect();
    assert_eq!(roles, ["compactionSummary", "toolResult", "bashExecution", "custom"]);
    assert_eq!(mjson(&projection.messages[1])["content"], json!([{ "type": "text", "text": "(elided)" }]));
}

#[test]
fn appending_to_a_ts_session_keeps_existing_lines() {
    let dir = tmp();
    let path = write_ts_session(dir.path());
    let original = std::fs::read_to_string(&path).unwrap();
    let sm = SessionManager::open(&path, None, None).unwrap();
    let id = sm.append_message(user_msg("from rust")).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.starts_with(&original));
    let last: Value = serde_json::from_str(content.lines().last().unwrap()).unwrap();
    assert_eq!(last["id"], json!(id));
    assert_eq!(last["parentId"], "f8a9b0c1");
    assert_eq!(last["message"]["content"], "from rust");
    // Field order matches the TS writer.
    let keys: Vec<&String> = last.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["type", "id", "parentId", "timestamp", "message"]);
}

#[test]
fn rust_written_entries_use_the_ts_layout() {
    let dir = tmp();
    let d = path_str(dir.path());
    let sm = SessionManager::create(&d, Some(&d), None).unwrap();
    sm.append_message(user_msg("q")).unwrap();
    sm.append_custom_entry("state", None).unwrap();
    sm.append_custom_message_entry("note", "hi".into(), true, None).unwrap();
    let target = sm.append_message(assistant_msg("a")).unwrap();
    sm.append_label_change(&target, None).unwrap();
    sm.append_context_edit(&target, None).unwrap();
    sm.append_compaction("s", None, 5, None, None, None).unwrap();
    sm.branch_with_summary(Some(&target), "b", None, None, None).unwrap();

    let lines: Vec<Value> = std::fs::read_to_string(sm.get_session_file().unwrap())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&lines[0]), ["type", "version", "id", "timestamp", "cwd"]);
    assert_eq!(keys(&lines[2]), ["type", "customType", "id", "parentId", "timestamp"]);
    assert_eq!(keys(&lines[3]), ["type", "customType", "content", "display", "id", "parentId", "timestamp"]);
    assert_eq!(keys(&lines[5]), ["type", "id", "parentId", "timestamp", "targetId"]);
    assert_eq!(keys(&lines[6]), ["type", "id", "parentId", "timestamp", "targetId", "replacement"]);
    assert_eq!(lines[6]["replacement"], Value::Null);
    assert_eq!(
        keys(&lines[7]),
        ["type", "id", "parentId", "timestamp", "summary", "firstKeptEntryId", "tokensBefore"]
    );
    assert_eq!(lines[7]["firstKeptEntryId"], lines[7]["id"]);
    assert_eq!(keys(&lines[8]), ["type", "id", "parentId", "timestamp", "fromId", "summary"]);
    // ISO timestamps with milliseconds, like Date.toISOString().
    let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$").unwrap();
    assert!(lines.iter().all(|l| re.is_match(l["timestamp"].as_str().unwrap())));
}

#[test]
fn unknown_entry_types_are_preserved_but_not_part_of_the_tree() {
    let dir = tmp();
    let src = dir.path().join("future.jsonl");
    let future = r###"{"type":"future_thing","id":"zz","parentId":null,"timestamp":"2025-06-01T10:00:00.000Z","payload":[1,2]}"###;
    std::fs::write(&src, format!("{}\n{future}\n{}\n", TS_SESSION[0], TS_SESSION[1])).unwrap();
    let sm = SessionManager::open(&path_str(&src), None, None).unwrap();
    assert_eq!(sm.get_entries().len(), 1);
    assert!(matches!(sm.get_entries()[0], SessionEntry::ModelChange(_)));

    let forked = SessionManager::fork_from(&path_str(&src), &path_str(dir.path()), Some(&path_str(dir.path())), None)
        .unwrap();
    let content = std::fs::read_to_string(forked.get_session_file().unwrap()).unwrap();
    assert_eq!(content.lines().nth(1).unwrap(), future);
    assert_eq!(content.lines().nth(2).unwrap(), TS_SESSION[1]);
}
