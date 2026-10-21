//! Small helpers that reproduce JavaScript string and number semantics the tool ports rely on
//! (UTF-16 lengths, `String.prototype.trim`, `Number.prototype.toString`, `localeCompare`).

use std::cmp::Ordering;

/// `str.length` in JavaScript: the number of UTF-16 code units.
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Byte index of the UTF-16 code unit offset `units` (clamped to the string). An offset inside a
/// surrogate pair rounds down to the start of that character.
pub fn utf16_offset_to_byte(s: &str, units: usize) -> usize {
    let mut count = 0usize;
    for (index, ch) in s.char_indices() {
        let next = count + ch.len_utf16();
        if next > units {
            return index;
        }
        count = next;
    }
    s.len()
}

/// `str.slice(0, units)` with UTF-16 offsets.
pub fn utf16_prefix(s: &str, units: usize) -> &str {
    &s[..utf16_offset_to_byte(s, units)]
}

/// Whitespace as `String.prototype.trim` defines it (WhiteSpace + LineTerminator).
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// `String.prototype.trimEnd`.
pub fn js_trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_whitespace)
}

/// `String(number)` for the values tools print (integers and short decimals).
pub fn js_number_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == value.trunc() && value.abs() < 1e21 {
        return format!("{}", value as i128);
    }
    let text = format!("{value}");
    if text.contains('e') {
        return text;
    }
    text
}

/// `Number.prototype.toFixed(digits)` for finite values below 1e21.
pub fn js_to_fixed(value: f64, digits: usize) -> String {
    // JS rounds the exact binary value half-up (away from zero); Rust's formatting rounds the
    // exact value half-to-even. They differ only on exact ties, which we detect on the exact
    // decimal expansion and round away from zero.
    let wide = format!("{:.*}", digits + 40, value.abs());
    let dot = wide.find('.').unwrap_or(wide.len());
    let tail = &wide[dot + 1 + digits..];
    let is_tie = tail.starts_with('5') && tail[1..].bytes().all(|b| b == b'0');
    if !is_tie {
        return format!("{value:.digits$}");
    }
    let factor = 10f64.powi(digits as i32);
    let up = ((value.abs() * factor).trunc() + 1.0) / factor;
    let signed = if value < 0.0 { -up } else { up };
    format!("{signed:.digits$}")
}

/// Collation weight class of an ASCII character in ICU root collation order (used by
/// `localeCompare` without locales): whitespace, punctuation and symbols, digits, letters.
fn icu_ascii_primary(c: char) -> Option<u32> {
    // Order of ASCII non-alphanumerics in the CLDR root collation.
    const ORDER: &str = "\t\n\u{b}\u{c}\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    if let Some(index) = ORDER.find(c) {
        return Some(index as u32);
    }
    if c.is_ascii_digit() {
        return Some(100 + (c as u32 - '0' as u32));
    }
    if c.is_ascii_alphabetic() {
        return Some(200 + (c.to_ascii_lowercase() as u32 - 'a' as u32));
    }
    None
}

fn primary_key(s: &str) -> Vec<u32> {
    use unicode_normalization::UnicodeNormalization;
    let mut key = Vec::with_capacity(s.len());
    for c in s.nfd() {
        // Combining marks only differ at the secondary level.
        if unicode_normalization::char::is_combining_mark(c) {
            continue;
        }
        match icu_ascii_primary(c) {
            Some(weight) => key.push(weight),
            None => {
                let lower = c.to_lowercase().next().unwrap_or(c);
                key.push(1000 + lower as u32);
            }
        }
    }
    key
}

/// Approximation of `String.prototype.localeCompare` with the ICU root collation: compares
/// primary weights first (punctuation < digits < letters, case and accents ignored), then accents,
/// then case (lowercase first), then code points.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    use unicode_normalization::UnicodeNormalization;
    primary_key(a)
        .cmp(&primary_key(b))
        .then_with(|| {
            let marks = |s: &str| -> Vec<char> {
                s.nfd()
                    .filter(|c| unicode_normalization::char::is_combining_mark(*c))
                    .collect()
            };
            marks(a).cmp(&marks(b))
        })
        .then_with(|| {
            let case = |s: &str| -> Vec<u8> {
                s.chars().map(|c| u8::from(c.is_uppercase())).collect()
            };
            case(a).cmp(&case(b))
        })
        .then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_helpers() {
        assert_eq!(utf16_len("a😀b"), 4);
        assert_eq!(utf16_prefix("a😀b", 3), "a😀");
        assert_eq!(utf16_prefix("a😀b", 2), "a");
        assert_eq!(js_trim_end("abc \u{feff}\t"), "abc");
    }

    #[test]
    fn numbers() {
        assert_eq!(js_number_string(5.0), "5");
        assert_eq!(js_number_string(2.5), "2.5");
        assert_eq!(js_to_fixed(1.25, 1), "1.3");
        assert_eq!(js_to_fixed(0.5, 1), "0.5");
        assert_eq!(js_to_fixed(1.45, 1), "1.4");
        assert_eq!(js_to_fixed(0.05, 1), "0.1");
        assert_eq!(js_to_fixed(2.5, 0), "3");
        assert_eq!(js_to_fixed(1023.0 / 1024.0, 1), "1.0");
    }

    #[test]
    fn collation() {
        let mut v = vec!["b", "_a", "A", "a", ".hidden", "10", "2", "a-b", "a_b", "é"];
        v.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(v, vec!["_a", ".hidden", "10", "2", "a", "A", "a_b", "a-b", "b", "é"]);
    }
}
