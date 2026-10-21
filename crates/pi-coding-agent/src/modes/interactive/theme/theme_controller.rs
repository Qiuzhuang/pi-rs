//! Port of `packages/coding-agent/src/modes/interactive/theme/theme-controller.ts`.

use crate::core::settings_manager::SettingsManager;
use crate::modes::interactive::theme::theme::TerminalTheme;
use crate::modes::interactive::theme::theme::Theme;
use pi_tui::terminal_colors::TerminalColors;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Query the terminal's colors and pass them to `apply` when the query completes or times out, and again
/// if the terminal answers after the timeout. A failed query applies no colors. Settles after the first apply.
pub async fn request_terminal_colors(
    ui: Arc<Tui>,
    apply: Arc<dyn Fn(Arc<TerminalColors>) + Send + Sync>,
) -> anyhow::Result<()> {
    todo!("port modes/interactive/theme/theme-controller.ts:requestTerminalColors")
}

#[derive(Clone)]
pub struct InteractiveThemeControllerOptions {
    pub get_settings_manager: Arc<dyn Fn() -> Arc<SettingsManager> + Send + Sync>,
    pub show_error: Arc<dyn Fn(String) + Send + Sync>,
    pub on_changed: Arc<dyn Fn() + Send + Sync>,
    pub initial_theme_setting: Option<String>,
}

/// Applies the theme setting and keeps it in sync with the terminal. The theme applies immediately, and the
/// terminal's colors update it when they arrive; the system theme renders in grayscale until then. Callers
/// that bake theme colors into content can wait for the colors with `waitForTerminalColors()`.
pub struct InteractiveThemeController {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl InteractiveThemeController {
    pub fn new(ui: Arc<Tui>, options: &InteractiveThemeControllerOptions) -> Self {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.constructor"
        )
    }

    pub fn rebind_tui(&self) {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.rebindTui"
        )
    }

    /// Apply the theme setting now and query the terminal's colors, which update the theme when they arrive.
    /// Theme pairs and the system theme follow terminal appearance changes.
    pub fn apply_from_settings(&self) {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.applyFromSettings"
        )
    }

    /// Wait until the latest color query completed or timed out. Content that bakes theme colors into
    /// strings, such as the startup header, should be built after this. Terminals answer the DA1 request
    /// right after the color replies, so this only takes the full timeout when a terminal answers nothing.
    pub async fn wait_for_terminal_colors(&self) -> anyhow::Result<()> {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.waitForTerminalColors"
        )
    }

    pub fn get_theme_selection(&self) -> Option<String> {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.getThemeSelection"
        )
    }

    pub fn set_theme_name(&self, theme_name: &str, show_error: Option<bool>) -> ThemeResult {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.setThemeName"
        )
    }

    pub fn set_theme_setting(&self, theme_setting: &str) {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.setThemeSetting"
        )
    }

    pub fn set_theme_instance(&self, theme_instance: Arc<Theme>) -> ThemeResult {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.setThemeInstance"
        )
    }

    pub fn preview(&self, theme_setting_or_name: &str) {
        todo!("port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.preview")
    }

    pub fn disable_auto_sync(&self) {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.disableAutoSync"
        )
    }

    pub fn dispose(&self) {
        todo!("port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.dispose")
    }

    pub fn get_terminal_theme(&self) -> TerminalTheme {
        todo!(
            "port modes/interactive/theme/theme-controller.ts:InteractiveThemeController.getTerminalTheme"
        )
    }
}
