//! Port of `packages/coding-agent/src/utils/frontmatter.ts`.
//!
//! YAML is parsed with `yaml-rust2` (YAML 1.2 core schema, like the `yaml` npm package) and
//! converted to JSON values.

use crate::utils::text::strip_bom;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use yaml_rust2::{Yaml, YamlLoader};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedFrontmatter {
    pub frontmatter: Value,
    pub body: String,
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

/// Returns `(yamlString, body)`; `yamlString` is `None` when there is no terminated frontmatter.
fn extract_frontmatter(content: &str) -> (Option<String>, String) {
    let normalized = normalize_newlines(&strip_bom(content));

    if !normalized.starts_with("---") {
        return (None, normalized);
    }

    let Some(end_index) = normalized[3..].find("\n---").map(|i| i + 3) else {
        return (None, normalized);
    };

    let yaml = normalized.get(4..end_index).unwrap_or("").to_string();
    let body = normalized
        .get(end_index + 4..)
        .unwrap_or("")
        .trim()
        .to_string();
    (Some(yaml), body)
}

fn yaml_key_to_string(key: &Yaml) -> String {
    match key {
        Yaml::String(s) | Yaml::Real(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".to_string(),
        other => yaml_to_json(other).to_string(),
    }
}

/// Convert a parsed YAML node to JSON.
pub fn yaml_to_json(yaml: &Yaml) -> Value {
    match yaml {
        Yaml::Real(s) => {
            let parsed = match s.as_str() {
                ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => None,
                "-.inf" | "-.Inf" | "-.INF" => None,
                ".nan" | ".NaN" | ".NAN" => None,
                other => other.parse::<f64>().ok(),
            };
            match parsed.and_then(Number::from_f64) {
                Some(n) => {
                    // Whole numbers are integers in JS.
                    let f = n.as_f64().unwrap_or(0.0);
                    if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
                        Value::Number(Number::from(f as i64))
                    } else {
                        Value::Number(n)
                    }
                }
                None => Value::Null,
            }
        }
        Yaml::Integer(i) => Value::Number(Number::from(*i)),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(items) => Value::Array(items.iter().map(yaml_to_json).collect()),
        Yaml::Hash(hash) => {
            let mut map = Map::new();
            for (k, v) in hash {
                map.insert(yaml_key_to_string(k), yaml_to_json(v));
            }
            Value::Object(map)
        }
        Yaml::Alias(_) | Yaml::Null | Yaml::BadValue => Value::Null,
    }
}

/// Parse a YAML document into JSON. Empty or comment-only documents yield `null`.
pub fn parse_yaml(source: &str) -> anyhow::Result<Value> {
    let docs = YamlLoader::load_from_str(source).map_err(|e| {
        anyhow::anyhow!(
            "{} at line {}, column {}",
            e.info(),
            e.marker().line(),
            e.marker().col() + 1
        )
    })?;
    Ok(docs.first().map(yaml_to_json).unwrap_or(Value::Null))
}

/// Parse YAML frontmatter. Errors when the frontmatter is not valid YAML.
pub fn parse_frontmatter(content: &str) -> anyhow::Result<ParsedFrontmatter> {
    let (yaml_string, body) = extract_frontmatter(content);
    let Some(yaml_string) = yaml_string.filter(|s| !s.is_empty()) else {
        return Ok(ParsedFrontmatter {
            frontmatter: Value::Object(Map::new()),
            body,
        });
    };
    let parsed = parse_yaml(&yaml_string)?;
    let frontmatter = if parsed.is_null() {
        Value::Object(Map::new())
    } else {
        parsed
    };
    Ok(ParsedFrontmatter { frontmatter, body })
}

pub fn strip_frontmatter(content: &str) -> anyhow::Result<String> {
    Ok(parse_frontmatter(content)?.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_strips_quotes_and_returns_body() {
        let input =
            "---\nname: \"skill-name\"\ndescription: 'A desc'\nfoo-bar: value\n---\n\nBody text";
        let parsed = parse_frontmatter(input).unwrap();
        assert_eq!(parsed.frontmatter["name"], "skill-name");
        assert_eq!(parsed.frontmatter["description"], "A desc");
        assert_eq!(parsed.frontmatter["foo-bar"], "value");
        assert_eq!(parsed.body, "Body text");
    }

    #[test]
    fn normalizes_newlines_and_handles_crlf() {
        let input = "---\r\nname: test\r\n---\r\nLine one\r\nLine two";
        assert_eq!(parse_frontmatter(input).unwrap().body, "Line one\nLine two");
    }

    #[test]
    fn errors_on_invalid_yaml_frontmatter() {
        let input = "---\nfoo: [bar\n---\nBody";
        assert!(parse_frontmatter(input).is_err());
    }

    #[test]
    fn parses_block_scalar_multiline() {
        let input = "---\ndescription: |\n  Line one\n  Line two\n---\n\nBody";
        let parsed = parse_frontmatter(input).unwrap();
        assert_eq!(parsed.frontmatter["description"], "Line one\nLine two\n");
        assert_eq!(parsed.body, "Body");
    }

    #[test]
    fn returns_original_content_when_missing_or_unterminated() {
        let no_frontmatter = "Just text\nsecond line";
        let missing_end = "---\nname: test\nBody without terminator";
        assert_eq!(
            parse_frontmatter(no_frontmatter).unwrap().body,
            "Just text\nsecond line"
        );
        assert_eq!(parse_frontmatter(missing_end).unwrap().body, missing_end);
    }

    #[test]
    fn returns_empty_object_for_comment_only_frontmatter() {
        let input = "---\n# just a comment\n---\nBody";
        assert_eq!(
            parse_frontmatter(input).unwrap().frontmatter,
            serde_json::json!({})
        );
    }

    #[test]
    fn strip_frontmatter_removes_and_trims() {
        assert_eq!(
            strip_frontmatter("---\nkey: value\n---\n\nBody\n").unwrap(),
            "Body"
        );
        assert_eq!(
            strip_frontmatter("\n  No frontmatter body  \n").unwrap(),
            "\n  No frontmatter body  \n"
        );
    }
}
