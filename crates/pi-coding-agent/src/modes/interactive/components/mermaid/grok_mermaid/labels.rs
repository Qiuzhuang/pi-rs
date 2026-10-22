//! Label text handling (grok-mermaid `labels.ts`).

use super::width::{measured, string_width};
use pi_tui::utils::{is_whitespace_ch, js_trim};

/// Node labels wrap to at most this many display columns per line ...
pub(crate) const WRAP_WIDTH: usize = 24;
/// ... and at most this many lines; overflow is truncated with an ellipsis.
pub(crate) const MAX_LINES: usize = 4;
/// Edge labels are truncated to this many columns.
pub(crate) const MAX_LABEL: usize = 28;

/// Identifier-boundary characters preferred as break points when a single word
/// is too wide to fit, so it is not sliced mid-segment.
pub(crate) const LABEL_BREAK_CHARS: [char; 4] = ['_', '-', '.', '/'];

/// ASCII-only case folding, matching Rust's `to_ascii_lowercase`.
pub(crate) fn ascii_lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

pub(crate) fn ascii_upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

/// C0 and C1 controls, less the `\t\n\r` the parsers and `src_lines` read.
fn is_control(c: char) -> bool {
    matches!(c, '\0'..='\x08' | '\x0b' | '\x0c' | '\x0e'..='\x1f' | '\x7f'..='\u{9f}')
}

/// Applied by every public entry point that takes untrusted source.
pub(crate) fn strip_controls(src: &str) -> String {
    src.chars().filter(|c| !is_control(*c)).collect()
}

/// Split source into lines the way Rust's `str::lines()` does.
pub(crate) fn src_lines(src: &str) -> Vec<String> {
    let mut out: Vec<String> =
        src.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
    if out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

/// Matches Rust's `char::is_alphanumeric` (`[\p{Alphabetic}\p{N}]`).
pub(crate) fn is_alphanumeric(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric()
}

/// Characters allowed in a bare node/state/class identifier.
pub(crate) fn is_id_char(c: char) -> bool {
    is_alphanumeric(c) || c == '_'
}

const ENTITY_LOOKAHEAD: usize = 10;

fn decode_entity_body(body: &str) -> Option<char> {
    match body {
        "lt" => return Some('<'),
        "gt" => return Some('>'),
        "amp" => return Some('&'),
        "quot" => return Some('"'),
        "apos" => return Some('\''),
        _ => {}
    }
    let num = body.strip_prefix('#')?;
    let hex = num.starts_with('x') || num.starts_with('X');
    let digits = if hex { &num[1..] } else { num };
    let valid = !digits.is_empty()
        && if hex { digits.chars().all(|c| c.is_ascii_hexdigit()) } else { digits.chars().all(|c| c.is_ascii_digit()) };
    if !valid {
        return None;
    }
    // Overflow means far beyond the code point range.
    let code = u64::from_str_radix(digits, if hex { 16 } else { 10 }).unwrap_or(u64::MAX);
    // Surrogates and out-of-range values are not characters at all.
    if code > 0x10ffff || (0xd800..=0xdfff).contains(&code) {
        return None;
    }
    // Reject control chars: NUL collides with the CONT sentinel and ESC would
    // inject ANSI into scrollback.
    if code < 0x20 || (0x7f..=0x9f).contains(&code) {
        return None;
    }
    char::from_u32(code as u32)
}

/// Decode HTML entities in label text.
pub(crate) fn decode_html_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // Scan a bounded window including the terminating `;`.
        let hi = (i + 1 + ENTITY_LOOKAHEAD).min(chars.len());
        let semi = (i + 1..hi).find(|&j| chars[j] == ';');
        let decoded = semi.and_then(|semi| decode_entity_body(&chars[i + 1..semi].iter().collect::<String>()));
        match (decoded, semi) {
            (Some(decoded), Some(semi)) => {
                // Resume past the `;`. The single pass never re-scans emitted text.
                out.push(decoded);
                i = semi + 1;
            }
            _ => {
                out.push('&');
                i += 1;
            }
        }
    }
    out
}

/// Strip markdown emphasis from a `` `backtick` `` label string.
pub(crate) fn strip_markdown(s: &str) -> String {
    let no_code: String = s.chars().filter(|&c| c != '`').collect();
    let no_strong = no_code.replace("**", "").replace("__", "");
    let chars: Vec<char> = no_strong.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        // Keep `*`/`_` only when they sit inside a word, so snake_case survives.
        let in_word = i > 0 && is_alphanumeric(chars[i - 1]) && chars.get(i + 1).is_some_and(|&n| is_alphanumeric(n));
        if (c == '*' || c == '_') && !in_word {
            continue;
        }
        out.push(c);
    }
    js_trim(&out).to_string()
}

/// Inline formatting tags that carry no meaning in a terminal.
const HTML_FORMAT_TAGS: [&str; 25] = [
    "b", "strong", "i", "em", "u", "s", "strike", "del", "ins", "mark", "small", "big", "sub", "sup", "code",
    "kbd", "samp", "var", "tt", "span", "font", "q", "abbr", "cite", "pre",
];

/// Read a tag starting at `start`, returning its name and the index after `>`.
fn html_tag_at(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut i = start + 1;
    if chars.get(i) == Some(&'/') {
        i += 1;
    }
    let name_start = i;
    while i < chars.len() && chars[i].is_ascii_alphanumeric() {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name: String = chars[name_start..i].iter().collect();
    while i < chars.len() && chars[i] != '>' {
        if chars[i] == '<' {
            return None;
        }
        i += 1;
    }
    (chars.get(i) == Some(&'>')).then_some((name, i + 1))
}

pub(crate) fn strip_html_tags(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<'
            && let Some((name, end)) = html_tag_at(&chars, i)
        {
            let lower = name.to_lowercase();
            if lower == "br" {
                out.push(' ');
                i = end;
                continue;
            }
            if HTML_FORMAT_TAGS.contains(&lower.as_str()) {
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Strip one matching pair of wrapping delimiters, if present.
fn unwrap<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    (s.len() >= open.len() + close.len() && s.starts_with(open) && s.ends_with(close))
        .then(|| &s[open.len()..s.len() - close.len()])
}

/// Normalise raw label text: strip markup, unquote, and decode entities.
pub(crate) fn clean_label(raw: &str) -> String {
    let stripped = strip_html_tags(js_trim(raw));
    let trimmed = js_trim(&stripped);
    let unquoted = js_trim(unwrap(trimmed, "\"", "\"").or_else(|| unwrap(trimmed, "'", "'")).unwrap_or(trimmed));
    match unwrap(unquoted, "`", "`") {
        None => decode_html_entities(unquoted),
        Some(md) => decode_html_entities(&strip_markdown(js_trim(md))),
    }
}

/// Byte index of the last identifier-boundary character.
fn last_break(s: &str) -> Option<usize> {
    s.rfind(LABEL_BREAK_CHARS)
}

/// JS `s.split(/\s+/).filter(w => w !== '')`.
pub(crate) fn split_ws(s: &str) -> Vec<&str> {
    s.split(is_whitespace_ch).filter(|w| !w.is_empty()).collect()
}

/// Wrap a label to `width` columns over at most `max_lines` lines, truncating the
/// last line with an ellipsis if it overflows.
pub(crate) fn wrap_label(label: &str, width: usize, max_lines: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;

    for word in split_ws(label) {
        let ww = string_width(word);
        if ww > width {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            let mut chunk = String::new();
            let mut chunk_w = 0;
            for (ch, cw) in measured(word) {
                if chunk_w + cw > width && !chunk.is_empty() {
                    match last_break(&chunk) {
                        None => {
                            lines.push(std::mem::take(&mut chunk));
                        }
                        Some(p) => {
                            let carry = chunk[p + 1..].to_string();
                            lines.push(chunk[..p + 1].to_string());
                            chunk = carry;
                        }
                    }
                    chunk_w = string_width(&chunk);
                }
                chunk.push_str(ch);
                chunk_w += cw;
            }
            cur = chunk;
            cur_w = chunk_w;
        } else if cur.is_empty() {
            cur = word.to_string();
            cur_w = ww;
        } else if cur_w + 1 + ww <= width {
            cur.push(' ');
            cur.push_str(word);
            cur_w += 1 + ww;
        } else {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
            cur_w = ww;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }

    if lines.len() > max_lines {
        lines.truncate(max_lines);
        let target = width.saturating_sub(1).max(1);
        let mut s = String::new();
        let mut sw = 0;
        if let Some(last) = lines.last() {
            for (ch, cw) in measured(last) {
                if sw + cw > target {
                    break;
                }
                s.push_str(ch);
                sw += cw;
            }
        }
        if let Some(last) = lines.last_mut() {
            *last = format!("{s}…");
        }
    }
    lines
}

/// Truncate to `inner` columns, leaving room for the ellipsis.
pub(crate) fn fit_label(label: &str, inner: usize) -> String {
    if string_width(label) <= inner {
        return label.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for (c, cw) in measured(label) {
        if used + cw + 1 > inner {
            break;
        }
        out.push_str(c);
        used += cw;
    }
    format!("{out}…")
}
