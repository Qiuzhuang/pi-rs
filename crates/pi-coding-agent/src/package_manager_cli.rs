//! Port of `packages/coding-agent/src/package-manager-cli.ts`.

use crate::core::extensions::types::InlineExtension;
use crate::core::settings_manager::SettingsManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageCommand {
    #[serde(rename = "install")]
    Install,
    #[serde(rename = "remove")]
    Remove,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "list")]
    List,
}

impl PackageCommand {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Remove => "remove",
            Self::Update => "update",
            Self::List => "list",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum UpdateTarget {
    #[serde(rename = "all")]
    All {},
    #[serde(rename = "self")]
    SelfValue {},
    #[serde(rename = "extensions")]
    Extensions {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    #[serde(rename = "models")]
    Models {},
}

pub fn cleanup_managed_install() {
    todo!("port package-manager-cli.ts:cleanupManagedInstall")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageCommandOptions {
    pub command: PackageCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_target: Option<UpdateTarget>,
    pub show_extensions_skipped_note: bool,
    pub local: bool,
    pub force: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_trust_override: Option<bool>,
    pub help: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_option: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_argument: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_option_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflicting_options: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfUpdatePlan {
    pub package_name: String,
    pub install_spec: String,
    pub version: String,
    pub should_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone)]
pub struct PackageCommandRuntimeOptions {
    pub extension_factories: Option<Vec<InlineExtension>>,
}

#[derive(Clone)]
pub struct CommandSettingsResult {
    pub settings_manager: Arc<SettingsManager>,
    pub project_trust_warnings: Vec<String>,
}

pub async fn handle_config_command(
    args: &[String],
    runtime_options: Option<&PackageCommandRuntimeOptions>,
) -> anyhow::Result<bool> {
    todo!("port package-manager-cli.ts:handleConfigCommand")
}

pub async fn handle_package_command(
    args: &[String],
    runtime_options: Option<&PackageCommandRuntimeOptions>,
) -> anyhow::Result<bool> {
    todo!("port package-manager-cli.ts:handlePackageCommand")
}
