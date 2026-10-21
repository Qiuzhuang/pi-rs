//! Port of `packages/coding-agent/src/core/session-export.ts`.

use crate::core::session_manager::SessionManager;
use serde_json::Value;
use std::sync::Arc;

pub type TrailingEntries = Arc<dyn Fn(Option<String>, String) -> Vec<Value> + Send + Sync>;

/// Serialize the current branch and optional export-only entries as JSONL.
pub fn serialize_session_branch(
    session_manager: Arc<SessionManager>,
    create_trailing_entries: Option<&TrailingEntries>,
) -> String {
    todo!("port core/session-export.ts:serializeSessionBranch")
}

/// Write the current session branch and optional export-only entries as JSONL.
pub fn export_session_to_jsonl(
    session_manager: Arc<SessionManager>,
    output_path: Option<&str>,
    create_trailing_entries: Option<&TrailingEntries>,
) -> String {
    todo!("port core/session-export.ts:exportSessionToJsonl")
}
