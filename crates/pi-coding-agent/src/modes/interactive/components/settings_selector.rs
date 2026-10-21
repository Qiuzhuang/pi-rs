//! Port of `packages/coding-agent/src/modes/interactive/components/settings-selector.ts`.

use crate::compat::WheelScrollLines;
use crate::core::settings_schema::CacheWarmingMode;
use crate::core::settings_schema::DefaultProjectTrust;
use crate::core::settings_schema::FullscreenExitOutput;
use crate::core::settings_schema::MermaidRenderingMode;
use crate::core::settings_schema::QuietStartup;
use crate::core::settings_schema::SessionShareDestination;
use crate::core::settings_schema::TuiMode;
use crate::core::settings_schema::WarningSettings;
use crate::modes::interactive::components::tree_selector::FilterMode;
use crate::modes::interactive::theme::theme::TerminalTheme;
use indexmap::IndexMap;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::Model;
use pi_ai::types::Transport;
use pi_tui::components::scroll_view::ScrollViewScrollbar;
use pi_tui::components::settings_list::SettingsList;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsConfigSteeringMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

impl SettingsConfigSteeringMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one-at-a-time",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsConfigDoubleEscapeAction {
    #[serde(rename = "fork")]
    Fork,
    #[serde(rename = "tree")]
    Tree,
    #[serde(rename = "none")]
    None,
}

impl SettingsConfigDoubleEscapeAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fork => "fork",
            Self::Tree => "tree",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsConfig {
    pub auto_compact: bool,
    pub default_model: String,
    pub current_model: Option<Model>,
    pub available_default_models: Vec<Model>,
    pub show_images: bool,
    pub image_width_cells: i64,
    pub auto_resize_images: bool,
    pub block_images: bool,
    pub enable_skill_commands: bool,
    pub steering_mode: SettingsConfigSteeringMode,
    pub follow_up_mode: SettingsConfigSteeringMode,
    pub transport: Transport,
    pub session_share_destination: SessionShareDestination,
    pub http_idle_timeout_ms: i64,
    pub cache_warming_mode: CacheWarmingMode,
    pub thinking_level: ThinkingLevel,
    pub available_thinking_levels: Vec<ThinkingLevel>,
    pub model_thinking_levels: IndexMap<String, ThinkingLevel>,
    pub current_theme: String,
    pub terminal_theme: TerminalTheme,
    pub available_themes: Vec<String>,
    pub hide_thinking_block: bool,
    pub mermaid_rendering_mode: MermaidRenderingMode,
    pub show_cache_miss_notices: bool,
    pub collapse_changelog: bool,
    pub enable_install_telemetry: bool,
    pub double_escape_action: SettingsConfigDoubleEscapeAction,
    pub tree_filter_mode: FilterMode,
    pub show_hardware_cursor: bool,
    pub editor_padding_x: i64,
    pub output_pad: i64,
    pub autocomplete_max_visible: i64,
    pub quiet_startup: QuietStartup,
    pub default_project_trust: DefaultProjectTrust,
    pub clear_on_shrink: bool,
    pub show_terminal_progress: bool,
    pub tui_mode: TuiMode,
    pub fullscreen_exit_output: FullscreenExitOutput,
    pub fullscreen_scrollbar: ScrollViewScrollbar,
    pub fullscreen_copy_on_select: bool,
    pub fullscreen_wheel_scroll_lines: WheelScrollLines,
    pub warnings: WarningSettings,
}

#[derive(Clone)]
pub struct SettingsCallbacks {
    pub on_auto_compact_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_show_images_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_image_width_cells_change: Arc<dyn Fn(i64) + Send + Sync>,
    pub on_auto_resize_images_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_block_images_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_enable_skill_commands_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_steering_mode_change: Arc<dyn Fn(SettingsConfigSteeringMode) + Send + Sync>,
    pub on_follow_up_mode_change: Arc<dyn Fn(SettingsConfigSteeringMode) + Send + Sync>,
    pub on_transport_change: Arc<dyn Fn(Transport) + Send + Sync>,
    pub on_session_share_destination_change: Arc<dyn Fn(SessionShareDestination) + Send + Sync>,
    pub on_http_idle_timeout_ms_change: Arc<dyn Fn(i64) + Send + Sync>,
    pub on_cache_warming_mode_change: Arc<dyn Fn(CacheWarmingMode) + Send + Sync>,
    pub on_model_thinking_level_change: Arc<dyn Fn(String, String, ThinkingLevel) + Send + Sync>,
    pub on_model_thinking_level_remove: Arc<dyn Fn(String, String) + Send + Sync>,
    pub on_theme_change: Arc<dyn Fn(String) + Send + Sync>,
    pub on_theme_preview: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub on_hide_thinking_block_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_mermaid_rendering_mode_change: Arc<dyn Fn(MermaidRenderingMode) + Send + Sync>,
    pub on_show_cache_miss_notices_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_collapse_changelog_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_enable_install_telemetry_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_double_escape_action_change: Arc<dyn Fn(SettingsConfigDoubleEscapeAction) + Send + Sync>,
    pub on_tree_filter_mode_change: Arc<dyn Fn(FilterMode) + Send + Sync>,
    pub on_show_hardware_cursor_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_editor_padding_x_change: Arc<dyn Fn(i64) + Send + Sync>,
    pub on_output_pad_change: Arc<dyn Fn(i64) + Send + Sync>,
    pub on_autocomplete_max_visible_change: Arc<dyn Fn(i64) + Send + Sync>,
    pub on_quiet_startup_change: Arc<dyn Fn(QuietStartup) + Send + Sync>,
    pub on_default_project_trust_change: Arc<dyn Fn(DefaultProjectTrust) + Send + Sync>,
    pub on_clear_on_shrink_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_show_terminal_progress_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_tui_mode_change: Arc<dyn Fn(TuiMode) + Send + Sync>,
    pub on_fullscreen_exit_output_change: Arc<dyn Fn(FullscreenExitOutput) + Send + Sync>,
    pub on_fullscreen_scrollbar_change: Arc<dyn Fn(ScrollViewScrollbar) + Send + Sync>,
    pub on_fullscreen_copy_on_select_change: Arc<dyn Fn(bool) + Send + Sync>,
    pub on_fullscreen_wheel_scroll_lines_change: Arc<dyn Fn(WheelScrollLines) + Send + Sync>,
    pub on_warnings_change: Arc<dyn Fn(WarningSettings) + Send + Sync>,
    pub on_cancel: Arc<dyn Fn() + Send + Sync>,
}

/// A submenu component for selecting from a list of options.
/// TS: `extends Container`
pub struct WarningSettingsSubmenu {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl WarningSettingsSubmenu {
    pub fn new(
        warnings: &WarningSettings,
        on_change: Arc<dyn Fn(WarningSettings) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/settings-selector.ts:WarningSettingsSubmenu.constructor"
        )
    }
}

// TODO(port): impl pi-tui::Container for WarningSettingsSubmenu

impl Component for WarningSettingsSubmenu {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/settings-selector.ts:WarningSettingsSubmenu.render"
        )
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/settings-selector.ts:WarningSettingsSubmenu.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}

/// TS: `extends Container`
pub struct ThemeSubmenu {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ThemeSubmenu {
    pub fn new(
        current_theme_setting: &str,
        terminal_theme: &TerminalTheme,
        available_themes: &[String],
        callbacks: &SettingsCallbacks,
        on_done: Arc<dyn Fn(Option<String>) + Send + Sync>,
    ) -> Self {
        todo!("port modes/interactive/components/settings-selector.ts:ThemeSubmenu.constructor")
    }
}

// TODO(port): impl pi-tui::Container for ThemeSubmenu

impl Component for ThemeSubmenu {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/settings-selector.ts:ThemeSubmenu.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/settings-selector.ts:ThemeSubmenu.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }
}

/// Main settings selector component.
/// TS: `extends Container`
pub struct SettingsSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl SettingsSelectorComponent {
    pub fn new(config: &SettingsConfig, callbacks: &SettingsCallbacks) -> Self {
        todo!(
            "port modes/interactive/components/settings-selector.ts:SettingsSelectorComponent.constructor"
        )
    }

    pub fn get_settings_list(&self) -> Arc<SettingsList> {
        todo!(
            "port modes/interactive/components/settings-selector.ts:SettingsSelectorComponent.getSettingsList"
        )
    }
}

// TODO(port): impl pi-tui::Container for SettingsSelectorComponent

impl Component for SettingsSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/settings-selector.ts:SettingsSelectorComponent.render"
        )
    }
}
