//! Port of `packages/coding-agent/src/cli/startup-ui.ts`.

use crate::core::settings_manager::SettingsManager;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributionMetadata {
    pub package_name: String,
    pub app_name: String,
    pub config_dir_name: String,
}

pub async fn create_startup_tui(
    settings_manager: Arc<SettingsManager>,
) -> anyhow::Result<Arc<Tui>> {
    todo!("port cli/startup-ui.ts:createStartupTui")
}

pub fn start_startup_tui(ui: Arc<Tui>, settings_manager: Arc<SettingsManager>) {
    todo!("port cli/startup-ui.ts:startStartupTui")
}

/// First-time setup runs when all of these hold:
/// - this is the official Pi distribution (not a fork/rebrand)
/// - experimental features are enabled (PI_EXPERIMENTAL=1)
/// - the default agent directory is used (no custom agent dir override)
/// - setup was not completed before (settings.json does not exist)
pub fn should_run_first_time_setup(settings_path: Option<&str>) -> bool {
    todo!("port cli/startup-ui.ts:shouldRunFirstTimeSetup")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowStartupSelectorOptions {
    pub label: String,
    pub value: Value,
}

pub async fn show_startup_selector(
    settings_manager: Arc<SettingsManager>,
    title: &str,
    options: &[ShowStartupSelectorOptions],
) -> anyhow::Result<Option<Value>> {
    todo!("port cli/startup-ui.ts:showStartupSelector")
}

/// Show the first-time setup dialog and persist the result
pub async fn show_first_time_setup(settings_manager: Arc<SettingsManager>) -> anyhow::Result<()> {
    todo!("port cli/startup-ui.ts:showFirstTimeSetup")
}

pub async fn show_startup_input(
    settings_manager: Arc<SettingsManager>,
    title: &str,
    placeholder: Option<&str>,
) -> anyhow::Result<Option<String>> {
    todo!("port cli/startup-ui.ts:showStartupInput")
}
