//! Entries this version does not fully know survive like in TS, where entries are plain JSON:
//! unknown fields of known entry types are written back on rewrite, and entries of unknown type are
//! tree entries (in `get_entries`, the branch, the leaf) written back verbatim.

use crate::common::*;
use pi_coding_agent::core::session_manager::{FileEntry, SessionEntry, SessionManager};
use serde_json::{Value, json};

const V2_SESSION: &[&str] = &[
    r#"{"type":"session","version":2,"id":"019a1b2c-3d4e-7f60-8a9b-0c1d2e3f4a5b","timestamp":"2025-06-01T10:00:00.000Z","cwd":"/work/project"}"#,
    r#"{"type":"message","id":"a1","parentId":null,"timestamp":"2025-06-01T10:00:01.000Z","message":{"role":"user","content":"hi","timestamp":1},"futureField":{"x":1}}"#,
    r#"{"type":"future_kind","id":"a2","parentId":"a1","timestamp":"2025-06-01T10:00:02.000Z","payload":[1,2,3]}"#,
    r#"{"type":"label","id":"a3","parentId":"a2","timestamp":"2025-06-01T10:00:03.000Z","targetId":"a1","label":"start","color":"red"}"#,
];

#[test]
fn unknown_fields_and_entry_types_round_trip() {
    for line in &V2_SESSION[1..] {
        let entry: FileEntry = serde_json::from_str(line).unwrap();
        assert!(matches!(entry, FileEntry::Entry(_)), "not a tree entry: {line}");
        assert_eq!(serde_json::to_string(&entry).unwrap(), *line);
    }
    let unknown: SessionEntry = serde_json::from_str(V2_SESSION[2]).unwrap();
    assert_eq!(unknown.type_name(), "future_kind");
    assert_eq!(unknown.id(), "a2");
    assert_eq!(unknown.parent_id(), Some("a1"));
}

#[test]
fn unknown_entries_are_tree_entries_and_rewrites_keep_unknown_data() {
    let dir = tmp();
    let path = dir.path().join("2025-06-01T10-00-00-000Z_019a1b2c-3d4e-7f60-8a9b-0c1d2e3f4a5b.jsonl");
    std::fs::write(&path, V2_SESSION.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    let sm = SessionManager::open(&path_str(&path), None, None).unwrap();

    let types: Vec<String> = sm.get_entries().iter().map(|e| e.type_name().to_string()).collect();
    assert_eq!(types, ["message", "future_kind", "label"]);
    assert_eq!(sm.get_leaf_id().as_deref(), Some("a3"));
    let branch: Vec<String> = sm.get_branch(None).iter().map(|e| e.id().to_string()).collect();
    assert_eq!(branch, ["a1", "a2", "a3"]);
    assert_eq!(sm.get_label("a1").as_deref(), Some("start"));
    // Context ignores the unknown entry.
    assert_eq!(sm.build_session_context().messages.len(), 1);

    // The v2 -> v3 migration rewrote the file; everything but the version is unchanged.
    let lines: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["version"], json!(3));
    for (line, original) in lines[1..].iter().zip(&V2_SESSION[1..]) {
        assert_eq!(serde_json::to_string(line).unwrap(), *original);
    }

    // A branched session copies the unknown entry and the unknown fields.
    sm.append_message(user_msg("next")).unwrap();
    let leaf = sm.get_leaf_id().unwrap();
    let branched = sm.create_branched_session(&leaf).unwrap().unwrap();
    let text = std::fs::read_to_string(&branched).unwrap();
    assert!(text.contains(r#""payload":[1,2,3]"#), "{text}");
    assert!(text.contains(r#""futureField":{"x":1}"#), "{text}");
}
