//! Port of `packages/coding-agent/src/core/export-html/ansi-to-html.ts`.
//!
//! ANSI escape code to HTML converter.
//!
//! Converts terminal ANSI color/style codes to HTML with inline styles.
//! Supports:
//! - Standard foreground colors (30-37) and bright variants (90-97)
//! - Standard background colors (40-47) and bright variants (100-107)
//! - 256-color palette (38;5;N and 48;5;N)
//! - RGB true color (38;2;R;G;B and 48;2;R;G;B)
//! - Text styles: bold (1), dim (2), italic (3), underline (4)
//! - Reset (0)

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Standard ANSI color palette (0-15)
const ANSI_COLORS: [&str; 16] = [
    "#000000", // 0: black
    "#800000", // 1: red
    "#008000", // 2: green
    "#808000", // 3: yellow
    "#000080", // 4: blue
    "#800080", // 5: magenta
    "#008080", // 6: cyan
    "#c0c0c0", // 7: white
    "#808080", // 8: bright black
    "#ff0000", // 9: bright red
    "#00ff00", // 10: bright green
    "#ffff00", // 11: bright yellow
    "#0000ff", // 12: bright blue
    "#ff00ff", // 13: bright magenta
    "#00ffff", // 14: bright cyan
    "#ffffff", // 15: bright white
];

/// Convert 256-color index to hex.
fn color256_to_hex(index: u64) -> String {
    // Standard colors (0-15)
    if index < 16 {
        return ANSI_COLORS[index as usize].to_string();
    }

    // Color cube (16-231): 6x6x6 = 216 colors
    if index < 232 {
        let cube_index = index - 16;
        let r = cube_index / 36;
        let g = (cube_index % 36) / 6;
        let b = cube_index % 6;
        let to_component = |n: u64| if n == 0 { 0 } else { 55 + n * 40 };
        return format!(
            "#{:02x}{:02x}{:02x}",
            to_component(r),
            to_component(g),
            to_component(b)
        );
    }

    // Grayscale (232-255): 24 shades
    let gray = 8 + (index - 232) * 10;
    format!("#{gray:02x}{gray:02x}{gray:02x}")
}

/// Escape HTML special characters.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    #[serde(default)]
    pub fg: Option<String>,
    #[serde(default)]
    pub bg: Option<String>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

fn style_to_inline_css(style: &TextStyle) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(fg) = style.fg.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("color:{fg}"));
    }
    if let Some(bg) = style.bg.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("background-color:{bg}"));
    }
    if style.bold {
        parts.push("font-weight:bold".into());
    }
    if style.dim {
        parts.push("opacity:0.6".into());
    }
    if style.italic {
        parts.push("font-style:italic".into());
    }
    if style.underline {
        parts.push("text-decoration:underline".into());
    }
    parts.join(";")
}

fn has_style(style: &TextStyle) -> bool {
    style.fg.is_some() || style.bg.is_some() || style.bold || style.dim || style.italic || style.underline
}

/// Parse an extended color (`5;N` or `2;R;G;B`) starting after the 38/48 code at `i`.
/// Returns the color and how many extra params it consumed.
fn extended_color(params: &[u64], i: usize) -> Option<(String, usize)> {
    match params.get(i + 1) {
        Some(5) if params.len() > i + 2 => Some((color256_to_hex(params[i + 2]), 2)),
        Some(2) if params.len() > i + 4 => Some((
            format!("rgb({},{},{})", params[i + 2], params[i + 3], params[i + 4]),
            4,
        )),
        _ => None,
    }
}

/// Parse ANSI SGR (Select Graphic Rendition) codes and update style.
fn apply_sgr_code(params: &[u64], style: &mut TextStyle) {
    let mut i = 0;
    while i < params.len() {
        let code = params[i];
        match code {
            0 => *style = TextStyle::default(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            // Reset bold/dim
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            // Standard foreground colors
            30..=37 => style.fg = Some(ANSI_COLORS[(code - 30) as usize].to_string()),
            // Extended foreground color
            38 => {
                if let Some((color, consumed)) = extended_color(params, i) {
                    style.fg = Some(color);
                    i += consumed;
                }
            }
            // Default foreground
            39 => style.fg = None,
            // Standard background colors
            40..=47 => style.bg = Some(ANSI_COLORS[(code - 40) as usize].to_string()),
            // Extended background color
            48 => {
                if let Some((color, consumed)) = extended_color(params, i) {
                    style.bg = Some(color);
                    i += consumed;
                }
            }
            // Default background
            49 => style.bg = None,
            // Bright foreground colors
            90..=97 => style.fg = Some(ANSI_COLORS[(code - 90 + 8) as usize].to_string()),
            // Bright background colors
            100..=107 => style.bg = Some(ANSI_COLORS[(code - 100 + 8) as usize].to_string()),
            // Ignore unrecognized codes
            _ => {}
        }
        i += 1;
    }
}

/// Match ANSI escape sequences: ESC[ followed by params and ending with 'm'
static ANSI_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\x1b\[([\d;]*)m").expect("valid regex"));

/// `parseInt(p, 10) || 0` for a run of ASCII digits.
fn parse_param(param: &str) -> u64 {
    if param.is_empty() {
        return 0;
    }
    param.bytes().fold(0u64, |acc, b| {
        acc.saturating_mul(10).saturating_add(u64::from(b.wrapping_sub(b'0')))
    })
}

/// Convert ANSI-escaped text to HTML with inline styles.
pub fn ansi_to_html(text: &str) -> String {
    let mut style = TextStyle::default();
    let mut result = String::new();
    let mut last_index = 0;
    let mut in_span = false;

    for captures in ANSI_REGEX.captures_iter(text) {
        let whole = captures.get(0).expect("match");
        // Add text before this escape sequence
        let before_text = &text[last_index..whole.start()];
        if !before_text.is_empty() {
            result.push_str(&escape_html(before_text));
        }

        // Parse SGR parameters
        let param_str = captures.get(1).map(|m| m.as_str()).unwrap_or("");
        let params: Vec<u64> = if param_str.is_empty() {
            vec![0]
        } else {
            param_str.split(';').map(parse_param).collect()
        };

        // Close existing span if we have one
        if in_span {
            result.push_str("</span>");
            in_span = false;
        }

        // Apply the codes
        apply_sgr_code(&params, &mut style);

        // Open new span if we have any styling
        if has_style(&style) {
            result.push_str(&format!("<span style=\"{}\">", style_to_inline_css(&style)));
            in_span = true;
        }

        last_index = whole.end();
    }

    // Add remaining text
    let remaining_text = &text[last_index..];
    if !remaining_text.is_empty() {
        result.push_str(&escape_html(remaining_text));
    }

    // Close any open span
    if in_span {
        result.push_str("</span>");
    }

    result
}

/// Convert array of ANSI-escaped lines to HTML.
/// Each line is wrapped in a div element.
pub fn ansi_lines_to_html(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| {
            let html = ansi_to_html(line);
            format!(
                "<div class=\"ansi-line\">{}</div>",
                if html.is_empty() { "&nbsp;" } else { &html }
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_escaped() {
        assert_eq!(
            ansi_to_html("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#039;&amp;&#039;&lt;/a&gt;"
        );
    }

    #[test]
    fn standard_and_bright_colors() {
        assert_eq!(
            ansi_to_html("\x1b[31mred\x1b[0m plain"),
            "<span style=\"color:#800000\">red</span> plain"
        );
        assert_eq!(
            ansi_to_html("\x1b[1;94mx\x1b[22m"),
            "<span style=\"color:#0000ff;font-weight:bold\">x</span><span style=\"color:#0000ff\"></span>"
        );
        assert_eq!(
            ansi_to_html("\x1b[103my\x1b[m"),
            "<span style=\"background-color:#ffff00\">y</span>"
        );
    }

    #[test]
    fn extended_colors() {
        assert_eq!(color256_to_hex(196), "#ff0000");
        assert_eq!(color256_to_hex(16), "#000000");
        assert_eq!(color256_to_hex(232), "#080808");
        assert_eq!(color256_to_hex(255), "#eeeeee");
        assert_eq!(color256_to_hex(9), "#ff0000");
        assert_eq!(
            ansi_to_html("\x1b[38;2;1;2;3;48;5;21;3;4;2mz"),
            "<span style=\"color:rgb(1,2,3);background-color:#0000ff;opacity:0.6;font-style:italic;text-decoration:underline\">z</span>"
        );
        assert_eq!(
            ansi_to_html("\x1b[38;5;100mq\x1b[39m"),
            "<span style=\"color:#878700\">q</span>"
        );
    }

    #[test]
    fn lines_wrap_in_divs_and_blank_lines_get_nbsp() {
        assert_eq!(
            ansi_lines_to_html(&["a".into(), "".into(), "\x1b[0m".into()]),
            "<div class=\"ansi-line\">a</div><div class=\"ansi-line\">&nbsp;</div><div class=\"ansi-line\">&nbsp;</div>"
        );
    }
}
