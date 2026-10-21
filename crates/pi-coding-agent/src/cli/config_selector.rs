//! Port of `packages/coding-agent/src/cli/config-selector.ts`.

use crate::core::settings_manager::SettingsManager;
use crate::core::settings_manager::SettingsScope;
use crate::modes::interactive::components::config_selector::ScopedResolvedPaths;
use std::sync::Arc;

#[derive(Clone)]
pub struct ConfigSelectorOptions {
    pub resolved_paths: ScopedResolvedPaths,
    pub settings_manager: Arc<SettingsManager>,
    pub cwd: String,
    pub agent_dir: String,
    pub write_scope: SettingsScope,
    pub project_mode_available: bool,
}

/// Show TUI config selector and return when closed
pub async fn select_config(options: &ConfigSelectorOptions) -> anyhow::Result<()> {
    todo!("port cli/config-selector.ts:selectConfig")
}
