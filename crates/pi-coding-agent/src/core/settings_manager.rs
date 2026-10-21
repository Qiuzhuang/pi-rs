//! Port of `packages/coding-agent/src/core/settings-manager.ts`.

use crate::compat::WheelScrollLines;
use crate::modes::interactive::components::tree_selector::FilterMode;
use indexmap::IndexMap;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::ProviderId;
use pi_tui::components::scroll_view::ScrollViewScrollbar;
use pi_tui::terminal_image::TerminalCapabilities;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub use crate::core::settings_schema::BranchSummarySettings;
pub use crate::core::settings_schema::CacheWarmingMode;
pub use crate::core::settings_schema::CodemodeMode;
pub use crate::core::settings_schema::CodemodeSettings;
pub use crate::core::settings_schema::CompactionModelOverride;
pub use crate::core::settings_schema::CompactionSettings;
pub use crate::core::settings_schema::DefaultProjectTrust;
pub use crate::core::settings_schema::FullscreenExitOutput;
pub use crate::core::settings_schema::ImageSettings;
pub use crate::core::settings_schema::MarkdownSettings;
pub use crate::core::settings_schema::MermaidRenderingMode;
pub use crate::core::settings_schema::PackageSource;
pub use crate::core::settings_schema::ProviderRetrySettings;
pub use crate::core::settings_schema::QuietStartup;
pub use crate::core::settings_schema::RetrySettings;
pub use crate::core::settings_schema::SessionShareDestination;
pub use crate::core::settings_schema::Settings;
pub use crate::core::settings_schema::TerminalSettings;
pub use crate::core::settings_schema::ThinkingBudgetsSettings;
pub use crate::core::settings_schema::TransportSetting;
pub use crate::core::settings_schema::TuiMode;
pub use crate::core::settings_schema::WarningSettings;

/// Cache-warming profile. "idle" also warms between agent runs.
pub const CACHE_WARMING_MODES: &[&str] = &["off", "streaming", "idle"];

/// Tools enabled at startup when `defaultTools` does not change them.
pub const DEFAULT_TOOL_NAMES: &[&str] = &["read", "bash", "edit", "write"];

/// Whether a tool selection entry is a `+name` or `-name` modifier.
pub fn is_tool_modifier(entry: Value) -> bool {
    todo!("port core/settings-manager.ts:isToolModifier")
}

/// Validate a tool list from `--tools` or the SDK `tools` option. It is either an allowlist of plain
/// names and patterns or a list of only `+name`/`-name` entries with exact names. Returns the
/// problem, or undefined when the list is valid.
pub fn get_tool_list_error(entries: &[String]) -> Option<String> {
    todo!("port core/settings-manager.ts:getToolListError")
}

/// Apply the `+name` and `-name` entries of `entries` to `base` in order: `+name` adds a tool and
/// `-name` removes one. Other entries are ignored.
pub fn apply_tool_modifiers(base: &[String], entries: &[String]) -> Vec<String> {
    todo!("port core/settings-manager.ts:applyToolModifiers")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsScope {
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "project")]
    Project,
}

impl SettingsScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerCreateOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_trusted: Option<bool>,
}

pub trait SettingsStorage: Send + Sync {
    fn with_lock(
        &self,
        scope: &SettingsScope,
        r#fn: Arc<dyn Fn(Option<String>) -> Option<String> + Send + Sync>,
    );
}

#[derive(Debug, Clone)]
pub struct SettingsError {
    pub scope: SettingsScope,
    pub path: Option<String>,
    pub error: Arc<anyhow::Error>,
}

pub type SettingsPaths = IndexMap<SettingsScope, String>;

/// TS: `implements SettingsStorage`
pub struct FileSettingsStorage {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl FileSettingsStorage {
    pub fn new(cwd: &str, agent_dir: &str) -> Self {
        todo!("port core/settings-manager.ts:FileSettingsStorage.constructor")
    }
}

impl SettingsStorage for FileSettingsStorage {
    fn with_lock(
        &self,
        scope: &SettingsScope,
        r#fn: Arc<dyn Fn(Option<String>) -> Option<String> + Send + Sync>,
    ) {
        todo!("port core/settings-manager.ts:FileSettingsStorage.with_lock")
    }
}

/// TS: `implements SettingsStorage`
pub struct InMemorySettingsStorage {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl InMemorySettingsStorage {
    pub fn new() -> Self {
        todo!("port core/settings-manager.ts:InMemorySettingsStorage.constructor")
    }
}

impl SettingsStorage for InMemorySettingsStorage {
    fn with_lock(
        &self,
        scope: &SettingsScope,
        r#fn: Arc<dyn Fn(Option<String>) -> Option<String> + Send + Sync>,
    ) {
        todo!("port core/settings-manager.ts:InMemorySettingsStorage.with_lock")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsManagerGetSteeringModeResult {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

impl SettingsManagerGetSteeringModeResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one-at-a-time",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerModel {
    pub provider: ProviderId,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerModel2 {
    pub provider: ProviderId,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerModel3 {
    pub provider: ProviderId,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerGetCompactionSettingsResult {
    pub enabled: bool,
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerGetBranchSummarySettingsResult {
    pub reserve_tokens: u64,
    pub skip_prompt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerGetRetrySettingsResult {
    pub enabled: bool,
    pub max_retries: i64,
    pub base_delay_ms: i64,
    pub max_agent_delay_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsManagerGetProviderRetrySettingsResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<i64>,
    pub max_retry_delay_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsManagerGetDoubleEscapeActionResult {
    #[serde(rename = "fork")]
    Fork,
    #[serde(rename = "tree")]
    Tree,
    #[serde(rename = "none")]
    None,
}

impl SettingsManagerGetDoubleEscapeActionResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fork => "fork",
            Self::Tree => "tree",
            Self::None => "none",
        }
    }
}

pub struct SettingsManager {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SettingsManager {
    pub fn new(
        storage: Arc<dyn SettingsStorage>,
        initial_global: &Settings,
        initial_project: &Settings,
        global_load_error: Option<Arc<anyhow::Error>>,
        project_load_error: Option<Arc<anyhow::Error>>,
        initial_errors: Option<&[SettingsError]>,
        project_trusted: Option<bool>,
        settings_paths: Option<&SettingsPaths>,
    ) -> Self {
        todo!("port core/settings-manager.ts:SettingsManager.constructor")
    }

    /// Create a SettingsManager that loads from files
    pub fn create(
        cwd: &str,
        agent_dir: Option<&str>,
        options: Option<&SettingsManagerCreateOptions>,
    ) -> Arc<SettingsManager> {
        todo!("port core/settings-manager.ts:SettingsManager.create")
    }

    /// Create a SettingsManager from an arbitrary storage backend
    pub fn from_storage(
        storage: Arc<dyn SettingsStorage>,
        options: Option<&SettingsManagerCreateOptions>,
    ) -> Arc<SettingsManager> {
        todo!("port core/settings-manager.ts:SettingsManager.fromStorage")
    }

    /// Create an in-memory SettingsManager (no file I/O)
    pub fn in_memory(
        settings: Option<&Settings>,
        options: Option<&SettingsManagerCreateOptions>,
    ) -> Arc<SettingsManager> {
        todo!("port core/settings-manager.ts:SettingsManager.inMemory")
    }

    /// A copy of the effective settings: global and project settings merged, with overrides.
    pub fn get_settings(&self) -> Settings {
        todo!("port core/settings-manager.ts:SettingsManager.getSettings")
    }

    pub fn get_global_settings(&self) -> Settings {
        todo!("port core/settings-manager.ts:SettingsManager.getGlobalSettings")
    }

    pub fn get_project_settings(&self) -> Settings {
        todo!("port core/settings-manager.ts:SettingsManager.getProjectSettings")
    }

    pub fn is_project_trusted(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.isProjectTrusted")
    }

    pub fn set_project_trusted(&self, trusted: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setProjectTrusted")
    }

    pub async fn reload(&self) -> anyhow::Result<()> {
        todo!("port core/settings-manager.ts:SettingsManager.reload")
    }

    /// Apply additional overrides on top of current settings
    pub fn apply_overrides(&self, overrides: &Settings) {
        todo!("port core/settings-manager.ts:SettingsManager.applyOverrides")
    }

    pub async fn flush(&self) -> anyhow::Result<()> {
        todo!("port core/settings-manager.ts:SettingsManager.flush")
    }

    pub fn drain_errors(&self) -> Vec<SettingsError> {
        todo!("port core/settings-manager.ts:SettingsManager.drainErrors")
    }

    pub fn get_last_changelog_version(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getLastChangelogVersion")
    }

    pub fn set_last_changelog_version(&self, version: &str) {
        todo!("port core/settings-manager.ts:SettingsManager.setLastChangelogVersion")
    }

    pub fn get_session_dir(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getSessionDir")
    }

    pub fn get_default_provider(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getDefaultProvider")
    }

    pub fn get_default_model(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getDefaultModel")
    }

    pub fn set_default_provider(&self, provider: &str) {
        todo!("port core/settings-manager.ts:SettingsManager.setDefaultProvider")
    }

    pub fn set_default_model(&self, model_id: &str) {
        todo!("port core/settings-manager.ts:SettingsManager.setDefaultModel")
    }

    pub fn set_default_model_and_provider(&self, provider: &str, model_id: &str) {
        todo!("port core/settings-manager.ts:SettingsManager.setDefaultModelAndProvider")
    }

    pub fn get_steering_mode(&self) -> SettingsManagerGetSteeringModeResult {
        todo!("port core/settings-manager.ts:SettingsManager.getSteeringMode")
    }

    pub fn set_steering_mode(&self, mode: SettingsManagerGetSteeringModeResult) {
        todo!("port core/settings-manager.ts:SettingsManager.setSteeringMode")
    }

    pub fn get_follow_up_mode(&self) -> SettingsManagerGetSteeringModeResult {
        todo!("port core/settings-manager.ts:SettingsManager.getFollowUpMode")
    }

    pub fn set_follow_up_mode(&self, mode: SettingsManagerGetSteeringModeResult) {
        todo!("port core/settings-manager.ts:SettingsManager.setFollowUpMode")
    }

    pub fn get_theme_setting(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getThemeSetting")
    }

    pub fn get_theme(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getTheme")
    }

    pub fn set_theme(&self, theme: &str) {
        todo!("port core/settings-manager.ts:SettingsManager.setTheme")
    }

    pub fn get_default_thinking_level(&self) -> Option<ThinkingLevel> {
        todo!("port core/settings-manager.ts:SettingsManager.getDefaultThinkingLevel")
    }

    pub fn set_default_thinking_level(&self, level: ThinkingLevel) {
        todo!("port core/settings-manager.ts:SettingsManager.setDefaultThinkingLevel")
    }

    pub fn get_model_thinking_level(
        &self,
        provider: &str,
        model_id: &str,
    ) -> Option<ThinkingLevel> {
        todo!("port core/settings-manager.ts:SettingsManager.getModelThinkingLevel")
    }

    pub fn get_all_model_thinking_levels(&self) -> IndexMap<String, ThinkingLevel> {
        todo!("port core/settings-manager.ts:SettingsManager.getAllModelThinkingLevels")
    }

    pub fn set_model_thinking_level(&self, provider: &str, model_id: &str, level: ThinkingLevel) {
        todo!("port core/settings-manager.ts:SettingsManager.setModelThinkingLevel")
    }

    pub fn remove_model_thinking_level(&self, provider: &str, model_id: &str) {
        todo!("port core/settings-manager.ts:SettingsManager.removeModelThinkingLevel")
    }

    pub fn get_transport(&self) -> TransportSetting {
        todo!("port core/settings-manager.ts:SettingsManager.getTransport")
    }

    pub fn set_transport(&self, transport: &TransportSetting) {
        todo!("port core/settings-manager.ts:SettingsManager.setTransport")
    }

    pub fn get_session_share_destination(&self) -> SessionShareDestination {
        todo!("port core/settings-manager.ts:SettingsManager.getSessionShareDestination")
    }

    pub fn set_session_share_destination(&self, destination: &SessionShareDestination) {
        todo!("port core/settings-manager.ts:SettingsManager.setSessionShareDestination")
    }

    pub fn get_compaction_enabled(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getCompactionEnabled")
    }

    pub fn set_compaction_enabled(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setCompactionEnabled")
    }

    pub fn get_compaction_reserve_tokens(&self, model: Option<&SettingsManagerModel>) -> u64 {
        todo!("port core/settings-manager.ts:SettingsManager.getCompactionReserveTokens")
    }

    pub fn get_compaction_keep_recent_tokens(&self, model: Option<&SettingsManagerModel2>) -> u64 {
        todo!("port core/settings-manager.ts:SettingsManager.getCompactionKeepRecentTokens")
    }

    /// Resolve each token setting through model override, ordinary setting, then built-in default.
    pub fn get_compaction_settings(
        &self,
        model: Option<&SettingsManagerModel3>,
    ) -> SettingsManagerGetCompactionSettingsResult {
        todo!("port core/settings-manager.ts:SettingsManager.getCompactionSettings")
    }

    pub fn get_branch_summary_settings(&self) -> SettingsManagerGetBranchSummarySettingsResult {
        todo!("port core/settings-manager.ts:SettingsManager.getBranchSummarySettings")
    }

    pub fn get_branch_summary_skip_prompt(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getBranchSummarySkipPrompt")
    }

    pub fn get_retry_enabled(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getRetryEnabled")
    }

    pub fn set_retry_enabled(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setRetryEnabled")
    }

    pub fn get_retry_settings(&self) -> SettingsManagerGetRetrySettingsResult {
        todo!("port core/settings-manager.ts:SettingsManager.getRetrySettings")
    }

    pub fn get_http_idle_timeout_ms(&self) -> i64 {
        todo!("port core/settings-manager.ts:SettingsManager.getHttpIdleTimeoutMs")
    }

    pub fn set_http_idle_timeout_ms(&self, timeout_ms: i64) -> anyhow::Result<()> {
        todo!("port core/settings-manager.ts:SettingsManager.setHttpIdleTimeoutMs")
    }

    /// Read from global settings only because warming costs money.
    pub fn get_cache_warming_mode(&self) -> CacheWarmingMode {
        todo!("port core/settings-manager.ts:SettingsManager.getCacheWarmingMode")
    }

    pub fn set_cache_warming_mode(&self, mode: &CacheWarmingMode) {
        todo!("port core/settings-manager.ts:SettingsManager.setCacheWarmingMode")
    }

    pub fn get_provider_retry_settings(&self) -> SettingsManagerGetProviderRetrySettingsResult {
        todo!("port core/settings-manager.ts:SettingsManager.getProviderRetrySettings")
    }

    pub fn get_web_socket_connect_timeout_ms(&self) -> Option<i64> {
        todo!("port core/settings-manager.ts:SettingsManager.getWebSocketConnectTimeoutMs")
    }

    pub fn get_hide_thinking_block(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getHideThinkingBlock")
    }

    pub fn get_show_cache_miss_notices(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getShowCacheMissNotices")
    }

    pub fn get_external_editor_command(&self) -> String {
        todo!("port core/settings-manager.ts:SettingsManager.getExternalEditorCommand")
    }

    pub fn set_hide_thinking_block(&self, hide: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setHideThinkingBlock")
    }

    pub fn set_show_cache_miss_notices(&self, show: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setShowCacheMissNotices")
    }

    pub fn get_shell_path(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getShellPath")
    }

    pub fn set_shell_path(&self, path: Option<&str>) {
        todo!("port core/settings-manager.ts:SettingsManager.setShellPath")
    }

    pub fn get_quiet_startup(&self) -> QuietStartup {
        todo!("port core/settings-manager.ts:SettingsManager.getQuietStartup")
    }

    pub fn set_quiet_startup(&self, quiet: &QuietStartup) {
        todo!("port core/settings-manager.ts:SettingsManager.setQuietStartup")
    }

    pub fn get_default_project_trust(&self) -> DefaultProjectTrust {
        todo!("port core/settings-manager.ts:SettingsManager.getDefaultProjectTrust")
    }

    pub fn set_default_project_trust(&self, default_project_trust: &DefaultProjectTrust) {
        todo!("port core/settings-manager.ts:SettingsManager.setDefaultProjectTrust")
    }

    pub fn get_shell_command_prefix(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getShellCommandPrefix")
    }

    pub fn set_shell_command_prefix(&self, prefix: Option<&str>) {
        todo!("port core/settings-manager.ts:SettingsManager.setShellCommandPrefix")
    }

    pub fn get_npm_command(&self) -> Option<Vec<String>> {
        todo!("port core/settings-manager.ts:SettingsManager.getNpmCommand")
    }

    pub fn set_npm_command(&self, command: Option<&[String]>) {
        todo!("port core/settings-manager.ts:SettingsManager.setNpmCommand")
    }

    pub fn get_collapse_changelog(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getCollapseChangelog")
    }

    pub fn set_collapse_changelog(&self, collapse: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setCollapseChangelog")
    }

    pub fn get_enable_install_telemetry(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getEnableInstallTelemetry")
    }

    pub fn set_enable_install_telemetry(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setEnableInstallTelemetry")
    }

    pub fn get_enable_analytics(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getEnableAnalytics")
    }

    pub fn get_tracking_id(&self) -> Option<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getTrackingId")
    }

    /// Set the analytics opt-in preference; generates a tracking identifier on first opt-in
    pub fn set_enable_analytics(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setEnableAnalytics")
    }

    /// Stable ID of this installation, e.g. sent to OpenAI as its agent host ID.
    /// Created on first use. Project settings are ignored so a committed project
    /// settings file cannot give every clone the same ID.
    pub fn get_or_create_device_id(&self) -> String {
        todo!("port core/settings-manager.ts:SettingsManager.getOrCreateDeviceId")
    }

    pub fn get_packages(&self) -> Vec<PackageSource> {
        todo!("port core/settings-manager.ts:SettingsManager.getPackages")
    }

    pub fn set_packages(&self, packages: &[PackageSource]) {
        todo!("port core/settings-manager.ts:SettingsManager.setPackages")
    }

    pub fn set_project_packages(&self, packages: &[PackageSource]) {
        todo!("port core/settings-manager.ts:SettingsManager.setProjectPackages")
    }

    pub fn get_extension_paths(&self) -> Vec<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getExtensionPaths")
    }

    pub fn set_extension_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setExtensionPaths")
    }

    pub fn set_project_extension_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setProjectExtensionPaths")
    }

    pub fn get_skill_paths(&self) -> Vec<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getSkillPaths")
    }

    pub fn set_skill_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setSkillPaths")
    }

    pub fn set_project_skill_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setProjectSkillPaths")
    }

    pub fn get_prompt_template_paths(&self) -> Vec<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getPromptTemplatePaths")
    }

    pub fn set_prompt_template_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setPromptTemplatePaths")
    }

    pub fn set_project_prompt_template_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setProjectPromptTemplatePaths")
    }

    pub fn get_theme_paths(&self) -> Vec<String> {
        todo!("port core/settings-manager.ts:SettingsManager.getThemePaths")
    }

    pub fn set_theme_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setThemePaths")
    }

    pub fn set_project_theme_paths(&self, paths: &[String]) {
        todo!("port core/settings-manager.ts:SettingsManager.setProjectThemePaths")
    }

    pub fn get_enable_skill_commands(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getEnableSkillCommands")
    }

    pub fn set_enable_skill_commands(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setEnableSkillCommands")
    }

    pub fn get_thinking_budgets(&self) -> Option<ThinkingBudgetsSettings> {
        todo!("port core/settings-manager.ts:SettingsManager.getThinkingBudgets")
    }

    pub fn get_terminal_capability_overrides(&self) -> TerminalCapabilities {
        todo!("port core/settings-manager.ts:SettingsManager.getTerminalCapabilityOverrides")
    }

    pub fn get_show_images(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getShowImages")
    }

    pub fn set_show_images(&self, show: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setShowImages")
    }

    pub fn get_image_width_cells(&self) -> i64 {
        todo!("port core/settings-manager.ts:SettingsManager.getImageWidthCells")
    }

    pub fn set_image_width_cells(&self, width: i64) {
        todo!("port core/settings-manager.ts:SettingsManager.setImageWidthCells")
    }

    pub fn get_clear_on_shrink(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getClearOnShrink")
    }

    pub fn set_clear_on_shrink(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setClearOnShrink")
    }

    pub fn get_show_terminal_progress(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getShowTerminalProgress")
    }

    pub fn set_show_terminal_progress(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setShowTerminalProgress")
    }

    pub fn get_tui_mode(&self) -> TuiMode {
        todo!("port core/settings-manager.ts:SettingsManager.getTuiMode")
    }

    pub fn set_tui_mode(&self, mode: &TuiMode) {
        todo!("port core/settings-manager.ts:SettingsManager.setTuiMode")
    }

    pub fn get_fullscreen_exit_output(&self) -> FullscreenExitOutput {
        todo!("port core/settings-manager.ts:SettingsManager.getFullscreenExitOutput")
    }

    pub fn set_fullscreen_exit_output(&self, output: &FullscreenExitOutput) {
        todo!("port core/settings-manager.ts:SettingsManager.setFullscreenExitOutput")
    }

    pub fn get_fullscreen_scrollbar(&self) -> ScrollViewScrollbar {
        todo!("port core/settings-manager.ts:SettingsManager.getFullscreenScrollbar")
    }

    pub fn set_fullscreen_scrollbar(&self, mode: ScrollViewScrollbar) {
        todo!("port core/settings-manager.ts:SettingsManager.setFullscreenScrollbar")
    }

    pub fn get_fullscreen_copy_on_select(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getFullscreenCopyOnSelect")
    }

    pub fn set_fullscreen_copy_on_select(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setFullscreenCopyOnSelect")
    }

    pub fn get_fullscreen_wheel_scroll_lines(&self) -> WheelScrollLines {
        todo!("port core/settings-manager.ts:SettingsManager.getFullscreenWheelScrollLines")
    }

    pub fn set_fullscreen_wheel_scroll_lines(&self, lines: &WheelScrollLines) {
        todo!("port core/settings-manager.ts:SettingsManager.setFullscreenWheelScrollLines")
    }

    pub fn get_image_auto_resize(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getImageAutoResize")
    }

    pub fn set_image_auto_resize(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setImageAutoResize")
    }

    pub fn get_block_images(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getBlockImages")
    }

    pub fn set_block_images(&self, blocked: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setBlockImages")
    }

    pub fn get_enabled_models(&self) -> Option<Vec<String>> {
        todo!("port core/settings-manager.ts:SettingsManager.getEnabledModels")
    }

    /// The resolved `defaultTools` selection, or undefined when no settings layer sets it.
    pub fn get_default_tools(&self) -> Option<Vec<String>> {
        todo!("port core/settings-manager.ts:SettingsManager.getDefaultTools")
    }

    pub fn set_enabled_models(&self, patterns: Option<&[String]>) {
        todo!("port core/settings-manager.ts:SettingsManager.setEnabledModels")
    }

    pub fn get_double_escape_action(&self) -> SettingsManagerGetDoubleEscapeActionResult {
        todo!("port core/settings-manager.ts:SettingsManager.getDoubleEscapeAction")
    }

    pub fn set_double_escape_action(&self, action: SettingsManagerGetDoubleEscapeActionResult) {
        todo!("port core/settings-manager.ts:SettingsManager.setDoubleEscapeAction")
    }

    pub fn get_tree_filter_mode(&self) -> FilterMode {
        todo!("port core/settings-manager.ts:SettingsManager.getTreeFilterMode")
    }

    pub fn set_tree_filter_mode(&self, mode: FilterMode) {
        todo!("port core/settings-manager.ts:SettingsManager.setTreeFilterMode")
    }

    pub fn get_show_hardware_cursor(&self) -> bool {
        todo!("port core/settings-manager.ts:SettingsManager.getShowHardwareCursor")
    }

    pub fn set_show_hardware_cursor(&self, enabled: bool) {
        todo!("port core/settings-manager.ts:SettingsManager.setShowHardwareCursor")
    }

    pub fn get_editor_padding_x(&self) -> i64 {
        todo!("port core/settings-manager.ts:SettingsManager.getEditorPaddingX")
    }

    pub fn set_editor_padding_x(&self, padding: i64) {
        todo!("port core/settings-manager.ts:SettingsManager.setEditorPaddingX")
    }

    pub fn get_output_pad(&self) -> i64 {
        todo!("port core/settings-manager.ts:SettingsManager.getOutputPad")
    }

    pub fn set_output_pad(&self, padding: i64) {
        todo!("port core/settings-manager.ts:SettingsManager.setOutputPad")
    }

    pub fn get_autocomplete_max_visible(&self) -> i64 {
        todo!("port core/settings-manager.ts:SettingsManager.getAutocompleteMaxVisible")
    }

    pub fn set_autocomplete_max_visible(&self, max_visible: i64) {
        todo!("port core/settings-manager.ts:SettingsManager.setAutocompleteMaxVisible")
    }

    pub fn get_code_block_indent(&self) -> String {
        todo!("port core/settings-manager.ts:SettingsManager.getCodeBlockIndent")
    }

    pub fn get_mermaid_rendering_mode(&self) -> MermaidRenderingMode {
        todo!("port core/settings-manager.ts:SettingsManager.getMermaidRenderingMode")
    }

    pub fn set_mermaid_rendering_mode(&self, mode: &MermaidRenderingMode) {
        todo!("port core/settings-manager.ts:SettingsManager.setMermaidRenderingMode")
    }

    pub fn get_warnings(&self) -> WarningSettings {
        todo!("port core/settings-manager.ts:SettingsManager.getWarnings")
    }

    pub fn set_warnings(&self, warnings: &WarningSettings) {
        todo!("port core/settings-manager.ts:SettingsManager.setWarnings")
    }
}
