//! Port of `packages/coding-agent/src/modes/interactive/theme/theme.ts`.

use crate::core::source_info::SourceInfo;
use crate::modes::interactive::theme::theme_schema::ValidatedThemeJson;
use indexmap::IndexMap;
use pi_agent::types::ThinkingLevel;
use pi_tui::colors::Color;
use pi_tui::colors::TerminalColorMode;
use pi_tui::components::select_list::SelectListTheme;
use pi_tui::components::settings_list::SettingsListTheme;
use pi_tui::terminal_colors::TerminalColors;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

pub use crate::modes::interactive::theme::system_theme::SYSTEM_THEME_NAME;
pub use crate::modes::interactive::theme::theme_schema::ValidatedThemeJson as ThemeJson;
pub use crate::modes::interactive::theme::theme_tokens::ThemeBg;
pub use crate::modes::interactive::theme::theme_tokens::ThemeColor;
pub use crate::modes::interactive::theme::theme_tokens::ThemeToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeStyleFg {
    ThemeColor(ThemeColor),
    IndexedColor(Value /* TS: pi-tui::IndexedColor */),
    RgbColorValue(Value /* TS: pi-tui::RgbColorValue */),
    OklchColorValue(Value /* TS: pi-tui::OklchColorValue */),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeStyleBg {
    ThemeBg(ThemeBg),
    IndexedColor(Value /* TS: pi-tui::IndexedColor */),
    RgbColorValue(Value /* TS: pi-tui::RgbColorValue */),
    OklchColorValue(Value /* TS: pi-tui::OklchColorValue */),
}

/// Tokens are only accepted in their own slot, because "" (terminal default) means the default foreground
/// or background depending on the slot. Use `theme.colors[token]` to use a token's color in the other slot.
// TS: extends TextAttributes (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<ThemeStyleFg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<ThemeStyleBg>,
}

pub type ThemeJsonValidator = Arc<dyn Fn(String, Value) -> ValidatedThemeJson + Send + Sync>;

/// Install full theme validation. Without it, documents are accepted as-is, which is what built-in
/// themes already do: validating user-authored JSON needs typebox, and a presentation that only uses
/// built-in themes should not pay ~17 MB of module graph for it.
pub fn set_theme_json_validator(validator: &ThemeJsonValidator) {
    todo!("port modes/interactive/theme/theme.ts:setThemeJsonValidator")
}

/// The background a theme is designed for.
pub type ThemeAppearance = TerminalTheme;

/// Record the terminal's reported colors. Themes use the default colors for tokens set to "" (terminal
/// default); the system theme is generated from all of them. Ends the pending state.
pub fn set_terminal_colors(colors: Arc<TerminalColors>) {
    todo!("port modes/interactive/theme/theme.ts:setTerminalColors")
}

/// Record the terminal's light/dark report, the fallback for terminals that do not report their background.
pub fn set_terminal_color_scheme(scheme: Option<&TerminalTheme>) {
    todo!("port modes/interactive/theme/theme.ts:setTerminalColorScheme")
}

/// Render the system theme in grayscale until `setTerminalColors()` reports the terminal's colors.
pub fn mark_terminal_colors_pending() {
    todo!("port modes/interactive/theme/theme.ts:markTerminalColorsPending")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeFgColors {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeBgColors {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_info: Option<SourceInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<ThemeAppearance>,
    /// Foreground tokens to render faint (SGR 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<Vec<ThemeColor>>,
}

pub struct Theme {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl Theme {
    pub fn new(
        fg_colors: &ThemeFgColors,
        bg_colors: &ThemeBgColors,
        mode: TerminalColorMode,
        options: Option<&ThemeOptions>,
    ) -> Self {
        todo!("port modes/interactive/theme/theme.ts:Theme.constructor")
    }

    pub fn name(&self) -> Option<String> {
        todo!("port modes/interactive/theme/theme.ts:Theme.name")
    }

    pub fn source_path(&self) -> Option<String> {
        todo!("port modes/interactive/theme/theme.ts:Theme.sourcePath")
    }

    pub fn source_info(&self) -> Option<SourceInfo> {
        todo!("port modes/interactive/theme/theme.ts:Theme.sourceInfo")
    }

    pub fn set_source_info(&self, value: Option<SourceInfo>) {
        todo!("port modes/interactive/theme/theme.ts:Theme.sourceInfo")
    }

    /// The background the theme is designed for: declared in the theme JSON, detected from its colors,
    /// or, for themes without usable colors, the terminal's appearance.
    pub fn appearance(&self) -> ThemeAppearance {
        todo!("port modes/interactive/theme/theme.ts:Theme.appearance")
    }

    /// Concrete colors for all tokens. Tokens set to "" (terminal default) use the terminal's reported
    /// default colors, or a guess based on `appearance` when the terminal did not report them. Faint
    /// tokens are approximated by mixing their color toward the background.
    pub fn colors(&self) -> IndexMap<ThemeToken, Color> {
        todo!("port modes/interactive/theme/theme.ts:Theme.colors")
    }

    pub fn style(&self, text: &str, options: &ThemeStyle) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.style")
    }

    pub fn fg(&self, color: &ThemeColor, text: &str) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.fg")
    }

    pub fn bg(&self, color: &ThemeBg, text: &str) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.bg")
    }

    pub fn bold(&self, text: &str) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.bold")
    }

    pub fn italic(&self, text: &str) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.italic")
    }

    pub fn underline(&self, text: &str) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.underline")
    }

    pub fn inverse(&self, text: &str) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.inverse")
    }

    pub fn strikethrough(&self, text: &str) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.strikethrough")
    }

    /// Opening escape sequence for a foreground token. Faint tokens include SGR 2, which `\x1b[22m` closes.
    pub fn get_fg_ansi(&self, color: &ThemeColor) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.getFgAnsi")
    }

    pub fn get_bg_ansi(&self, color: &ThemeBg) -> String {
        todo!("port modes/interactive/theme/theme.ts:Theme.getBgAnsi")
    }

    pub fn get_color_mode(&self) -> TerminalColorMode {
        todo!("port modes/interactive/theme/theme.ts:Theme.getColorMode")
    }

    pub fn get_thinking_border_color(
        &self,
        level: ThinkingLevel,
    ) -> Arc<dyn Fn(String) -> String + Send + Sync> {
        todo!("port modes/interactive/theme/theme.ts:Theme.getThinkingBorderColor")
    }

    pub fn get_bash_mode_border_color(&self) -> Arc<dyn Fn(String) -> String + Send + Sync> {
        todo!("port modes/interactive/theme/theme.ts:Theme.getBashModeBorderColor")
    }
}

pub fn get_available_themes() -> Vec<String> {
    todo!("port modes/interactive/theme/theme.ts:getAvailableThemes")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub fn get_available_themes_with_paths() -> Vec<ThemeInfo> {
    todo!("port modes/interactive/theme/theme.ts:getAvailableThemesWithPaths")
}

pub fn load_theme_from_path(theme_path: &str, mode: Option<TerminalColorMode>) -> Arc<Theme> {
    todo!("port modes/interactive/theme/theme.ts:loadThemeFromPath")
}

pub fn get_theme_by_name(name: &str) -> Option<Arc<Theme>> {
    todo!("port modes/interactive/theme/theme.ts:getThemeByName")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalTheme {
    #[serde(rename = "dark")]
    Dark,
    #[serde(rename = "light")]
    Light,
}

impl TerminalTheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseAutoThemeSettingParseAutoThemeSettingResult {
    pub light_theme: String,
    pub dark_theme: String,
}

pub fn parse_auto_theme_setting(
    theme_setting: Option<&str>,
) -> Option<ParseAutoThemeSettingParseAutoThemeSettingResult> {
    todo!("port modes/interactive/theme/theme.ts:parseAutoThemeSetting")
}

pub fn resolve_theme_setting(
    theme_setting: Option<&str>,
    terminal_theme: &TerminalTheme,
) -> Option<String> {
    todo!("port modes/interactive/theme/theme.ts:resolveThemeSetting")
}

/// Dark or light from the `COLORFGBG` environment variable some terminals set, or undefined without a
/// usable background index. The value is `fg;bg` or `fg;xpm;bg` (rxvt), where a field is an ANSI color
/// index or `default` when the color is not in the palette. The index refers to the terminal's own palette,
/// whose colors are unknown here, so it is classified by index like Vim does: 0-6 and 8 (bright black, e.g.
/// Solarized Dark's background) are dark, 7 and 9-15 are light.
pub fn detect_color_fg_bg_theme(env: Option<Value>) -> Option<TerminalTheme> {
    todo!("port modes/interactive/theme/theme.ts:detectColorFgBgTheme")
}

/// Whether the terminal is dark or light. The background it renders decides, classified the same way the
/// system theme does. Without a reported background: the terminal's light/dark report, then COLORFGBG,
/// then dark.
pub fn detect_terminal_theme(
    colors: Option<Arc<TerminalColors>>,
    reported_scheme: Option<&TerminalTheme>,
    env: Option<Value>,
) -> TerminalTheme {
    todo!("port modes/interactive/theme/theme.ts:detectTerminalTheme")
}

/// Whether the terminal is dark or light, from everything it reported so far. See `detectTerminalTheme()`.
pub fn get_terminal_theme() -> TerminalTheme {
    todo!("port modes/interactive/theme/theme.ts:getTerminalTheme")
}

pub static THEME: LazyLock<Arc<Theme>> =
    LazyLock::new(|| todo!("port modes/interactive/theme/theme.ts:theme"));

pub fn set_registered_themes(themes: Vec<Arc<Theme>>) {
    todo!("port modes/interactive/theme/theme.ts:setRegisteredThemes")
}

pub fn init_theme(theme_name: Option<&str>, enable_watcher: Option<bool>) {
    todo!("port modes/interactive/theme/theme.ts:initTheme")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetThemeSetThemeResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn set_theme(name: &str, enable_watcher: Option<bool>) -> SetThemeSetThemeResult {
    todo!("port modes/interactive/theme/theme.ts:setTheme")
}

pub fn set_theme_instance(theme_instance: Arc<Theme>) {
    todo!("port modes/interactive/theme/theme.ts:setThemeInstance")
}

pub fn on_theme_change(callback: Arc<dyn Fn() + Send + Sync>) {
    todo!("port modes/interactive/theme/theme.ts:onThemeChange")
}

pub fn stop_theme_watcher() {
    todo!("port modes/interactive/theme/theme.ts:stopThemeWatcher")
}

/// Get resolved theme colors as CSS-compatible hex strings.
/// Used by HTML export to generate CSS custom properties.
pub fn get_resolved_theme_colors(theme_name: Option<&str>) -> IndexMap<String, String> {
    todo!("port modes/interactive/theme/theme.ts:getResolvedThemeColors")
}

/// Check if a theme is a "light" theme (for CSS that needs light/dark variants).
pub fn is_light_theme(theme_name: Option<&str>) -> bool {
    todo!("port modes/interactive/theme/theme.ts:isLightTheme")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetThemeExportColorsGetThemeExportColorsResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_bg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_bg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_bg: Option<String>,
}

/// Get explicit export colors from theme JSON, if specified.
/// Returns undefined for each color that isn't explicitly set.
pub fn get_theme_export_colors(
    theme_name: Option<&str>,
) -> GetThemeExportColorsGetThemeExportColorsResult {
    todo!("port modes/interactive/theme/theme.ts:getThemeExportColors")
}

pub type CliHighlightTheme = IndexMap<String, Arc<dyn Fn(String) -> String + Send + Sync>>;

/// Highlight code with syntax coloring based on file extension or language.
/// Returns array of highlighted lines.
pub fn highlight_code(code: &str, lang: Option<&str>) -> Vec<String> {
    todo!("port modes/interactive/theme/theme.ts:highlightCode")
}

/// Get language identifier from file path extension.
pub fn get_language_from_path(file_path: &str) -> Option<String> {
    todo!("port modes/interactive/theme/theme.ts:getLanguageFromPath")
}

pub fn get_markdown_theme() -> Value /* TS: pi-tui::MarkdownTheme */ {
    todo!("port modes/interactive/theme/theme.ts:getMarkdownTheme")
}

pub fn get_select_list_theme() -> Arc<SelectListTheme> {
    todo!("port modes/interactive/theme/theme.ts:getSelectListTheme")
}

pub fn get_editor_theme() -> Value /* TS: pi-tui::EditorTheme */ {
    todo!("port modes/interactive/theme/theme.ts:getEditorTheme")
}

pub fn get_settings_list_theme() -> Arc<SettingsListTheme> {
    todo!("port modes/interactive/theme/theme.ts:getSettingsListTheme")
}
