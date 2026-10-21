//! Colors and text styling. Port of `packages/tui/src/colors.ts`.

use std::sync::LazyLock;

use regex::Regex;

use crate::oklab::{
    js_round, linear_srgb_to_rgb, okhsl_to_rgb, oklab_to_linear_srgb, rgb_to_okhsl, rgb_to_oklab,
};
use crate::terminal_colors::RgbColor;

/// A concrete color. Every color can be converted to sRGB, so color math never fails.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Indexed { index: u8 },
    Rgb { r: f64, g: f64, b: f64 },
    Oklch { l: f64, c: f64, h: f64 },
}

impl Color {
    pub fn kind(&self) -> &'static str {
        match self {
            Color::Indexed { .. } => "indexed",
            Color::Rgb { .. } => "rgb",
            Color::Oklch { .. } => "oklch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalColorMode {
    #[default]
    Color256,
    Truecolor,
}

impl TerminalColorMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Color256 => "256color",
            Self::Truecolor => "truecolor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMixSpace {
    #[default]
    Oklch,
    Srgb,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OklchChannels {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

/// OKHSL channels: hue in degrees, saturation and lightness 0-1.
pub type OkhslChannels = crate::oklab::Okhsl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextAttributes {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextStyle {
    pub attrs: TextAttributes,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
}

fn require_finite(value: f64, name: &str) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{name} must be finite"));
    }
    Ok(())
}

/// Format a number the way JavaScript's `String(number)` does for typical values.
pub(crate) fn js_number(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if v == v.trunc() && v.abs() < 1e21 {
        return format!("{}", v as i64);
    }
    let s = format!("{v}");
    s
}

pub fn indexed_color(index: i64) -> Result<Color, String> {
    if !(0..=255).contains(&index) {
        return Err(format!("ANSI color index must be an integer from 0 to 255: {index}"));
    }
    Ok(Color::Indexed { index: index as u8 })
}

pub fn rgb_color(r: f64, g: f64, b: f64) -> Result<Color, String> {
    for (name, value) in [("r", r), ("g", g), ("b", b)] {
        require_finite(value, name)?;
        if !(0.0..=255.0).contains(&value) {
            return Err(format!("{name} must be between 0 and 255: {}", js_number(value)));
        }
    }
    Ok(Color::Rgb { r, g, b })
}

pub fn oklch_color(l: f64, c: f64, h: f64) -> Result<Color, String> {
    require_finite(l, "l")?;
    require_finite(c, "c")?;
    require_finite(h, "h")?;
    if !(0.0..=1.0).contains(&l) {
        return Err(format!("l must be between 0 and 1: {}", js_number(l)));
    }
    if c < 0.0 {
        return Err(format!("c must not be negative: {}", js_number(c)));
    }
    Ok(Color::Oklch { l, c, h: ((h % 360.0) + 360.0) % 360.0 })
}

/// An OKHSL color, converted to sRGB.
pub fn okhsl_color(h: f64, s: f64, l: f64) -> Result<Color, String> {
    require_finite(h, "h")?;
    require_finite(s, "s")?;
    require_finite(l, "l")?;
    if !(0.0..=1.0).contains(&s) {
        return Err(format!("s must be between 0 and 1: {}", js_number(s)));
    }
    if !(0.0..=1.0).contains(&l) {
        return Err(format!("l must be between 0 and 1: {}", js_number(l)));
    }
    let rgb = okhsl_to_rgb(h, s, l);
    rgb_color(rgb.r, rgb.g, rgb.b)
}

pub fn color_to_okhsl(color: &Color) -> OkhslChannels {
    rgb_to_okhsl(color_to_rgb(color))
}

const NUMBER_PATTERN: &str = r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?";
static OKLCH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^oklch\(\s*({NUMBER_PATTERN})(%)?\s+({NUMBER_PATTERN})\s+({NUMBER_PATTERN})(?:deg)?\s*\)$"
    ))
    .unwrap()
});
static OKHSL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^okhsl\(\s*({NUMBER_PATTERN})(?:deg)?\s+({NUMBER_PATTERN})(%)?\s+({NUMBER_PATTERN})(%)?\s*\)$"
    ))
    .unwrap()
});

fn parse_float(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// A color value: a palette index or a string such as `#rrggbb`, `oklch(...)`, `okhsl(...)`.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorValue {
    Index(i64),
    Str(String),
}

impl From<i64> for ColorValue {
    fn from(v: i64) -> Self {
        ColorValue::Index(v)
    }
}
impl From<&str> for ColorValue {
    fn from(v: &str) -> Self {
        ColorValue::Str(v.to_string())
    }
}
impl From<String> for ColorValue {
    fn from(v: String) -> Self {
        ColorValue::Str(v)
    }
}

/// Parse a color string (`#rgb`, `#rrggbb`, `oklch(L C H)`, `okhsl(H S L)`).
pub fn parse_color_str(value: &str) -> Result<Color, String> {
    if let Some(hex) = value.strip_prefix('#') {
        if (hex.len() == 3 || hex.len() == 6) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            let digits: String =
                if hex.len() == 3 { hex.chars().flat_map(|d| [d, d]).collect() } else { hex.to_string() };
            let ch = |a: usize| i64::from_str_radix(&digits[a..a + 2], 16).unwrap() as f64;
            return rgb_color(ch(0), ch(2), ch(4));
        }
    }
    if let Some(m) = OKLCH_PATTERN.captures(value) {
        let lightness = parse_float(&m[1]) / if m.get(2).is_some() { 100.0 } else { 1.0 };
        return oklch_color(lightness, parse_float(&m[3]), parse_float(&m[4]));
    }
    if let Some(m) = OKHSL_PATTERN.captures(value) {
        let saturation = parse_float(&m[2]) / if m.get(3).is_some() { 100.0 } else { 1.0 };
        let lightness = parse_float(&m[4]) / if m.get(5).is_some() { 100.0 } else { 1.0 };
        return okhsl_color(parse_float(&m[1]), saturation, lightness);
    }
    Err(format!("Invalid color value: {value}"))
}

/// Parse a color value (palette index or string).
pub fn parse_color(value: impl Into<ColorValue>) -> Result<Color, String> {
    match value.into() {
        ColorValue::Index(i) => indexed_color(i),
        ColorValue::Str(s) => parse_color_str(&s),
    }
}

const BASIC_COLORS: [(f64, f64, f64); 16] = [
    (0.0, 0.0, 0.0),
    (128.0, 0.0, 0.0),
    (0.0, 128.0, 0.0),
    (128.0, 128.0, 0.0),
    (0.0, 0.0, 128.0),
    (128.0, 0.0, 128.0),
    (0.0, 128.0, 128.0),
    (192.0, 192.0, 192.0),
    (128.0, 128.0, 128.0),
    (255.0, 0.0, 0.0),
    (0.0, 255.0, 0.0),
    (255.0, 255.0, 0.0),
    (0.0, 0.0, 255.0),
    (255.0, 0.0, 255.0),
    (0.0, 255.0, 255.0),
    (255.0, 255.0, 255.0),
];
const CUBE_VALUES: [f64; 6] = [0.0, 95.0, 135.0, 175.0, 215.0, 255.0];

fn gray_values() -> [f64; 24] {
    std::array::from_fn(|i| 8.0 + i as f64 * 10.0)
}

fn indexed_to_rgb(index: u8) -> RgbColor {
    let index = index as usize;
    if index < 16 {
        let (r, g, b) = BASIC_COLORS[index];
        return RgbColor { r, g, b };
    }
    if index < 232 {
        let cube = index - 16;
        return RgbColor { r: CUBE_VALUES[cube / 36], g: CUBE_VALUES[(cube % 36) / 6], b: CUBE_VALUES[cube % 6] };
    }
    let gray = 8.0 + (index - 232) as f64 * 10.0;
    RgbColor { r: gray, g: gray, b: gray }
}

fn is_in_srgb_gamut(linear: &[f64; 3]) -> bool {
    let epsilon = 1e-7;
    linear.iter().all(|&c| c >= -epsilon && c <= 1.0 + epsilon)
}

fn oklch_to_rgb(l: f64, c: f64, h: f64) -> RgbColor {
    let radians = (h * std::f64::consts::PI) / 180.0;
    let cos = radians.cos();
    let sin = radians.sin();
    let at_chroma = |chroma: f64| oklab_to_linear_srgb([l, chroma * cos, chroma * sin]);

    let direct = at_chroma(c);
    if is_in_srgb_gamut(&direct) {
        return linear_srgb_to_rgb(direct);
    }
    let mut linear = at_chroma(0.0);
    let mut low = 0.0;
    let mut high = c;
    for _ in 0..20 {
        let chroma = (low + high) / 2.0;
        let candidate = at_chroma(chroma);
        if is_in_srgb_gamut(&candidate) {
            low = chroma;
            linear = candidate;
        } else {
            high = chroma;
        }
    }
    linear_srgb_to_rgb(linear)
}

pub fn color_to_rgb(color: &Color) -> RgbColor {
    match *color {
        Color::Indexed { index } => indexed_to_rgb(index),
        Color::Rgb { r, g, b } => RgbColor { r, g, b },
        Color::Oklch { l, c, h } => oklch_to_rgb(l, c, h),
    }
}

pub fn color_to_oklch(color: &Color) -> OklchChannels {
    if let Color::Oklch { l, c, h } = *color {
        return OklchChannels { l, c, h };
    }
    let [l, a, b] = rgb_to_oklab(color_to_rgb(color));
    OklchChannels { l, c: a.hypot(b), h: ((b.atan2(a) * 180.0) / std::f64::consts::PI + 360.0) % 360.0 }
}

pub fn color_to_hex(color: &Color) -> String {
    let rgb = color_to_rgb(color);
    let channel = |v: f64| format!("{:02x}", js_round(v) as i64);
    format!("#{}{}{}", channel(rgb.r), channel(rgb.g), channel(rgb.b))
}

pub fn mix_colors(first: &Color, second: &Color, amount: f64, space: ColorMixSpace) -> Result<Color, String> {
    require_finite(amount, "amount")?;
    if !(0.0..=1.0).contains(&amount) {
        return Err(format!("amount must be between 0 and 1: {}", js_number(amount)));
    }
    if space == ColorMixSpace::Srgb {
        let a = color_to_rgb(first);
        let b = color_to_rgb(second);
        return rgb_color(a.r + (b.r - a.r) * amount, a.g + (b.g - a.g) * amount, a.b + (b.b - a.b) * amount);
    }
    let a = color_to_oklch(first);
    let b = color_to_oklch(second);
    let first_hue = if a.c < 1e-7 { b.h } else { a.h };
    let second_hue = if b.c < 1e-7 { first_hue } else { b.h };
    let hue_delta = js_mod(second_hue - first_hue + 540.0, 360.0) - 180.0;
    oklch_color(a.l + (b.l - a.l) * amount, a.c + (b.c - a.c) * amount, first_hue + hue_delta * amount)
}

/// JavaScript `%` (truncated remainder, sign of dividend) — identical to Rust's `%` on f64.
fn js_mod(a: f64, b: f64) -> f64 {
    a % b
}

fn find_closest(values: &[f64], target: f64) -> usize {
    let mut closest_index = 0;
    let mut closest_distance = f64::INFINITY;
    for (index, v) in values.iter().enumerate() {
        let distance = (target - v).abs();
        if distance < closest_distance {
            closest_index = index;
            closest_distance = distance;
        }
    }
    closest_index
}

fn color_distance(a: RgbColor, b: RgbColor) -> f64 {
    let dr = a.r - b.r;
    let dg = a.g - b.g;
    let db = a.b - b.b;
    dr * dr * 0.299 + dg * dg * 0.587 + db * db * 0.114
}

fn rgb_to_ansi256(color: RgbColor) -> usize {
    let r_index = find_closest(&CUBE_VALUES, color.r);
    let g_index = find_closest(&CUBE_VALUES, color.g);
    let b_index = find_closest(&CUBE_VALUES, color.b);
    let cube_color = RgbColor { r: CUBE_VALUES[r_index], g: CUBE_VALUES[g_index], b: CUBE_VALUES[b_index] };
    let cube_index = 16 + 36 * r_index + 6 * g_index + b_index;

    let grays = gray_values();
    let gray = js_round(0.299 * color.r + 0.587 * color.g + 0.114 * color.b);
    let gray_offset = find_closest(&grays, gray);
    let gray_value = grays[gray_offset];
    let max = color.r.max(color.g).max(color.b);
    let min = color.r.min(color.g).min(color.b);
    let spread = max - min;
    if spread < 10.0
        && color_distance(color, RgbColor { r: gray_value, g: gray_value, b: gray_value })
            < color_distance(color, cube_color)
    {
        return 232 + gray_offset;
    }
    cube_index
}

fn color_ansi(color: &Color, mode: TerminalColorMode, background: bool) -> String {
    let code = if background { 48 } else { 38 };
    if let Color::Indexed { index } = color {
        return format!("\x1b[{code};5;{index}m");
    }
    let rgb = color_to_rgb(color);
    if mode == TerminalColorMode::Truecolor {
        return format!(
            "\x1b[{code};2;{};{};{}m",
            js_round(rgb.r) as i64,
            js_round(rgb.g) as i64,
            js_round(rgb.b) as i64
        );
    }
    format!("\x1b[{code};5;{}m", rgb_to_ansi256(rgb))
}

pub fn foreground_ansi(color: &Color, mode: TerminalColorMode) -> String {
    color_ansi(color, mode, false)
}

pub fn background_ansi(color: &Color, mode: TerminalColorMode) -> String {
    color_ansi(color, mode, true)
}

pub fn style_text(text: &str, options: &TextStyle, mode: TerminalColorMode) -> String {
    let fg = options.fg.as_ref().map(|c| foreground_ansi(c, mode));
    let bg = options.bg.as_ref().map(|c| background_ansi(c, mode));
    style_text_with_ansi(text, fg.as_deref(), bg.as_deref(), &options.attrs)
}

/// Like [`style_text`], but with precomputed color escape sequences.
pub fn style_text_with_ansi(
    text: &str,
    fg_ansi: Option<&str>,
    bg_ansi: Option<&str>,
    options: &TextAttributes,
) -> String {
    let mut prefix = String::new();
    let mut suffix = String::new();
    if let Some(fg) = fg_ansi.filter(|s| !s.is_empty()) {
        prefix.push_str(fg);
        suffix = "\x1b[39m".to_string();
    }
    if let Some(bg) = bg_ansi.filter(|s| !s.is_empty()) {
        prefix.push_str(bg);
        suffix = format!("\x1b[49m{suffix}");
    }
    if options.bold {
        prefix.push_str("\x1b[1m");
    }
    if options.dim {
        prefix.push_str("\x1b[2m");
    }
    if options.bold || options.dim {
        suffix = format!("\x1b[22m{suffix}");
    }
    if options.italic {
        prefix.push_str("\x1b[3m");
        suffix = format!("\x1b[23m{suffix}");
    }
    if options.underline {
        prefix.push_str("\x1b[4m");
        suffix = format!("\x1b[24m{suffix}");
    }
    if options.inverse {
        prefix.push_str("\x1b[7m");
        suffix = format!("\x1b[27m{suffix}");
    }
    if options.strikethrough {
        prefix.push_str("\x1b[9m");
        suffix = format!("\x1b[29m{suffix}");
    }
    format!("{prefix}{text}{suffix}")
}
