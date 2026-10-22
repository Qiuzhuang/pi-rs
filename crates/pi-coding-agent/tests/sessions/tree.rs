//! Port of `test/session-manager/tree-traversal.test.ts` and `labels.test.ts`.

use crate::common::*;
use pi_ai::types::Usage;
use pi_coding_agent::core::session_manager::{SessionEntry, SessionManager};
use serde_json::json;

fn ids(entries: &[SessionEntry]) -> Vec<String> {
    entries.iter().map(|e| e.id().to_string()).collect()
}

fn sample_usage() -> Usage {
    serde_json::from_value(json!({
        "input": 10, "output": 20, "cacheRead": 30, "cacheWrite": 40, "totalTokens": 100,
        "cost": { "input": 0.1, "output": 0.2, "cacheRead": 0.3, "cacheWrite": 0.4, "total": 1 }
    }))
    .unwrap()
}

#[test]
fn append_message_creates_entry_with_correct_parent_chain() {
    let session = in_memory();
    let id1 = session.append_message(user_msg("first")).unwrap();
    let id2 = session.append_message(assistant_msg("second")).unwrap();
    let id3 = session.append_message(user_msg("third")).unwrap();

    let entries = session.get_entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id(), id1);
    assert_eq!(entries[0].parent_id(), None);
    assert_eq!(entries[0].type_name(), "message");
    assert_eq!(entries[1].id(), id2);
    assert_eq!(entries[1].parent_id(), Some(id1.as_str()));
    assert_eq!(entries[2].id(), id3);
    assert_eq!(entries[2].parent_id(), Some(id2.as_str()));
    // TS ids are the first 8 hex chars of a UUID.
    assert_eq!(id1.len(), 8);
}

#[test]
fn setup_entries_integrate_into_tree() {
    let session = in_memory();
    let msg_id = session.append_message(user_msg("hello")).unwrap();
    let thinking_id = session.append_thinking_level_change("high").unwrap();
    let model_id = session.append_model_change("openai", "gpt-4").unwrap();
    session.append_message(assistant_msg("response")).unwrap();

    let entries = session.get_entries();
    assert_eq!(entries[1].id(), thinking_id);
    assert_eq!(entries[1].parent_id(), Some(msg_id.as_str()));
    let SessionEntry::ModelChange(model) = &entries[2] else {
        panic!("expected model change")
    };
    assert_eq!(model.id, model_id);
    assert_eq!(model.parent_id.as_deref(), Some(thinking_id.as_str()));
    assert_eq!(model.provider, "openai");
    assert_eq!(model.model_id, "gpt-4");
    assert_eq!(entries[3].parent_id(), Some(model_id.as_str()));
}

#[test]
fn append_compaction_integrates_into_tree() {
    let session = in_memory();
    let id1 = session.append_message(user_msg("1")).unwrap();
    let id2 = session.append_message(assistant_msg("2")).unwrap();
    let usage = sample_usage();
    let compaction_id = session
        .append_compaction("summary", Some(&id1), 1000, None, Some(false), Some(usage.clone()))
        .unwrap();
    session.append_message(user_msg("3")).unwrap();

    let entries = session.get_entries();
    let SessionEntry::Compaction(compaction) = &entries[2] else {
        panic!("expected compaction")
    };
    assert_eq!(compaction.id, compaction_id);
    assert_eq!(compaction.parent_id.as_deref(), Some(id2.as_str()));
    assert_eq!(compaction.summary, "summary");
    assert_eq!(compaction.first_kept_entry_id, id1);
    assert_eq!(compaction.tokens_before, 1000);
    assert_eq!(compaction.usage.as_ref(), Some(&usage));
    assert_eq!(entries[3].parent_id(), Some(compaction_id.as_str()));
}

#[test]
fn append_compaction_without_first_kept_uses_its_own_id() {
    let session = in_memory();
    session.append_message(user_msg("1")).unwrap();
    let id = session.append_compaction("s", None, 1, None, None, None).unwrap();
    let Some(SessionEntry::Compaction(c)) = session.get_entry(&id) else {
        panic!()
    };
    assert_eq!(c.first_kept_entry_id, id);
}

#[test]
fn leaf_pointer_advances_after_each_append() {
    let session = in_memory();
    assert_eq!(session.get_leaf_id(), None);
    let id1 = session.append_message(user_msg("1")).unwrap();
    assert_eq!(session.get_leaf_id(), Some(id1));
    let id2 = session.append_message(assistant_msg("2")).unwrap();
    assert_eq!(session.get_leaf_id(), Some(id2));
    let id3 = session.append_thinking_level_change("high").unwrap();
    assert_eq!(session.get_leaf_id(), Some(id3));
}

#[test]
fn get_branch_paths() {
    let session = in_memory();
    assert!(session.get_branch(None).is_empty());
    let id1 = session.append_message(user_msg("1")).unwrap();
    let id2 = session.append_message(assistant_msg("2")).unwrap();
    let id3 = session.append_thinking_level_change("high").unwrap();
    let id4 = session.append_message(user_msg("3")).unwrap();
    assert_eq!(ids(&session.get_branch(None)), [id1.clone(), id2.clone(), id3, id4]);
    assert_eq!(ids(&session.get_branch(Some(&id2))), [id1, id2]);
}

#[test]
fn get_tree_linear_and_branched() {
    let session = in_memory();
    assert!(session.get_tree().is_empty());
    let id1 = session.append_message(user_msg("1")).unwrap();
    let id2 = session.append_message(assistant_msg("2")).unwrap();
    let id3 = session.append_message(user_msg("3")).unwrap();

    let tree = session.get_tree();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].entry.id(), id1);
    assert_eq!(tree[0].children.len(), 1);
    assert_eq!(tree[0].children[0].entry.id(), id2);
    assert_eq!(tree[0].children[0].children[0].entry.id(), id3);
    assert!(tree[0].children[0].children[0].children.is_empty());

    session.branch(&id2).unwrap();
    let id4 = session.append_message(user_msg("4-branch")).unwrap();
    let tree = session.get_tree();
    let node2 = &tree[0].children[0];
    let mut child_ids: Vec<String> = node2.children.iter().map(|c| c.entry.id().to_string()).collect();
    child_ids.sort();
    let mut expected = vec![id3, id4];
    expected.sort();
    assert_eq!(child_ids, expected);
}

#[test]
fn get_tree_handles_deep_branching() {
    let session = in_memory();
    session.append_message(user_msg("1")).unwrap();
    let id2 = session.append_message(assistant_msg("2")).unwrap();
    let id3 = session.append_message(user_msg("3")).unwrap();
    session.append_message(assistant_msg("4")).unwrap();
    session.branch(&id2).unwrap();
    let id5 = session.append_message(user_msg("5")).unwrap();
    session.append_message(assistant_msg("6")).unwrap();
    session.branch(&id5).unwrap();
    session.append_message(user_msg("7")).unwrap();

    let tree = session.get_tree();
    let node2 = &tree[0].children[0];
    assert_eq!(node2.children.len(), 2);
    let node5 = node2.children.iter().find(|c| c.entry.id() == id5).unwrap();
    assert_eq!(node5.children.len(), 2);
    let node3 = node2.children.iter().find(|c| c.entry.id() == id3).unwrap();
    assert_eq!(node3.children.len(), 1);
}

#[test]
fn get_tree_handles_very_deep_linear_sessions() {
    let session = in_memory();
    for i in 0..20_000 {
        session.append_thinking_level_change(if i % 2 == 0 { "low" } else { "high" }).unwrap();
    }
    let tree = session.get_tree();
    assert_eq!(tree.len(), 1);
    let mut depth = 1;
    let mut node = &tree[0];
    while let Some(child) = node.children.first() {
        depth += 1;
        node = child;
    }
    assert_eq!(depth, 20_000);
}

#[test]
fn branch_moves_leaf_and_new_appends_become_children() {
    let session = in_memory();
    let id1 = session.append_message(user_msg("1")).unwrap();
    session.append_message(assistant_msg("2")).unwrap();
    session.branch(&id1).unwrap();
    assert_eq!(session.get_leaf_id(), Some(id1.clone()));
    let id3 = session.append_message(user_msg("branched")).unwrap();
    assert_eq!(session.get_entry(&id3).unwrap().parent_id(), Some(id1.as_str()));
    assert_eq!(
        session.branch("nonexistent").unwrap_err().to_string(),
        "Entry nonexistent not found"
    );
}

#[test]
fn branch_with_summary_inserts_summary_and_advances_leaf() {
    let session = in_memory();
    let id1 = session.append_message(user_msg("1")).unwrap();
    session.append_message(assistant_msg("2")).unwrap();
    let id3 = session.append_message(user_msg("3")).unwrap();
    let usage = sample_usage();
    let summary_id = session
        .branch_with_summary(Some(&id1), "Summary of abandoned work", None, Some(false), Some(usage.clone()))
        .unwrap();
    assert_eq!(session.get_leaf_id(), Some(summary_id.clone()));
    let Some(SessionEntry::BranchSummary(summary)) = session.get_entry(&summary_id) else {
        panic!()
    };
    assert_eq!(summary.parent_id.as_deref(), Some(id1.as_str()));
    assert_eq!(summary.from_id, id3);
    assert_eq!(summary.summary, "Summary of abandoned work");
    assert_eq!(summary.usage, Some(usage));

    assert_eq!(
        session
            .branch_with_summary(Some("nonexistent"), "summary", None, None, None)
            .unwrap_err()
            .to_string(),
        "Entry nonexistent not found"
    );
}

#[test]
fn leaf_entry_and_entry_lookup() {
    let session = in_memory();
    assert!(session.get_leaf_entry().is_none());
    assert!(session.get_entry("nonexistent").is_none());
    let id1 = session.append_message(user_msg("first")).unwrap();
    let id2 = session.append_message(assistant_msg("second")).unwrap();
    assert_eq!(session.get_leaf_entry().unwrap().id(), id2);
    let entry1 = session.get_entry(&id1).unwrap();
    assert_eq!(entry1.type_name(), "message");
    assert_eq!(mjson(entry1.message().unwrap())["content"], "first");
    let entry2 = session.get_entry(&id2).unwrap();
    assert_eq!(mjson(entry2.message().unwrap())["content"][0]["text"], "second");
}

#[test]
fn build_session_context_returns_current_branch_only() {
    let session = in_memory();
    session.append_message(user_msg("msg1")).unwrap();
    let id2 = session.append_message(assistant_msg("msg2")).unwrap();
    session.append_message(user_msg("msg3")).unwrap();
    session.branch(&id2).unwrap();
    session.append_message(assistant_msg("msg4-branch")).unwrap();

    let ctx = session.build_session_context();
    assert_eq!(ctx.messages.len(), 3);
    assert_eq!(mjson(&ctx.messages[0])["content"], "msg1");
    assert_eq!(mjson(&ctx.messages[1])["content"][0]["text"], "msg2");
    assert_eq!(mjson(&ctx.messages[2])["content"][0]["text"], "msg4-branch");
}

#[test]
fn reset_leaf_creates_new_root() {
    let session = in_memory();
    session.append_message(user_msg("1")).unwrap();
    session.reset_leaf();
    assert!(session.build_session_context().messages.is_empty());
    let id = session.append_message(user_msg("again")).unwrap();
    assert_eq!(session.get_entry(&id).unwrap().parent_id(), None);
    assert_eq!(session.get_tree().len(), 2);
}

// createBranchedSession

#[test]
fn create_branched_session_in_memory() {
    let session = in_memory();
    session.append_message(user_msg("hello")).unwrap();
    assert_eq!(
        session.create_branched_session("nonexistent").unwrap_err().to_string(),
        "Entry nonexistent not found"
    );

    let session = in_memory();
    let id1 = session.append_message(user_msg("1")).unwrap();
    let id2 = session.append_message(assistant_msg("2")).unwrap();
    let id3 = session.append_message(user_msg("3")).unwrap();
    session.append_message(assistant_msg("4")).unwrap();
    session.branch(&id3).unwrap();
    session.append_message(user_msg("5")).unwrap();

    assert_eq!(session.create_branched_session(&id2).unwrap(), None);
    assert_eq!(ids(&session.get_entries()), [id1, id2]);
    assert!(is_uuid_v7(&session.get_session_id()));
    assert_eq!(session.get_header().unwrap().id, session.get_session_id());
}

#[test]
fn create_branched_session_extracts_correct_path() {
    let session = in_memory();
    let id1 = session.append_message(user_msg("1")).unwrap();
    let id2 = session.append_message(assistant_msg("2")).unwrap();
    session.append_message(user_msg("3")).unwrap();
    session.branch(&id2).unwrap();
    let id4 = session.append_message(user_msg("4")).unwrap();
    let id5 = session.append_message(assistant_msg("5")).unwrap();
    session.create_branched_session(&id5).unwrap();
    assert_eq!(ids(&session.get_entries()), [id1, id2, id4, id5]);
}

#[test]
fn fork_from_before_first_user_message_does_not_duplicate_entries() {
    let dir = tmp();
    let d = path_str(dir.path());
    let session = SessionManager::create(&d, Some(&d), None).unwrap();
    let model_change_id = session.append_model_change("anthropic", "claude-sonnet-4-5").unwrap();
    session.append_message(user_msg("first question")).unwrap();
    session.append_message(assistant_msg("first answer")).unwrap();

    let new_file = session.create_branched_session(&model_change_id).unwrap().unwrap();
    assert!(!std::path::Path::new(&new_file).exists());
    session.append_message(user_msg("new question")).unwrap();
    assert!(std::path::Path::new(&new_file).exists());
    session.append_custom_entry("preset-state", Some(json!({ "name": "plan" }))).unwrap();
    session.append_message(assistant_msg("new answer")).unwrap();
    assert_eq!(
        read_session_file_roles(&new_file),
        ["session", "model_change", "user", "custom", "assistant"]
    );
}

#[test]
fn preserves_tool_and_summary_usage_across_file_backed_reload() {
    let dir = tmp();
    let d = path_str(dir.path());
    let session = SessionManager::create(&d, Some(&d), None).unwrap();
    let root_id = session.append_message(user_msg("question")).unwrap();
    session.append_message(assistant_msg("answer")).unwrap();
    let usage = sample_usage();
    session
        .append_message(agent_message(json!({
            "role": "toolResult", "toolCallId": "call-1", "toolName": "nested-model",
            "content": [{ "type": "text", "text": "result" }], "isError": false,
            "usage": serde_json::to_value(&usage).unwrap(), "timestamp": now_ms()
        })))
        .unwrap();
    session
        .append_compaction("summary", Some(&root_id), 100, None, Some(false), Some(usage.clone()))
        .unwrap();
    session
        .branch_with_summary(Some(&root_id), "branch summary", None, Some(false), Some(usage.clone()))
        .unwrap();

    let file = session.get_session_file().unwrap();
    let reopened = SessionManager::open(&file, Some(&d), None).unwrap();
    let entries = reopened.get_entries();
    assert!(entries.iter().any(|e| matches!(e, SessionEntry::Compaction(c) if c.usage.as_ref() == Some(&usage))));
    assert!(entries.iter().any(|e| matches!(e, SessionEntry::BranchSummary(b) if b.usage.as_ref() == Some(&usage))));
    assert!(entries.iter().any(|e| matches!(
        e.message(),
        Some(pi_agent::types::AgentMessage::ToolResult(r)) if r.usage.as_ref() == Some(&usage)
    )));
    // Everything written is read back identically.
    assert_eq!(entries, session.get_entries());
}

#[test]
fn writes_file_immediately_when_forking_at_a_user_message() {
    let dir = tmp();
    let d = path_str(dir.path());
    let session = SessionManager::create(&d, Some(&d), None).unwrap();
    let id1 = session.append_message(user_msg("first question")).unwrap();
    session.append_message(assistant_msg("first answer")).unwrap();
    let new_file = session.create_branched_session(&id1).unwrap().unwrap();
    assert!(std::path::Path::new(&new_file).exists());
    session.append_message(assistant_msg("new answer")).unwrap();
    assert_eq!(read_session_file_roles(&new_file), ["session", "user", "assistant"]);
    let header = session.get_header().unwrap();
    assert!(header.parent_session.is_some());
}

// labels

#[test]
fn sets_gets_and_clears_labels() {
    let session = in_memory();
    let msg_id = session.append_message(user_msg("hello")).unwrap();
    assert_eq!(session.get_label(&msg_id), None);
    let label_id = session.append_label_change(&msg_id, Some("checkpoint")).unwrap();
    assert_eq!(session.get_label(&msg_id).as_deref(), Some("checkpoint"));
    let Some(SessionEntry::Label(label)) = session.get_entries().into_iter().find(|e| e.type_name() == "label") else {
        panic!()
    };
    assert_eq!(label.id, label_id);
    assert_eq!(label.target_id, msg_id);
    assert_eq!(label.label.as_deref(), Some("checkpoint"));

    session.append_label_change(&msg_id, None).unwrap();
    assert_eq!(session.get_label(&msg_id), None);
}

#[test]
fn last_label_wins() {
    let session = in_memory();
    let msg_id = session.append_message(user_msg("hello")).unwrap();
    session.append_label_change(&msg_id, Some("first")).unwrap();
    session.append_label_change(&msg_id, Some("second")).unwrap();
    let last = session.append_label_change(&msg_id, Some("third")).unwrap();
    assert_eq!(session.get_label(&msg_id).as_deref(), Some("third"));
    let last_entry = session.get_entry(&last).unwrap();
    let tree = session.get_tree();
    let node = tree.iter().find(|n| n.entry.id() == msg_id).unwrap();
    assert_eq!(node.label_timestamp.as_deref(), Some(last_entry.timestamp()));
}

#[test]
fn labels_are_included_in_tree_nodes_and_preserved_in_branched_sessions() {
    let session = in_memory();
    let msg1 = session.append_message(user_msg("hello")).unwrap();
    let msg2 = session.append_message(assistant_msg("hi")).unwrap();
    let l1 = session.append_label_change(&msg1, Some("important")).unwrap();
    let l2 = session.append_label_change(&msg2, Some("also-important")).unwrap();
    let l1_ts = session.get_entry(&l1).unwrap().timestamp().to_string();
    let l2_ts = session.get_entry(&l2).unwrap().timestamp().to_string();

    let tree = session.get_tree();
    let node1 = tree.iter().find(|n| n.entry.id() == msg1).unwrap();
    assert_eq!(node1.label.as_deref(), Some("important"));
    assert_eq!(node1.label_timestamp.as_deref(), Some(l1_ts.as_str()));
    let node2 = node1.children.iter().find(|n| n.entry.id() == msg2).unwrap();
    assert_eq!(node2.label.as_deref(), Some("also-important"));

    session.create_branched_session(&msg2).unwrap();
    assert_eq!(session.get_label(&msg1).as_deref(), Some("important"));
    assert_eq!(session.get_label(&msg2).as_deref(), Some("also-important"));
    let labels: Vec<SessionEntry> = session
        .get_entries()
        .into_iter()
        .filter(|e| e.type_name() == "label")
        .collect();
    assert_eq!(labels.len(), 2);
    let tree = session.get_tree();
    let node1 = tree.iter().find(|n| n.entry.id() == msg1).unwrap();
    let node2 = node1.children.iter().find(|n| n.entry.id() == msg2).unwrap();
    assert_eq!(node1.label_timestamp.as_deref(), Some(l1_ts.as_str()));
    assert_eq!(node2.label_timestamp.as_deref(), Some(l2_ts.as_str()));
}

#[test]
fn rewires_children_of_removed_labels_when_forking() {
    let session = in_memory();
    let msg1 = session.append_message(user_msg("hello")).unwrap();
    session.append_label_change(&msg1, Some("checkpoint")).unwrap();
    let model_change_id = session.append_model_change("anthropic", "claude-test").unwrap();
    let msg2 = session.append_message(user_msg("followup")).unwrap();
    session.create_branched_session(&msg2).unwrap();
    assert_eq!(
        session.get_entry(&model_change_id).unwrap().parent_id(),
        Some(msg1.as_str())
    );
}

#[test]
fn labels_not_on_path_are_not_preserved() {
    let session = in_memory();
    let msg1 = session.append_message(user_msg("hello")).unwrap();
    let msg2 = session.append_message(assistant_msg("hi")).unwrap();
    let msg3 = session.append_message(user_msg("followup")).unwrap();
    session.append_label_change(&msg1, Some("first")).unwrap();
    session.append_label_change(&msg2, Some("second")).unwrap();
    session.append_label_change(&msg3, Some("third")).unwrap();
    session.create_branched_session(&msg2).unwrap();
    assert_eq!(session.get_label(&msg1).as_deref(), Some("first"));
    assert_eq!(session.get_label(&msg2).as_deref(), Some("second"));
    assert_eq!(session.get_label(&msg3), None);
}

#[test]
fn labels_are_not_in_context_and_unknown_targets_fail() {
    let session = in_memory();
    let msg_id = session.append_message(user_msg("hello")).unwrap();
    session.append_label_change(&msg_id, Some("checkpoint")).unwrap();
    let ctx = session.build_session_context();
    assert_eq!(ctx.messages.len(), 1);
    assert_eq!(ctx.messages[0].role(), "user");
    assert_eq!(
        session
            .append_label_change("non-existent", Some("label"))
            .unwrap_err()
            .to_string(),
        "Entry non-existent not found"
    );
}

#[test]
fn session_names_are_sanitized_and_clearable() {
    let session = in_memory();
    assert_eq!(session.get_session_name(), None);
    session.append_session_info("  My\r\nSession\n").unwrap();
    assert_eq!(session.get_session_name().as_deref(), Some("My Session"));
    session.append_session_info("   ").unwrap();
    assert_eq!(session.get_session_name(), None);
}

// suite/regressions/8989-fork-compaction-label-boundary.test.ts
#[test]
fn preserves_compaction_context_when_a_fork_removes_the_boundary_label() {
    let session = in_memory();
    let old_id = session.append_message(user_msg("old")).unwrap();
    // findCutPoint() can move a compaction boundary back to this context-invisible label.
    let label_id = session.append_label_change(&old_id, Some("checkpoint")).unwrap();
    let kept_id = session.append_message(user_msg("kept")).unwrap();
    let compaction_id = session.append_compaction("summary", Some(&label_id), 100, None, None, None).unwrap();
    let leaf_id = session.append_message(user_msg("after")).unwrap();

    session.create_branched_session(&leaf_id).unwrap();

    let compaction = serde_json::to_value(session.get_entry(&compaction_id).unwrap()).unwrap();
    assert_eq!(compaction["firstKeptEntryId"], serde_json::json!(kept_id));
    let messages: Vec<serde_json::Value> = session.build_session_context().messages.iter().map(mjson).collect();
    assert_eq!(messages.len(), 3);
    assert_eq!((messages[0]["role"].as_str(), messages[0]["summary"].as_str()), (Some("compactionSummary"), Some("summary")));
    assert_eq!((messages[1]["role"].as_str(), messages[1]["content"].as_str()), (Some("user"), Some("kept")));
    assert_eq!((messages[2]["role"].as_str(), messages[2]["content"].as_str()), (Some("user"), Some("after")));
}
