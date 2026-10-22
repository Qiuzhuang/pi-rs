//! Port of `packages/coding-agent/src/utils/html.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedHtmlEntity {
    pub text: String,
    /// Length of the entity in bytes (including `&` and `;`).
    pub length: usize,
}

fn decode_code_point(code_point: Option<i64>) -> Option<String> {
    let code_point = code_point?;
    if !(0..=0x10ffff).contains(&code_point) {
        return None;
    }
    // Lone surrogates cannot be represented in a Rust string; JS would produce one.
    char::from_u32(code_point as u32).map(String::from)
}

/// JS `Number.parseInt(value, radix)`: optional leading whitespace and sign, then the longest
/// run of valid digits. `None` for NaN.
fn parse_int(value: &str, radix: u32) -> Option<i64> {
    let trimmed = value.trim_start();
    let (negative, rest) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let rest = if radix == 16 {
        rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")).unwrap_or(rest)
    } else {
        rest
    };
    let digits: String = rest.chars().take_while(|c| c.is_digit(radix)).collect();
    if digits.is_empty() {
        return None;
    }
    let value = i64::from_str_radix(&digits, radix).unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
}

pub fn decode_html_entity(entity: &str) -> Option<String> {
    match entity {
        "amp" => return Some("&".into()),
        "lt" => return Some("<".into()),
        "gt" => return Some(">".into()),
        "quot" => return Some("\"".into()),
        "apos" => return Some("'".into()),
        _ => {}
    }
    if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
        return decode_code_point(parse_int(hex, 16));
    }
    if let Some(dec) = entity.strip_prefix('#') {
        return decode_code_point(parse_int(dec, 10));
    }
    None
}

/// Decode the entity starting at byte `index` (the `&`).
pub fn decode_html_entity_at(html: &str, index: usize) -> Option<DecodedHtmlEntity> {
    let search_from = index + 1;
    if search_from > html.len() {
        return None;
    }
    let semicolon_index = search_from + html.get(search_from..)?.find(';')?;
    // TS measures the distance in UTF-16 units; entities are ASCII, so bytes are equivalent
    // whenever the entity can decode.
    if semicolon_index - index > 16 {
        return None;
    }
    let entity = html.get(index + 1..semicolon_index)?;
    let decoded = decode_html_entity(entity)?;
    Some(DecodedHtmlEntity { text: decoded, length: semicolon_index - index + 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(decode_html_entity("amp").as_deref(), Some("&"));
        assert_eq!(decode_html_entity("#39").as_deref(), Some("'"));
        assert_eq!(decode_html_entity("#x27").as_deref(), Some("'"));
        assert_eq!(decode_html_entity("#X1F600").as_deref(), Some("😀"));
        assert_eq!(decode_html_entity("nbsp"), None);
        assert_eq!(decode_html_entity("#xZZ"), None);
        assert_eq!(decode_html_entity("#x110000"), None);
    }

    #[test]
    fn decodes_entities_in_place() {
        let html = "a &lt;b&gt; &amp;&unknown;";
        assert_eq!(
            decode_html_entity_at(html, 2),
            Some(DecodedHtmlEntity { text: "<".into(), length: 4 })
        );
        assert_eq!(decode_html_entity_at(html, 17), None);
        assert_eq!(decode_html_entity_at("&amp", 0), None);
        assert_eq!(decode_html_entity_at("&aaaaaaaaaaaaaaaaaaaa;", 0), None);
    }
}
