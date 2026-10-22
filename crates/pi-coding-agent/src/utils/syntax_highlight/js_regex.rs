//! Translation of JavaScript regular expression sources (as used by highlight.js grammars)
//! into `fancy_regex` syntax with JavaScript semantics where they differ.
//!
//! - `\d`, `\w` (and negations) are ASCII-only, as in JavaScript.
//! - `.` does not match line terminators (`\n`, `\r`, U+2028, U+2029).
//! - `[^]` matches any character; `[]` matches nothing.
//! - Annex B literal braces and brackets (`a{`, `]`) are escaped.
//! - Identity escapes of letters (`\a` = `a`) and `\uXXXX`/`\xXX`/`\cX` are rewritten.
//!
//! `\b`/`\B` keep Unicode word semantics (fancy-regex has no ASCII-only word boundary),
//! which only differs next to non-ASCII letters.

/// Count capturing groups in a JavaScript regex source.
pub fn count_capture_groups(source: &str) -> usize {
    let chars: Vec<char> = source.chars().collect();
    let mut count = 0;
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            '(' if !in_class => {
                if chars.get(i + 1) != Some(&'?') {
                    count += 1;
                } else if chars.get(i + 2) == Some(&'<')
                    && !matches!(chars.get(i + 3), Some('=') | Some('!'))
                {
                    count += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    count
}

fn push_literal(out: &mut String, c: char, in_class: bool) {
    let needs_escape = if in_class {
        matches!(c, '\\' | ']' | '[' | '^' | '-' | '&' | '~')
    } else {
        matches!(c, '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '#')
    };
    if needs_escape {
        out.push('\\');
        out.push(c);
    } else if c.is_whitespace() || c.is_control() {
        out.push_str(&format!("\\x{{{:X}}}", c as u32));
    } else {
        out.push(c);
    }
}

fn hex_value(chars: &[char], start: usize, len: usize) -> Option<u32> {
    if start + len > chars.len() {
        return None;
    }
    let s: String = chars[start..start + len].iter().collect();
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(&s, 16).ok()
}

fn push_code_point(out: &mut String, value: u32, in_class: bool) {
    match char::from_u32(value) {
        Some(c) => push_literal(out, c, in_class),
        // Lone surrogates cannot occur in Rust strings: match nothing.
        None => out.push_str(if in_class { "" } else { "[^\\x00-\\x{10FFFF}]" }),
    }
}

/// Whether `chars[i..]` starts a valid quantifier `{n}`, `{n,}` or `{n,m}`; returns its length.
fn quantifier_len(chars: &[char], i: usize) -> Option<usize> {
    let mut j = i + 1;
    let digits_start = j;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j == digits_start {
        return None;
    }
    if j < chars.len() && chars[j] == ',' {
        j += 1;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
    }
    if j < chars.len() && chars[j] == '}' { Some(j + 1 - i) } else { None }
}

/// Translate a JavaScript regex source.
pub fn translate(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len() + 16);
    let mut in_class = false;
    let mut class_start = false;
    // The previous class atom was a class escape (`\w`, `\d`, `\s`, ...), after which `-` is literal.
    let mut after_class_escape = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at_class_start = class_start;
        class_start = false;
        let prev_class_escape = after_class_escape;
        after_class_escape = false;
        match c {
            '\\' => {
                let Some(&e) = chars.get(i + 1) else {
                    out.push_str("\\\\");
                    i += 1;
                    continue;
                };
                i += 2;
                if in_class && matches!(e, 'd' | 'D' | 'w' | 'W' | 's' | 'S') {
                    after_class_escape = true;
                }
                match e {
                    'd' => out.push_str(if in_class { "0-9" } else { "[0-9]" }),
                    'D' => out.push_str(if in_class { "\\x00-/:-\\x{10FFFF}" } else { "[^0-9]" }),
                    'w' => out.push_str(if in_class { "0-9A-Za-z_" } else { "[0-9A-Za-z_]" }),
                    'W' => out.push_str(if in_class {
                        "\\x00-/:-@\\[-\\^`{-\\x{10FFFF}"
                    } else {
                        "[^0-9A-Za-z_]"
                    }),
                    's' => out.push_str("\\s"),
                    'S' => out.push_str("\\S"),
                    'b' => out.push_str(if in_class { "\\x{8}" } else { "\\b" }),
                    'B' => {
                        if in_class {
                            out.push('B');
                        } else {
                            out.push_str("\\B");
                        }
                    }
                    'n' => out.push_str("\\n"),
                    'r' => out.push_str("\\r"),
                    't' => out.push_str("\\t"),
                    'f' => out.push_str("\\x{C}"),
                    'v' => out.push_str("\\x{B}"),
                    '0' if !chars.get(i).is_some_and(|c| c.is_ascii_digit()) => out.push_str("\\x{0}"),
                    '1'..='9' if !in_class => {
                        // Backreference.
                        out.push('\\');
                        out.push(e);
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            out.push(chars[i]);
                            i += 1;
                        }
                    }
                    'x' => match hex_value(&chars, i, 2) {
                        Some(v) => {
                            push_code_point(&mut out, v, in_class);
                            i += 2;
                        }
                        None => push_literal(&mut out, 'x', in_class),
                    },
                    'u' => match hex_value(&chars, i, 4) {
                        Some(v) => {
                            push_code_point(&mut out, v, in_class);
                            i += 4;
                        }
                        None => push_literal(&mut out, 'u', in_class),
                    },
                    'c' => match chars.get(i) {
                        Some(l) if l.is_ascii_alphabetic() => {
                            push_code_point(&mut out, (*l as u32) % 32, in_class);
                            i += 1;
                        }
                        _ => {
                            push_literal(&mut out, '\\', in_class);
                            push_literal(&mut out, 'c', in_class);
                        }
                    },
                    'k' if !in_class && chars.get(i) == Some(&'<') => out.push_str("\\k"),
                    other => push_literal(&mut out, other, in_class),
                }
                continue;
            }
            '[' if !in_class => {
                if chars.get(i + 1) == Some(&'^') && chars.get(i + 2) == Some(&']') {
                    out.push_str("(?s:.)");
                    i += 3;
                    continue;
                }
                if chars.get(i + 1) == Some(&']') {
                    out.push_str("[^\\x00-\\x{10FFFF}]");
                    i += 2;
                    continue;
                }
                in_class = true;
                out.push('[');
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                class_start = true;
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '-' if in_class => {
                // A range operator between two class atoms; literal at the start or end.
                let next_closes = chars.get(i + 1) == Some(&']');
                let next_is_class_escape = chars.get(i + 1) == Some(&'\\')
                    && matches!(chars.get(i + 2), Some('d' | 'D' | 'w' | 'W' | 's' | 'S'));
                if at_class_start || next_closes || prev_class_escape || next_is_class_escape {
                    out.push_str("\\-");
                } else {
                    out.push('-');
                }
            }
            c if in_class => push_literal(&mut out, c, true),
            '{' => match quantifier_len(&chars, i) {
                Some(len) => {
                    out.extend(&chars[i..i + len]);
                    i += len;
                    continue;
                }
                None => out.push_str("\\{"),
            },
            '}' | ']' => {
                out.push('\\');
                out.push(c);
            }
            '.' => out.push_str("[^\\n\\r\\x{2028}\\x{2029}]"),
            '#' => out.push_str("\\#"),
            c if c.is_whitespace() => push_literal(&mut out, c, false),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Compile a JavaScript regex source with highlight.js flags (`m`, plus `i` when the
/// language is case-insensitive).
pub fn compile(source: &str, case_insensitive: bool) -> Result<fancy_regex::Regex, String> {
    let flags = if case_insensitive { "(?mi)" } else { "(?m)" };
    let pattern = format!("{flags}(?:{})", translate(source));
    fancy_regex::RegexBuilder::new(&pattern)
        .backtrack_limit(2_000_000)
        .build()
        .map_err(|e| format!("{e}: {pattern}"))
}

/// Escape a string for a literal regex (highlight.js `escape()`).
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if "-/\\^$*+?.()|[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_match(source: &str, text: &str) -> bool {
        compile(source, false).unwrap().is_match(text).unwrap()
    }

    #[test]
    fn translates_js_specific_syntax() {
        assert!(is_match(r"\d+", "123"));
        assert!(!is_match(r"^\w+$", "é"));
        assert!(is_match(r"[^]", "\n"));
        assert!(!is_match(r"[]", "a"));
        assert!(is_match(r"a{", "a{"));
        assert!(is_match(r"a{2}", "aa"));
        assert!(is_match(r"\/\*", "/*"));
        assert!(is_match(r"[\w-]+", "a-b"));
        assert!(is_match(r"\u0041", "A"));
        assert!(is_match(r"(a)\1", "aa"));
        assert!(is_match(r"(?=x)x", "x"));
        assert!(is_match(r"[[]", "["));
        assert!(is_match(r"#", "#"));
        assert!(!is_match(r"^.$", "\r"));
        assert!(is_match("[A-Za-z0-9\\\\._:-]+", "a.b:c-d"));
    }

    #[test]
    fn counts_groups() {
        assert_eq!(count_capture_groups(r"(a)(?:b)(?=c)(?<n>d)[(]\("), 2);
    }
}
