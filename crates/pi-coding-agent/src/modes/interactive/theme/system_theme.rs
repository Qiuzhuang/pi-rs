//! Port of `packages/coding-agent/src/modes/interactive/theme/system-theme.ts`.

use crate::modes::interactive::theme::theme::ThemeAppearance;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use crate::modes::interactive::theme::theme_tokens::ThemeToken;
use indexmap::IndexMap;
use pi_tui::terminal_colors::RgbColor;
use serde::{Deserialize, Serialize};

pub const SYSTEM_THEME_NAME: &str = "system";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilySaturation {
    pub min: i64,
    pub max: i64,
}

/// A family's OKHSL hue and saturation: `max` at mid lightness, falling toward `min` at black and white.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Family {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub hue: f64,
    pub saturation: FamilySaturation,
    /// ANSI palette slot the family takes its hue and saturation from.
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub slot: f64,
}

pub type FamilyName = String;

/// Target-lightness curves: a polynomial in the surface's OKLab lightness giving the OKLab lightness a token
/// needs on it. `reachable` is the range of surface lightness where the level can be reached; beyond it the
/// level is relaxed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Curve {
    pub coefficients: Vec<f64>,
    pub reachable: (f64, f64),
}

pub type Level = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Surface {
    #[serde(rename = "selectedBg")]
    SelectedBg,
    #[serde(rename = "searchMatchBg")]
    SearchMatchBg,
    #[serde(rename = "userMessageBg")]
    UserMessageBg,
    #[serde(rename = "customMessageBg")]
    CustomMessageBg,
    #[serde(rename = "toolPendingBg")]
    ToolPendingBg,
    #[serde(rename = "toolSuccessBg")]
    ToolSuccessBg,
    #[serde(rename = "toolErrorBg")]
    ToolErrorBg,
    #[serde(rename = "background")]
    Background,
    #[serde(rename = "scrollbarTrack")]
    ScrollbarTrack,
}

impl Surface {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SelectedBg => "selectedBg",
            Self::SearchMatchBg => "searchMatchBg",
            Self::UserMessageBg => "userMessageBg",
            Self::CustomMessageBg => "customMessageBg",
            Self::ToolPendingBg => "toolPendingBg",
            Self::ToolSuccessBg => "toolSuccessBg",
            Self::ToolErrorBg => "toolErrorBg",
            Self::Background => "background",
            Self::ScrollbarTrack => "scrollbarTrack",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub token: ThemeToken,
    pub on: Vec<Surface>,
    pub level: Level,
}

#[derive(Debug, Clone)]
pub struct SystemThemeInput {
    pub foreground: Option<RgbColor>,
    pub background: Option<RgbColor>,
    /// ANSI colors 0-15.
    pub palette: Option<Vec<RgbColor>>,
    /// Saturation multiplier from 0 (grayscale) to 1. The first frame renders in grayscale until colors arrive.
    pub saturation: Option<f64>,
    /// Appearance when the terminal did not report its background, e.g. from its light/dark report or COLORFGBG.
    pub appearance_hint: Option<ThemeAppearance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemThemeColorsColors {
    String(String),
    Number(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemThemeColors {
    /// Hex colors, ANSI palette indices, or "" for the terminal default.
    pub colors: IndexMap<ThemeToken, SystemThemeColorsColors>,
    /// Foreground tokens rendered faint (SGR 2), for terminals that did not report colors.
    pub dim: Vec<ThemeColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<ThemeAppearance>,
}

/// WCAG 2 relative luminance.
pub fn relative_luminance(options: RgbColor) -> i64 {
    todo!("port modes/interactive/theme/system-theme.ts:relativeLuminance")
}

/// WCAG 2 contrast ratio, 1-21.
pub fn wcag_contrast(first: RgbColor, second: RgbColor) -> f64 {
    todo!("port modes/interactive/theme/system-theme.ts:wcagContrast")
}

/// Whether a terminal is dark or light, from its reported colors: the direction of its own foreground
/// when text can be readable that way, otherwise dark when white text has more contrast on the
/// background than black text.
pub fn terminal_appearance(background: RgbColor, foreground: Option<RgbColor>) -> ThemeAppearance {
    todo!("port modes/interactive/theme/system-theme.ts:terminalAppearance")
}

/// Generate the system theme's colors from the terminal's reported colors.
pub fn generate_system_theme_colors(input: &SystemThemeInput) -> SystemThemeColors {
    todo!("port modes/interactive/theme/system-theme.ts:generateSystemThemeColors")
}

/// A terminal color's OKHSL channels and its OKLCH chroma.
// TS: extends OkhslChannels (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceColor {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub h: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub s: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub l: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub chroma: f64,
}
