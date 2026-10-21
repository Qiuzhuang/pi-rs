//! Port of `packages/coding-agent/src/core/client-identity.ts`.

use crate::core::settings_manager::SettingsManager;
use pi_ai::utils::identity::ClientIdentity;
use std::sync::Arc;

/// How pi's requests identify it to providers: a detailed `User-Agent` with pi's version, and attribution to pi as a
/// CLI agent while install telemetry is enabled.
pub fn get_client_identity(settings_manager: Arc<SettingsManager>) -> ClientIdentity {
    todo!("port core/client-identity.ts:getClientIdentity")
}
