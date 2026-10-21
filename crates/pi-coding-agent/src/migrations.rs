//! Port of `packages/coding-agent/src/migrations.ts`.

use serde::{Deserialize, Serialize};

/// Migrate legacy oauth.json and settings.json apiKeys to auth.json.
///
/// @returns Array of provider names that were migrated
pub fn migrate_auth_to_auth_json() -> Vec<String> {
    todo!("port migrations.ts:migrateAuthToAuthJson")
}

/// Migrate sessions from ~/.pi/agent/*.jsonl to proper session directories.
///
/// Bug in v0.30.0: Sessions were saved to ~/.pi/agent/ instead of
/// ~/.pi/agent/sessions/<encoded-cwd>/. This migration moves them
/// to the correct location based on the cwd in their session header.
///
/// See: https://github.com/earendil-works/pi-mono/issues/320
pub fn migrate_sessions_from_agent_root() {
    todo!("port migrations.ts:migrateSessionsFromAgentRoot")
}

/// Print deprecation warnings and wait for keypress.
pub async fn show_deprecation_warnings(warnings: &[String]) -> anyhow::Result<()> {
    todo!("port migrations.ts:showDeprecationWarnings")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyRunMigrationsResult {
    pub migrated_auth_providers: Vec<String>,
    pub deprecation_warnings: Vec<String>,
}

/// Run all migrations. Called once on startup.
///
/// @returns Object with migration results and deprecation warnings
pub fn run_migrations(cwd: &str) -> EmptyRunMigrationsResult {
    todo!("port migrations.ts:runMigrations")
}
