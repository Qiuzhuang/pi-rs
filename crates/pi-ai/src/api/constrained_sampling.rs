//! Provider constrained sampling helpers (port of `api/constrained-sampling.ts`).

use serde_json::{Value, json};

use crate::types::{ConstrainedSamplingConfig, JsonSchemaStrictness, Tool};

#[derive(Debug)]
struct UnsupportedStrictJsonSchemaError(String);

/// Returns true when a provider's strict mode rejects this schema keyword with this value.
pub type UnsupportedStrictSchemaKeywordCheck = fn(&str, &Value) -> bool;

const UNSUPPORTED_STRICT_SCHEMA_KEYS: [&str; 16] = [
    "$ref",
    "$defs",
    "definitions",
    "allOf",
    "oneOf",
    "patternProperties",
    "dependentSchemas",
    "dependencies",
    "unevaluatedProperties",
    "propertyNames",
    "contains",
    "prefixItems",
    "not",
    "if",
    "then",
    "else",
];

fn is_structured_schema(schema: &Value) -> bool {
    let Some(obj) = schema.as_object() else { return false };
    let types: Vec<&str> = match obj.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    types.contains(&"object") || types.contains(&"array") || obj.contains_key("properties") || obj.contains_key("items")
}

fn schema_allows_null(schema: &Value) -> bool {
    let Some(obj) = schema.as_object() else { return false };
    match obj.get("type") {
        Some(Value::String(t)) if t == "null" => return true,
        Some(Value::Array(a)) if a.iter().any(|t| t == "null") => return true,
        _ => {}
    }
    if obj.get("const") == Some(&Value::Null) {
        return true;
    }
    if let Some(Value::Array(options)) = obj.get("enum") {
        if options.contains(&Value::Null) {
            return true;
        }
    }
    matches!(obj.get("anyOf"), Some(Value::Array(variants)) if variants.iter().any(schema_allows_null))
}

fn make_node_strict(schema: &mut Value, check: Option<UnsupportedStrictSchemaKeywordCheck>) -> Result<(), UnsupportedStrictJsonSchemaError> {
    let Some(obj) = schema.as_object_mut() else {
        return Err(UnsupportedStrictJsonSchemaError("boolean schemas are unsupported".into()));
    };
    for key in UNSUPPORTED_STRICT_SCHEMA_KEYS {
        if obj.contains_key(key) {
            return Err(UnsupportedStrictJsonSchemaError(format!("{key} schemas are unsupported")));
        }
    }
    if let Some(check) = check {
        for (key, value) in obj.iter() {
            if check(key, value) {
                return Err(UnsupportedStrictJsonSchemaError(format!("{key}: {value} is unsupported")));
            }
        }
    }
    if let Some(any_of) = obj.get_mut("anyOf") {
        let Some(variants) = any_of.as_array_mut().filter(|a| !a.is_empty()) else {
            return Err(UnsupportedStrictJsonSchemaError("anyOf must contain at least one schema".into()));
        };
        for variant in variants.iter_mut() {
            if is_structured_schema(variant) {
                return Err(UnsupportedStrictJsonSchemaError("object and array unions are unsupported".into()));
            }
            make_node_strict(variant, check)?;
        }
    }
    if let Some(items) = obj.get_mut("items") {
        if items.is_array() {
            return Err(UnsupportedStrictJsonSchemaError("tuple schemas are unsupported".into()));
        }
        make_node_strict(items, check)?;
    }
    let is_object_schema = obj.get("type").and_then(Value::as_str) == Some("object");
    if obj.contains_key("properties") && !is_object_schema {
        return Err(UnsupportedStrictJsonSchemaError("properties require type object".into()));
    }
    if !is_object_schema {
        return Ok(());
    }
    if let Some(additional) = obj.get("additionalProperties") {
        if additional != &Value::Bool(false) {
            return Err(UnsupportedStrictJsonSchemaError("schema-valued or true additionalProperties is unsupported".into()));
        }
    }
    if let Some(properties) = obj.get("properties") {
        if !properties.is_object() {
            return Err(UnsupportedStrictJsonSchemaError("object properties must be a schema map".into()));
        }
    }
    if let Some(required) = obj.get("required") {
        let valid = required.as_array().is_some_and(|r| r.iter().all(Value::is_string));
        if !valid {
            return Err(UnsupportedStrictJsonSchemaError("object required must be a string array".into()));
        }
    }
    let required: Vec<String> = obj
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let property_names: Vec<String> =
        obj.get("properties").and_then(Value::as_object).map(|p| p.keys().cloned().collect()).unwrap_or_default();
    if required.iter().any(|k| !property_names.contains(k)) {
        return Err(UnsupportedStrictJsonSchemaError("required contains an unknown property".into()));
    }
    if let Some(Value::Object(properties)) = obj.get_mut("properties") {
        for (key, property) in properties.iter_mut() {
            make_node_strict(property, check)?;
            if !required.contains(key) && !schema_allows_null(property) {
                let inner = property.take();
                *property = json!({ "anyOf": [inner, { "type": "null" }] });
            }
        }
    }
    obj.insert("required".into(), Value::Array(property_names.into_iter().map(Value::String).collect()));
    obj.insert("additionalProperties".into(), Value::Bool(false));
    Ok(())
}

/// Convert a tool schema to the strict subset expected by provider constrained sampling.
pub fn make_strict_json_schema(schema: &Value, check: Option<UnsupportedStrictSchemaKeywordCheck>) -> Result<Value, String> {
    let mut cloned = schema.clone();
    if !cloned.is_object() {
        return Err("root schema must have type object".into());
    }
    make_node_strict(&mut cloned, check).map_err(|e| e.0)?;
    if cloned.get("type").and_then(Value::as_str) != Some("object") {
        return Err("root schema must have type object".into());
    }
    Ok(cloned)
}

pub fn get_json_schema_tool_parameters(tool: &Tool, strict: Option<bool>) -> Value {
    if strict == Some(true) {
        make_strict_json_schema(&tool.parameters, None).unwrap_or_else(|_| tool.parameters.clone())
    } else {
        tool.parameters.clone()
    }
}

/// Decide whether a JSON-schema tool is sent in strict mode.
pub fn resolve_json_schema_strict_sampling(
    tool: &Tool,
    supports_strict_mode: bool,
    check: Option<UnsupportedStrictSchemaKeywordCheck>,
) -> anyhow::Result<Option<bool>> {
    let Some(ConstrainedSamplingConfig::JsonSchema { strict }) = tool.constrained_sampling.as_ref().and_then(|c| c.config()) else {
        return Ok(None);
    };
    if supports_strict_mode {
        return match make_strict_json_schema(&tool.parameters, check) {
            Ok(_) => Ok(Some(true)),
            Err(message) => {
                if *strict != JsonSchemaStrictness::Require {
                    Ok(None)
                } else {
                    anyhow::bail!("Tool \"{}\" requires JSON-schema constrained sampling, but {message}.", tool.name)
                }
            }
        };
    }
    if *strict == JsonSchemaStrictness::Require {
        anyhow::bail!("Tool \"{}\" requires JSON-schema constrained sampling, but strict tools are unsupported.", tool.name);
    }
    Ok(None)
}

/// Grammar-constrained tool sampling (OpenAI custom tools).
#[derive(Debug, Clone, PartialEq)]
pub struct GrammarConstrainedSampling {
    pub format: String,
    pub definition: String,
    pub input_property: String,
}

fn infer_grammar_input_property(tool: &Tool) -> Result<String, String> {
    let schema = &tool.parameters;
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err("grammar constrained sampling requires an object parameter schema".into());
    }
    let required = schema.get("required").and_then(Value::as_array);
    let Some(input) = required.filter(|r| r.len() == 1).and_then(|r| r[0].as_str()) else {
        return Err("grammar constrained sampling requires exactly one required string property".into());
    };
    let Some(property) = schema.get("properties").and_then(|p| p.get(input)) else {
        return Err(format!("grammar constrained sampling requires a properties entry for {input}"));
    };
    if property.get("type").and_then(Value::as_str) != Some("string") {
        return Err(format!("grammar constrained sampling property {input} must have type string"));
    }
    Ok(input.to_string())
}

pub fn resolve_grammar_constrained_sampling(
    tool: &Tool,
    supports_openai_grammar_tools: bool,
) -> anyhow::Result<Option<GrammarConstrainedSampling>> {
    let Some(ConstrainedSamplingConfig::Grammar { variants }) = tool.constrained_sampling.as_ref().and_then(|c| c.config()) else {
        return Ok(None);
    };
    if !supports_openai_grammar_tools {
        return Ok(None);
    }
    let lark = variants.get(&crate::types::GrammarFormat::OpenaiLark).filter(|d| !d.trim().is_empty());
    let regex = variants.get(&crate::types::GrammarFormat::OpenaiRegex).filter(|d| !d.trim().is_empty());
    if lark.is_none() && regex.is_none() {
        anyhow::bail!("Tool \"{}\" cannot use grammar constrained sampling: no supported grammar variant was provided.", tool.name);
    }
    let input_property = infer_grammar_input_property(tool)
        .map_err(|m| anyhow::anyhow!("Tool \"{}\" cannot use grammar constrained sampling: {m}.", tool.name))?;
    Ok(Some(GrammarConstrainedSampling {
        format: if lark.is_some() { "lark".into() } else { "regex".into() },
        definition: lark.or(regex).cloned().unwrap_or_default(),
        input_property,
    }))
}

/// Streaming state for re-encoding grammar tool input as JSON argument deltas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GrammarToolInputJsonBuffer {
    pub input: String,
    pub started: bool,
    pub closed: bool,
}

/// The raw grammar input string of a grammar tool call.
pub fn get_grammar_tool_input(
    tool_name: &str,
    arguments: &serde_json::Map<String, Value>,
    input_property: &str,
) -> anyhow::Result<String> {
    match arguments.get(input_property) {
        Some(Value::String(input)) => Ok(input.clone()),
        _ => anyhow::bail!("Grammar tool call \"{tool_name}\" requires argument \"{input_property}\" to be a string."),
    }
}

/// Append grammar input and return the JSON text delta for `{"<property>":"<input>"}`.
pub fn append_grammar_tool_input_json_delta(
    buffer: &mut GrammarToolInputJsonBuffer,
    input_property: &str,
    next_input: &str,
    close: bool,
) -> anyhow::Result<Option<String>> {
    if buffer.closed {
        if close && next_input == buffer.input {
            return Ok(None);
        }
        anyhow::bail!("grammar tool input for property \"{input_property}\" changed after it was closed");
    }
    if !next_input.starts_with(buffer.input.as_str()) {
        anyhow::bail!("grammar tool input for property \"{input_property}\" changed non-monotonically");
    }
    let input_delta = &next_input[buffer.input.len()..];
    if !close && input_delta.is_empty() {
        return Ok(None);
    }
    let mut delta = String::new();
    if !buffer.started {
        delta.push('{');
        delta.push_str(&Value::String(input_property.to_string()).to_string());
        delta.push_str(":\"");
        buffer.started = true;
    }
    let encoded = Value::String(input_delta.to_string()).to_string();
    delta.push_str(&encoded[1..encoded.len() - 1]);
    buffer.input = next_input.to_string();
    if close {
        delta.push_str("\"}");
        buffer.closed = true;
    }
    Ok(Some(delta))
}

/// Map of grammar tool names to their input property.
pub fn create_grammar_tool_input_properties(
    tools: &[Tool],
    supports_openai_grammar_tools: bool,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let mut properties = std::collections::HashMap::new();
    for tool in tools {
        if let Some(grammar) = resolve_grammar_constrained_sampling(tool, supports_openai_grammar_tools)? {
            properties.insert(tool.name.clone(), grammar.input_property);
        }
    }
    Ok(properties)
}
