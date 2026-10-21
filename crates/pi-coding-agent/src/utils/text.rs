//! Port of `packages/coding-agent/src/utils/text.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitBomResult {
    pub bom: String,
    pub text: String,
}

/// Split a leading UTF-8 byte order mark from decoded text.
pub fn split_bom(content: &str) -> SplitBomResult {
    match content.strip_prefix('\u{FEFF}') {
        Some(text) => SplitBomResult {
            bom: "\u{FEFF}".to_string(),
            text: text.to_string(),
        },
        None => SplitBomResult {
            bom: String::new(),
            text: content.to_string(),
        },
    }
}

/// Remove a leading UTF-8 byte order mark from decoded text.
pub fn strip_bom(content: &str) -> String {
    content
        .strip_prefix('\u{FEFF}')
        .unwrap_or(content)
        .to_string()
}
