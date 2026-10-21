//! Port of `packages/coding-agent/src/core/session-cwd.ts`.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCwdIssue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub session_cwd: String,
    pub fallback_cwd: String,
}

pub trait SessionCwdSource: Send + Sync {
    fn get_cwd(&self) -> String;
    fn get_session_file(&self) -> Option<String>;
}

pub fn get_missing_session_cwd_issue(
    session_manager: Arc<dyn SessionCwdSource>,
    fallback_cwd: &str,
) -> Option<SessionCwdIssue> {
    todo!("port core/session-cwd.ts:getMissingSessionCwdIssue")
}

pub fn format_missing_session_cwd_error(issue: &SessionCwdIssue) -> String {
    todo!("port core/session-cwd.ts:formatMissingSessionCwdError")
}

pub fn format_missing_session_cwd_prompt(issue: &SessionCwdIssue) -> String {
    todo!("port core/session-cwd.ts:formatMissingSessionCwdPrompt")
}

/// TS: `extends Error`
#[derive(Debug, Clone)]
pub struct MissingSessionCwdError {
    pub message: String,
}

impl std::fmt::Display for MissingSessionCwdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MissingSessionCwdError {}

impl MissingSessionCwdError {
    pub fn new(issue: &SessionCwdIssue) -> Self {
        todo!("port core/session-cwd.ts:MissingSessionCwdError.constructor")
    }

    pub fn issue(&self) -> SessionCwdIssue {
        todo!("port core/session-cwd.ts:MissingSessionCwdError.issue")
    }
}

pub fn assert_session_cwd_exists(
    session_manager: Arc<dyn SessionCwdSource>,
    fallback_cwd: &str,
) -> anyhow::Result<()> {
    todo!("port core/session-cwd.ts:assertSessionCwdExists")
}
