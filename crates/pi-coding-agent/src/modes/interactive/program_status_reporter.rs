//! Port of `packages/coding-agent/src/modes/interactive/program-status-reporter.ts`.

use crate::config::APP_NAME;
use crate::core::agent_session::AgentSessionEvent;
use crate::core::extensions::types::CompactionReason;
use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::types::StopReason;
use pi_tui::program_status::{BlockedKind, ProgramState, ProgramStatus};
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

    fn to_tui(self) -> BlockedKind {
        match self {
            Self::Permission => BlockedKind::Permission,
            Self::Question => BlockedKind::Question,
            Self::Auth => BlockedKind::Auth,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockedStatus {
    pub kind: BlockedStatusKind,
    pub message: String,
}

fn first_line(text: Option<&str>) -> String {
    let line = text.and_then(|t| t.split('\n').next()).map(|l| l.strip_suffix('\r').unwrap_or(l).trim());
    match line {
        Some(l) if !l.is_empty() => l.to_string(),
        _ => "Error".to_string(),
    }
}

fn status(state: ProgramState, message: Option<String>) -> ProgramStatus {
    ProgramStatus { state, app: None, kind: None, message }
}

struct State {
    run_active: bool,
    compacting: bool,
    /// Outcome of the current run, reported once it settles.
    run_result: ProgramStatus,
    /// Status while no run is active.
    resting_status: ProgramStatus,
    /// Open dialogs by source, in the order they opened. The most recent one is reported.
    blocked: IndexMap<String, BlockedStatus>,
    last_report: Option<ProgramStatus>,
}

/// Reports interactive-mode state to the terminal (OSC 7501): `working` during agent runs and
/// compaction, `blocked` while a dialog waits for the user, then `done`, `error`, or `idle` once the run
/// settles. Messages are limited to the session name, dialog titles, and the first line of errors; prompts
/// and assistant output are never reported.
pub struct ProgramStatusReporter {
    get_terminal: Arc<dyn Fn() -> Arc<dyn Terminal> + Send + Sync>,
    get_session_name: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    state: Mutex<State>,
}

impl ProgramStatusReporter {
    pub fn new(
        get_terminal: Arc<dyn Fn() -> Arc<dyn Terminal> + Send + Sync>,
        get_session_name: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    ) -> Self {
        Self {
            get_terminal,
            get_session_name,
            state: Mutex::new(State {
                run_active: false,
                compacting: false,
                run_result: status(ProgramState::Done, None),
                resting_status: status(ProgramState::Idle, None),
                blocked: IndexMap::new(),
                last_report: None,
            }),
        }
    }

    pub fn handle_event(&self, event: &AgentSessionEvent) {
        {
            let mut s = self.state.lock();
            match event {
                AgentSessionEvent::AgentStart => {
                    s.run_active = true;
                    s.run_result = status(ProgramState::Done, None);
                }
                AgentSessionEvent::MessageEnd { message } => {
                    // The latest response decides the outcome, so a retried error is replaced by its successful retry.
                    let AgentMessage::Assistant(message) = message else { return };
                    s.run_result = if message.stop_reason == StopReason::Error {
                        status(ProgramState::Error, Some(first_line(message.error_message.as_deref())))
                    } else {
                        status(ProgramState::Done, None)
                    };
                }
                AgentSessionEvent::CompactionStart { .. } => {
                    s.compacting = true;
                }
                AgentSessionEvent::CompactionEnd { reason, aborted, error_message, .. } => {
                    s.compacting = false;
                    if s.run_active {
                        // A failed recovery compaction ends the run unless a later response succeeds.
                        if *aborted {
                            s.run_result = status(ProgramState::Idle, None);
                        } else if let Some(error) = error_message {
                            s.run_result = status(ProgramState::Error, Some(first_line(Some(error))));
                        }
                    } else if *aborted {
                        s.resting_status = status(ProgramState::Idle, None);
                    } else if *reason == CompactionReason::Manual {
                        s.resting_status = match error_message {
                            Some(error) => status(ProgramState::Error, Some(first_line(Some(error)))),
                            None => status(ProgramState::Done, None),
                        };
                    }
                }
                AgentSessionEvent::AgentSettled { aborted } => {
                    s.run_active = false;
                    s.resting_status =
                        if *aborted { status(ProgramState::Idle, None) } else { s.run_result.clone() };
                }
                AgentSessionEvent::SessionInfoChanged { .. } => {
                    // The session name is part of working and done reports.
                }
                _ => return,
            }
        }
        self.report();
    }

    /// Report `blocked` for a dialog until it is cleared with `None`. Reopening a source replaces it.
    pub fn set_blocked(&self, source: &str, status: Option<&BlockedStatus>) {
        {
            let mut s = self.state.lock();
            s.blocked.shift_remove(source);
            if let Some(status) = status {
                s.blocked.insert(source.to_string(), status.clone());
            }
        }
        self.report();
    }

    /// Forget the previous session's run, for example after switching sessions.
    pub fn reset(&self) {
        {
            let mut s = self.state.lock();
            s.run_active = false;
            s.compacting = false;
            s.run_result = status(ProgramState::Done, None);
            s.resting_status = status(ProgramState::Idle, None);
        }
        self.report();
    }

    pub fn report(&self) {
        let mut status = self.current_status();
        status.app = Some(APP_NAME.to_string());
        {
            let mut s = self.state.lock();
            if s.last_report.as_ref() == Some(&status) {
                return;
            }
            s.last_report = Some(status.clone());
        }
        (self.get_terminal)().set_program_status(&status);
    }

    fn current_status(&self) -> ProgramStatus {
        let (blocked, compacting, run_active, resting) = {
            let s = self.state.lock();
            (s.blocked.values().last().cloned(), s.compacting, s.run_active, s.resting_status.clone())
        };
        if let Some(blocked) = blocked {
            return ProgramStatus {
                state: ProgramState::Blocked,
                app: None,
                kind: Some(blocked.kind.to_tui()),
                message: Some(blocked.message),
            };
        }
        if compacting {
            return status(ProgramState::Working, Some("Compacting context".to_string()));
        }
        let current = if run_active { status(ProgramState::Working, None) } else { resting };
        if matches!(current.state, ProgramState::Working | ProgramState::Done) {
            return ProgramStatus { message: (self.get_session_name)(), ..current };
        }
        current
    }
}
