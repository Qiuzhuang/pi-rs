//! Port of `packages/coding-agent/src/core/tools/truncate.ts`.

use serde::{Deserialize, Serialize};

/// Shared truncation utilities for tool outputs.
///
/// Truncation is based on two independent limits - whichever is hit first wins:
/// - Line limit (default: 2000 lines)
/// - Byte limit (default: 50KB)
///
/// Never returns partial lines (except bash tail truncation edge case).
pub const DEFAULT_MAX_LINES: i64 = 2000;

pub const DEFAULT_MAX_BYTES: i64 = 51200;

pub const GREP_MAX_LINE_LENGTH: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TruncationResultTruncatedBy {
    #[serde(rename = "lines")]
    Lines,
    #[serde(rename = "bytes")]
    Bytes,
}

impl TruncationResultTruncatedBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lines => "lines",
            Self::Bytes => "bytes",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationResult {
    /// The truncated content
    pub content: String,
    /// Whether truncation occurred
    pub truncated: bool,
    /// Which limit was hit: "lines", "bytes", or null if not truncated
    #[serde(default)]
    pub truncated_by: Option<TruncationResultTruncatedBy>,
    /// Total number of lines in the original content
    pub total_lines: i64,
    /// Total number of bytes in the original content
    pub total_bytes: i64,
    /// Number of complete lines in the truncated output
    pub output_lines: i64,
    /// Number of bytes in the truncated output
    pub output_bytes: i64,
    /// Whether the last line was partially truncated (only for tail truncation edge case)
    pub last_line_partial: bool,
    /// Whether the first line exceeded the byte limit (for head truncation)
    pub first_line_exceeds_limit: bool,
    /// The max lines limit that was applied
    pub max_lines: i64,
    /// The max bytes limit that was applied
    pub max_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationOptions {
    /// Maximum number of lines (default: 2000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<i64>,
    /// Maximum number of bytes (default: 50KB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
}

/// Format bytes as human-readable size.
pub fn format_size(bytes: i64) -> String {
    todo!("port core/tools/truncate.ts:formatSize")
}

/// Truncate content from the head (keep first N lines/bytes).
/// Suitable for file reads where you want to see the beginning.
///
/// Never returns partial lines. If first line exceeds byte limit,
/// returns empty content with firstLineExceedsLimit=true.
pub fn truncate_head(content: &str, options: Option<&TruncationOptions>) -> TruncationResult {
    todo!("port core/tools/truncate.ts:truncateHead")
}

/// Truncate content from the tail (keep last N lines/bytes).
/// Suitable for bash output where you want to see the end (errors, final results).
///
/// May return partial first line if the last line of original content exceeds byte limit.
pub fn truncate_tail(content: &str, options: Option<&TruncationOptions>) -> TruncationResult {
    todo!("port core/tools/truncate.ts:truncateTail")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncateLineResult {
    pub text: String,
    pub was_truncated: bool,
}

/// Truncate a single line to max characters, adding [truncated] suffix.
/// Used for grep match lines.
pub fn truncate_line(line: &str, max_chars: Option<i64>) -> TruncateLineResult {
    todo!("port core/tools/truncate.ts:truncateLine")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddleTruncationResult {
    /// The start and end of the content with a `…N chars truncated…` marker between them.
    pub content: String,
    pub truncated: bool,
    /// Characters left out.
    pub removed_chars: i64,
    pub total_bytes: i64,
    pub total_lines: i64,
}

/// Keep the start and the end of `content`, half of `maxBytes` each, and replace the middle with a
/// `…N chars truncated…` marker, like Codex does for tool output. Cuts only at character boundaries.
pub fn truncate_middle(content: &str, max_bytes: i64) -> MiddleTruncationResult {
    todo!("port core/tools/truncate.ts:truncateMiddle")
}
