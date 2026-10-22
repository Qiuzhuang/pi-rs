//! `JSON.parse` with the error messages of V8 (Node 22), which pi shows verbatim in several places
//! (invalid settings or models files, RPC parse errors).
//!
//! Parsing itself is serde_json; when it fails, a scanner that follows V8's `JsonParser` finds the
//! error V8 would report and formats it like V8 (`Expected ',' or '}' after property value in JSON at
//! position 7 (line 1 column 8)`, `Unexpected token 'o', "not json" is not valid JSON`, ...).
//! Positions, lines and columns count UTF-16 code units, as in JavaScript.

use serde::de::DeserializeOwned;
use serde_json::Value;

/// `JSON.parse(text)`: the value, or the message of the `SyntaxError` V8 throws.
pub fn js_json_parse(text: &str) -> Result<Value, String> {
    js_json_parse_as(text)
}

/// `JSON.parse(text)` into `T`. Syntax errors carry V8's message; a valid document that does not
/// match `T` reports serde's message.
pub fn js_json_parse_as<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    match serde_json::from_str::<T>(text) {
        Ok(value) => Ok(value),
        Err(error) if error.is_syntax() || error.is_eof() => {
            Err(v8_json_syntax_error(text).unwrap_or_else(|| error.to_string()))
        }
        Err(error) => Err(error.to_string()),
    }
}

/// The message V8's `JSON.parse` throws for `text`, or `None` when V8 would accept it.
pub fn v8_json_syntax_error(text: &str) -> Option<String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut scanner = Scanner { s: &units, i: 0 };
    scanner.document().err()
}

/// Minimum source length (UTF-16 units) for which V8 quotes context instead of the whole source.
const MIN_LENGTH_FOR_CONTEXT: usize = 21;
/// Characters of context V8 quotes on each side of an unexpected token.
const MAX_CONTEXT: usize = 10;

struct Scanner<'a> {
    s: &'a [u16],
    i: usize,
}

type Scan = Result<(), String>;

fn is_digit(c: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&c)
}

fn is_hex(c: u16) -> bool {
    is_digit(c) || (b'a' as u16..=b'f' as u16).contains(&c) || (b'A' as u16..=b'F' as u16).contains(&c)
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u16> {
        self.s.get(self.i).copied()
    }

    fn is(&self, c: u8) -> bool {
        self.peek() == Some(c as u16)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0a | 0x0d)) {
            self.i += 1;
        }
    }

    fn text(&self, from: usize, to: usize) -> String {
        String::from_utf16_lossy(&self.s[from.min(self.s.len())..to.min(self.s.len())])
    }

    /// `<message> in JSON at position <pos> (line <l> column <c>)`.
    fn at(&self, message: &str, pos: usize) -> String {
        let before = &self.s[..pos.min(self.s.len())];
        let line = 1 + before.iter().filter(|&&c| c == 0x0a).count();
        let line_start = before.iter().rposition(|&c| c == 0x0a).map(|i| i + 1).unwrap_or(0);
        // V8's message for trailing input already ends in "after JSON".
        let infix = if message.ends_with("after JSON") { "" } else { " in JSON" };
        format!("{message}{infix} at position {pos} (line {line} column {})", pos - line_start + 1)
    }

    /// V8 `ReportUnexpectedToken` without a specific message.
    fn unexpected(&self, pos: usize) -> String {
        let len = self.s.len();
        let Some(&c) = self.s.get(pos) else {
            return "Unexpected end of JSON input".to_string();
        };
        if is_digit(c) || c == b'-' as u16 {
            return self.at("Unexpected number", pos);
        }
        if c == b'"' as u16 {
            return self.at("Unexpected string", pos);
        }
        let source = self.text(0, len);
        if matches!(source.as_str(), "undefined" | "NaN" | "Infinity" | "[object Object]") {
            return format!("\"{source}\" is not valid JSON");
        }
        let token = String::from_utf16_lossy(&[c]);
        if len < MIN_LENGTH_FOR_CONTEXT {
            return format!("Unexpected token '{token}', \"{source}\" is not valid JSON");
        }
        if pos < MAX_CONTEXT {
            return format!(
                "Unexpected token '{token}', \"{}\"... is not valid JSON",
                self.text(0, pos + MAX_CONTEXT)
            );
        }
        let start = pos - MAX_CONTEXT;
        let end = pos + MAX_CONTEXT;
        if end >= len {
            format!("Unexpected token '{token}', ...\"{}\" is not valid JSON", self.text(start, len))
        } else {
            format!("Unexpected token '{token}', ...\"{}\"... is not valid JSON", self.text(start, end))
        }
    }

    fn document(&mut self) -> Scan {
        self.value()?;
        self.skip_ws();
        if self.i < self.s.len() {
            return Err(self.at("Unexpected non-whitespace character after JSON", self.i));
        }
        Ok(())
    }

    fn value(&mut self) -> Scan {
        self.skip_ws();
        let Some(c) = self.peek() else {
            return Err(self.unexpected(self.i));
        };
        match c {
            0x22 => self.string(),
            0x2d | 0x30..=0x39 => self.number(),
            0x7b => self.object(),
            0x5b => self.array(),
            0x74 => self.literal("true"),
            0x66 => self.literal("false"),
            0x6e => self.literal("null"),
            _ => Err(self.unexpected(self.i)),
        }
    }

    fn literal(&mut self, word: &str) -> Scan {
        for (k, expected) in word.bytes().enumerate() {
            match self.s.get(self.i + k) {
                Some(&c) if c == expected as u16 => {}
                _ => return Err(self.unexpected(self.i + k)),
            }
        }
        self.i += word.len();
        Ok(())
    }

    fn string(&mut self) -> Scan {
        self.i += 1;
        loop {
            let Some(c) = self.peek() else {
                return Err(self.at("Unterminated string", self.i));
            };
            match c {
                0x22 => {
                    self.i += 1;
                    return Ok(());
                }
                0x5c => {
                    self.i += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.unexpected(self.i));
                    };
                    match e {
                        0x22 | 0x5c | 0x2f | 0x62 | 0x66 | 0x6e | 0x72 | 0x74 => self.i += 1,
                        0x75 => {
                            self.i += 1;
                            for _ in 0..4 {
                                match self.peek() {
                                    Some(h) if is_hex(h) => self.i += 1,
                                    _ => return Err(self.at("Bad Unicode escape", self.i)),
                                }
                            }
                        }
                        _ => return Err(self.at("Bad escaped character", self.i)),
                    }
                }
                c if c < 0x20 => return Err(self.at("Bad control character in string literal", self.i)),
                _ => self.i += 1,
            }
        }
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(is_digit) {
            self.i += 1;
        }
    }

    fn number(&mut self) -> Scan {
        if self.is(b'-') {
            self.i += 1;
            if !self.peek().is_some_and(is_digit) {
                return Err(self.at("No number after minus sign", self.i));
            }
        }
        if self.is(b'0') {
            self.i += 1;
            if self.peek().is_some_and(is_digit) {
                return Err(self.unexpected(self.i));
            }
        } else {
            self.digits();
        }
        if self.is(b'.') {
            self.i += 1;
            if !self.peek().is_some_and(is_digit) {
                return Err(self.at("Unterminated fractional number", self.i));
            }
            self.digits();
        }
        if self.is(b'e') || self.is(b'E') {
            self.i += 1;
            if self.is(b'+') || self.is(b'-') {
                self.i += 1;
            }
            if !self.peek().is_some_and(is_digit) {
                return Err(self.at("Exponent part is missing a number", self.i));
            }
            self.digits();
        }
        Ok(())
    }

    fn object(&mut self) -> Scan {
        self.i += 1;
        self.skip_ws();
        if self.is(b'}') {
            self.i += 1;
            return Ok(());
        }
        let mut expected_name = "Expected property name or '}'";
        loop {
            if !self.is(b'"') {
                return Err(self.at(expected_name, self.i));
            }
            self.string()?;
            self.skip_ws();
            if !self.is(b':') {
                return Err(self.at("Expected ':' after property name", self.i));
            }
            self.i += 1;
            self.value()?;
            self.skip_ws();
            if self.is(b',') {
                self.i += 1;
                self.skip_ws();
                expected_name = "Expected double-quoted property name";
                continue;
            }
            if self.is(b'}') {
                self.i += 1;
                return Ok(());
            }
            return Err(self.at("Expected ',' or '}' after property value", self.i));
        }
    }

    fn array(&mut self) -> Scan {
        self.i += 1;
        self.skip_ws();
        if self.is(b']') {
            self.i += 1;
            return Ok(());
        }
        loop {
            self.value()?;
            self.skip_ws();
            if self.is(b',') {
                self.i += 1;
                continue;
            }
            if self.is(b']') {
                self.i += 1;
                return Ok(());
            }
            return Err(self.at("Expected ',' or ']' after array element", self.i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (input, message) pairs produced by `JSON.parse` in Node 22.
    const V8_CASES: &[(&str, &str)] = &[
        ("", "Unexpected end of JSON input"),
        (" ", "Unexpected end of JSON input"),
        ("not json", "Unexpected token 'o', \"not json\" is not valid JSON"),
        ("nope\n", "Unexpected token 'o', \"nope\n\" is not valid JSON"),
        ("{bad", "Expected property name or '}' in JSON at position 1 (line 1 column 2)"),
        ("{\"providers\": {\n", "Expected property name or '}' in JSON at position 16 (line 2 column 1)"),
        ("[1,]", "Unexpected token ']', \"[1,]\" is not valid JSON"),
        ("{\"a\":1,}", "Expected double-quoted property name in JSON at position 7 (line 1 column 8)"),
        ("{\"a\" 1}", "Expected ':' after property name in JSON at position 5 (line 1 column 6)"),
        ("{\"a\":1 \"b\":2}", "Expected ',' or '}' after property value in JSON at position 7 (line 1 column 8)"),
        ("[1 2]", "Expected ',' or ']' after array element in JSON at position 3 (line 1 column 4)"),
        ("tru", "Unexpected end of JSON input"),
        ("trux", "Unexpected token 'x', \"trux\" is not valid JSON"),
        ("nulll", "Unexpected non-whitespace character after JSON at position 4 (line 1 column 5)"),
        ("-", "No number after minus sign in JSON at position 1 (line 1 column 2)"),
        ("01", "Unexpected number in JSON at position 1 (line 1 column 2)"),
        ("1.e5", "Unterminated fractional number in JSON at position 2 (line 1 column 3)"),
        ("1e+", "Exponent part is missing a number in JSON at position 3 (line 1 column 4)"),
        ("\"abc", "Unterminated string in JSON at position 4 (line 1 column 5)"),
        ("\"a\nb\"", "Bad control character in string literal in JSON at position 2 (line 1 column 3)"),
        ("\"\\x\"", "Bad escaped character in JSON at position 2 (line 1 column 3)"),
        ("\"\\u12G4\"", "Bad Unicode escape in JSON at position 5 (line 1 column 6)"),
        ("\"\\u", "Bad Unicode escape in JSON at position 3 (line 1 column 4)"),
        ("\"\\", "Unexpected end of JSON input"),
        ("undefined", "\"undefined\" is not valid JSON"),
        ("[object Object]", "\"[object Object]\" is not valid JSON"),
        ("{\n  \"a\": 1,\n  \"b\": [1, 2,]\n}", "Unexpected token ']', ...\"b\": [1, 2,]\n}\" is not valid JSON"),
        ("abcdefghijklmnopqrstuvwxyz0123456789", "Unexpected token 'a', \"abcdefghij\"... is not valid JSON"),
        (
            "{\"key\": \"value\", \"other\": undefined, \"x\": 1}",
            "Unexpected token 'u', ...\" \"other\": undefined,\"... is not valid JSON",
        ),
        ("[\"aaaaaaaaaaaaaaaaaaaa\", bad]", "Unexpected token 'b', ...\"aaaaaaa\", bad]\" is not valid JSON"),
        ("{\"a\":", "Unexpected end of JSON input"),
        ("{", "Expected property name or '}' in JSON at position 1 (line 1 column 2)"),
        ("[1,", "Unexpected end of JSON input"),
        ("{\"a\":\"\u{1F600}\" x}", "Expected ',' or '}' after property value in JSON at position 10 (line 1 column 11)"),
        ("\r\n{x", "Expected property name or '}' in JSON at position 3 (line 2 column 2)"),
        ("{\"a\":1}\r\n\r\n x", "Unexpected non-whitespace character after JSON at position 12 (line 3 column 2)"),
        ("-01", "Unexpected number in JSON at position 2 (line 1 column 3)"),
        ("{\"a\":tru}", "Unexpected token '}', \"{\"a\":tru}\" is not valid JSON"),
        ("{\"a\"::1}", "Unexpected token ':', \"{\"a\"::1}\" is not valid JSON"),
        ("[-]", "No number after minus sign in JSON at position 2 (line 1 column 3)"),
        ("[truex]", "Expected ',' or ']' after array element in JSON at position 5 (line 1 column 6)"),
        ("[t]", "Unexpected token ']', \"[t]\" is not valid JSON"),
        ("\u{a0}{}", "Unexpected token '\u{a0}', \"\u{a0}{}\" is not valid JSON"),
        ("xyyyyyyyyyyyyyyyyyyy", "Unexpected token 'x', \"xyyyyyyyyyyyyyyyyyyy\" is not valid JSON"),
        ("xyyyyyyyyyyyyyyyyyyyy", "Unexpected token 'x', \"xyyyyyyyyy\"... is not valid JSON"),
    ];

    #[test]
    fn matches_v8_messages() {
        for (input, expected) in V8_CASES {
            assert_eq!(v8_json_syntax_error(input).as_deref(), Some(*expected), "input {input:?}");
            assert_eq!(js_json_parse(input).unwrap_err(), *expected, "input {input:?}");
        }
    }

    #[test]
    fn context_window_edges_match_v8() {
        let end = format!("{}x{}", " ".repeat(20), "y".repeat(9));
        assert_eq!(
            v8_json_syntax_error(&end).unwrap(),
            format!("Unexpected token 'x', ...\"{}x{}\" is not valid JSON", " ".repeat(10), "y".repeat(9))
        );
        let surround = format!("{}x{}", " ".repeat(19), "y".repeat(10));
        assert_eq!(
            v8_json_syntax_error(&surround).unwrap(),
            format!("Unexpected token 'x', ...\"{}x{}\"... is not valid JSON", " ".repeat(10), "y".repeat(9))
        );
    }

    #[test]
    fn accepts_valid_json() {
        for input in ["{}", " [1, -0.5e+3, \"\\u00e9\", true, false, null, {\"a\": []}] ", "\"x\""] {
            assert_eq!(v8_json_syntax_error(input), None, "{input}");
            assert!(js_json_parse(input).is_ok());
        }
    }
}
