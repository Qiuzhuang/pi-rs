//! Port of `test/session-manager/{file-operations,custom-session-id,load-entries,migration}.test.ts`,
//! `session-info-modified-timestamp.test.ts` and the JSONL export part of `export-jsonl-share.test.ts`.

use crate::common::*;
use pi_coding_agent::core::session_export::{TrailingEntries, export_session_to_jsonl, serialize_session_branch};
use pi_coding_agent::core::session_manager::{
    FileEntry, NewSessionOptions, SessionEntry, SessionHeader, SessionListProgress, SessionManager,
    find_most_recent_session, load_entries_from_file, migrate_session_entries,
};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

const HEADER_SCAN_LIMIT_BYTES: usize = 1024 * 1024;

fn write(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn write_session_header(file: &Path, cwd: &str, id: &str, prefix: &str) {
    let header = json!({ "type": "session", "version": 3, "id": id, "timestamp": "2025-01-01T00:00:00Z", "cwd": cwd });
    write(file, &format!("{prefix}{header}\n"));
}

const VALID_HEADER: &str = r#"{"type":"session","id":"abc","timestamp":"2025-01-01T00:00:00Z","cwd":"/tmp"}"#;
const VALID_MESSAGE: &str = r#"{"type":"message","id":"1","parentId":null,"timestamp":"2025-01-01T00:00:01Z","message":{"role":"user","content":"hi","timestamp":1}}"#;

// ---------------------------------------------------------------------------
// loadEntriesFromFile
// ---------------------------------------------------------------------------

#[test]
fn load_entries_handles_missing_empty_and_invalid_files() {
    let dir = tmp();
    assert!(load_entries_from_file(&path_str(&dir.path().join("nonexistent.jsonl"))).unwrap().is_empty());

    let empty = dir.path().join("empty.jsonl");
    write(&empty, "");
    assert!(load_entries_from_file(&path_str(&empty)).unwrap().is_empty());

    let no_header = dir.path().join("no-header.jsonl");
    write(&no_header, "{\"type\":\"message\",\"id\":\"1\"}\n");
    assert!(load_entries_from_file(&path_str(&no_header)).unwrap().is_empty());

    let malformed = dir.path().join("malformed.jsonl");
    write(&malformed, "not json\n");
    assert!(load_entries_from_file(&path_str(&malformed)).unwrap().is_empty());
}

#[test]
fn load_entries_loads_valid_files_and_skips_malformed_lines() {
    let dir = tmp();
    let valid = dir.path().join("valid.jsonl");
    write(&valid, &format!("{VALID_HEADER}\n{VALID_MESSAGE}\n"));
    let entries = load_entries_from_file(&path_str(&valid)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].type_name(), "session");
    assert_eq!(entries[1].type_name(), "message");

    let mixed = dir.path().join("mixed.jsonl");
    write(&mixed, &format!("{VALID_HEADER}\nnot valid json\n{VALID_MESSAGE}\n"));
    assert_eq!(load_entries_from_file(&path_str(&mixed)).unwrap().len(), 2);
}

#[test]
fn load_entries_repairs_unterminated_files_only_when_valid() {
    let dir = tmp();
    let file = dir.path().join("unterminated.jsonl");
    let content = format!("{VALID_HEADER}\n{VALID_MESSAGE}");
    write(&file, &content);
    assert_eq!(load_entries_from_file(&path_str(&file)).unwrap().len(), 2);
    assert_eq!(read(&file), format!("{content}\n"));

    let file = dir.path().join("malformed-tail.jsonl");
    let content = format!("{VALID_HEADER}\n{{\"type\":\"message\"");
    write(&file, &content);
    assert_eq!(load_entries_from_file(&path_str(&file)).unwrap().len(), 1);
    assert_eq!(read(&file), format!("{content}\n"));

    let file = dir.path().join("invalid.jsonl");
    let content = "{\"type\":\"message\",\"id\":\"1\"}";
    write(&file, content);
    assert!(load_entries_from_file(&path_str(&file)).unwrap().is_empty());
    assert_eq!(read(&file), content);
}

#[test]
fn reads_cwd_from_sessions_with_unusual_prefixes() {
    let cases = [
        ("\n  \n", "leading-blank".to_string()),
        ("not json\n{broken json\n", "leading-malformed".to_string()),
        ("", "a".repeat(8192)),
    ];
    for (prefix, session_id) in cases {
        let dir = tmp();
        let file = dir.path().join("header.jsonl");
        let stored_cwd = path_str(&dir.path().join("stored-project"));
        write_session_header(&file, &stored_cwd, &session_id, prefix);
        let sm = SessionManager::open(&path_str(&file), Some(&path_str(dir.path())), None).unwrap();
        assert_eq!(sm.get_session_id(), session_id);
        assert_eq!(sm.get_cwd(), stored_cwd);
    }
}

#[test]
fn opens_compatible_sessions_beyond_the_discovery_scan_limit() {
    let dir = tmp();
    let stored_cwd = path_str(&dir.path().join("stored-project"));
    let override_cwd = path_str(&dir.path().join("override-project"));
    let cases = [
        ("large-header", "a".repeat(HEADER_SCAN_LIMIT_BYTES + 1), String::new()),
        (
            "large-prefix",
            "large-prefix".to_string(),
            format!("{}\n", "x".repeat(HEADER_SCAN_LIMIT_BYTES + 1)),
        ),
    ];
    for (name, id, prefix) in cases {
        let file = dir.path().join(format!("{name}.jsonl"));
        write_session_header(&file, &stored_cwd, &id, &prefix);
        for cwd_override in [None, Some(override_cwd.as_str())] {
            let sm = SessionManager::open(&path_str(&file), Some(&path_str(dir.path())), cwd_override).unwrap();
            assert_eq!(sm.get_session_id(), id);
            assert_eq!(sm.get_cwd(), cwd_override.unwrap_or(&stored_cwd));
        }
    }
}

#[test]
fn opens_session_files_with_huge_garbage_lines() {
    let dir = tmp();
    let file = dir.path().join("large.jsonl");
    let mut content = String::from(
        "{\"type\":\"session\",\"version\":3,\"id\":\"abc\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
    );
    // Several multi-megabyte non-JSON lines.
    for _ in 0..3 {
        content.push_str(&"\0".repeat(4 * 1024 * 1024));
        content.push('\n');
    }
    content.push_str(VALID_MESSAGE);
    content.push('\n');
    write(&file, &content);

    let sm = SessionManager::open(&path_str(&file), Some(&path_str(dir.path())), None).unwrap();
    assert_eq!(sm.get_session_id(), "abc");
    assert_eq!(sm.get_entries().len(), 1);
    let messages = sm.build_session_context().messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(mjson(&messages[0]), json!({ "role": "user", "content": "hi", "timestamp": 1 }));
}

// ---------------------------------------------------------------------------
// findMostRecentSession
// ---------------------------------------------------------------------------

#[test]
fn find_most_recent_session_cases() {
    let dir = tmp();
    let d = path_str(dir.path());
    assert_eq!(find_most_recent_session(&d, None), None);
    assert_eq!(find_most_recent_session(&path_str(&dir.path().join("nonexistent")), None), None);

    write(&dir.path().join("file.txt"), "hello");
    write(&dir.path().join("file.json"), "{}");
    assert_eq!(find_most_recent_session(&d, None), None);

    write(&dir.path().join("invalid.jsonl"), "{\"type\":\"message\"}\n");
    assert_eq!(find_most_recent_session(&d, None), None);

    let older = dir.path().join("older.jsonl");
    write(&older, &format!("{VALID_HEADER}\n"));
    assert_eq!(find_most_recent_session(&d, None), Some(path_str(&older)));
    std::thread::sleep(std::time::Duration::from_millis(20));
    let newer = dir.path().join("newer.jsonl");
    write(&newer, &format!("{VALID_HEADER}\n"));
    assert_eq!(find_most_recent_session(&d, None), Some(path_str(&newer)));

    // Oversized corrupt files are skipped.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(&dir.path().join("oversized.jsonl"), &"x".repeat(HEADER_SCAN_LIMIT_BYTES + 1));
    assert_eq!(find_most_recent_session(&d, None), Some(path_str(&newer)));
}

#[test]
fn find_most_recent_session_filters_by_cwd() {
    let dir = tmp();
    let d = path_str(dir.path());
    let project_a = path_str(&dir.path().join("project-a"));
    let project_b = path_str(&dir.path().join("project-b"));
    let file_a = dir.path().join("a.jsonl");
    let file_b = dir.path().join("b.jsonl");
    write_session_header(&file_a, &project_a, "a", "");
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_session_header(&file_b, &project_b, "b", "");
    assert_eq!(find_most_recent_session(&d, Some(&project_a)), Some(path_str(&file_a)));
    assert_eq!(find_most_recent_session(&d, Some(&project_b)), Some(path_str(&file_b)));
}

// ---------------------------------------------------------------------------
// Custom flat session directory, listing
// ---------------------------------------------------------------------------

fn create_persisted_session(cwd: &str, dir: &str, label: &str) -> String {
    let session = SessionManager::create(cwd, Some(dir), None).unwrap();
    session.append_message(user_msg(label)).unwrap();
    session.append_message(assistant_msg(&format!("reply to {label}"))).unwrap();
    session.get_session_file().expect("persisted session file")
}

#[tokio::test]
async fn scopes_current_folder_apis_by_cwd_while_listing_all_flat_sessions() {
    let dir = tmp();
    let d = path_str(dir.path());
    let project_a = path_str(&dir.path().join("project-a"));
    let project_b = path_str(&dir.path().join("project-b"));
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();
    let session_a = create_persisted_session(&project_a, &d, "from A");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let session_b = create_persisted_session(&project_b, &d, "from B");

    let current_a = SessionManager::list(&project_a, Some(&d), None, None).await.unwrap();
    assert_eq!(current_a.iter().map(|s| s.path.clone()).collect::<Vec<_>>(), std::slice::from_ref(&session_a));
    assert_eq!(current_a[0].first_message, "from A");
    assert_eq!(current_a[0].all_messages_text, "from A reply to from A");
    assert_eq!(current_a[0].message_count, 2);
    assert_eq!(current_a[0].cwd, project_a);

    let all = SessionManager::list_all(Some(&d), None, None).await.unwrap();
    let mut paths: Vec<String> = all.iter().map(|s| s.path.clone()).collect();
    paths.sort();
    let mut expected = vec![session_a.clone(), session_b];
    expected.sort();
    assert_eq!(paths, expected);

    let continued = SessionManager::continue_recent(&project_a, Some(&d)).unwrap();
    assert_eq!(continued.get_session_file(), Some(session_a));
}

#[tokio::test]
async fn rejects_a_cancelled_session_listing() {
    let dir = tmp();
    let d = path_str(dir.path());
    let project_a = path_str(&dir.path().join("project-a"));
    let project_b = path_str(&dir.path().join("project-b"));
    create_persisted_session(&project_a, &d, "from A");
    create_persisted_session(&project_b, &d, "from B");
    let token = tokio_util::sync::CancellationToken::new();
    let cancel = token.clone();
    let progress: SessionListProgress = Arc::new(move |_loaded, _total, partial| {
        if partial.is_some() {
            cancel.cancel();
        }
    });
    let error = SessionManager::list_all(Some(&d), Some(progress), Some(token.clone()))
        .await
        .unwrap_err();
    assert!(error.downcast_ref::<pi_coding_agent::utils::abort::OperationAbortedError>().is_some());
    let error = SessionManager::list_all(None, None, Some(token)).await.unwrap_err();
    assert!(error.downcast_ref::<pi_coding_agent::utils::abort::OperationAbortedError>().is_some());
}

#[tokio::test]
async fn session_info_modified_uses_last_message_timestamp() {
    let dir = tmp();
    let file = dir.path().join("pi-session-modified.jsonl");
    let header = json!({ "type": "session", "id": "test-session", "version": 3, "timestamp": "1970-01-01T00:00:00.000Z", "cwd": "/tmp" });
    write(&file, &format!("{header}\n"));
    let mgr = SessionManager::open(&path_str(&file), None, None).unwrap();
    mgr.append_message(assistant_msg("hi")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    let mgr = SessionManager::open(&path_str(&file), None, None).unwrap();
    let msg_time = now_ms() + 5000;
    let mut message = mjson(&assistant_msg("later"));
    message["timestamp"] = json!(msg_time);
    mgr.append_message(agent_message(message)).unwrap();

    let sessions = SessionManager::list("/tmp", Some(&path_str(dir.path())), None, None)
        .await
        .unwrap();
    let info = sessions.iter().find(|s| s.path == path_str(&file)).unwrap();
    assert_eq!(info.modified.timestamp_millis(), msg_time);
    assert_eq!(info.created.timestamp_millis(), 0);
    assert_eq!(info.first_message, "(no messages)");
}

#[tokio::test]
async fn session_info_reports_names_and_parent_sessions() {
    let dir = tmp();
    let d = path_str(dir.path());
    let session = SessionManager::create(&d, Some(&d), Some(&NewSessionOptions {
        id: None,
        parent_session: Some("/parent.jsonl".into()),
    }))
    .unwrap();
    session.append_message(user_msg("question")).unwrap();
    session.append_session_info("Named").unwrap();
    let info = SessionManager::list(&d, Some(&d), None, None).await.unwrap();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].name.as_deref(), Some("Named"));
    assert_eq!(info[0].parent_session_path.as_deref(), Some("/parent.jsonl"));
    assert_eq!(info[0].id, session.get_session_id());
    assert!(SessionManager::find_by_id(&d, &session.get_session_id(), Some(&d)).unwrap().is_some());
    assert!(SessionManager::find_by_id(&d, "missing", Some(&d)).unwrap().is_none());
}

// ---------------------------------------------------------------------------
// setSessionFile with corrupted files
// ---------------------------------------------------------------------------

#[test]
fn truncates_and_rewrites_empty_file_with_valid_header() {
    let dir = tmp();
    let d = path_str(dir.path());
    let empty = dir.path().join("empty.jsonl");
    write(&empty, "");
    let sm = SessionManager::open(&path_str(&empty), Some(&d), None).unwrap();
    assert!(!sm.get_session_id().is_empty());
    assert!(sm.get_header().is_some());
    assert_eq!(sm.get_session_file(), Some(path_str(&empty)));
    let content = read(&empty);
    let lines: Vec<&str> = content.trim().split('\n').filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1);
    let header: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(header["type"], "session");
    assert_eq!(header["id"], sm.get_session_id());

    let sm2 = SessionManager::open(&path_str(&empty), Some(&d), None).unwrap();
    assert_eq!(sm2.get_session_id(), sm.get_session_id());
}

#[test]
fn throws_and_preserves_files_without_valid_header() {
    let dir = tmp();
    let d = path_str(dir.path());
    for (name, content) in [
        (
            "no-header.jsonl",
            "{\"type\":\"message\",\"id\":\"abc\",\"parentId\":\"orphaned\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"content\":\"test\"}}\n",
        ),
        ("not-a-session.log", "{\"type\":\"event\",\"data\":\"not a session\"}\n"),
    ] {
        let file = dir.path().join(name);
        write(&file, content);
        let error = SessionManager::open(&path_str(&file), Some(&d), None).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Session file is not a valid pi session: {}", path_str(&file))
        );
        assert_eq!(read(&file), content);
    }
}

// ---------------------------------------------------------------------------
// Session file creation
// ---------------------------------------------------------------------------

#[test]
fn session_file_is_created_lazily_and_appended() {
    let dir = tmp();
    let d = path_str(dir.path());
    let session = SessionManager::create(&d, Some(&d), None).unwrap();
    session.append_model_change("anthropic", "claude-sonnet-4-5").unwrap();
    session.append_thinking_level_change("off").unwrap();
    let file = session.get_session_file().unwrap();
    assert!(!Path::new(&file).exists());

    session.append_message(user_msg("first question")).unwrap();
    assert_eq!(read_session_file_roles(&file), ["session", "model_change", "thinking_level_change", "user"]);
    assert_eq!(
        SessionManager::open(&file, Some(&d), None).unwrap().build_session_context().messages.len(),
        1
    );

    session.append_custom_entry("preset-state", Some(json!({ "name": "plan" }))).unwrap();
    session.append_message(assistant_msg("first answer")).unwrap();
    assert_eq!(
        read_session_file_roles(&file),
        ["session", "model_change", "thinking_level_change", "user", "custom", "assistant"]
    );
}

// ---------------------------------------------------------------------------
// Custom session ids
// ---------------------------------------------------------------------------

#[test]
fn custom_session_ids() {
    let session = in_memory();
    session.new_session(Some(&NewSessionOptions { id: Some("my-custom-id".into()), parent_session: None })).unwrap();
    assert_eq!(session.get_session_id(), "my-custom-id");
    assert_eq!(session.get_header().unwrap().id, "my-custom-id");

    let session = SessionManager::in_memory(
        None,
        Some(&NewSessionOptions { id: Some("memory-session-id".into()), parent_session: None }),
        None,
    )
    .unwrap();
    assert_eq!(session.get_session_id(), "memory-session-id");
    assert_eq!(session.get_session_file(), None);

    session.new_session(Some(&NewSessionOptions { id: Some("abc-123_def.456".into()), parent_session: None })).unwrap();
    assert_eq!(session.get_session_id(), "abc-123_def.456");

    for id in ["", "-abc", "abc-", "_abc", "abc_", ".abc", "abc.", "abc/def", "abc\\def", "abc def"] {
        let session = in_memory();
        let error = session
            .new_session(Some(&NewSessionOptions { id: Some(id.into()), parent_session: None }))
            .unwrap_err();
        assert!(error
            .to_string()
            .starts_with("Session id must be non-empty, contain only alphanumeric characters"));
    }

    let session = in_memory();
    assert!(is_uuid_v7(&session.get_session_id()));
    session.new_session(None).unwrap();
    assert!(is_uuid_v7(&session.get_session_id()));
    session
        .new_session(Some(&NewSessionOptions { id: None, parent_session: Some("parent.jsonl".into()) }))
        .unwrap();
    assert!(is_uuid_v7(&session.get_session_id()));
    assert_eq!(session.get_header().unwrap().parent_session.as_deref(), Some("parent.jsonl"));
}

#[test]
fn persisted_session_file_names_use_the_session_id() {
    let dir = tmp();
    let d = path_str(dir.path());
    let session = SessionManager::create(&d, Some(&d), Some(&NewSessionOptions {
        id: Some("created-session-id".into()),
        parent_session: None,
    }))
    .unwrap();
    assert_eq!(session.get_session_id(), "created-session-id");
    let file = session.get_session_file().unwrap();
    let name = Path::new(&file).file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}-\d{3}Z_created-session-id\.jsonl$")
            .unwrap()
            .is_match(&name),
        "{name}"
    );
    assert!(!Path::new(&file).exists());
}

#[test]
fn fork_from_another_session_file() {
    let dir = tmp();
    let d = path_str(dir.path());
    let source = dir.path().join("source.jsonl");
    let header = json!({ "type": "session", "version": 3, "id": "legacy-session-id", "timestamp": "2025-01-01T00:00:00.000Z", "cwd": d });
    let message = json!({
        "type": "message", "id": "entry-1", "parentId": null, "timestamp": "2025-01-01T00:00:00.000Z",
        "message": {
            "role": "assistant", "content": [{ "type": "text", "text": "hello" }],
            "api": "openai-responses", "provider": "openai", "model": "gpt-5.4",
            "usage": usage_json(0, 0), "stopReason": "stop", "timestamp": 1
        }
    });
    write(&source, &format!("{header}\n{message}\n"));

    let forked = SessionManager::fork_from(&path_str(&source), &d, Some(&d), None).unwrap();
    let forked_header = forked.get_header().unwrap();
    assert!(is_uuid_v7(&forked_header.id));
    assert_eq!(forked_header.parent_session.as_deref(), Some(path_str(&source).as_str()));
    assert_eq!(forked.get_entries().len(), 1);
    assert_eq!(forked.get_entries()[0].id(), "entry-1");
    // The copied entry is byte-identical to the source line.
    let forked_content = read(Path::new(&forked.get_session_file().unwrap()));
    assert_eq!(forked_content.lines().nth(1).unwrap(), message.to_string());

    let forked = SessionManager::fork_from(
        &path_str(&source),
        &d,
        Some(&d),
        Some(&NewSessionOptions { id: Some("forked-session-id".into()), parent_session: None }),
    )
    .unwrap();
    assert_eq!(forked.get_header().unwrap().id, "forked-session-id");
    assert!(forked.get_session_file().unwrap().contains("forked-session-id"));

    let empty = dir.path().join("empty.jsonl");
    write(&empty, "");
    assert!(SessionManager::fork_from(&path_str(&empty), &d, Some(&d), None)
        .unwrap_err()
        .to_string()
        .starts_with("Cannot fork: source session file is empty or invalid"));
}

// ---------------------------------------------------------------------------
// In-memory sessions from preloaded entries (load-entries.test.ts)
// ---------------------------------------------------------------------------

fn stored_entries(build: impl FnOnce(&SessionManager)) -> Vec<SessionEntry> {
    let source = SessionManager::in_memory(Some("/project"), None, None).unwrap();
    build(&source);
    source.get_entries()
}

fn as_file_entries(entries: &[SessionEntry]) -> Vec<FileEntry> {
    entries.iter().cloned().map(FileEntry::Entry).collect()
}

#[test]
fn in_memory_adopts_entries_verbatim_and_continues() {
    let entries = stored_entries(|s| {
        s.append_message(user_msg("hello")).unwrap();
        s.append_model_change("anthropic", "claude-opus-4-5").unwrap();
        s.append_message(user_msg("again")).unwrap();
    });
    let session = SessionManager::in_memory(Some("/project"), None, Some(as_file_entries(&entries))).unwrap();
    assert_eq!(session.get_entries(), entries);

    let last_id = entries.last().unwrap().id().to_string();
    let appended = session.append_message(user_msg("continued")).unwrap();
    assert_eq!(session.get_leaf_id(), Some(appended.clone()));
    assert_eq!(session.get_entry(&appended).unwrap().parent_id(), Some(last_id.as_str()));
    assert!(!entries.iter().any(|e| e.id() == appended));
    assert_eq!(session.get_session_file(), None);
    assert!(!session.is_persisted());
}

#[test]
fn in_memory_rebuilds_branches_labels_and_compactions() {
    let entries = stored_entries(|s| {
        let first = s.append_message(user_msg("hello")).unwrap();
        s.append_message(user_msg("abandoned")).unwrap();
        s.branch(&first).unwrap();
        s.append_message(user_msg("kept")).unwrap();
    });
    let session = SessionManager::in_memory(Some("/project"), None, Some(as_file_entries(&entries))).unwrap();
    let roots = session.get_tree();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].children.len(), 2);

    let mut labelled = String::new();
    let entries = stored_entries(|s| {
        labelled = s.append_message(user_msg("hello")).unwrap();
        s.append_label_change(&labelled, Some("checkpoint")).unwrap();
    });
    let session = SessionManager::in_memory(Some("/project"), None, Some(as_file_entries(&entries))).unwrap();
    assert_eq!(session.get_label(&labelled).as_deref(), Some("checkpoint"));

    let mut kept = String::new();
    let entries = stored_entries(|s| {
        s.append_message(user_msg("dropped")).unwrap();
        kept = s.append_message(user_msg("kept")).unwrap();
        s.append_compaction("summary so far", Some(&kept), 1000, None, None, None).unwrap();
    });
    let session = SessionManager::in_memory(Some("/project"), None, Some(as_file_entries(&entries))).unwrap();
    assert!(session.build_context_entries().iter().any(|e| e.id() == kept));
}

#[test]
fn in_memory_header_handling() {
    let entries = stored_entries(|s| {
        s.append_message(user_msg("hello")).unwrap();
    });
    let session = SessionManager::in_memory(
        Some("/project"),
        Some(&NewSessionOptions { id: Some("restored-session".into()), parent_session: None }),
        Some(as_file_entries(&entries)),
    )
    .unwrap();
    assert_eq!(session.get_session_id(), "restored-session");
    assert_eq!(session.get_header().unwrap().cwd, "/project");

    let session = SessionManager::in_memory(Some("/project"), None, Some(as_file_entries(&entries))).unwrap();
    assert!(is_uuid_v7(&session.get_session_id()));

    let session = SessionManager::in_memory(
        Some("/project"),
        Some(&NewSessionOptions { id: Some("empty-session".into()), parent_session: None }),
        Some(Vec::new()),
    )
    .unwrap();
    assert_eq!(session.get_session_id(), "empty-session");
    assert!(session.get_entries().is_empty());
    assert_eq!(session.get_leaf_id(), None);

    let mut with_header = vec![FileEntry::Session(SessionHeader {
        version: Some(3),
        id: "stored-session".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        cwd: "/stored".into(),
        parent_session: None,
    })];
    with_header.extend(as_file_entries(&entries));
    let session = SessionManager::in_memory(
        Some("/project"),
        Some(&NewSessionOptions { id: Some("ignored".into()), parent_session: None }),
        Some(with_header),
    )
    .unwrap();
    assert_eq!(session.get_session_id(), "stored-session");
    assert_eq!(session.get_header().unwrap().cwd, "/stored");
}

#[test]
fn in_memory_migrates_entries_restored_with_an_older_header() {
    let hook = json!({
        "type": "message", "id": "abc12345", "parentId": null, "timestamp": "2026-01-01T00:00:01Z",
        "message": { "role": "hookMessage", "content": "from a hook", "timestamp": 1 }
    });
    let entries = vec![
        serde_json::from_value::<FileEntry>(
            json!({ "type": "session", "version": 2, "id": "v2-session", "timestamp": "2026-01-01T00:00:00Z", "cwd": "/project" }),
        )
        .unwrap(),
        serde_json::from_value::<FileEntry>(hook.clone()).unwrap(),
    ];
    let session = SessionManager::in_memory(Some("/project"), None, Some(entries)).unwrap();
    assert_eq!(session.get_header().unwrap().version, Some(3));
    let restored = &session.get_entries()[0];
    assert_eq!(restored.message().unwrap().role(), "custom");
    assert_eq!(restored.id(), "abc12345");

    // Headerless entries are adopted as current-version without migrating them.
    let session =
        SessionManager::in_memory(Some("/project"), None, Some(vec![serde_json::from_value(hook).unwrap()])).unwrap();
    assert_eq!(session.get_entries()[0].message().unwrap().role(), "hookMessage");
}

// ---------------------------------------------------------------------------
// migrateSessionEntries
// ---------------------------------------------------------------------------

#[test]
fn migration_adds_ids_to_v1_entries() {
    let mut entries: Vec<FileEntry> = [
        json!({ "type": "session", "id": "sess-1", "timestamp": "2025-01-01T00:00:00Z", "cwd": "/tmp" }),
        json!({ "type": "message", "timestamp": "2025-01-01T00:00:01Z", "message": { "role": "user", "content": "hi", "timestamp": 1 } }),
        json!({ "type": "message", "timestamp": "2025-01-01T00:00:02Z", "message": {
            "role": "assistant", "content": [{ "type": "text", "text": "hello" }], "api": "test", "provider": "test",
            "model": "test", "usage": { "input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0 },
            "stopReason": "stop", "timestamp": 2
        } }),
    ]
    .into_iter()
    .map(|v| serde_json::from_value(v).unwrap())
    .collect();

    migrate_session_entries(&mut entries);

    assert_eq!(entries[0].as_header().unwrap().version, Some(3));
    let msg1 = entries[1].as_entry().unwrap();
    let msg2 = entries[2].as_entry().unwrap();
    assert_eq!(msg1.id().len(), 8);
    assert_eq!(msg1.parent_id(), None);
    assert_eq!(msg2.id().len(), 8);
    assert_eq!(msg2.parent_id(), Some(msg1.id()));
    // The incomplete assistant message (no totalTokens/cost) survives verbatim.
    let written = serde_json::to_value(&entries[2]).unwrap();
    assert_eq!(written["message"]["usage"], json!({ "input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0 }));
}

#[test]
fn migration_is_idempotent_for_migrated_entries() {
    let mut entries: Vec<FileEntry> = [
        json!({ "type": "session", "id": "sess-1", "version": 2, "timestamp": "2025-01-01T00:00:00Z", "cwd": "/tmp" }),
        json!({ "type": "message", "id": "abc12345", "parentId": null, "timestamp": "2025-01-01T00:00:01Z", "message": { "role": "user", "content": "hi", "timestamp": 1 } }),
        json!({ "type": "message", "id": "def67890", "parentId": "abc12345", "timestamp": "2025-01-01T00:00:02Z", "message": { "role": "user", "content": "hi", "timestamp": 2 } }),
    ]
    .into_iter()
    .map(|v| serde_json::from_value(v).unwrap())
    .collect();
    migrate_session_entries(&mut entries);
    assert_eq!(entries[1].as_entry().unwrap().id(), "abc12345");
    assert_eq!(entries[2].as_entry().unwrap().id(), "def67890");
    assert_eq!(entries[2].as_entry().unwrap().parent_id(), Some("abc12345"));
}

#[test]
fn v1_files_are_migrated_and_rewritten_on_open() {
    let dir = tmp();
    let file = dir.path().join("v1.jsonl");
    let lines = [
        json!({ "type": "session", "id": "v1-session", "timestamp": "2025-01-01T00:00:00Z", "cwd": "/tmp", "provider": "anthropic" }),
        json!({ "type": "message", "timestamp": "2025-01-01T00:00:01Z", "message": { "role": "user", "content": "one", "timestamp": 1 } }),
        json!({ "type": "message", "timestamp": "2025-01-01T00:00:02Z", "message": { "role": "user", "content": "two", "timestamp": 2 } }),
        json!({ "type": "compaction", "timestamp": "2025-01-01T00:00:03Z", "summary": "sum", "firstKeptEntryIndex": 2, "tokensBefore": 10 }),
        json!({ "type": "message", "timestamp": "2025-01-01T00:00:04Z", "message": { "role": "hookMessage", "customType": "x", "content": "hook", "display": true, "timestamp": 3 } }),
    ];
    write(
        &file,
        &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    );

    let sm = SessionManager::open(&path_str(&file), None, None).unwrap();
    let entries = sm.get_entries();
    assert_eq!(entries.len(), 4);
    let SessionEntry::Compaction(compaction) = &entries[2] else {
        panic!("expected compaction")
    };
    assert_eq!(compaction.first_kept_entry_id, entries[1].id());
    assert_eq!(entries[3].message().unwrap().role(), "custom");

    let ctx = sm.build_session_context();
    let roles: Vec<&str> = ctx.messages.iter().map(|m| m.role()).collect();
    assert_eq!(roles, ["compactionSummary", "user", "custom"]);

    // The file was rewritten as v3 with ids and without the index field.
    let rewritten: Vec<Value> = read(&file).lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(rewritten[0]["version"], 3);
    assert_eq!(rewritten[3]["firstKeptEntryId"], json!(entries[1].id()));
    assert!(rewritten[3].get("firstKeptEntryIndex").is_none());
    assert_eq!(rewritten[2]["parentId"], json!(entries[0].id()));

    // Reopening is stable.
    let again = SessionManager::open(&path_str(&file), None, None).unwrap();
    assert_eq!(again.get_entries(), entries);
}

// ---------------------------------------------------------------------------
// JSONL export (session-export.ts)
// ---------------------------------------------------------------------------

#[test]
fn serialize_session_branch_rechains_the_active_branch_and_appends_trailing_entries() {
    let session = SessionManager::in_memory(Some("/project"), None, None).unwrap();
    let first = session.append_message(user_msg("hello")).unwrap();
    session.append_message(assistant_msg("abandoned")).unwrap();
    session.branch(&first).unwrap();
    let label = session.append_label_change(&first, Some("x")).unwrap();
    let answer = session.append_message(assistant_msg("kept")).unwrap();

    let trailing: TrailingEntries = Arc::new(|parent_id, timestamp| {
        vec![json!({ "type": "custom", "customType": "pi.share", "data": {}, "id": "share", "parentId": parent_id, "timestamp": timestamp })]
    });
    let jsonl = serialize_session_branch(&session, Some(&trailing));
    assert!(jsonl.ends_with('\n'));
    let records: Vec<Value> = jsonl.trim().split('\n').map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(records[0]["type"], "session");
    assert_eq!(records[0]["id"], json!(session.get_session_id()));
    assert_eq!(records[0]["cwd"], "/project");
    assert!(records[0].get("parentSession").is_none());
    let conversation: Vec<&Value> = records[1..records.len() - 1].iter().collect();
    assert_eq!(
        conversation.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(),
        vec![json!(first), json!(label), json!(answer)]
    );
    assert_eq!(
        conversation.iter().map(|r| r["parentId"].clone()).collect::<Vec<_>>(),
        vec![Value::Null, json!(first), json!(label)]
    );
    let share = records.last().unwrap();
    assert_eq!(share["parentId"], json!(answer));
    assert_eq!(share["timestamp"], records[0]["timestamp"]);

    // The export loads back as a session whose leaf is the trailing entry.
    let dir = tmp();
    let path = path_str(&dir.path().join("share.jsonl"));
    let written = export_session_to_jsonl(&session, Some(&path), Some(&trailing)).unwrap();
    assert_eq!(written, path);
    let imported = SessionManager::open(&path, None, None).unwrap();
    assert_eq!(imported.get_leaf_id().as_deref(), Some("share"));
    let roles: Vec<String> = imported
        .build_session_context()
        .messages
        .iter()
        .map(|m| m.role().to_string())
        .collect();
    assert_eq!(roles, ["user", "assistant"]);
}
