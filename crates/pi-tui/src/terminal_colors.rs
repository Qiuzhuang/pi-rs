//! Terminal color reports (OSC 4/10/11 replies, color scheme reports).
//! Port of `packages/tui/src/terminal-colors.ts`.

use crate::oklab::js_round;

/// An sRGB color with channels 0-255 (channels may be fractional after mixing).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RgbColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl RgbColor {
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalColorScheme {
    Dark,
    Light,
}

impl TerminalColorScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Colors the terminal reports for its current theme.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerminalColors {
    /// Default foreground (OSC 10).
    pub foreground: Option<RgbColor>,
    /// Default background (OSC 11).
    pub background: Option<RgbColor>,
    /// ANSI colors 0-15 (OSC 4). Only set when the terminal reported all 16.
    pub palette: Option<Vec<RgbColor>>,
}

fn parse_hex_prefix(s: &str) -> f64 {
    // parseInt(s, 16): parse the leading hex digits, NaN when none.
    let digits: String = s.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    u64::from_str_radix(&digits, 16).map(|v| v as f64).unwrap_or(f64::NAN)
}

fn hex_to_rgb(hex: &str) -> RgbColor {
    let normalized = hex.strip_prefix('#').unwrap_or(hex);
    let part = |a: usize, b: usize| normalized.get(a..b.min(normalized.len())).map_or(f64::NAN, parse_hex_prefix);
    RgbColor { r: part(0, 2), g: part(2, 4), b: part(4, 6) }
}

fn parse_osc_hex_channel(channel: &str) -> Option<f64> {
    if channel.is_empty() || !channel.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let max = 16f64.powi(channel.len() as i32) - 1.0;
    if max <= 0.0 {
        return None;
    }
    let value = parse_hex_prefix(channel);
    Some(js_round((value / max) * 255.0))
}

/// What an OSC color reply reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscColorTarget {
    Foreground,
    Background,
    Palette(u32),
}

/// A parsed OSC color reply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OscColorResponse {
    pub target: OscColorTarget,
    pub rgb: Option<RgbColor>,
}

/// Parse an OSC 10, 11, or 4 color reply. Returns `None` when `data` is not such a reply;
/// `rgb` is `None` when it is a reply with an unparseable color.
pub fn parse_osc_color_response(data: &str) -> Option<OscColorResponse> {
    // /^\x1b\](?:(1[01])|4;(\d{1,3}));([^\x07\x1b]*)(?:\x07|\x1b\\)$/i
    let body = data.strip_prefix("\x1b]")?;
    let body = if let Some(b) = body.strip_suffix('\x07') { b } else { body.strip_suffix("\x1b\\")? };
    let (target, rest) = if let Some(rest) = body.strip_prefix("10;") {
        (OscColorTarget::Foreground, rest)
    } else if let Some(rest) = body.strip_prefix("11;") {
        (OscColorTarget::Background, rest)
    } else if let Some(rest) = body.strip_prefix("4;") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || digits.len() > 3 || rest.as_bytes().get(digits.len()) != Some(&b';') {
            return None;
        }
        (OscColorTarget::Palette(digits.parse().ok()?), &rest[digits.len() + 1..])
    } else {
        return None;
    };
    if rest.contains('\x07') || rest.contains('\x1b') {
        return None;
    }
    Some(OscColorResponse { target, rgb: parse_osc_color_value(rest) })
}

fn parse_osc_color_value(raw: &str) -> Option<RgbColor> {
    let value = crate::utils::js_trim(raw);
    if let Some(hex) = value.strip_prefix('#') {
        let all_hex = hex.chars().all(|c| c.is_ascii_hexdigit());
        if hex.len() == 6 && all_hex {
            return Some(hex_to_rgb(value));
        }
        if hex.len() == 12 && all_hex {
            let r = parse_osc_hex_channel(&hex[0..4])?;
            let g = parse_osc_hex_channel(&hex[4..8])?;
            let b = parse_osc_hex_channel(&hex[8..12])?;
            return Some(RgbColor { r, g, b });
        }
        return None;
    }
    let lower = value.to_ascii_lowercase();
    let rgb_value = if lower.starts_with("rgba:") {
        &value[5..]
    } else if lower.starts_with("rgb:") {
        &value[4..]
    } else {
        value
    };
    let parts: Vec<&str> = rgb_value.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    let r = parse_osc_hex_channel(parts[0])?;
    let g = parse_osc_hex_channel(parts[1])?;
    let b = parse_osc_hex_channel(parts[2])?;
    Some(RgbColor { r, g, b })
}

/// Parse a DEC color scheme report (`CSI ? 997 ; 1|2 n`, possibly repeated).
pub fn parse_terminal_color_scheme_report(data: &str) -> Option<TerminalColorScheme> {
    // /^(?:\x1b\[\?997;(1|2)n)+$/
    let mut rest = data;
    let mut last = None;
    while !rest.is_empty() {
        let r = rest.strip_prefix("\x1b[?997;")?;
        let mode = r.as_bytes().first().copied()?;
        if mode != b'1' && mode != b'2' {
            return None;
        }
        let r = r[1..].strip_prefix('n')?;
        last = Some(mode);
        rest = r;
    }
    match last? {
        b'2' => Some(TerminalColorScheme::Light),
        _ => Some(TerminalColorScheme::Dark),
    }
}
