//! Port of `packages/coding-agent/src/core/settings-diagnostics.ts`.

use crate::core::agent_session_services::{
    AgentSessionRuntimeDiagnostic, AgentSessionRuntimeDiagnosticType,
};
use crate::core::settings_manager::SettingsManager;
use std::collections::HashSet;
use std::sync::Arc;

pub fn collect_settings_diagnostics(
    settings_manager: Arc<SettingsManager>,
) -> Vec<AgentSessionRuntimeDiagnostic> {
    settings_manager
        .drain_errors()
        .into_iter()
        .map(|error| AgentSessionRuntimeDiagnostic {
            r#type: AgentSessionRuntimeDiagnosticType::Warning,
            message: match &error.path {
                Some(path) => format!("Invalid settings file {path}: {}", error.error),
                None => format!("Invalid {} settings: {}", error.scope.as_str(), error.error),
            },
        })
        .collect()
}

/// Remove duplicate type/message diagnostics while preserving their first occurrence.
/// Startup and runtime settings managers can report the same file error.
pub fn deduplicate_diagnostics(
    diagnostics: &[AgentSessionRuntimeDiagnostic],
) -> Vec<AgentSessionRuntimeDiagnostic> {
    let mut seen = HashSet::new();
    diagnostics
        .iter()
        .filter(|d| seen.insert(format!("{}\0{}", d.r#type.as_str(), d.message)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings_manager::{SettingsLockFn, SettingsScope, SettingsStorage};

    #[test]
    fn includes_the_settings_file_path_for_file_backed_storage() {
        let temp = tempfile::tempdir().unwrap();
        let agent_dir = temp.path().join("agent");
        let settings_path = agent_dir.join("settings.json");
        std::fs::create_dir(&agent_dir).unwrap();
        std::fs::write(&settings_path, "{").unwrap();

        let diagnostics = collect_settings_diagnostics(SettingsManager::create(
            temp.path().to_str().unwrap(),
            Some(agent_dir.to_str().unwrap()),
            None,
        ));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].r#type,
            AgentSessionRuntimeDiagnosticType::Warning
        );
        assert!(diagnostics[0].message.contains(&format!(
            "Invalid settings file {}:",
            settings_path.display()
        )));
    }

    struct FailingStorage;
    impl SettingsStorage for FailingStorage {
        fn with_lock(&self, scope: SettingsScope, f: SettingsLockFn<'_>) -> anyhow::Result<()> {
            if scope == SettingsScope::Global {
                anyhow::bail!("backend failed");
            }
            f(None)?;
            Ok(())
        }
    }

    #[test]
    fn falls_back_to_the_settings_scope_for_storage_without_file_paths() {
        let diagnostics =
            collect_settings_diagnostics(SettingsManager::from_storage(Arc::new(FailingStorage), None));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "Invalid global settings: backend failed");
    }

    #[test]
    fn deduplicates_diagnostics_by_type_and_message() {
        let warning = AgentSessionRuntimeDiagnostic {
            r#type: AgentSessionRuntimeDiagnosticType::Warning,
            message: "Invalid settings file /tmp/settings.json".to_string(),
        };
        let error = AgentSessionRuntimeDiagnostic {
            r#type: AgentSessionRuntimeDiagnosticType::Error,
            message: warning.message.clone(),
        };
        let result = deduplicate_diagnostics(&[warning.clone(), warning.clone(), error.clone()]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].r#type, AgentSessionRuntimeDiagnosticType::Warning);
        assert_eq!(result[1].r#type, AgentSessionRuntimeDiagnosticType::Error);
    }
}
