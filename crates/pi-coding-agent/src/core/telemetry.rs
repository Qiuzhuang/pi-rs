//! Port of `packages/coding-agent/src/core/telemetry.ts`.

use crate::core::settings_manager::SettingsManager;
use std::sync::Arc;

fn is_truthy_env_flag(value: Option<&str>) -> bool {
    match value {
        None | Some("") => false,
        Some(value) => {
            let lower = value.to_lowercase();
            value == "1" || lower == "true" || lower == "yes"
        }
    }
}

/// `telemetry_env` defaults to `PI_TELEMETRY`; pass `Some(..)` to override it (tests).
pub fn is_install_telemetry_enabled(
    settings_manager: Arc<SettingsManager>,
    telemetry_env: Option<&str>,
) -> bool {
    let from_env = std::env::var("PI_TELEMETRY").ok();
    let telemetry_env = telemetry_env.or(from_env.as_deref());
    match telemetry_env {
        Some(value) => is_truthy_env_flag(Some(value)),
        None => settings_manager.get_enable_install_telemetry(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_flags() {
        assert!(is_truthy_env_flag(Some("1")));
        assert!(is_truthy_env_flag(Some("TRUE")));
        assert!(is_truthy_env_flag(Some("Yes")));
        assert!(!is_truthy_env_flag(Some("0")));
        assert!(!is_truthy_env_flag(Some("")));
        assert!(!is_truthy_env_flag(None));
    }
}
