//! Port of `packages/coding-agent/src/core/session-export.ts`.

use crate::core::session_manager::{
    CURRENT_SESSION_VERSION, FileEntry, SessionHeader, SessionManager, now_iso,
};
use crate::utils::paths::resolve_path;
use serde_json::Value;
use std::sync::Arc;

/// Builds export-only entries appended after the branch: `(parentId, timestamp) -> entries`.
pub type TrailingEntries = Arc<dyn Fn(Option<String>, String) -> Vec<Value> + Send + Sync>;

/// Serialize the current branch and optional export-only entries as JSONL.
pub fn serialize_session_branch(
    session_manager: &SessionManager,
    create_trailing_entries: Option<&TrailingEntries>,
) -> String {
    let timestamp = now_iso();
    let header = FileEntry::Session(SessionHeader {
        version: Some(CURRENT_SESSION_VERSION),
        id: session_manager.get_session_id(),
        timestamp: timestamp.clone(),
        cwd: session_manager.get_cwd(),
        parent_session: None,
    });
    let mut lines: Vec<String> = vec![serde_json::to_string(&header).unwrap_or_default()];
    let mut parent_id: Option<String> = None;
    for mut entry in session_manager.get_branch(None) {
        let id = entry.id().to_string();
        entry.set_parent_id(parent_id.clone());
        lines.push(serde_json::to_string(&entry).unwrap_or_default());
        parent_id = Some(id);
    }
    if let Some(create) = create_trailing_entries {
        for entry in create(parent_id, timestamp) {
            lines.push(serde_json::to_string(&entry).unwrap_or_default());
        }
    }
    format!("{}\n", lines.join("\n"))
}

/// Write the current session branch and optional export-only entries as JSONL.
pub fn export_session_to_jsonl(
    session_manager: &SessionManager,
    output_path: Option<&str>,
    create_trailing_entries: Option<&TrailingEntries>,
) -> anyhow::Result<String> {
    let default_name = format!("session-{}.jsonl", now_iso().replace([':', '.'], "-"));
    let cwd = std::env::current_dir()?.to_string_lossy().into_owned();
    let file_path = resolve_path(output_path.unwrap_or(&default_name), Some(&cwd), None);
    if let Some(dir) = std::path::Path::new(&file_path).parent()
        && !dir.as_os_str().is_empty()
        && !dir.exists()
    {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file_path, serialize_session_branch(session_manager, create_trailing_entries))?;
    Ok(file_path)
}
