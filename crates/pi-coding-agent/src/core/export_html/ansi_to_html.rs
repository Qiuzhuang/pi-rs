//! Port of `packages/coding-agent/src/core/export-html/ansi-to-html.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Convert ANSI-escaped text to HTML with inline styles.
pub fn ansi_to_html(text: &str) -> String {
    todo!("port core/export-html/ansi-to-html.ts:ansiToHtml")
}

/// Convert array of ANSI-escaped lines to HTML.
/// Each line is wrapped in a div element.
pub fn ansi_lines_to_html(lines: &[String]) -> String {
    todo!("port core/export-html/ansi-to-html.ts:ansiLinesToHtml")
}
