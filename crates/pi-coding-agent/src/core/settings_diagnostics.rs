//! Port of `packages/coding-agent/src/core/settings-diagnostics.ts`.

use crate::core::agent_session_services::AgentSessionRuntimeDiagnostic;
use crate::core::settings_manager::SettingsManager;
use std::sync::Arc;

pub fn collect_settings_diagnostics(
    settings_manager: Arc<SettingsManager>,
) -> Vec<AgentSessionRuntimeDiagnostic> {
    todo!("port core/settings-diagnostics.ts:collectSettingsDiagnostics")
}

/// Remove duplicate type/message diagnostics while preserving their first occurrence.
/// Startup and runtime settings managers can report the same file error.
pub fn deduplicate_diagnostics(
    diagnostics: &[AgentSessionRuntimeDiagnostic],
) -> Vec<AgentSessionRuntimeDiagnostic> {
    todo!("port core/settings-diagnostics.ts:deduplicateDiagnostics")
}
