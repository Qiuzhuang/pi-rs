//! Port of `packages/coding-agent/src/modes/interactive/program-status-reporter.ts`.

use crate::core::agent_session::AgentSessionEvent;
use pi_tui::terminal::Terminal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockedStatusKind {
    #[serde(rename = "permission")]
    Permission,
    #[serde(rename = "question")]
    Question,
    #[serde(rename = "auth")]
    Auth,
}

impl BlockedStatusKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedStatus {
    pub kind: BlockedStatusKind,
    pub message: String,
}

/// Reports interactive-mode state to the terminal (OSC 7501): `working` during agent runs and
/// compaction, `blocked` while a dialog waits for the user, then `done`, `error`, or `idle` once the run
/// settles. Messages are limited to the session name, dialog titles, and the first line of errors; prompts
/// and assistant output are never reported.
pub struct ProgramStatusReporter {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ProgramStatusReporter {
    pub fn new(
        get_terminal: Arc<dyn Fn() -> Arc<dyn Terminal> + Send + Sync>,
        get_session_name: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    ) -> Self {
        todo!("port modes/interactive/program-status-reporter.ts:ProgramStatusReporter.constructor")
    }

    pub fn handle_event(&self, event: &AgentSessionEvent) {
        todo!("port modes/interactive/program-status-reporter.ts:ProgramStatusReporter.handleEvent")
    }

    /// Report `blocked` for a dialog until it is cleared with `undefined`. Reopening a source replaces it.
    pub fn set_blocked(&self, source: &str, status: Option<&BlockedStatus>) {
        todo!("port modes/interactive/program-status-reporter.ts:ProgramStatusReporter.setBlocked")
    }

    /// Forget the previous session's run, for example after switching sessions.
    pub fn reset(&self) {
        todo!("port modes/interactive/program-status-reporter.ts:ProgramStatusReporter.reset")
    }

    pub fn report(&self) {
        todo!("port modes/interactive/program-status-reporter.ts:ProgramStatusReporter.report")
    }
}
