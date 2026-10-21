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

/// `Number.MAX_SAFE_INTEGER`, used as "no line limit".
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationOptions {
    /// Maximum number of lines (default: 2000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<i64>,
    /// Maximum number of bytes (default: 50KB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
}

fn split_lines_for_counting(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = content.split('\n').collect();
    if content.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn resolve_limits(options: Option<&TruncationOptions>) -> (i64, i64) {
    let max_lines = options
        .and_then(|o| o.max_lines)
        .unwrap_or(DEFAULT_MAX_LINES);
    let max_bytes = options
        .and_then(|o| o.max_bytes)
        .unwrap_or(DEFAULT_MAX_BYTES);
    (max_lines, max_bytes)
}

fn untruncated(content: &str, total_lines: i64, total_bytes: i64, max_lines: i64, max_bytes: i64) -> TruncationResult {
    TruncationResult {
        content: content.to_string(),
        truncated: false,
        truncated_by: None,
        total_lines,
        total_bytes,
        output_lines: total_lines,
        output_bytes: total_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

/// Format bytes as human-readable size.
pub fn format_size(bytes: i64) -> String {
    use crate::core::tools::js_util::js_to_fixed;
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{}KB", js_to_fixed(bytes as f64 / 1024.0, 1))
    } else {
        format!("{}MB", js_to_fixed(bytes as f64 / (1024.0 * 1024.0), 1))
    }
}

/// Truncate content from the head (keep first N lines/bytes).
/// Suitable for file reads where you want to see the beginning.
///
/// Never returns partial lines. If first line exceeds byte limit,
/// returns empty content with firstLineExceedsLimit=true.
pub fn truncate_head(content: &str, options: Option<&TruncationOptions>) -> TruncationResult {
    let (max_lines, max_bytes) = resolve_limits(options);
    let total_bytes = content.len() as i64;
    let lines = split_lines_for_counting(content);
    let total_lines = lines.len() as i64;

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return untruncated(content, total_lines, total_bytes, max_lines, max_bytes);
    }

    // Check if first line alone exceeds byte limit.
    let first_line_bytes = lines.first().map_or(0, |l| l.len()) as i64;
    if first_line_bytes > max_bytes {
        return TruncationResult {
            content: String::new(),
            truncated: true,
            truncated_by: Some(TruncationResultTruncatedBy::Bytes),
            total_lines,
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            last_line_partial: false,
            first_line_exceeds_limit: true,
            max_lines,
            max_bytes,
        };
    }

    // Collect complete lines that fit.
    let mut output: Vec<&str> = Vec::new();
    let mut output_bytes_count: i64 = 0;
    let mut truncated_by = TruncationResultTruncatedBy::Lines;
    for (i, line) in lines.iter().enumerate() {
        if i as i64 >= max_lines {
            break;
        }
        let line_bytes = line.len() as i64 + if i > 0 { 1 } else { 0 };
        if output_bytes_count + line_bytes > max_bytes {
            truncated_by = TruncationResultTruncatedBy::Bytes;
            break;
        }
        output.push(line);
        output_bytes_count += line_bytes;
    }

    if output.len() as i64 >= max_lines && output_bytes_count <= max_bytes {
        truncated_by = TruncationResultTruncatedBy::Lines;
    }

    let output_content = output.join("\n");
    let final_output_bytes = output_content.len() as i64;
    TruncationResult {
        content: output_content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines: output.len() as i64,
        output_bytes: final_output_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

/// Truncate content from the tail (keep last N lines/bytes).
/// Suitable for bash output where you want to see the end (errors, final results).
///
/// May return partial first line if the last line of original content exceeds byte limit.
pub fn truncate_tail(content: &str, options: Option<&TruncationOptions>) -> TruncationResult {
    let (max_lines, max_bytes) = resolve_limits(options);
    let total_bytes = content.len() as i64;
    let lines = split_lines_for_counting(content);
    let total_lines = lines.len() as i64;

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return untruncated(content, total_lines, total_bytes, max_lines, max_bytes);
    }

    // Work backwards from the end.
    let mut output: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut output_bytes_count: i64 = 0;
    let mut truncated_by = TruncationResultTruncatedBy::Lines;
    let mut last_line_partial = false;

    for line in lines.iter().rev() {
        if output.len() as i64 >= max_lines {
            break;
        }
        let line_bytes = line.len() as i64 + if output.is_empty() { 0 } else { 1 };
        if output_bytes_count + line_bytes > max_bytes {
            truncated_by = TruncationResultTruncatedBy::Bytes;
            // Edge case: if we haven't added ANY lines yet and this line exceeds maxBytes,
            // take the end of the line (partial).
            if output.is_empty() {
                let truncated_line = truncate_string_to_bytes_from_end(line, max_bytes.max(0) as usize);
                output_bytes_count = truncated_line.len() as i64;
                output.push_front(truncated_line.to_string());
                last_line_partial = true;
            }
            break;
        }
        output.push_front(line.to_string());
        output_bytes_count += line_bytes;
    }

    if output.len() as i64 >= max_lines && output_bytes_count <= max_bytes {
        truncated_by = TruncationResultTruncatedBy::Lines;
    }

    let output_lines = output.len() as i64;
    let output_content = Vec::from(output).join("\n");
    let final_output_bytes = output_content.len() as i64;
    TruncationResult {
        content: output_content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines,
        output_bytes: final_output_bytes,
        last_line_partial,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

/// Truncate a string to fit within a byte limit (from the end).
/// Handles multi-byte UTF-8 characters correctly.
fn truncate_string_to_bytes_from_end(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
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
    use crate::core::tools::js_util::{utf16_len, utf16_prefix};
    let max_chars = max_chars.unwrap_or(GREP_MAX_LINE_LENGTH).max(0) as usize;
    if utf16_len(line) <= max_chars {
        return TruncateLineResult {
            text: line.to_string(),
            was_truncated: false,
        };
    }
    TruncateLineResult {
        text: format!("{}... [truncated]", utf16_prefix(line, max_chars)),
        was_truncated: true,
    }
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
    let len = content.len();
    let total_lines = split_lines_for_counting(content).len() as i64;
    let max_bytes = max_bytes.max(0) as usize;
    if len <= max_bytes {
        return MiddleTruncationResult {
            content: content.to_string(),
            truncated: false,
            removed_chars: 0,
            total_bytes: len as i64,
            total_lines,
        };
    }
    let mut head_end = max_bytes / 2;
    while head_end > 0 && !content.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = len - (max_bytes - max_bytes / 2);
    while tail_start < len && !content.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let head = &content[..head_end];
    let tail = &content[tail_start..];
    // `Array.from(str).length` counts code points.
    let removed_chars = content[head_end..tail_start].chars().count() as i64;
    MiddleTruncationResult {
        content: format!("{head}…{removed_chars} chars truncated…{tail}"),
        truncated: true,
        removed_chars,
        total_bytes: len as i64,
        total_lines,
    }
}
