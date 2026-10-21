//! Port of `packages/coding-agent/src/core/package-manager.ts`.

use crate::core::settings_manager::SettingsManager;
use crate::core::source_info::SourceOrigin;
use crate::modes::interactive::components::config_selector::SettingsScope;
use crate::utils::git::GitSource;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathMetadata {
    pub source: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedResource {
    pub path: String,
    pub enabled: bool,
    pub metadata: PathMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPaths {
    pub extensions: Vec<ResolvedResource>,
    pub skills: Vec<ResolvedResource>,
    pub prompts: Vec<ResolvedResource>,
    pub themes: Vec<ResolvedResource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MissingSourceAction {
    #[serde(rename = "install")]
    Install,
    #[serde(rename = "skip")]
    Skip,
    #[serde(rename = "error")]
    Error,
}

impl MissingSourceAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Skip => "skip",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProgressEventType {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "progress")]
    Progress,
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "error")]
    Error,
}

impl ProgressEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Progress => "progress",
            Self::Complete => "complete",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProgressEventAction {
    #[serde(rename = "install")]
    Install,
    #[serde(rename = "remove")]
    Remove,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "clone")]
    Clone,
    #[serde(rename = "pull")]
    Pull,
}

impl ProgressEventAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Remove => "remove",
            Self::Update => "update",
            Self::Clone => "clone",
            Self::Pull => "pull",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub r#type: ProgressEventType,
    pub action: ProgressEventAction,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub type ProgressCallback = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageUpdateType {
    #[serde(rename = "npm")]
    Npm,
    #[serde(rename = "git")]
    Git,
}

impl PackageUpdateType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Git => "git",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUpdate {
    pub source: String,
    pub display_name: String,
    pub r#type: PackageUpdateType,
    pub scope: SettingsScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguredPackage {
    pub source: String,
    pub scope: SettingsScope,
    pub filtered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerInstallOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerInstallAndPersistOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerRemoveOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerRemoveAndPersistOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerResolveExtensionSourcesOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporary: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerAddSourceToSettingsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerRemoveSourceFromSettingsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[async_trait::async_trait]
pub trait PackageManager: Send + Sync {
    async fn resolve(
        &self,
        on_missing: Option<
            Arc<
                dyn Fn(String) -> BoxFuture<'static, anyhow::Result<MissingSourceAction>>
                    + Send
                    + Sync,
            >,
        >,
    ) -> anyhow::Result<ResolvedPaths>;
    async fn install(
        &self,
        source: &str,
        options: Option<&PackageManagerInstallOptions>,
    ) -> anyhow::Result<()>;
    async fn install_and_persist(
        &self,
        source: &str,
        options: Option<&PackageManagerInstallAndPersistOptions>,
    ) -> anyhow::Result<()>;
    async fn remove(
        &self,
        source: &str,
        options: Option<&PackageManagerRemoveOptions>,
    ) -> anyhow::Result<()>;
    async fn remove_and_persist(
        &self,
        source: &str,
        options: Option<&PackageManagerRemoveAndPersistOptions>,
    ) -> anyhow::Result<bool>;
    async fn update(&self, source: Option<&str>) -> anyhow::Result<()>;
    fn list_configured_packages(&self) -> Vec<ConfiguredPackage>;
    async fn resolve_extension_sources(
        &self,
        sources: &[String],
        options: Option<&PackageManagerResolveExtensionSourcesOptions>,
    ) -> anyhow::Result<ResolvedPaths>;
    fn add_source_to_settings(
        &self,
        source: &str,
        options: Option<&PackageManagerAddSourceToSettingsOptions>,
    ) -> bool;
    fn remove_source_from_settings(
        &self,
        source: &str,
        options: Option<&PackageManagerRemoveSourceFromSettingsOptions>,
    ) -> bool;
    fn set_progress_callback(&self, callback: Option<&ProgressCallback>);
    fn get_installed_path(&self, source: &str, scope: SettingsScope) -> Option<String>;
}

#[derive(Clone)]
pub struct PackageManagerOptions {
    pub cwd: String,
    pub agent_dir: String,
    pub settings_manager: Arc<SettingsManager>,
    /// Names of built-in extensions, resolved as `builtin:<name>` extension resources.
    pub builtin_extensions: Option<Vec<String>>,
}

/// TS: a local `SourceScope` alias identical to `source-info.ts`.
pub use crate::core::source_info::SourceScope;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmSource {
    /// Always "npm".
    pub r#type: String,
    pub spec: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSource {
    /// Always "local".
    pub r#type: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParsedSource {
    NpmSource(NpmSource),
    GitSource(GitSource),
    LocalSource(LocalSource),
}

pub type InstalledSourceScope = SettingsScope;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguredUpdateSource {
    pub source: String,
    pub scope: InstalledSourceScope,
}

// TS: extends ConfiguredUpdateSource (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmUpdateTarget {
    pub source: String,
    pub scope: InstalledSourceScope,
    pub parsed: NpmSource,
}

// TS: extends ConfiguredUpdateSource (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitUpdateTarget {
    pub source: String,
    pub scope: InstalledSourceScope,
    pub parsed: GitSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAccumulatorExtensions {
    pub metadata: PathMetadata,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAccumulatorSkills {
    pub metadata: PathMetadata,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAccumulatorPrompts {
    pub metadata: PathMetadata,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAccumulatorThemes {
    pub metadata: PathMetadata,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAccumulator {
    pub extensions: IndexMap<String, ResourceAccumulatorExtensions>,
    pub skills: IndexMap<String, ResourceAccumulatorSkills>,
    pub prompts: IndexMap<String, ResourceAccumulatorPrompts>,
    pub themes: IndexMap<String, ResourceAccumulatorThemes>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoload: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub themes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    #[serde(rename = "extensions")]
    Extensions,
    #[serde(rename = "skills")]
    Skills,
    #[serde(rename = "prompts")]
    Prompts,
    #[serde(rename = "themes")]
    Themes,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Extensions => "extensions",
            Self::Skills => "skills",
            Self::Prompts => "prompts",
            Self::Themes => "themes",
        }
    }
}

pub type IgnoreMatcher = Value;

pub fn get_extension_temp_folder(agent_dir: &str) -> String {
    todo!("port core/package-manager.ts:getExtensionTempFolder")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillDiscoveryMode {
    #[serde(rename = "pi")]
    Pi,
    #[serde(rename = "agents")]
    Agents,
}

impl SkillDiscoveryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Agents => "agents",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPackageManagerAddSourceToSettingsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPackageManagerRemoveSourceFromSettingsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPackageManagerResolveExtensionSourcesOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporary: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPackageManagerInstallOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPackageManagerInstallAndPersistOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPackageManagerRemoveOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPackageManagerRemoveAndPersistOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
}

/// TS: `implements PackageManager`
pub struct DefaultPackageManager {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl DefaultPackageManager {
    pub fn new(options: &PackageManagerOptions) -> Self {
        todo!("port core/package-manager.ts:DefaultPackageManager.constructor")
    }

    pub async fn check_for_available_updates(&self) -> anyhow::Result<Vec<PackageUpdate>> {
        todo!("port core/package-manager.ts:DefaultPackageManager.checkForAvailableUpdates")
    }
}

#[async_trait::async_trait]
impl PackageManager for DefaultPackageManager {
    async fn resolve(
        &self,
        on_missing: Option<
            Arc<
                dyn Fn(String) -> BoxFuture<'static, anyhow::Result<MissingSourceAction>>
                    + Send
                    + Sync,
            >,
        >,
    ) -> anyhow::Result<ResolvedPaths> {
        todo!("port core/package-manager.ts:DefaultPackageManager.resolve")
    }
    async fn install(
        &self,
        source: &str,
        options: Option<&PackageManagerInstallOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/package-manager.ts:DefaultPackageManager.install")
    }
    async fn install_and_persist(
        &self,
        source: &str,
        options: Option<&PackageManagerInstallAndPersistOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/package-manager.ts:DefaultPackageManager.install_and_persist")
    }
    async fn remove(
        &self,
        source: &str,
        options: Option<&PackageManagerRemoveOptions>,
    ) -> anyhow::Result<()> {
        todo!("port core/package-manager.ts:DefaultPackageManager.remove")
    }
    async fn remove_and_persist(
        &self,
        source: &str,
        options: Option<&PackageManagerRemoveAndPersistOptions>,
    ) -> anyhow::Result<bool> {
        todo!("port core/package-manager.ts:DefaultPackageManager.remove_and_persist")
    }
    async fn update(&self, source: Option<&str>) -> anyhow::Result<()> {
        todo!("port core/package-manager.ts:DefaultPackageManager.update")
    }
    fn list_configured_packages(&self) -> Vec<ConfiguredPackage> {
        todo!("port core/package-manager.ts:DefaultPackageManager.list_configured_packages")
    }
    async fn resolve_extension_sources(
        &self,
        sources: &[String],
        options: Option<&PackageManagerResolveExtensionSourcesOptions>,
    ) -> anyhow::Result<ResolvedPaths> {
        todo!("port core/package-manager.ts:DefaultPackageManager.resolve_extension_sources")
    }
    fn add_source_to_settings(
        &self,
        source: &str,
        options: Option<&PackageManagerAddSourceToSettingsOptions>,
    ) -> bool {
        todo!("port core/package-manager.ts:DefaultPackageManager.add_source_to_settings")
    }
    fn remove_source_from_settings(
        &self,
        source: &str,
        options: Option<&PackageManagerRemoveSourceFromSettingsOptions>,
    ) -> bool {
        todo!("port core/package-manager.ts:DefaultPackageManager.remove_source_from_settings")
    }
    fn set_progress_callback(&self, callback: Option<&ProgressCallback>) {
        todo!("port core/package-manager.ts:DefaultPackageManager.set_progress_callback")
    }
    fn get_installed_path(&self, source: &str, scope: SettingsScope) -> Option<String> {
        todo!("port core/package-manager.ts:DefaultPackageManager.get_installed_path")
    }
}
