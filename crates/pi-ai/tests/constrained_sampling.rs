//! Tests for strict JSON-schema conversion and grammar input buffers (the provider-independent
//! part of `constrained-sampling.test.ts`).

use pi_ai::api::constrained_sampling::{
    GrammarToolInputJsonBuffer, append_grammar_tool_input_json_delta, make_strict_json_schema,
    resolve_json_schema_strict_sampling,
};
use pi_ai::*;
use serde_json::{Value, json};

#[test]
fn derives_strict_schemas_without_changing_tool_definitions() {
    let parameters = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" },
            "offset": { "type": "number" },
            "metadata": { "type": "object", "properties": { "enabled": { "type": "boolean" } } },
            "nullable": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
        },
        "required": ["path", "metadata"]
    });
    let original = parameters.clone();
    let strict = make_strict_json_schema(&parameters, None).unwrap();
    assert_eq!(parameters, original);
    assert!(parameters.get("additionalProperties").is_none());
    assert_eq!(strict["additionalProperties"], false);
    assert_eq!(strict["required"], json!(["path", "offset", "metadata", "nullable"]));
    assert_eq!(strict["properties"]["offset"], json!({ "anyOf": [{ "type": "number" }, { "type": "null" }] }));
    assert_eq!(strict["properties"]["metadata"]["additionalProperties"], false);
    assert_eq!(strict["properties"]["metadata"]["required"], json!(["enabled"]));
    assert_eq!(strict["properties"]["metadata"]["properties"]["enabled"], json!({ "anyOf": [{ "type": "boolean" }, { "type": "null" }] }));
    assert_eq!(strict["properties"]["nullable"], json!({ "anyOf": [{ "type": "string" }, { "type": "null" }] }));
}

#[test]
fn falls_back_or_rejects_unconvertible_schemas() {
    let cases: Vec<(Value, &str)> = vec![
        (
            json!({ "type": "object", "properties": { "metadata": { "type": "object", "properties": {}, "additionalProperties": { "type": "string" } } }, "required": ["metadata"] }),
            "additionalProperties is unsupported",
        ),
        (
            json!({ "allOf": [{ "type": "object", "properties": { "a": { "type": "string" } }, "required": ["a"] }, { "type": "object", "properties": { "b": { "type": "number" } }, "required": ["b"] }] }),
            "allOf schemas are unsupported",
        ),
        (
            json!({ "type": "object", "properties": { "value": { "anyOf": [{ "type": "object", "properties": { "nested": { "type": "string" } }, "required": ["nested"] }, { "type": "null" }] } }, "required": ["value"] }),
            "object and array unions are unsupported",
        ),
        (
            json!({ "type": "object", "properties": { "child": { "$ref": "https://example.com/child.json" } }, "required": ["child"] }),
            "$ref schemas are unsupported",
        ),
    ];
    for (parameters, error) in cases {
        let mut tool = Tool::new("sample_tool", "Sample tool", parameters.clone());
        tool.constrained_sampling =
            Some(ConstrainedSampling::Config(ConstrainedSamplingConfig::JsonSchema { strict: JsonSchemaStrictness::Prefer }));
        let message = make_strict_json_schema(&parameters, None).unwrap_err();
        assert!(message.contains(error), "{message}");
        assert_eq!(resolve_json_schema_strict_sampling(&tool, true, None).unwrap(), None);
        tool.constrained_sampling =
            Some(ConstrainedSampling::Config(ConstrainedSamplingConfig::JsonSchema { strict: JsonSchemaStrictness::Require }));
        let message = resolve_json_schema_strict_sampling(&tool, true, None).unwrap_err().to_string();
        assert!(message.contains(error), "{message}");
    }
}

#[test]
fn grammar_input_json_deltas_are_append_only() {
    let mut buffer = GrammarToolInputJsonBuffer::default();
    let first = append_grammar_tool_input_json_delta(&mut buffer, "payload", "a\"", false).unwrap().unwrap_or_default();
    let second = append_grammar_tool_input_json_delta(&mut buffer, "payload", "a\"\nb", true).unwrap().unwrap_or_default();
    let parsed: Value = serde_json::from_str(&format!("{first}{second}")).unwrap();
    assert_eq!(parsed, json!({ "payload": "a\"\nb" }));
    assert_eq!(append_grammar_tool_input_json_delta(&mut buffer, "payload", "a\"\nb", true).unwrap(), None);
    assert_eq!(
        append_grammar_tool_input_json_delta(&mut buffer, "payload", "changed", true).unwrap_err().to_string(),
        "grammar tool input for property \"payload\" changed after it was closed"
    );
}
