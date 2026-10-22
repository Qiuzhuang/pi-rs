//! Port of `packages/coding-agent/src/utils/ansi.ts`.
//!
//! Portions are derived from ansi-regex and strip-ansi
//! (MIT License, Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (https://sindresorhus.com)).

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

// Valid string terminator sequences are BEL, ESC\, and 0x9c
const ST: &str = r"(?:\x07|\x1b\\|\x{9c})";
// OSC sequences: ESC ] ... ST
const OSC_START: &str = r"\x1b\]";
// CSI and related: ESC/C1, optional intermediates, optional params (supports ; and :), then final byte
const CSI_START: &str = r"[\x1b\x{9b}][\[\]()#;?]*(?:[0-9]{1,4}(?:[;:][0-9]{0,4})*)?";
const CSI_FINAL: &str = r"[0-9A-PR-TZcf-nq-uy=><~]";

// Complete sequences. OSC is non-greedy until the first ST.
static ANSI_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("(?:{OSC_START}(?s:.)*?{ST})|{CSI_START}{CSI_FINAL}")).expect("valid regex")
});

// Unfinished sequence at the end of the text: OSC without its ST (a trailing ESC may start ESC\),
// or CSI without its final byte. The TS `\u001B(?!\\)` lookahead is expressed as a run of ESCs
// followed by a plain character or the end of the text.
static UNFINISHED_ANSI_AT_END_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?:{OSC_START}(?:[^\x07\x{{9c}}\x1b]|\x1b+(?:$|[^\\\x07\x{{9c}}\x1b]))*|{CSI_START})$"
    ))
    .expect("valid regex")
});

// Longest unfinished sequence held back while streaming. Longer ones are processed as-is.
const MAX_PENDING_ANSI_LENGTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitIncompleteAnsiSuffixResult {
    pub complete: String,
    pub pending: String,
}

fn has_ansi_introducer(value: &str) -> bool {
    value.contains('\u{1b}') || value.contains('\u{9b}')
}

/// Byte index of the start of the last `units` UTF-16 code units (on a char boundary).
fn utf16_window_start(value: &str, units: usize) -> usize {
    let mut remaining = units;
    let mut start = value.len();
    for (index, ch) in value.char_indices().rev() {
        let width = ch.len_utf16();
        if width > remaining {
            break;
        }
        remaining -= width;
        start = index;
        if remaining == 0 {
            break;
        }
    }
    start
}

/// Split streamed text into a part that is safe to pass to stripAnsi now and a trailing
/// unfinished escape sequence that should be prepended to the next chunk.
pub fn split_incomplete_ansi_suffix(value: &str) -> SplitIncompleteAnsiSuffixResult {
    if !has_ansi_introducer(value) {
        return SplitIncompleteAnsiSuffixResult { complete: value.to_string(), pending: String::new() };
    }
    let window_start = utf16_window_start(value, MAX_PENDING_ANSI_LENGTH);
    let Some(found) = UNFINISHED_ANSI_AT_END_REGEX.find(&value[window_start..]) else {
        return SplitIncompleteAnsiSuffixResult { complete: value.to_string(), pending: String::new() };
    };
    let split_at = window_start + found.start();
    SplitIncompleteAnsiSuffixResult {
        complete: value[..split_at].to_string(),
        pending: value[split_at..].to_string(),
    }
}

/// Remove ANSI escape sequences. (The TS type check for non-string input cannot fail in Rust.)
pub fn strip_ansi(value: &str) -> anyhow::Result<String> {
    // Fast path: ANSI codes require ESC (7-bit) or CSI (8-bit) introducer
    if !has_ansi_introducer(value) {
        return Ok(value.to_string());
    }
    Ok(ANSI_REGEX.replace_all(value, "").into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(value: &str) -> String {
        strip_ansi(value).unwrap()
    }

    fn reference_strip_ansi(value: &str) -> String {
        let st = r"(?:\x07|\x1b\\|\x{9c})";
        let osc = format!(r"(?:\x1b\](?s:.)*?{st})");
        let csi = r"[\x1b\x{9b}][\[\]()#;?]*(?:[0-9]{1,4}(?:[;:][0-9]{0,4})*)?[0-9A-PR-TZcf-nq-uy=><~]";
        let re = Regex::new(&format!("{osc}|{csi}")).unwrap();
        if !value.contains('\u{1b}') && !value.contains('\u{9b}') {
            return value.to_string();
        }
        re.replace_all(value, "").into_owned()
    }

    fn compatibility_inputs() -> Vec<String> {
        let mut inputs: Vec<String> = [
            "plain",
            "a\x1b[31mred\x1b[0mz",
            "a\x1b]8;;https://example.com\x07link\x1b]8;;\x07z",
            "a\x1b]unterminated",
            "a\x1b]funterminated",
            "a\x1bPabc\x1b\\z",
            "a\x1b^abc\x07z",
            "a\x1b_abc\u{9c}z",
            "a\u{90}abc\u{9c}z",
            "a\u{9d}abc\u{9c}z",
            "a\u{9b}31mred",
            "a\x1b(0x",
            "a\x1b*0x",
            "a\x1b+c",
            "a\x1b/0x",
            "a\x1bcok",
            "a\x1b\\ok",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let chars = [
            "a", "f", "0", "1", ";", ":", "[", "]", "(", ")", "#", "?", "m", "P", "_", "\\", "\x07",
            "\x1b", "\u{9b}", "\u{9c}", "\u{90}", "\u{9d}",
        ];
        for ch in chars {
            inputs.push(format!("x\x1b{ch}y"));
            inputs.push(format!("x\u{9b}{ch}y"));
            for other in chars.iter().step_by(3) {
                inputs.push(format!("x\x1b{ch}{other}y"));
            }
        }
        inputs
    }

    #[test]
    fn matches_strip_ansi_for_generated_compatibility_inputs() {
        for input in compatibility_inputs() {
            assert_eq!(strip(&input), reference_strip_ansi(&input), "input {input:?}");
        }
    }

    #[test]
    fn strips_ris_without_leaking_the_final_byte() {
        assert_eq!(strip("\x1bcdone"), "done");
    }

    #[test]
    fn strips_single_byte_esc_sequences_without_leaking_final_bytes() {
        for ch in ('g'..='m').chain('r'..='t') {
            assert_eq!(strip(&format!("\x1b{ch}ok")), "ok");
        }
    }

    #[test]
    fn strips_common_ansi_sequences_used_in_tool_output() {
        let input = "a\x1b[31mred\x1b[0m\x1b]8;;https://example.com\x07link\x1b]8;;\x07z";
        assert_eq!(strip(input), "aredlinkz");
    }

    fn split(value: &str) -> (String, String) {
        let r = split_incomplete_ansi_suffix(value);
        (r.complete, r.pending)
    }

    #[test]
    fn split_holds_back_unfinished_sequences() {
        assert_eq!(split("hello"), ("hello".into(), "".into()));
        assert_eq!(split("text\x1b[31"), ("text".into(), "\x1b[31".into()));
        assert_eq!(split("text\x1b"), ("text".into(), "\x1b".into()));
        assert_eq!(split("text\x1b]0;title\x1b"), ("text".into(), "\x1b]0;title\x1b".into()));
        assert_eq!(split("a\x1b]0;t\x1b\\"), ("a\x1b]0;t\x1b\\".into(), "".into()));
        let long = format!("\x1b]8;;{}", "x".repeat(400));
        assert_eq!(split(&long), (long.clone(), "".into()));
    }
}
