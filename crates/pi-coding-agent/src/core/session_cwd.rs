//! Port of `packages/coding-agent/src/core/session-cwd.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCwdIssue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub session_cwd: String,
    pub fallback_cwd: String,
}

/// The parts of a session manager needed to check its stored cwd.
pub trait SessionCwdSource: Send + Sync {
    fn get_cwd(&self) -> String;
    fn get_session_file(&self) -> Option<String>;
}

pub fn get_missing_session_cwd_issue(
    session_manager: &dyn SessionCwdSource,
    fallback_cwd: &str,
) -> Option<SessionCwdIssue> {
    let session_file = session_manager.get_session_file().filter(|f| !f.is_empty())?;

    let session_cwd = session_manager.get_cwd();
    if session_cwd.is_empty() || std::path::Path::new(&session_cwd).exists() {
        return None;
    }

    Some(SessionCwdIssue {
        session_file: Some(session_file),
        session_cwd,
        fallback_cwd: fallback_cwd.to_string(),
    })
}

pub fn format_missing_session_cwd_error(issue: &SessionCwdIssue) -> String {
    let session_file = match issue.session_file.as_deref() {
        Some(file) if !file.is_empty() => format!("\nSession file: {file}"),
        _ => String::new(),
    };
    format!(
        "Stored session working directory does not exist: {}{}\nCurrent working directory: {}",
        issue.session_cwd, session_file, issue.fallback_cwd
    )
}

pub fn format_missing_session_cwd_prompt(issue: &SessionCwdIssue) -> String {
    format!(
        "cwd from session file does not exist\n{}\n\ncontinue in current cwd\n{}",
        issue.session_cwd, issue.fallback_cwd
    )
}

/// TS: `extends Error`
#[derive(Debug, Clone)]
pub struct MissingSessionCwdError {
    pub message: String,
    issue: SessionCwdIssue,
}

impl std::fmt::Display for MissingSessionCwdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MissingSessionCwdError {}

impl MissingSessionCwdError {
    pub fn new(issue: &SessionCwdIssue) -> Self {
        Self {
            message: format_missing_session_cwd_error(issue),
            issue: issue.clone(),
        }
    }

    pub fn issue(&self) -> SessionCwdIssue {
        self.issue.clone()
    }

    /// The JS error name.
    pub fn name(&self) -> &'static str {
        "MissingSessionCwdError"
    }
}

/// Fails with [`MissingSessionCwdError`] (downcastable from the `anyhow::Error`) when the stored
/// session cwd no longer exists.
pub fn assert_session_cwd_exists(
    session_manager: &dyn SessionCwdSource,
    fallback_cwd: &str,
) -> anyhow::Result<()> {
    if let Some(issue) = get_missing_session_cwd_issue(session_manager, fallback_cwd) {
        return Err(MissingSessionCwdError::new(&issue).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session_manager::SessionManager;

    struct Source {
        cwd: String,
        file: Option<String>,
    }

    impl SessionCwdSource for Source {
        fn get_cwd(&self) -> String {
            self.cwd.clone()
        }
        fn get_session_file(&self) -> Option<String> {
            self.file.clone()
        }
    }

    #[test]
    fn returns_issue_for_persisted_session_with_missing_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist").to_string_lossy().into_owned();
        let fallback = tmp.path().to_string_lossy().into_owned();
        let source = Source {
            cwd: missing.clone(),
            file: Some("/tmp/session.jsonl".into()),
        };
        let issue = get_missing_session_cwd_issue(&source, &fallback).unwrap();
        assert_eq!(issue.session_cwd, missing);
        assert_eq!(issue.fallback_cwd, fallback);
        assert_eq!(issue.session_file.as_deref(), Some("/tmp/session.jsonl"));

        let error = assert_session_cwd_exists(&source, &fallback).unwrap_err();
        let error = error.downcast_ref::<MissingSessionCwdError>().unwrap();
        assert_eq!(error.issue(), issue);
        assert!(error.message.contains("Stored session working directory does not exist"));
        assert!(error.message.contains("Session file: /tmp/session.jsonl"));
    }

    #[test]
    fn ignores_in_memory_sessions_and_existing_cwds() {
        let tmp = tempfile::tempdir().unwrap();
        let existing = tmp.path().to_string_lossy().into_owned();
        let in_memory = Source {
            cwd: "/definitely/missing/dir".into(),
            file: None,
        };
        assert!(get_missing_session_cwd_issue(&in_memory, &existing).is_none());
        let present = Source {
            cwd: existing.clone(),
            file: Some("/tmp/session.jsonl".into()),
        };
        assert!(get_missing_session_cwd_issue(&present, &existing).is_none());
        assert!(assert_session_cwd_exists(&present, &existing).is_ok());
    }

    #[test]
    fn session_manager_reports_missing_cwd_after_open() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&session_dir).unwrap();
        let missing = tmp.path().join("gone").to_string_lossy().into_owned();
        let file = session_dir.join("s.jsonl");
        std::fs::write(
            &file,
            format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"abc\",\"timestamp\":\"2025-01-01T00:00:00.000Z\",\"cwd\":{}}}\n",
                serde_json::to_string(&missing).unwrap()
            ),
        )
        .unwrap();
        let sm = SessionManager::open(&file.to_string_lossy(), None, None).unwrap();
        let issue = get_missing_session_cwd_issue(sm.as_ref(), "/fallback").unwrap();
        assert_eq!(issue.session_cwd, missing);
        assert_eq!(issue.session_file, sm.get_session_file());

        // Overriding the effective cwd when opening a session.
        let fallback = tmp.path().to_string_lossy().into_owned();
        let overridden = SessionManager::open(&file.to_string_lossy(), None, Some(&fallback)).unwrap();
        assert_eq!(overridden.get_cwd(), fallback);
        assert!(get_missing_session_cwd_issue(overridden.as_ref(), &fallback).is_none());
        assert_eq!(
            format_missing_session_cwd_prompt(&issue),
            format!("cwd from session file does not exist\n{missing}\n\ncontinue in current cwd\n/fallback")
        );
    }

    #[test]
    fn formats_error_without_session_file() {
        let issue = SessionCwdIssue {
            session_file: None,
            session_cwd: "/a".into(),
            fallback_cwd: "/b".into(),
        };
        assert_eq!(
            format_missing_session_cwd_error(&issue),
            "Stored session working directory does not exist: /a\nCurrent working directory: /b"
        );
    }
}
