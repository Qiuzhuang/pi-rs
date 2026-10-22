//! Port of `packages/coding-agent/src/utils/json.ts`.

use std::sync::LazyLock;

use regex::Regex;

static COMMENTS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(?:\\(?s:.)|[^"\\])*"|//[^\n]*"#).expect("valid regex"));
static TRAILING_COMMA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(?:\\(?s:.)|[^"\\])*"|,(\s*[}\]])"#).expect("valid regex"));

/// Strip `//` line comments and trailing commas from JSON, leaving string literals untouched.
pub fn strip_json_comments(input: &str) -> String {
    let without_comments = COMMENTS_RE.replace_all(input, |caps: &regex::Captures| {
        let m = &caps[0];
        if m.starts_with('"') { m.to_string() } else { String::new() }
    });
    TRAILING_COMMA_RE
        .replace_all(&without_comments, |caps: &regex::Captures| match caps.get(1) {
            Some(tail) => tail.as_str().to_string(),
            None => {
                let m = &caps[0];
                if m.starts_with('"') { m.to_string() } else { String::new() }
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_trailing_commas() {
        let input = "{\n  // comment\n  \"a\": \"http://x\", // tail\n  \"b\": [1, 2,],\n}";
        let out = strip_json_comments(input);
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value, serde_json::json!({"a": "http://x", "b": [1, 2]}));
    }

    #[test]
    fn keeps_string_contents() {
        assert_eq!(strip_json_comments(r#"{"a": "x,]// y"}"#), r#"{"a": "x,]// y"}"#);
        assert_eq!(strip_json_comments(r#"{"a": "q\"//,}"}"#), r#"{"a": "q\"//,}"}"#);
    }
}
