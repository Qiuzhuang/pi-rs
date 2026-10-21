//! Port of `packages/coding-agent/src/modes/interactive/session-share.ts`.

use crate::core::agent_session::AgentSession;
use pi_tui::components::editor::EditorComponent;
use pi_tui::tui::Container;
use pi_tui::tui::Tui;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct SessionShareContext {
    pub session: Arc<AgentSession>,
    pub ui: Arc<Tui>,
    pub editor_container: Arc<Container>,
    pub editor: Arc<dyn EditorComponent>,
    pub show_status: Arc<dyn Fn(String) + Send + Sync>,
    pub show_error: Arc<dyn Fn(String) + Send + Sync>,
}

/// Trailing `pi.share` entry carrying the system prompt and tool schemas for the session viewer.
pub fn create_share_trailing_entries(
    session: Arc<AgentSession>,
    parent_id: Option<&str>,
    timestamp: &str,
) -> Vec<Value> {
    todo!("port modes/interactive/session-share.ts:createShareTrailingEntries")
}

/// Export the current branch with presentation metadata for Radius.
pub fn export_session_for_share(file_path: &str, session: Arc<AgentSession>) {
    todo!("port modes/interactive/session-share.ts:exportSessionForShare")
}

/// Share the current session through Radius, falling back to a private gist.
pub async fn share_session(context: &SessionShareContext) -> anyhow::Result<()> {
    todo!("port modes/interactive/session-share.ts:shareSession")
}
