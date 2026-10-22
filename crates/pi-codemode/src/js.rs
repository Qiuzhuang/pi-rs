//! Small JavaScript-compatible helpers: `Number.prototype.toString()`, `JSON.stringify()`,
//! `String.prototype.trim()` and UTF-16 lengths/ordering, so rendered text matches the TS
//! implementation byte for byte.

use std::cmp::Ordering;

use serde_json::Value;

pub use pi_json::{number_to_string, object_keys, quote};

/// JavaScript `String(value)` of a JSON number.
pub fn json_number_to_string(number: &serde_json::Number) -> String {
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }
    number_to_string(number.as_f64().unwrap_or(f64::NAN))
}

/// `JSON.stringify(value)` without indentation.
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&json_number_to_string(n)),
        Value::String(s) => out.push_str(&quote(s)),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, key) in object_keys(map).into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&quote(key));
                out.push(':');
                write_json(&map[key.as_str()], out);
            }
            out.push('}');
        }
    }
}

/// ECMAScript WhiteSpace or LineTerminator.
pub fn is_js_whitespace(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// `String.prototype.trim()`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `String.prototype.trimStart()`.
pub fn trim_start(text: &str) -> &str {
    text.trim_start_matches(is_js_whitespace)
}

/// `string.length`: UTF-16 code units.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The default `Array.prototype.sort()` order of strings: by UTF-16 code units.
pub fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_format_like_javascript() {
        for (value, expected) in [
            (1.0, "1"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (123.456, "123.456"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-2.5e-10, "-2.5e-10"),
            (f64::NAN, "NaN"),
        ] {
            assert_eq!(number_to_string(value), expected);
        }
    }

    #[test]
    fn stringify_orders_index_keys_first() {
        assert_eq!(stringify(&json!({"b": 1, "1": 2.5, "a": [true, null, "x\n"]})), r#"{"1":2.5,"b":1,"a":[true,null,"x\n"]}"#);
        assert_eq!(quote("\u{1}\u{2028}"), "\"\\u0001\u{2028}\"");
    }
}
