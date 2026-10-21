//! Small JavaScript-compatible helpers: `Number.prototype.toString()`, `JSON.stringify()`,
//! `String.prototype.trim()` and UTF-16 lengths/ordering, so rendered text matches the TS
//! implementation byte for byte.

use std::cmp::Ordering;

use serde_json::Value;

/// JavaScript `String(number)` for a finite or non-finite double.
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let negative = value < 0.0;
    // Rust's `{:e}` prints the shortest round-tripping digits, like ECMAScript's Number::toString.
    let formatted = format!("{:e}", value.abs());
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((formatted.as_str(), "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let k = digits.len() as i64;
    let n = exponent + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        let e = n - 1;
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if e >= 0 { '+' } else { '-' });
        out.push_str(&e.abs().to_string());
    }
    out
}

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

/// `JSON.stringify(string)`.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether `key` is an array index, which JavaScript orders before other keys.
fn is_array_index(key: &str) -> bool {
    if key == "0" {
        return true;
    }
    !key.is_empty()
        && !key.starts_with('0')
        && key.len() <= 10
        && key.bytes().all(|b| b.is_ascii_digit())
        && key.parse::<u64>().is_ok_and(|n| n < u32::MAX as u64)
}

/// Object keys in JavaScript property order: array indices ascending, then insertion order.
pub fn object_keys(map: &serde_json::Map<String, Value>) -> Vec<&String> {
    let mut indices: Vec<&String> = map.keys().filter(|key| is_array_index(key)).collect();
    indices.sort_by_key(|key| key.parse::<u64>().unwrap_or(0));
    indices.extend(map.keys().filter(|key| !is_array_index(key)));
    indices
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
