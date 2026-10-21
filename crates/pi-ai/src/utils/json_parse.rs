//! JSON parsing with repair and partial (streaming) parsing (port of `utils/json-parse.ts`
//! and the `partial-json` package it uses).

use serde_json::{Map, Value};

use crate::types::JsonObject;

fn is_valid_json_escape(c: char) -> bool {
    matches!(c, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't')
}

fn escape_control_character(c: char) -> String {
    match c {
        '\u{8}' => "\\b".to_string(),
        '\u{c}' => "\\f".to_string(),
        '\n' => "\\n".to_string(),
        '\r' => "\\r".to_string(),
        '\t' => "\\t".to_string(),
        other => format!("\\u{:04x}", other as u32),
    }
}

/// Repairs malformed JSON string literals by escaping raw control characters inside
/// strings and doubling backslashes before invalid escape characters.
pub fn repair_json(json: &str) -> String {
    let chars: Vec<char> = json.chars().collect();
    let mut repaired = String::with_capacity(json.len());
    let mut in_string = false;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if !in_string {
            repaired.push(c);
            if c == '"' {
                in_string = true;
            }
            index += 1;
            continue;
        }
        if c == '"' {
            repaired.push(c);
            in_string = false;
            index += 1;
            continue;
        }
        if c == '\\' {
            let Some(&next) = chars.get(index + 1) else {
                repaired.push_str("\\\\");
                index += 1;
                continue;
            };
            if next == 'u' {
                let digits: String = chars.iter().skip(index + 2).take(4).collect();
                if digits.len() == 4 && digits.chars().all(|d| d.is_ascii_hexdigit()) {
                    repaired.push_str("\\u");
                    repaired.push_str(&digits);
                    index += 6;
                    continue;
                }
            }
            if is_valid_json_escape(next) {
                repaired.push('\\');
                repaired.push(next);
                index += 2;
                continue;
            }
            repaired.push_str("\\\\");
            index += 1;
            continue;
        }
        if (c as u32) <= 0x1f {
            repaired.push_str(&escape_control_character(c));
        } else {
            repaired.push(c);
        }
        index += 1;
    }
    repaired
}

/// `JSON.parse` with a repair fallback.
pub fn parse_json_with_repair(json: &str) -> Result<Value, serde_json::Error> {
    match serde_json::from_str::<Value>(json) {
        Ok(v) => Ok(v),
        Err(error) => {
            let repaired = repair_json(json);
            if repaired != json {
                serde_json::from_str::<Value>(&repaired)
            } else {
                Err(error)
            }
        }
    }
}

/// Parsed tool call arguments: `arguments` plus `argumentsError` when the text was invalid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedToolCallArguments {
    pub arguments: JsonObject,
    pub arguments_error: Option<String>,
}

/// Mimic the message of a JavaScript `SyntaxError` from `JSON.parse` loosely.
fn json_error_message(error: &serde_json::Error) -> String {
    let text = error.to_string();
    // serde_json appends " at line X column Y"; keep it, it is informative.
    text
}

/// Parses the model's complete arguments text of a finished tool call.
pub fn parse_tool_call_arguments(json: Option<&str>) -> ParsedToolCallArguments {
    let Some(json) = json.filter(|j| !j.trim().is_empty()) else {
        return ParsedToolCallArguments::default();
    };
    let error = match parse_json_with_repair(json) {
        Ok(Value::Object(map)) => return ParsedToolCallArguments { arguments: map, arguments_error: None },
        Ok(_) => "Tool arguments must be a JSON object".to_string(),
        Err(e) => json_error_message(&e),
    };
    ParsedToolCallArguments {
        arguments: Map::new(),
        arguments_error: Some(format!("{error}\n\nReceived arguments:\n{json}")),
    }
}

/// Attempts to parse potentially incomplete JSON during streaming. Always returns a value;
/// an empty object when nothing could be parsed.
pub fn parse_streaming_json(partial_json: Option<&str>) -> Value {
    let Some(partial_json) = partial_json.filter(|p| !p.trim().is_empty()) else {
        return Value::Object(Map::new());
    };
    if let Ok(v) = parse_json_with_repair(partial_json) {
        return v;
    }
    if let Ok(v) = partial_parse(partial_json) {
        return v;
    }
    if let Ok(v) = partial_parse(&repair_json(partial_json)) {
        return v;
    }
    Value::Object(Map::new())
}

/// [`parse_streaming_json`] narrowed to an object (non-objects yield `{}`).
pub fn parse_streaming_json_object(partial_json: Option<&str>) -> JsonObject {
    match parse_streaming_json(partial_json) {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

// ---------------------------------------------------------------------------
// partial-json (Allow.ALL)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum PartialJsonError {
    Partial(String),
    Malformed(String),
}

impl std::fmt::Display for PartialJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartialJsonError::Partial(m) | PartialJsonError::Malformed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PartialJsonError {}

/// Parse incomplete JSON, allowing every type to be partial (`partial-json`'s `parse`).
/// `NaN`/`Infinity` become `null`.
pub fn partial_parse(json: &str) -> Result<Value, PartialJsonError> {
    if json.trim().is_empty() {
        return Err(PartialJsonError::Malformed(format!("{json} is empty")));
    }
    let trimmed = json.trim();
    let mut parser = PartialParser { s: trimmed.chars().collect(), index: 0 };
    parser.parse_any()
}

struct PartialParser {
    s: Vec<char>,
    index: usize,
}

/// JavaScript `substring(start, end)` semantics on a char vector.
fn js_substring(s: &[char], start: isize, end: isize) -> String {
    let len = s.len() as isize;
    let mut a = start.clamp(0, len);
    let mut b = end.clamp(0, len);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    s[a as usize..b as usize].iter().collect()
}

fn last_index_of(s: &[char], c: char) -> isize {
    s.iter().rposition(|&x| x == c).map(|p| p as isize).unwrap_or(-1)
}

impl PartialParser {
    fn len(&self) -> usize {
        self.s.len()
    }

    fn at(&self, i: usize) -> Option<char> {
        self.s.get(i).copied()
    }

    fn partial(&self, msg: &str) -> PartialJsonError {
        PartialJsonError::Partial(format!("{msg} at position {}", self.index))
    }

    fn malformed(&self, msg: &str) -> PartialJsonError {
        PartialJsonError::Malformed(format!("{msg} at position {}", self.index))
    }

    fn sub(&self, start: usize, end: usize) -> String {
        js_substring(&self.s, start as isize, end as isize)
    }

    fn rest_is_prefix_of(&self, word: &str, max_len: usize) -> bool {
        let remaining = self.len() - self.index;
        remaining < max_len && word.starts_with(&self.sub(self.index, self.len()))
    }

    fn parse_any(&mut self) -> Result<Value, PartialJsonError> {
        self.skip_blank();
        if self.index >= self.len() {
            return Err(self.partial("Unexpected end of input"));
        }
        match self.at(self.index) {
            Some('"') => return self.parse_str(),
            Some('{') => return self.parse_obj(),
            Some('[') => return self.parse_arr(),
            _ => {}
        }
        let i = self.index;
        if self.sub(i, i + 4) == "null" || self.rest_is_prefix_of("null", 4) {
            self.index += 4;
            return Ok(Value::Null);
        }
        if self.sub(i, i + 4) == "true" || self.rest_is_prefix_of("true", 4) {
            self.index += 4;
            return Ok(Value::Bool(true));
        }
        if self.sub(i, i + 5) == "false" || self.rest_is_prefix_of("false", 5) {
            self.index += 5;
            return Ok(Value::Bool(false));
        }
        if self.sub(i, i + 8) == "Infinity" || self.rest_is_prefix_of("Infinity", 8) {
            self.index += 8;
            return Ok(Value::Null);
        }
        let remaining = self.len() - i;
        if self.sub(i, i + 9) == "-Infinity"
            || (1 < remaining && remaining < 9 && "-Infinity".starts_with(&self.sub(i, self.len())))
        {
            self.index += 9;
            return Ok(Value::Null);
        }
        if self.sub(i, i + 3) == "NaN" || self.rest_is_prefix_of("NaN", 3) {
            self.index += 3;
            return Ok(Value::Null);
        }
        self.parse_num()
    }

    fn parse_str(&mut self) -> Result<Value, PartialJsonError> {
        let start = self.index;
        let mut escape = false;
        self.index += 1;
        while self.index < self.len()
            && (self.s[self.index] != '"' || (escape && self.s[self.index - 1] == '\\'))
        {
            escape = if self.s[self.index] == '\\' { !escape } else { false };
            self.index += 1;
        }
        if self.at(self.index) == Some('"') {
            self.index += 1;
            let end = self.index - usize::from(escape);
            let text = self.sub(start, end);
            return serde_json::from_str::<Value>(&text).map_err(|e| self.malformed(&e.to_string()));
        }
        // Allow.STR
        let text = self.sub(start, self.index - usize::from(escape)) + "\"";
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Ok(v),
            Err(_) => {
                let cut = last_index_of(&self.s, '\\');
                let text = js_substring(&self.s, start as isize, cut) + "\"";
                serde_json::from_str::<Value>(&text).map_err(|e| self.malformed(&e.to_string()))
            }
        }
    }

    fn parse_obj(&mut self) -> Result<Value, PartialJsonError> {
        self.index += 1;
        self.skip_blank();
        let mut obj = Map::new();
        loop {
            if self.at(self.index) == Some('}') {
                break;
            }
            self.skip_blank();
            if self.index >= self.len() {
                return Ok(Value::Object(obj));
            }
            let key = match self.parse_str() {
                Ok(k) => k,
                Err(_) => return Ok(Value::Object(obj)),
            };
            self.skip_blank();
            self.index += 1; // skip colon
            match self.parse_any() {
                Ok(value) => {
                    let key = match key {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    obj.insert(key, value);
                }
                Err(_) => return Ok(Value::Object(obj)),
            }
            self.skip_blank();
            if self.at(self.index) == Some(',') {
                self.index += 1;
            }
        }
        self.index += 1;
        Ok(Value::Object(obj))
    }

    fn parse_arr(&mut self) -> Result<Value, PartialJsonError> {
        self.index += 1;
        let mut arr = Vec::new();
        loop {
            if self.at(self.index) == Some(']') {
                break;
            }
            match self.parse_any() {
                Ok(v) => arr.push(v),
                Err(_) => return Ok(Value::Array(arr)),
            }
            self.skip_blank();
            if self.at(self.index) == Some(',') {
                self.index += 1;
            }
        }
        self.index += 1;
        Ok(Value::Array(arr))
    }

    fn parse_num(&mut self) -> Result<Value, PartialJsonError> {
        if self.index == 0 {
            let whole: String = self.s.iter().collect();
            if whole == "-" {
                return Err(self.malformed("Not sure what '-' is"));
            }
            return match serde_json::from_str::<Value>(&whole) {
                Ok(v) => Ok(v),
                Err(e) => {
                    let cut = last_index_of(&self.s, 'e');
                    if let Ok(v) = serde_json::from_str::<Value>(&js_substring(&self.s, 0, cut)) {
                        return Ok(v);
                    }
                    Err(self.malformed(&e.to_string()))
                }
            };
        }
        let start = self.index;
        if self.at(self.index) == Some('-') {
            self.index += 1;
        }
        while let Some(c) = self.at(self.index) {
            if ",]}".contains(c) {
                break;
            }
            self.index += 1;
        }
        let text = self.sub(start, self.index);
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Ok(v),
            Err(_) => {
                if text == "-" {
                    return Err(self.partial("Not sure what '-' is"));
                }
                let cut = last_index_of(&self.s, 'e');
                serde_json::from_str::<Value>(&js_substring(&self.s, start as isize, cut))
                    .map_err(|e| self.malformed(&e.to_string()))
            }
        }
    }

    fn skip_blank(&mut self) {
        while let Some(c) = self.at(self.index) {
            if " \n\r\t".contains(c) {
                self.index += 1;
            } else {
                break;
            }
        }
    }
}
