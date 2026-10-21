//! Port of `packages/coding-agent/src/core/telemetry.ts`.

use crate::core::settings_manager::SettingsManager;
use std::sync::Arc;

pub fn is_install_telemetry_enabled(
    settings_manager: Arc<SettingsManager>,
    telemetry_env: Option<&str>,
) -> bool {
    todo!("port core/telemetry.ts:isInstallTelemetryEnabled")
}
