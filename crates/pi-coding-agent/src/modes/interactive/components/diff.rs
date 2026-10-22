//! Port of `packages/coding-agent/src/modes/interactive/components/diff.ts`.

use crate::core::tools::jsdiff::{Change, diff_token_components};
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use pi_tui::utils::{is_whitespace_ch, js_trim};
use serde::{Deserialize, Serialize};

struct ParsedDiffLine<'a> {
    prefix: char,
    line_num: &'a str,
    content: &'a str,
}

/// Parse diff line to extract prefix, line number, and content.
/// Format: "+123 content" or "-123 content" or " 123 content" or "     ..."
fn parse_diff_line(line: &str) -> Option<ParsedDiffLine<'_>> {
    // /^([+-\s])(\s*\d*)\s(.*)$/ (`.` does not match line terminators)
    let mut chars = line.char_indices();
    let (_, prefix) = chars.next()?;
    if !(prefix == '+' || prefix == '-' || is_whitespace_ch(prefix)) {
        return None;
    }
    let rest = &line[prefix.len_utf8()..];
    if rest.chars().any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')) {
        return None;
    }
    // `\s*\d*` is greedy, then one `\s` is required; backtrack as the regex would.
    let ws_end = rest.char_indices().find(|(_, c)| !is_whitespace_ch(*c)).map_or(rest.len(), |(i, _)| i);
    let digits_end = rest[ws_end..].char_indices().find(|(_, c)| !c.is_ascii_digit()).map_or(rest.len(), |(i, _)| ws_end + i);
    // Try the longest `\s*\d*` first, then shorter ones.
    let mut candidates: Vec<usize> = Vec::new();
    let mut i = digits_end;
    loop {
        candidates.push(i);
        if i == 0 {
            break;
        }
        // Step back one char.
        let prev = rest[..i].chars().next_back().map(char::len_utf8).unwrap_or(1);
        i -= prev;
    }
    for end in candidates {
        if let Some(c) = rest[end..].chars().next()
            && is_whitespace_ch(c)
        {
            // The group must still match `\s*\d*`: digits may only follow the whitespace run.
            let group = &rest[..end];
            let ws_len = group.char_indices().find(|(_, c)| !is_whitespace_ch(*c)).map_or(group.len(), |(i, _)| i);
            if group[ws_len..].chars().all(|c| c.is_ascii_digit()) {
                return Some(ParsedDiffLine { prefix, line_num: group, content: &rest[end + c.len_utf8()..] });
            }
        }
    }
    None
}

/// Replace tabs with spaces for consistent rendering.
fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
}

// ---------------------------------------------------------------------------
// jsdiff `diffWords` (diff 8.0.4, without an Intl segmenter)
// ---------------------------------------------------------------------------

fn is_extended_word_char(c: char) -> bool {
    matches!(c,
        'a'..='z' | 'A'..='Z' | '0'..='9' | '_'
        | '\u{AD}'
        | '\u{C0}'..='\u{D6}'
        | '\u{D8}'..='\u{F6}'
        | '\u{F8}'..='\u{2C6}'
        | '\u{2C8}'..='\u{2D7}'
        | '\u{2DE}'..='\u{2FF}'
        | '\u{1E00}'..='\u{1EFF}')
}

fn has_ws(s: &str) -> bool {
    s.chars().any(is_whitespace_ch)
}

/// `WordDiff.tokenize`.
fn tokenize_words(value: &str) -> Vec<String> {
    // value.match(/[W]+|\s+|[^W]/ug)
    let mut parts: Vec<&str> = Vec::new();
    let mut iter = value.char_indices().peekable();
    while let Some((start, c)) = iter.next() {
        let mut end = start + c.len_utf8();
        if is_extended_word_char(c) {
            while let Some(&(i, n)) = iter.peek() {
                if !is_extended_word_char(n) {
                    break;
                }
                end = i + n.len_utf8();
                iter.next();
            }
        } else if is_whitespace_ch(c) {
            while let Some(&(i, n)) = iter.peek() {
                if !is_whitespace_ch(n) {
                    break;
                }
                end = i + n.len_utf8();
                iter.next();
            }
        }
        parts.push(&value[start..end]);
    }
    let mut tokens: Vec<String> = Vec::new();
    let mut prev_part: Option<&str> = None;
    for part in parts {
        if has_ws(part) {
            match prev_part {
                None => tokens.push(part.to_string()),
                Some(_) => {
                    let last = tokens.pop().unwrap_or_default();
                    tokens.push(last + part);
                }
            }
        } else if let Some(prev) = prev_part.filter(|p| has_ws(p)) {
            if tokens.last().map(String::as_str) == Some(prev) {
                let last = tokens.pop().unwrap_or_default();
                tokens.push(last + part);
            } else {
                tokens.push(format!("{prev}{part}"));
            }
        } else {
            tokens.push(part.to_string());
        }
        prev_part = Some(part);
    }
    tokens
}

/// `WordDiff.join`.
fn join_words(tokens: &[String]) -> String {
    let mut out = String::new();
    for (i, token) in tokens.iter().enumerate() {
        if i == 0 {
            out.push_str(token);
        } else {
            out.push_str(token.trim_start_matches(is_whitespace_ch));
        }
    }
    out
}

fn leading_ws(s: &str) -> &str {
    let end = s.char_indices().find(|(_, c)| !is_whitespace_ch(*c)).map_or(s.len(), |(i, _)| i);
    &s[..end]
}

fn trailing_ws(s: &str) -> &str {
    let start = s.char_indices().rev().find(|(_, c)| !is_whitespace_ch(*c)).map_or(0, |(i, c)| i + c.len_utf8());
    &s[start..]
}

fn longest_common_prefix(a: &str, b: &str) -> String {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x).collect()
}

fn longest_common_suffix(a: &str, b: &str) -> String {
    let mut out: Vec<char> = a.chars().rev().zip(b.chars().rev()).take_while(|(x, y)| x == y).map(|(x, _)| x).collect();
    out.reverse();
    out.into_iter().collect()
}

fn replace_prefix(s: &str, old_prefix: &str, new_prefix: &str) -> String {
    match s.strip_prefix(old_prefix) {
        Some(rest) => format!("{new_prefix}{rest}"),
        // jsdiff throws here ("this is a bug"); keep the string unchanged.
        None => s.to_string(),
    }
}

fn replace_suffix(s: &str, old_suffix: &str, new_suffix: &str) -> String {
    if old_suffix.is_empty() {
        return format!("{s}{new_suffix}");
    }
    match s.strip_suffix(old_suffix) {
        Some(rest) => format!("{rest}{new_suffix}"),
        None => s.to_string(),
    }
}

fn remove_prefix(s: &str, p: &str) -> String {
    replace_prefix(s, p, "")
}

fn remove_suffix(s: &str, p: &str) -> String {
    replace_suffix(s, p, "")
}

/// `maximumOverlap(string1, string2)`: the longest suffix of `a` that is a prefix of `b`.
fn maximum_overlap(a: &str, b: &str) -> String {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let max = a.len().min(b.len());
    for len in (0..=max).rev() {
        if a[a.len() - len..] == b[..len] {
            return b[..len].iter().collect();
        }
    }
    String::new()
}

fn dedupe_whitespace_in_change_objects(
    changes: &mut [Change],
    start_keep: Option<usize>,
    deletion: Option<usize>,
    insertion: Option<usize>,
    end_keep: Option<usize>,
) {
    if let (Some(del), Some(ins)) = (deletion, insertion) {
        let old_ws_prefix = leading_ws(&changes[del].value).to_string();
        let old_ws_suffix = trailing_ws(&changes[del].value).to_string();
        let new_ws_prefix = leading_ws(&changes[ins].value).to_string();
        let new_ws_suffix = trailing_ws(&changes[ins].value).to_string();
        if let Some(sk) = start_keep {
            let common = longest_common_prefix(&old_ws_prefix, &new_ws_prefix);
            changes[sk].value = replace_suffix(&changes[sk].value, &new_ws_prefix, &common);
            changes[del].value = remove_prefix(&changes[del].value, &common);
            changes[ins].value = remove_prefix(&changes[ins].value, &common);
        }
        if let Some(ek) = end_keep {
            let common = longest_common_suffix(&old_ws_suffix, &new_ws_suffix);
            changes[ek].value = replace_prefix(&changes[ek].value, &new_ws_suffix, &common);
            changes[del].value = remove_suffix(&changes[del].value, &common);
            changes[ins].value = remove_suffix(&changes[ins].value, &common);
        }
    } else if let Some(ins) = insertion {
        if start_keep.is_some() {
            let ws = leading_ws(&changes[ins].value).len();
            changes[ins].value = changes[ins].value[ws..].to_string();
        }
        if let Some(ek) = end_keep {
            let ws = leading_ws(&changes[ek].value).len();
            changes[ek].value = changes[ek].value[ws..].to_string();
        }
    } else if let (Some(sk), Some(ek), Some(del)) = (start_keep, end_keep, deletion) {
        let new_ws_full = leading_ws(&changes[ek].value).to_string();
        let del_ws_start = leading_ws(&changes[del].value).to_string();
        let del_ws_end = trailing_ws(&changes[del].value).to_string();
        let new_ws_start = longest_common_prefix(&new_ws_full, &del_ws_start);
        changes[del].value = remove_prefix(&changes[del].value, &new_ws_start);
        let new_ws_end = longest_common_suffix(&remove_prefix(&new_ws_full, &new_ws_start), &del_ws_end);
        changes[del].value = remove_suffix(&changes[del].value, &new_ws_end);
        changes[ek].value = replace_prefix(&changes[ek].value, &new_ws_full, &new_ws_end);
        let keep_len = new_ws_full.len() - new_ws_end.len();
        changes[sk].value = replace_suffix(&changes[sk].value, &new_ws_full, &new_ws_full[..keep_len]);
    } else if let (Some(ek), Some(del)) = (end_keep, deletion) {
        let end_keep_ws_prefix = leading_ws(&changes[ek].value).to_string();
        let deletion_ws_suffix = trailing_ws(&changes[del].value).to_string();
        let overlap = maximum_overlap(&deletion_ws_suffix, &end_keep_ws_prefix);
        changes[del].value = remove_suffix(&changes[del].value, &overlap);
    } else if let (Some(sk), Some(del)) = (start_keep, deletion) {
        let start_keep_ws_suffix = trailing_ws(&changes[sk].value).to_string();
        let deletion_ws_prefix = leading_ws(&changes[del].value).to_string();
        let overlap = maximum_overlap(&start_keep_ws_suffix, &deletion_ws_prefix);
        changes[del].value = remove_prefix(&changes[del].value, &overlap);
    }
}

/// jsdiff `diffWords(oldStr, newStr)` with default options.
pub(crate) fn diff_words(old_str: &str, new_str: &str) -> Vec<Change> {
    let old: Vec<String> = tokenize_words(old_str).into_iter().filter(|t| !t.is_empty()).collect();
    let new: Vec<String> = tokenize_words(new_str).into_iter().filter(|t| !t.is_empty()).collect();
    let equals = |a: &String, b: &String| js_trim(a) == js_trim(b);
    let components = diff_token_components(&old, &new, &equals);
    let mut changes = Vec::with_capacity(components.len());
    let (mut new_pos, mut old_pos) = (0usize, 0usize);
    for (count, added, removed) in components {
        let value = if !removed {
            let value = join_words(&new[new_pos..new_pos + count]);
            new_pos += count;
            if !added {
                old_pos += count;
            }
            value
        } else {
            let value = join_words(&old[old_pos..old_pos + count]);
            old_pos += count;
            value
        };
        changes.push(Change { value, added, removed, count });
    }

    // postProcess
    let mut last_keep: Option<usize> = None;
    let mut insertion: Option<usize> = None;
    let mut deletion: Option<usize> = None;
    for i in 0..changes.len() {
        if changes[i].added {
            insertion = Some(i);
        } else if changes[i].removed {
            deletion = Some(i);
        } else {
            if insertion.is_some() || deletion.is_some() {
                dedupe_whitespace_in_change_objects(&mut changes, last_keep, deletion, insertion, Some(i));
            }
            last_keep = Some(i);
            insertion = None;
            deletion = None;
        }
    }
    if insertion.is_some() || deletion.is_some() {
        dedupe_whitespace_in_change_objects(&mut changes, last_keep, deletion, insertion, None);
    }
    changes
}

/// Compute word-level diff and render with inverse on changed parts.
/// Uses diffWords which groups whitespace with adjacent words for cleaner highlighting.
/// Strips leading whitespace from inverse to avoid highlighting indentation.
fn render_intra_line_diff(old_content: &str, new_content: &str) -> (String, String) {
    let t = theme();
    let word_diff = diff_words(old_content, new_content);

    let mut removed_line = String::new();
    let mut added_line = String::new();
    let mut is_first_removed = true;
    let mut is_first_added = true;

    for part in &word_diff {
        if part.removed {
            let mut value = part.value.as_str();
            // Strip leading whitespace from the first removed part
            if is_first_removed {
                let leading = leading_ws(value);
                removed_line.push_str(leading);
                value = &value[leading.len()..];
                is_first_removed = false;
            }
            if !value.is_empty() {
                removed_line.push_str(&t.inverse(value));
            }
        } else if part.added {
            let mut value = part.value.as_str();
            // Strip leading whitespace from the first added part
            if is_first_added {
                let leading = leading_ws(value);
                added_line.push_str(leading);
                value = &value[leading.len()..];
                is_first_added = false;
            }
            if !value.is_empty() {
                added_line.push_str(&t.inverse(value));
            }
        } else {
            removed_line.push_str(&part.value);
            added_line.push_str(&part.value);
        }
    }

    (removed_line, added_line)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderDiffOptions {
    /// File path (unused, kept for API compatibility)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// Render a diff string with colored lines and intra-line change highlighting.
/// - Context lines: dim/gray
/// - Removed lines: red, with inverse on changed tokens
/// - Added lines: green, with inverse on changed tokens
pub fn render_diff(diff_text: &str, _options: Option<&RenderDiffOptions>) -> String {
    let t = theme();
    let lines: Vec<&str> = diff_text.split('\n').collect();
    let mut result: Vec<String> = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let Some(parsed) = parse_diff_line(line) else {
            result.push(t.fg(&ThemeColor::ToolDiffContext, line));
            i += 1;
            continue;
        };

        if parsed.prefix == '-' {
            // Collect consecutive removed lines
            let mut removed_lines: Vec<(&str, &str)> = Vec::new();
            while i < lines.len() {
                match parse_diff_line(lines[i]) {
                    Some(p) if p.prefix == '-' => {
                        removed_lines.push((p.line_num, p.content));
                        i += 1;
                    }
                    _ => break,
                }
            }

            // Collect consecutive added lines
            let mut added_lines: Vec<(&str, &str)> = Vec::new();
            while i < lines.len() {
                match parse_diff_line(lines[i]) {
                    Some(p) if p.prefix == '+' => {
                        added_lines.push((p.line_num, p.content));
                        i += 1;
                    }
                    _ => break,
                }
            }

            // Only do intra-line diffing when there's exactly one removed and one added line
            // (indicating a single line modification). Otherwise, show lines as-is.
            if removed_lines.len() == 1 && added_lines.len() == 1 {
                let (removed_num, removed_content) = removed_lines[0];
                let (added_num, added_content) = added_lines[0];

                let (removed_line, added_line) =
                    render_intra_line_diff(&replace_tabs(removed_content), &replace_tabs(added_content));

                result.push(t.fg(&ThemeColor::ToolDiffRemoved, &format!("-{removed_num} {removed_line}")));
                result.push(t.fg(&ThemeColor::ToolDiffAdded, &format!("+{added_num} {added_line}")));
            } else {
                // Show all removed lines first, then all added lines
                for (num, content) in &removed_lines {
                    result.push(t.fg(&ThemeColor::ToolDiffRemoved, &format!("-{num} {}", replace_tabs(content))));
                }
                for (num, content) in &added_lines {
                    result.push(t.fg(&ThemeColor::ToolDiffAdded, &format!("+{num} {}", replace_tabs(content))));
                }
            }
        } else if parsed.prefix == '+' {
            // Standalone added line
            result.push(t.fg(
                &ThemeColor::ToolDiffAdded,
                &format!("+{} {}", parsed.line_num, replace_tabs(parsed.content)),
            ));
            i += 1;
        } else {
            // Context line
            result.push(t.fg(
                &ThemeColor::ToolDiffContext,
                &format!(" {} {}", parsed.line_num, replace_tabs(parsed.content)),
            ));
            i += 1;
        }
    }

    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(old: &str, new: &str) -> Vec<(String, bool, bool)> {
        diff_words(old, new).into_iter().map(|c| (c.value, c.added, c.removed)).collect()
    }

    fn c(v: &str, added: bool, removed: bool) -> (String, bool, bool) {
        (v.to_string(), added, removed)
    }

    // Expected values produced by jsdiff 8.0.4 `diffWords`.
    #[test]
    fn diff_words_matches_jsdiff() {
        assert_eq!(words("foo bar baz", "foo baz"), vec![c("foo ", false, false), c("bar ", false, true), c("baz", false, false)]);
        assert_eq!(
            words("foo bar baz", "foo qux baz"),
            vec![c("foo ", false, false), c("bar", false, true), c("qux", true, false), c(" baz", false, false)]
        );
        assert_eq!(
            words("foo   bar baz", "foo  baz"),
            vec![c("foo  ", false, false), c(" bar ", false, true), c("baz", false, false)]
        );
        assert_eq!(
            words("  const x = 1;", "  const y = 2;"),
            vec![
                c("  const ", false, false),
                c("x", false, true),
                c("y", true, false),
                c(" = ", false, false),
                c("1", false, true),
                c("2", true, false),
                c(";", false, false)
            ]
        );
    }

    #[test]
    fn diff_words_whitespace_cases_match_jsdiff() {
        assert_eq!(words("foo\nbar baz", "foo baz"), vec![c("foo", false, false), c("\nbar", false, true), c(" baz", false, false)]);
        assert_eq!(words("foo baz", "foo\nbar baz"), vec![c("foo\n", false, false), c("bar ", true, false), c("baz", false, false)]);
        assert_eq!(words("\tlet a = foo(1, 2);", "\tlet a = bar(1, 3);"), vec![c("\tlet a = ", false, false), c("foo", false, true), c("bar", true, false), c("(1, ", false, false), c("2", false, true), c("3", true, false), c(");", false, false)]);
        assert_eq!(words("café au lait", "cafe au lait"), vec![c("café", false, true), c("cafe", true, false), c(" au lait", false, false)]);
        assert_eq!(words("a b c d", "x b y d"), vec![c("a", false, true), c("x", true, false), c(" b ", false, false), c("c", false, true), c("y", true, false), c(" d", false, false)]);
        assert_eq!(words("   ", "  x"), vec![c("   ", false, true), c("  x", true, false)]);
        assert_eq!(words("return x", "return"), vec![c("return", false, false), c(" x", false, true)]);
        assert_eq!(words("", "new"), vec![c("new", true, false)]);
        assert_eq!(words("old", ""), vec![c("old", false, true)]);
        assert_eq!(words("hello, world!", "hello world"), vec![c("hello", false, false), c(",", false, true), c(" world", false, false), c("!", false, true)]);
    }

    #[test]
    fn parses_diff_lines() {
        let p = parse_diff_line("+12 hello").unwrap();
        assert_eq!((p.prefix, p.line_num, p.content), ('+', "12", "hello"));
        let p = parse_diff_line("    ...").unwrap();
        assert_eq!((p.prefix, p.line_num, p.content), (' ', "  ", "..."));
        let p = parse_diff_line("- 3 x").unwrap();
        assert_eq!((p.prefix, p.line_num, p.content), ('-', " 3", "x"));
        assert!(parse_diff_line("x1 a").is_none());
        assert!(parse_diff_line("+12").is_none());
    }
}
