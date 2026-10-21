//! Port of `packages/coding-agent/src/modes/interactive/theme/system-theme.ts`.
//!
//! The `system` theme: pi's colors derived from the terminal's own theme.
//!
//! Every token belongs to a color family (its hue) and has contrast rules: it must reach a contrast level
//! on the background and on the panels it is drawn on. Hue and saturation come from the terminal's palette
//! color for the family's ANSI slot, or from the family's own hue when the terminal reports no palette.
//! Lightness comes from the rules alone. Colors are built in OKHSL, whose saturation is relative to the
//! sRGB gamut, and fade toward gray near black and white. A palette color never gains OKLCH chroma when it
//! moves to another lightness, so pastel palettes stay pastel.
//!
//! A contrast level is a target-lightness curve: the OKLab lightness a token needs, given the lightness of
//! the surface below it. On dark backgrounds the curves aim for nearly fixed lightness; on light
//! backgrounds the required difference grows as the background darkens.
//!
//! Depending on what the terminal reports, the theme is generated in one of three tiers:
//! - background and palette: hues from the palette, lightness from the background;
//! - background only: the families' own hues, lightness from the background;
//! - nothing: ANSI palette indices and the default colors, which the terminal renders itself.

use crate::modes::interactive::theme::theme::ThemeAppearance;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use crate::modes::interactive::theme::theme_tokens::ThemeToken;
use indexmap::IndexMap;
use pi_tui::colors::{
    OkhslChannels, color_to_okhsl, color_to_oklch, color_to_rgb, okhsl_color, oklch_color, rgb_color,
};
use pi_tui::oklab::oklab_to_okhsl_lightness;
use pi_tui::terminal_colors::RgbColor;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

pub const SYSTEM_THEME_NAME: &str = "system";

// ============================================================================
// Recipe: color families and their tokens
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilySaturation {
    pub min: f64,
    pub max: f64,
}

/// A family's OKHSL hue and saturation: `max` at mid lightness, falling toward `min` at black and white.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

const fn family(hue: f64, min: f64, max: f64, slot: f64) -> Family {
    Family { hue, saturation: FamilySaturation { min, max }, slot }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fam {
    Neutral,
    Blue,
    Green,
    Red,
    Yellow,
    Orange,
    Violet,
    Calamine,
    ThinkingSlate,
    ThinkingBlue,
    ThinkingPeriwinkle,
    ThinkingViolet,
    ThinkingMagenta,
    ThinkingRed,
}

impl Fam {
    fn family(self) -> Family {
        match self {
            Fam::Neutral => family(231.49, 0.02, 0.08, 8.0),
            Fam::Blue => family(231.49, 0.1, 0.68, 4.0),
            Fam::Green => family(158.68, 0.1, 0.76, 2.0),
            Fam::Red => family(20.0, 0.1, 0.92, 1.0),
            Fam::Yellow => family(82.36, 0.5, 1.0, 3.0),
            Fam::Orange => family(52.0, 0.12, 0.85, 3.0),
            Fam::Violet => family(295.0, 0.2, 0.6, 5.0),
            Fam::Calamine => family(202.43, 0.1, 0.74, 6.0),
            Fam::ThinkingSlate => family(231.49, 0.08, 0.2, 4.0),
            Fam::ThinkingBlue => family(231.49, 0.2, 0.45, 4.0),
            Fam::ThinkingPeriwinkle => family(263.25, 0.3, 0.6, 6.0),
            Fam::ThinkingViolet => family(295.0, 0.4, 0.75, 5.0),
            Fam::ThinkingMagenta => family(337.5, 0.5, 0.85, 13.0),
            Fam::ThinkingRed => family(20.0, 0.95, 1.0, 1.0),
        }
    }
}

/// `TOKEN_FAMILIES`, in the TS declaration order (which is the order of the generated colors).
static TOKEN_FAMILIES: &[(ThemeToken, Fam)] = {
    use Fam::*;
    use ThemeToken as T;
    &[
        (T::SelectedBg, Blue),
        (T::SearchMatchBg, Orange),
        (T::UserMessageBg, Blue),
        (T::CustomMessageBg, Violet),
        (T::ToolPendingBg, Neutral),
        (T::ToolSuccessBg, Green),
        (T::ToolErrorBg, Red),
        (T::Text, Neutral),
        (T::UserMessageText, Neutral),
        (T::CustomMessageText, Neutral),
        (T::ToolTitle, Neutral),
        (T::SyntaxOperator, Neutral),
        (T::SyntaxPunctuation, Neutral),
        (T::Muted, Neutral),
        (T::Dim, Neutral),
        (T::ThinkingText, Neutral),
        (T::ToolOutput, Neutral),
        (T::MdLinkUrl, Neutral),
        (T::MdQuote, Neutral),
        (T::MdQuoteBorder, Neutral),
        (T::MdHr, Neutral),
        (T::MdCodeBlockBorder, Neutral),
        (T::ToolDiffContext, Neutral),
        (T::SyntaxComment, Neutral),
        (T::ScrollbarTrack, Neutral),
        (T::ScrollbarThumb, Neutral),
        (T::SearchMatchText, Neutral),
        (T::BorderMuted, Neutral),
        (T::Accent, Violet),
        (T::BorderAccent, Violet),
        (T::CustomMessageLabel, Violet),
        (T::MdCode, Violet),
        (T::MdListBullet, Violet),
        (T::SyntaxType, Violet),
        (T::Border, Blue),
        (T::MdLink, Blue),
        (T::SyntaxKeyword, Blue),
        (T::SyntaxVariable, Calamine),
        (T::Success, Green),
        (T::MdCodeBlock, Green),
        (T::ToolDiffAdded, Green),
        (T::BashMode, Green),
        (T::SyntaxNumber, Green),
        (T::Error, Red),
        (T::ToolDiffRemoved, Red),
        (T::Warning, Yellow),
        (T::MdHeading, Yellow),
        (T::SyntaxFunction, Yellow),
        (T::SyntaxString, Orange),
        (T::ThinkingOff, Neutral),
        (T::ThinkingMinimal, ThinkingSlate),
        (T::ThinkingLow, ThinkingBlue),
        (T::ThinkingMedium, ThinkingPeriwinkle),
        (T::ThinkingHigh, ThinkingViolet),
        (T::ThinkingXhigh, ThinkingMagenta),
        (T::ThinkingMax, ThinkingRed),
    ]
};

fn token_family(token: ThemeToken) -> Fam {
    TOKEN_FAMILIES
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, f)| *f)
        .unwrap_or(Fam::Neutral)
}

/// Palette slots for tokens that would otherwise share a hue with a similar token.
fn token_slot(token: ThemeToken) -> Option<usize> {
    match token {
        ThemeToken::SyntaxString => Some(2),
        ThemeToken::SyntaxNumber => Some(5),
        ThemeToken::SearchMatchBg => Some(3),
        _ => None,
    }
}

// ============================================================================
// Contrast levels and rules
// ============================================================================

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Lvl {
    Panel,
    Track,
    Thinking0,
    Thinking1,
    Thinking2,
    Thinking3,
    Thinking4,
    Thinking5,
    Thinking6,
    Subtle,
    Thumb,
    Readable,
    Emphasis,
    TextOnPanel,
    Text,
}

struct StaticCurve {
    coefficients: [f64; 6],
    reachable: (f64, f64),
}

const fn curve(coefficients: [f64; 6], low: f64, high: f64) -> StaticCurve {
    StaticCurve { coefficients, reachable: (low, high) }
}

impl Lvl {
    fn curve(self, appearance: ThemeAppearance) -> StaticCurve {
        let dark = appearance == ThemeAppearance::Dark;
        match self {
            Lvl::Panel => {
                if dark {
                    curve([0.29131, -0.39746, 2.33185, -0.85524, -1.2076, 0.86276], 0.0, 0.979)
                } else {
                    curve([-3.74073, 27.94549, -78.44258, 112.6798, -79.60015, 22.11277], 0.348, 1.0)
                }
            }
            Lvl::Track => {
                if dark {
                    curve([0.39028, -0.23015, 0.83573, 2.43829, -4.38292, 2.01582], 0.0, 0.946)
                } else {
                    curve([-5.24921, 38.37322, -107.28833, 152.10005, -106.17127, 29.18061], 0.368, 1.0)
                }
            }
            Lvl::Thinking0 => {
                if dark {
                    curve([0.52988, -0.05809, -0.30924, 4.63567, -6.52933, 2.89108], 0.0, 0.873)
                } else {
                    curve([-28.27749, 182.85284, -469.62416, 603.15916, -384.59976, 97.35147], 0.51, 1.0)
                }
            }
            Lvl::Thinking1 => {
                if dark {
                    curve([0.55278, -0.03667, -0.45659, 4.95347, -6.90265, 3.0706], 0.0, 0.858)
                } else {
                    curve([-37.10484, 235.86282, -596.62344, 754.3633, -474.00763, 118.3551], 0.535, 1.0)
                }
            }
            Lvl::Thinking2 => {
                if dark {
                    curve([0.57486, -0.01765, -0.58987, 5.25227, -7.27175, 3.25532], 0.0, 0.842)
                } else {
                    curve([-59.89653, 377.05024, -945.07843, 1182.03145, -734.96375, 181.68658], 0.556, 1.0)
                }
            }
            Lvl::Thinking3 => {
                if dark {
                    curve([0.59621, -0.00062, -0.71148, 5.53588, -7.6392, 3.44606], 0.0, 0.827)
                } else {
                    curve([-72.07122, 445.84082, -1099.57352, 1353.88793, -829.53392, 202.26164], 0.58, 1.0)
                }
            }
            Lvl::Thinking4 => {
                if dark {
                    curve([0.61691, 0.01462, -0.82288, 5.80651, -8.00641, 3.64333], 0.0, 0.811)
                } else {
                    curve([-110.14338, 674.21488, -1645.75941, 2004.32367, -1215.15899, 293.3183], 0.6, 1.0)
                }
            }
            Lvl::Thinking5 => {
                if dark {
                    curve([0.63702, 0.02826, -0.92498, 6.06465, -8.37246, 3.84651], 0.0, 0.795)
                } else {
                    curve([-175.47701, 1063.54495, -2570.70594, 3098.80776, -1860.15527, 444.76392], 0.62, 1.0)
                }
            }
            Lvl::Thinking6 => {
                if dark {
                    curve([0.65658, 0.04044, -1.01835, 6.30989, -8.73529, 4.05439], 0.0, 0.779)
                } else {
                    curve([-183.81712, 1094.70055, -2602.68539, 3088.71276, -1826.91131, 430.75931], 0.643, 1.0)
                }
            }
            Lvl::Subtle => {
                if dark {
                    curve([0.56762, -0.02475, -0.5383, 5.12628, -7.10931, 3.17324], 0.0, 0.848)
                } else {
                    curve([-232.85459, 1376.54473, -3249.11801, 3827.91186, -2248.29472, 526.55751], 0.657, 1.0)
                }
            }
            Lvl::Thumb => {
                if dark {
                    curve([0.60323, 0.00278, -0.73328, 5.57157, -7.68067, 3.46933], 0.0, 0.823)
                } else {
                    curve([-82.89897, 511.01355, -1255.98095, 1540.76821, -940.68087, 228.58523], 0.586, 1.0)
                }
            }
            Lvl::Readable => {
                if dark {
                    curve([0.66937, 0.04704, -1.06871, 6.43941, -8.9332, 4.17229], 0.0, 0.77)
                } else {
                    curve(
                        [-1554.52576, 8733.56817, -19604.93507, 21977.72696, -12300.99599, 2749.81288],
                        0.751,
                        1.0,
                    )
                }
            }
            Lvl::Emphasis => {
                if dark {
                    curve([0.7303, 0.07695, -1.31626, 7.1681, -10.14436, 4.92846], 0.0, 0.712)
                } else {
                    curve(
                        [-4948.31942, 26870.91986, -58334.48399, 63280.17197, -34298.01053, 7430.30146],
                        0.811,
                        1.0,
                    )
                }
            }
            Lvl::TextOnPanel => {
                if dark {
                    curve([0.86713, 0.05232, -0.89428, 4.79014, -5.5432, 1.75023], 0.0, 0.542)
                } else {
                    curve(
                        [-8570.89457, 43954.60805, -90084.00702, 92220.6791, -47152.15802, 9632.27113],
                        0.867,
                        1.0,
                    )
                }
            }
            Lvl::Text => {
                if dark {
                    curve([0.89242, 0.02311, -0.44862, 2.34417, -0.06084, -2.63844], 0.0, 0.5)
                } else {
                    curve(
                        [-2004.67048, 6664.47299, -6060.70202, -1792.61209, 5133.82359, -1939.85583],
                        0.894,
                        1.0,
                    )
                }
            }
        }
    }
}

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

    /// The token a non-background surface is.
    fn token(&self) -> Option<ThemeToken> {
        match self {
            Self::SelectedBg => Some(ThemeToken::SelectedBg),
            Self::SearchMatchBg => Some(ThemeToken::SearchMatchBg),
            Self::UserMessageBg => Some(ThemeToken::UserMessageBg),
            Self::CustomMessageBg => Some(ThemeToken::CustomMessageBg),
            Self::ToolPendingBg => Some(ThemeToken::ToolPendingBg),
            Self::ToolSuccessBg => Some(ThemeToken::ToolSuccessBg),
            Self::ToolErrorBg => Some(ThemeToken::ToolErrorBg),
            Self::Background => None,
            Self::ScrollbarTrack => Some(ThemeToken::ScrollbarTrack),
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

struct RuleDef {
    token: ThemeToken,
    on: Vec<Surface>,
    level: Lvl,
}

const TOOL_PANELS: [Surface; 3] = [Surface::ToolPendingBg, Surface::ToolSuccessBg, Surface::ToolErrorBg];
const MESSAGE_PANELS: [Surface; 2] = [Surface::UserMessageBg, Surface::CustomMessageBg];
const PANELS: [ThemeToken; 7] = [
    ThemeToken::UserMessageBg,
    ThemeToken::ToolPendingBg,
    ThemeToken::ToolSuccessBg,
    ThemeToken::ToolErrorBg,
    ThemeToken::SelectedBg,
    ThemeToken::SearchMatchBg,
    ThemeToken::CustomMessageBg,
];
const THINKING: [ThemeToken; 7] = [
    ThemeToken::ThinkingOff,
    ThemeToken::ThinkingMinimal,
    ThemeToken::ThinkingLow,
    ThemeToken::ThinkingMedium,
    ThemeToken::ThinkingHigh,
    ThemeToken::ThinkingXhigh,
    ThemeToken::ThinkingMax,
];
const THINKING_LEVELS: [Lvl; 7] =
    [Lvl::Thinking0, Lvl::Thinking1, Lvl::Thinking2, Lvl::Thinking3, Lvl::Thinking4, Lvl::Thinking5, Lvl::Thinking6];

fn cat(parts: &[&[Surface]]) -> Vec<Surface> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

static RULES: LazyLock<Vec<RuleDef>> = LazyLock::new(|| {
    use Surface as S;
    use ThemeToken as T;
    let mut rules = Vec::new();
    let each = |tokens: &[ThemeToken], on: Vec<Surface>, level: Lvl, rules: &mut Vec<RuleDef>| {
        for token in tokens {
            rules.push(RuleDef { token: *token, on: on.clone(), level });
        }
    };
    each(&PANELS, vec![S::Background], Lvl::Panel, &mut rules);
    rules.push(RuleDef { token: T::Text, on: vec![S::Background], level: Lvl::Text });
    rules.push(RuleDef { token: T::Text, on: vec![S::SelectedBg], level: Lvl::TextOnPanel });
    rules.push(RuleDef { token: T::UserMessageText, on: vec![S::UserMessageBg], level: Lvl::TextOnPanel });
    rules.push(RuleDef { token: T::ToolTitle, on: TOOL_PANELS.to_vec(), level: Lvl::TextOnPanel });
    each(
        &[T::Accent, T::Success, T::Error, T::Warning],
        cat(&[&[S::Background, S::SelectedBg], &TOOL_PANELS]),
        Lvl::Readable,
        &mut rules,
    );
    rules.push(RuleDef {
        token: T::Muted,
        on: cat(&[&[S::Background, S::SelectedBg, S::CustomMessageBg], &TOOL_PANELS]),
        level: Lvl::Readable,
    });
    rules.push(RuleDef {
        token: T::Dim,
        on: cat(&[&[S::Background, S::SelectedBg, S::CustomMessageBg], &TOOL_PANELS]),
        level: Lvl::Subtle,
    });
    rules.push(RuleDef { token: T::ThinkingText, on: vec![S::Background], level: Lvl::Readable });
    rules.push(RuleDef {
        token: T::CustomMessageText,
        on: cat(&[&[S::CustomMessageBg], &TOOL_PANELS]),
        level: Lvl::Readable,
    });
    rules.push(RuleDef {
        token: T::CustomMessageLabel,
        on: cat(&[&[S::Background, S::CustomMessageBg, S::SelectedBg], &TOOL_PANELS]),
        level: Lvl::Readable,
    });
    rules.push(RuleDef { token: T::ToolOutput, on: cat(&[&[S::Background], &TOOL_PANELS]), level: Lvl::Readable });
    each(
        &[T::MdHeading, T::MdLink, T::MdLinkUrl, T::MdCode, T::MdQuote, T::MdCodeBlockBorder, T::MdListBullet],
        cat(&[&[S::Background], &MESSAGE_PANELS]),
        Lvl::Readable,
        &mut rules,
    );
    rules.push(RuleDef {
        token: T::MdCodeBlock,
        on: cat(&[&[S::Background], &MESSAGE_PANELS, &TOOL_PANELS]),
        level: Lvl::Readable,
    });
    each(
        &[T::ToolDiffAdded, T::ToolDiffRemoved, T::ToolDiffContext],
        cat(&[&[S::Background], &TOOL_PANELS]),
        Lvl::Readable,
        &mut rules,
    );
    each(
        &[
            T::SyntaxComment,
            T::SyntaxKeyword,
            T::SyntaxFunction,
            T::SyntaxVariable,
            T::SyntaxString,
            T::SyntaxNumber,
            T::SyntaxType,
            T::SyntaxOperator,
            T::SyntaxPunctuation,
        ],
        cat(&[&[S::Background], &MESSAGE_PANELS, &TOOL_PANELS]),
        Lvl::Readable,
        &mut rules,
    );
    rules.push(RuleDef { token: T::SearchMatchText, on: vec![S::SearchMatchBg], level: Lvl::Readable });
    each(&[T::BashMode, T::Border, T::BorderAccent], vec![S::Background], Lvl::Readable, &mut rules);
    rules.push(RuleDef { token: T::BorderMuted, on: vec![S::Background], level: Lvl::Subtle });
    each(
        &[T::MdQuoteBorder, T::MdHr],
        cat(&[&[S::Background], &MESSAGE_PANELS, &TOOL_PANELS]),
        Lvl::Readable,
        &mut rules,
    );
    rules.push(RuleDef { token: T::ScrollbarTrack, on: vec![S::Background], level: Lvl::Track });
    rules.push(RuleDef { token: T::ScrollbarThumb, on: vec![S::ScrollbarTrack], level: Lvl::Thumb });
    for (index, token) in THINKING.iter().enumerate() {
        rules.push(RuleDef { token: *token, on: vec![S::Background], level: THINKING_LEVELS[index] });
    }
    rules
});

/// Relaxation compresses levels stronger than this one toward it before weakening all levels.
fn readable_floor(appearance: ThemeAppearance) -> Lvl {
    match appearance {
        ThemeAppearance::Dark => Lvl::Readable,
        ThemeAppearance::Light => Lvl::Subtle,
    }
}

/// Body text uses the terminal's foreground when it reaches this level, which is clearly stronger than muted.
const FOREGROUND_LEVEL: Lvl = Lvl::Emphasis;

/// Text-level tokens that take the terminal's foreground.
const FOREGROUND_TOKENS: [ThemeToken; 3] = [ThemeToken::Text, ThemeToken::UserMessageText, ThemeToken::ToolTitle];

/// WCAG 2 contrast ratio that body text must reach on the surfaces it is drawn on.
const TEXT_MINIMUM_WCAG_CONTRAST: f64 = 4.5;

/// Tokens in dependency order: every surface before the tokens drawn on it.
static SOLVE_ORDER: LazyLock<Vec<ThemeToken>> = LazyLock::new(|| {
    fn visit(token: ThemeToken, order: &mut Vec<ThemeToken>) {
        if order.contains(&token) {
            return;
        }
        for rule in RULES.iter() {
            if rule.token != token {
                continue;
            }
            for surface in &rule.on {
                if let Some(t) = surface.token() {
                    visit(t, order);
                }
            }
        }
        order.push(token);
    }
    let mut order = Vec::new();
    for rule in RULES.iter() {
        visit(rule.token, &mut order);
    }
    order
});

// ============================================================================
// Public API
// ============================================================================

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

fn rgb(color: RgbColor) -> pi_tui::colors::Color {
    rgb_color(color.r.clamp(0.0, 255.0), color.g.clamp(0.0, 255.0), color.b.clamp(0.0, 255.0))
        .unwrap_or(pi_tui::colors::Color::Rgb { r: 0.0, g: 0.0, b: 0.0 })
}

/// OKLab lightness of an sRGB color, 0-1.
fn oklab_lightness(color: RgbColor) -> f64 {
    color_to_oklch(&rgb(color)).l
}

/// WCAG 2 relative luminance.
pub fn relative_luminance(options: RgbColor) -> f64 {
    let linear = |channel: f64| {
        let value = channel / 255.0;
        if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(options.r) + 0.7152 * linear(options.g) + 0.0722 * linear(options.b)
}

/// WCAG 2 contrast ratio, 1-21.
pub fn wcag_contrast(first: RgbColor, second: RgbColor) -> f64 {
    let a = relative_luminance(first);
    let b = relative_luminance(second);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

const WHITE: RgbColor = RgbColor { r: 255.0, g: 255.0, b: 255.0 };
const BLACK: RgbColor = RgbColor { r: 0.0, g: 0.0, b: 0.0 };

/// Whether a terminal is dark or light, from its reported colors: the direction of its own foreground
/// when text can be readable that way, otherwise dark when white text has more contrast on the
/// background than black text.
pub fn terminal_appearance(background: RgbColor, foreground: Option<RgbColor>) -> ThemeAppearance {
    let white_contrast = wcag_contrast(WHITE, background);
    let black_contrast = wcag_contrast(BLACK, background);
    if let Some(foreground) = foreground {
        let foreground_l = oklab_lightness(foreground);
        let background_l = oklab_lightness(background);
        if (foreground_l - background_l).abs() > 0.05 {
            let appearance =
                if foreground_l > background_l { ThemeAppearance::Dark } else { ThemeAppearance::Light };
            let best = if appearance == ThemeAppearance::Dark { white_contrast } else { black_contrast };
            if best >= TEXT_MINIMUM_WCAG_CONTRAST {
                return appearance;
            }
        }
    }
    if white_contrast >= black_contrast { ThemeAppearance::Dark } else { ThemeAppearance::Light }
}

// ============================================================================
// Generation
// ============================================================================

fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn hex_of(color: RgbColor) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        js_round(color.r) as i64,
        js_round(color.g) as i64,
        js_round(color.b) as i64
    )
}

/// Saturation weight at a lightness: a Gaussian (center 0.5, sigma 0.25), 0 at black and white, 1 in the middle.
fn bell_weight(lightness: f64) -> f64 {
    let gaussian = |x: f64| (-((x - 0.5).powi(2)) / (2.0 * 0.25f64.powi(2))).exp();
    (gaussian(lightness) - gaussian(0.0)) / (1.0 - gaussian(0.0))
}

/// A family's saturation curve relative to its maximum: 1 at mid lightness, `min / max` at black and white.
fn saturation_curve(family: &Family, lightness: f64) -> f64 {
    let FamilySaturation { min, max } = family.saturation;
    let floor = if max > 0.0 { min / max } else { 1.0 };
    floor + (1.0 - floor) * bell_weight(lightness)
}

/// The target lightness for a level on a surface, or None where the level cannot be reached.
fn level_target(level: Lvl, appearance: ThemeAppearance, surface_l: f64) -> Option<f64> {
    let curve = level.curve(appearance);
    if surface_l < curve.reachable.0 || surface_l > curve.reachable.1 {
        return None;
    }
    Some(
        curve
            .coefficients
            .iter()
            .enumerate()
            .fold(0.0, |sum, (power, coefficient)| sum + coefficient * surface_l.powi(power as i32)),
    )
}

/// `okhslColor` as an sRGB color; inputs are clamped into range.
fn okhsl_rgb(h: f64, s: f64, l: f64) -> RgbColor {
    let color = okhsl_color(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
        .unwrap_or(pi_tui::colors::Color::Rgb { r: 0.0, g: 0.0, b: 0.0 });
    color_to_rgb(&color)
}

/// Generate the system theme's colors from the terminal's reported colors.
pub fn generate_system_theme_colors(input: &SystemThemeInput) -> SystemThemeColors {
    let saturation = input.saturation.unwrap_or(1.0).clamp(0.0, 1.0);
    let Some(background) = input.background else {
        return indexed_colors(saturation, input.appearance_hint);
    };
    let foreground = input.foreground;
    let palette: Option<Vec<SourceColor>> = input
        .palette
        .as_ref()
        .filter(|p| p.len() == 16)
        .map(|p| p.iter().copied().map(source_of).collect());

    let appearance = terminal_appearance(background, foreground);
    let lighter = appearance == ThemeAppearance::Dark;
    let extreme = if lighter { 1.0 } else { 0.0 };
    let background_l = oklab_lightness(background);

    // A token's color at an OKLab lightness. With a palette, the palette color's saturation applies at its
    // own lightness and falls off toward black and white along the family's curve, never rising above it.
    let paint = |token: ThemeToken, oklab_l: f64| -> RgbColor {
        let lightness = oklab_to_okhsl_lightness(oklab_l);
        let family = token_family(token).family();
        match &palette {
            None => {
                let FamilySaturation { min, max } = family.saturation;
                okhsl_rgb(family.hue, (min + (max - min) * bell_weight(lightness)) * saturation, lightness)
            }
            Some(palette) => {
                let slot = token_slot(token).unwrap_or(family.slot as usize);
                anchored(&palette[slot], &family, lightness, saturation)
            }
        }
    };

    // The lightness a rule needs on a surface, relaxed by `t`: from 0 to 1, levels stronger than the readable
    // floor move toward it; from 1 to 2, all levels move toward the surface itself.
    let target = |level: Lvl, surface_l: f64, t: f64| -> Option<f64> {
        let reached = level_target(level, appearance, surface_l);
        if reached.is_none() && t == 0.0 {
            return None;
        }
        let distance = reached.unwrap_or(extreme) - surface_l;
        let floor =
            level_target(readable_floor(appearance), appearance, surface_l).unwrap_or(extreme) - surface_l;
        let compressed =
            if distance.abs() > floor.abs() { distance - (distance - floor) * t.min(1.0) } else { distance };
        Some(surface_l + compressed * (1.0 - (t - 1.0).max(0.0)))
    };

    // Keep a panel light enough (or dark enough) that white (or black) text still reaches the body text
    // minimum on it. This only matters for backgrounds near mid-gray, where it barely does on the background.
    let extreme_text = if lighter { WHITE } else { BLACK };
    let readable = |color: RgbColor| wcag_contrast(extreme_text, color) >= TEXT_MINIMUM_WCAG_CONTRAST;
    let limit_panel = |token: ThemeToken, l: f64| -> RgbColor {
        let color = paint(token, l);
        if readable(color) {
            return color;
        }
        let (mut low, mut high) = (background_l, l);
        for _ in 0..20 {
            let middle = (low + high) / 2.0;
            if readable(paint(token, middle)) {
                low = middle;
            } else {
                high = middle;
            }
        }
        paint(token, low)
    };

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum Key {
        Background,
        Token(ThemeToken),
    }
    let key_of = |surface: &Surface| surface.token().map(Key::Token).unwrap_or(Key::Background);

    let solve = |t: f64| -> Option<HashMap<Key, RgbColor>> {
        let mut colors: HashMap<Key, RgbColor> = HashMap::new();
        colors.insert(Key::Background, background);
        for token in SOLVE_ORDER.iter() {
            let mut targets: Vec<f64> = Vec::new();
            for rule in RULES.iter() {
                if rule.token != *token {
                    continue;
                }
                for surface in &rule.on {
                    let surface_color = colors.get(&key_of(surface)).copied().unwrap_or(background);
                    let value = target(rule.level, oklab_lightness(surface_color), t)?;
                    if !(0.0..=1.0).contains(&value) {
                        return None;
                    }
                    targets.push(value);
                }
            }
            let l = if lighter {
                targets.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            } else {
                targets.iter().copied().fold(f64::INFINITY, f64::min)
            };
            let color = if PANELS.contains(token) { limit_panel(*token, l) } else { paint(*token, l) };
            colors.insert(Key::Token(*token), color);
        }
        Some(colors)
    };

    let mut relaxation = 0.0;
    let mut colors = solve(0.0);
    if colors.is_none() {
        // Mid-gray backgrounds cannot fit every level: relax as little as possible. Full relaxation always fits.
        let (mut low, mut high) = (0.0, 2.0);
        colors = solve(high);
        for _ in 0..20 {
            let middle = (low + high) / 2.0;
            if let Some(attempt) = solve(middle) {
                high = middle;
                colors = Some(attempt);
            } else {
                low = middle;
            }
        }
        relaxation = high;
    }
    let solved = colors.unwrap_or_default();
    let surfaces_of = |token: ThemeToken| -> Vec<RgbColor> {
        RULES
            .iter()
            .filter(|rule| rule.token == token)
            .flat_map(|rule| {
                rule.on.iter().map(|surface| solved.get(&key_of(surface)).copied().unwrap_or(background))
            })
            .collect()
    };

    let mut result: IndexMap<ThemeToken, SystemThemeColorsColors> = IndexMap::new();
    for (token, _) in TOKEN_FAMILIES {
        let value = match solved.get(&Key::Token(*token)) {
            Some(color) => hex_of(*color),
            None => String::new(),
        };
        result.insert(*token, SystemThemeColorsColors::String(value));
    }

    for token in FOREGROUND_TOKENS {
        let surfaces = surfaces_of(token);
        // Body text uses the terminal's own foreground where it is clearly stronger than muted text; otherwise
        // the foreground's hue at just enough lightness.
        let mut text = solved.get(&Key::Token(token)).copied();
        if let Some(foreground) = foreground {
            let targets: Vec<Option<f64>> = surfaces
                .iter()
                .map(|surface| target(FOREGROUND_LEVEL, oklab_lightness(*surface), relaxation))
                .collect();
            if targets.iter().all(|value| matches!(value, Some(v) if (0.0..=1.0).contains(v))) {
                let values = targets.iter().map(|v| v.unwrap());
                let needed = if lighter {
                    values.fold(f64::NEG_INFINITY, f64::max)
                } else {
                    values.fold(f64::INFINITY, f64::min)
                };
                let foreground_l = oklab_lightness(foreground);
                if if lighter { foreground_l >= needed } else { foreground_l <= needed } {
                    result.insert(token, SystemThemeColorsColors::String(String::new()));
                    continue;
                }
                text = Some(anchored(
                    &source_of(foreground),
                    &Fam::Neutral.family(),
                    oklab_to_okhsl_lightness(needed),
                    saturation,
                ));
            }
        }
        // Body text keeps at least 4.5:1 on the surfaces it is drawn on, even on relaxed mid-gray backgrounds.
        if let Some(text) = text {
            result.insert(token, SystemThemeColorsColors::String(hex_of(with_text_contrast(text, &surfaces, lighter))));
        }
    }
    SystemThemeColors { colors: result, dim: Vec::new(), appearance: Some(appearance) }
}

fn okhsl_of(color: RgbColor) -> OkhslChannels {
    color_to_okhsl(&rgb(color))
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

fn source_of(color: RgbColor) -> SourceColor {
    let okhsl = okhsl_of(color);
    SourceColor { h: okhsl.h, s: okhsl.s, l: okhsl.l, chroma: color_to_oklch(&rgb(color)).c }
}

/// A source color's hue at another OKHSL lightness. Its saturation applies at its own lightness and falls off
/// toward black and white along the family's saturation curve, never rising above it. Chroma is also capped
/// at the source's, with the same falloff.
fn anchored(source: &SourceColor, family: &Family, lightness: f64, saturation: f64) -> RgbColor {
    let anchor = saturation_curve(family, source.l);
    let falloff = if anchor > 0.0 { (saturation_curve(family, lightness) / anchor).min(1.0) } else { 1.0 };
    let color = okhsl_rgb(source.h, source.s * falloff * saturation, lightness);
    let cap = source.chroma * falloff * saturation;
    let oklch = color_to_oklch(&rgb(color));
    if oklch.c <= cap {
        return color;
    }
    match oklch_color(oklch.l.clamp(0.0, 1.0), cap.max(0.0), source.h) {
        Ok(capped) => color_to_rgb(&capped),
        Err(_) => color,
    }
}

/// Move a text color toward white or black until it reaches the WCAG minimum on every surface.
fn with_text_contrast(color: RgbColor, surfaces: &[RgbColor], lighter: bool) -> RgbColor {
    let meets = |candidate: RgbColor| {
        surfaces.iter().all(|surface| wcag_contrast(candidate, *surface) >= TEXT_MINIMUM_WCAG_CONTRAST)
    };
    if meets(color) {
        return color;
    }
    let OkhslChannels { h, s, l } = okhsl_of(color);
    let at = |lightness: f64| okhsl_rgb(h, s, lightness);
    let extreme = if lighter { 1.0 } else { 0.0 };
    if !meets(at(extreme)) {
        return at(extreme);
    }
    let (mut low, mut high) = (l, extreme);
    for _ in 0..20 {
        let middle = (low + high) / 2.0;
        if meets(at(middle)) {
            high = middle;
        } else {
            low = middle;
        }
    }
    at(high)
}

/// Colors for terminals that reported nothing: the terminal renders ANSI indices 0-15 and the default
/// colors with its own theme, so they fit any background. Neutral tokens below body text are faint (SGR 2)
/// instead of bright black, which some themes make nearly invisible. Panels have no background.
fn indexed_colors(saturation: f64, appearance: Option<ThemeAppearance>) -> SystemThemeColors {
    let mut colors = IndexMap::new();
    let mut dim = Vec::new();
    for (token, family_name) in TOKEN_FAMILIES {
        if PANELS.contains(token) {
            colors.insert(*token, SystemThemeColorsColors::String(String::new()));
            continue;
        }
        let neutral = *family_name == Fam::Neutral;
        let value = if !neutral && saturation > 0.0 {
            SystemThemeColorsColors::Number(token_slot(*token).unwrap_or(family_name.family().slot as usize) as i64)
        } else {
            SystemThemeColorsColors::String(String::new())
        };
        colors.insert(*token, value);
        if neutral
            && !FOREGROUND_TOKENS.contains(token)
            && let Some(color) = theme_color_of(*token)
        {
            dim.push(color);
        }
    }
    SystemThemeColors { colors, dim, appearance }
}

/// The foreground token for a token, if it is one.
pub(crate) fn theme_color_of(token: ThemeToken) -> Option<ThemeColor> {
    serde_json::from_value(serde_json::Value::String(token.as_str().to_string())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_tui::colors::parse_color;

    fn hex(value: &str) -> RgbColor {
        color_to_rgb(&parse_color(value).unwrap())
    }
    fn lightness(color: RgbColor) -> f64 {
        oklab_lightness(color)
    }

    fn dracula() -> SystemThemeInput {
        SystemThemeInput {
            background: Some(hex("#282a36")),
            foreground: Some(hex("#f8f8f2")),
            palette: Some(
                [
                    "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
                    "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
                ]
                .iter()
                .map(|h| hex(h))
                .collect(),
            ),
            ..Default::default()
        }
    }

    fn terminals() -> Vec<(&'static str, SystemThemeInput)> {
        vec![
            ("dracula", dracula()),
            (
                "solarizedLight",
                SystemThemeInput {
                    background: Some(hex("#fdf6e3")),
                    foreground: Some(hex("#657b83")),
                    ..Default::default()
                },
            ),
            ("backgroundOnly", SystemThemeInput { background: Some(hex("#1e1e1e")), ..Default::default() }),
            (
                "midGray",
                SystemThemeInput {
                    background: Some(hex("#808080")),
                    foreground: Some(hex("#ffffff")),
                    ..Default::default()
                },
            ),
        ]
    }

    const PANEL_TOKENS: [ThemeToken; 5] = [
        ThemeToken::UserMessageBg,
        ThemeToken::ToolPendingBg,
        ThemeToken::ToolSuccessBg,
        ThemeToken::ToolErrorBg,
        ThemeToken::SelectedBg,
    ];

    fn resolved(input: &SystemThemeInput, token: ThemeToken) -> RgbColor {
        match &generate_system_theme_colors(input).colors[&token] {
            SystemThemeColorsColors::String(s) if s.is_empty() => {
                if PANEL_TOKENS.contains(&token) {
                    input.background.unwrap()
                } else {
                    input.foreground.unwrap()
                }
            }
            SystemThemeColorsColors::String(s) => hex(s),
            SystemThemeColorsColors::Number(_) => panic!("unexpected index"),
        }
    }

    #[test]
    fn keeps_body_text_readable() {
        for (name, input) in terminals() {
            let text = resolved(&input, ThemeToken::Text);
            for surface in [input.background.unwrap(), resolved(&input, ThemeToken::SelectedBg)] {
                assert!(wcag_contrast(text, surface) >= 4.5, "{name}");
            }
            assert!(
                wcag_contrast(resolved(&input, ThemeToken::ToolTitle), resolved(&input, ThemeToken::ToolErrorBg))
                    >= 4.5,
                "{name}"
            );
        }
    }

    #[test]
    fn orders_foreground_roles_and_keeps_panels_close() {
        for (name, input) in terminals() {
            let background = lightness(input.background.unwrap());
            let offset = |token| lightness(resolved(&input, token)) - background;
            if name != "midGray" {
                assert!(offset(ThemeToken::Text).abs() > offset(ThemeToken::Muted).abs(), "{name}");
                assert!(offset(ThemeToken::Muted).abs() > offset(ThemeToken::Dim).abs(), "{name}");
            }
            let lighter = generate_system_theme_colors(&input).appearance == Some(ThemeAppearance::Dark);
            for panel in PANEL_TOKENS {
                assert!(wcag_contrast(resolved(&input, panel), input.background.unwrap()) < 2.0, "{name} {panel:?}");
                assert_eq!(offset(panel) > 0.0, lighter, "{name} {panel:?}");
            }
        }
    }

    #[test]
    fn uses_terminal_foreground_and_palette_hues() {
        assert_eq!(
            generate_system_theme_colors(&dracula()).colors[&ThemeToken::Text],
            SystemThemeColorsColors::String(String::new())
        );
        let solarized = &terminals()[1].1;
        assert_ne!(
            generate_system_theme_colors(solarized).colors[&ThemeToken::Text],
            SystemThemeColorsColors::String(String::new())
        );
        let hue = |c: RgbColor| color_to_oklch(&rgb(c)).h;
        let d = dracula();
        assert!((hue(resolved(&d, ThemeToken::Error)) - hue(d.palette.as_ref().unwrap()[1])).abs() < 8.0);
    }

    #[test]
    fn keeps_pastel_palette_colors_pastel() {
        let frappe = SystemThemeInput {
            background: Some(hex("#303446")),
            foreground: Some(hex("#c6d0f5")),
            palette: Some(
                [
                    "#51576d", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#f4b8e4", "#81c8be", "#b5bfe2",
                    "#626880", "#e67172", "#8ec772", "#d9ba73", "#7b9ef0", "#f2a4db", "#5abfb5", "#a5adce",
                ]
                .iter()
                .map(|h| hex(h))
                .collect(),
            ),
            ..Default::default()
        };
        let chroma = |c: RgbColor| color_to_oklch(&rgb(c)).c;
        let pink = frappe.palette.as_ref().unwrap()[5];
        let accent = resolved(&frappe, ThemeToken::Accent);
        assert!(lightness(accent) < lightness(pink) - 0.05);
        assert!(chroma(accent) <= chroma(pink) * 1.03);
        for panel in [ThemeToken::UserMessageBg, ThemeToken::CustomMessageBg] {
            assert!(chroma(resolved(&frappe, panel)) <= 0.1, "{panel:?}");
        }
    }

    #[test]
    fn renders_grayscale_at_zero_saturation() {
        let input = SystemThemeInput { saturation: Some(0.0), ..dracula() };
        let colors = generate_system_theme_colors(&input).colors;
        let SystemThemeColorsColors::String(error) = &colors[&ThemeToken::Error] else { panic!() };
        assert!(color_to_oklch(&parse_color(error.as_str()).unwrap()).c < 0.005);
    }

    #[test]
    fn falls_back_to_palette_indices_without_background() {
        let result = generate_system_theme_colors(&SystemThemeInput {
            appearance_hint: Some(ThemeAppearance::Light),
            ..Default::default()
        });
        assert_eq!(result.appearance, Some(ThemeAppearance::Light));
        assert_eq!(result.colors[&ThemeToken::Error], SystemThemeColorsColors::Number(1));
        assert_eq!(result.colors[&ThemeToken::Text], SystemThemeColorsColors::String(String::new()));
        assert_eq!(result.colors[&ThemeToken::UserMessageBg], SystemThemeColorsColors::String(String::new()));
        assert!(result.dim.contains(&ThemeColor::Muted));
        assert_eq!(
            generate_system_theme_colors(&SystemThemeInput { saturation: Some(0.0), ..Default::default() }).colors
                [&ThemeToken::Error],
            SystemThemeColorsColors::String(String::new())
        );
    }
}
