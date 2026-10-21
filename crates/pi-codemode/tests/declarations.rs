//! Port of `packages/codemode/test/declarations.test.ts`.

use pi_codemode::{
    CodemodeToolDeclaration, RenderDeclarationsOptions, render_declarations, render_tool_sample, render_tool_signature,
    schema_to_type,
};
use serde_json::{Value, json};

fn ty(schema: Value) -> String {
    schema_to_type(&schema, None)
}

fn decl(name: &str) -> CodemodeToolDeclaration {
    CodemodeToolDeclaration { name: name.to_string(), ..Default::default() }
}

#[test]
fn renders_primitives_literals_and_unions() {
    assert_eq!(ty(json!({ "type": "string" })), "string");
    assert_eq!(ty(json!({ "type": "integer" })), "number");
    assert_eq!(ty(json!({ "type": ["string", "null"] })), "string | null");
    assert_eq!(ty(json!({ "const": "a" })), "\"a\"");
    assert_eq!(ty(json!({ "enum": ["a", 1, null] })), "\"a\" | 1 | null");
    assert_eq!(ty(json!({ "anyOf": [{ "type": "string" }, { "type": "number" }] })), "string | number");
    assert_eq!(ty(json!({ "anyOf": [{ "type": "string" }, {}] })), "unknown");
    assert_eq!(
        ty(json!({ "allOf": [{ "anyOf": [{ "type": "string" }, { "type": "number" }] }, { "const": 1 }] })),
        "(string | number) & 1"
    );
    assert_eq!(ty(json!({ "$ref": "#/defs/x" })), "unknown");
    assert_eq!(ty(json!(true)), "unknown");
    assert_eq!(ty(json!(false)), "never");
}

#[test]
fn renders_objects_on_one_line_with_sorted_properties() {
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": { "city": { "type": "string" }, "max-lines": { "type": "number" } },
            "required": ["city"],
            "additionalProperties": false,
        })),
        "{ city: string; \"max-lines\"?: number; }"
    );
    assert_eq!(ty(json!({ "type": "object", "additionalProperties": { "type": "number" } })), "{ [key: string]: number; }");
    assert_eq!(ty(json!({ "type": "object" })), "{ [key: string]: unknown; }");
    assert_eq!(ty(json!({ "type": "object", "properties": {}, "additionalProperties": false })), "{}");
}

#[test]
fn puts_property_descriptions_on_comment_lines() {
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": {
                "weather": {
                    "type": "array",
                    "description": "look up weather for a given list of locations",
                    "items": { "type": "object", "properties": { "location": { "type": "string" } }, "required": ["location"] },
                },
            },
            "required": ["weather"],
        })),
        "{\n  // look up weather for a given list of locations\n  weather: Array<{ location: string; }>;\n}"
    );
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": {
                "outer": {
                    "type": "object",
                    "description": "Outer",
                    "properties": { "inner": { "type": "string", "description": "Inner" } },
                },
            },
        })),
        "{\n  // Outer\n  outer?: {\n    // Inner\n    inner?: string;\n  };\n}"
    );
}

// https://github.com/earendil-works/pi/issues/10707
#[test]
fn puts_constraints_the_type_cannot_express_on_property_comment_lines() {
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": {
                "timeoutMs": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 30000,
                    "default": 0,
                    "description": "Wait in milliseconds.",
                },
                "id": { "type": "string", "pattern": "^[a-z]+$", "maxLength": 8 },
                "tags": { "type": "array", "items": { "type": "string" }, "maxItems": 5, "uniqueItems": true, "deprecated": true },
                "name": { "type": "string" },
            },
            "required": ["id"],
        })),
        [
            "{",
            "  // @maxLength 8 @pattern ^[a-z]+$",
            "  id: string;",
            "  name?: string;",
            "  // @maxItems 5 @uniqueItems @deprecated",
            "  tags?: Array<string>;",
            "  // Wait in milliseconds.",
            "  // @integer @minimum 0 @maximum 30000 @default 0",
            "  timeoutMs?: number;",
            "}",
        ]
        .join("\n")
    );
}

#[test]
fn reads_constraints_of_optional_fields_from_the_non_null_branch() {
    // Pydantic's `limit: int | None = Field(None, ge=1, le=100)`.
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": {
                "limit": {
                    "anyOf": [{ "type": "integer", "minimum": 1, "maximum": 100 }, { "type": "null" }],
                    "default": null,
                    "title": "Limit",
                },
            },
        })),
        "{\n  // @integer @minimum 1 @maximum 100\n  limit?: number | null;\n}"
    );
    // Constraints of the property and the branch both apply.
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": {
                "n": {
                    "anyOf": [{ "type": "integer", "minimum": 10, "maximum": 50, "format": "int32" }, { "type": "null" }],
                    "minimum": 0,
                    "maximum": 20,
                    "format": "port",
                },
            },
        })),
        "{\n  // @integer @minimum 10 @maximum 20 @format port @format int32\n  n?: number | null;\n}"
    );
}

#[test]
fn reads_draft_04_boolean_exclusive_bounds() {
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": {
                "n": { "type": "integer", "minimum": 0, "exclusiveMinimum": true, "maximum": 10, "exclusiveMaximum": false },
                "m": { "anyOf": [{ "type": "number", "maximum": 5, "exclusiveMaximum": true }, { "type": "null" }], "maximum": 8 },
            },
        })),
        [
            "{",
            "  // @maximum 8 @exclusiveMaximum 5",
            "  m?: number | null;",
            "  // @integer @exclusiveMinimum 0 @maximum 10",
            "  n?: number;",
            "}",
        ]
        .join("\n")
    );
}

#[test]
fn leaves_out_long_tags_and_escapes_line_terminators() {
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": { "config": { "type": "object", "default": { "text": "x".repeat(100) } } },
        })),
        "{ config?: { [key: string]: unknown; }; }"
    );
    assert_eq!(
        ty(json!({
            "type": "object",
            "properties": {
                "sep": { "type": "string", "pattern": "^[\r\n\u{2028}\u{2029}]$", "default": "\u{2028}", "description": "a\u{2028}b" },
            },
        })),
        [
            "{",
            "  // a",
            "  // b",
            "  // @pattern ^[\\u000d\\u000a\\u2028\\u2029]$ @default \"\\u2028\"",
            "  sep?: string;",
            "}",
        ]
        .join("\n")
    );
}

#[test]
fn drops_constraint_tags_before_rendering_a_type_over_the_budget_as_unknown() {
    let schema = json!({ "type": "object", "properties": { "n": { "type": "integer", "minimum": 1 } } });
    assert_eq!(schema_to_type(&schema, None), "{\n  // @integer @minimum 1\n  n?: number;\n}");
    assert_eq!(schema_to_type(&schema, Some(20)), "{ n?: number; }");
    assert_eq!(schema_to_type(&schema, Some(10)), "unknown");
}

#[test]
fn resolves_local_references_and_stops_at_recursive_ones() {
    let schema = json!({
        "type": "object",
        "properties": {
            "item": { "$ref": "#/$defs/Item" },
            "legacy": { "$ref": "#/definitions/Legacy" },
            "remote": { "$ref": "https://example.com/schema.json" },
        },
        "required": ["item"],
        "$defs": {
            "Item": {
                "type": "object",
                "properties": { "id": { "type": "string" }, "parent": { "$ref": "#/$defs/Item" } },
                "required": ["id"],
            },
        },
        "definitions": { "Legacy": { "enum": ["a", "b"] } },
    });
    assert_eq!(
        schema_to_type(&schema, None),
        "{ item: { id: string; parent?: unknown; }; legacy?: \"a\" | \"b\"; remote?: unknown; }"
    );
}

#[test]
fn renders_arrays_and_tuples() {
    assert_eq!(ty(json!({ "type": "array", "items": { "type": "string" } })), "Array<string>");
    assert_eq!(ty(json!({ "type": "array", "prefixItems": [{ "type": "string" }, { "type": "number" }] })), "[string, number]");
    assert_eq!(ty(json!({ "type": "array" })), "unknown[]");
}

#[test]
fn renders_types_over_the_budget_as_unknown() {
    let properties: serde_json::Map<String, Value> =
        (0..50).map(|i| (format!("field{i}"), json!({ "type": "string" }))).collect();
    let schema = json!({ "type": "object", "properties": properties });
    assert_eq!(schema_to_type(&schema, Some(100)), "unknown");
    assert!(schema_to_type(&schema, None).contains("field49?: string;"));
}

#[test]
fn renders_signatures_with_normalized_identifiers() {
    let tool = CodemodeToolDeclaration {
        input_schema: Some(json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"],
            "additionalProperties": false,
        })),
        output_schema: Some(json!({ "type": "object", "properties": { "ok": { "type": "boolean" } }, "required": ["ok"] })),
        ..decl("hidden-dynamic-tool")
    };
    assert_eq!(render_tool_signature(&tool, None), "hidden_dynamic_tool(args: { city: string; }): Promise<{ ok: boolean; }>;");
    assert_eq!(render_tool_signature(&decl("free"), None), "free(args: unknown): Promise<unknown>;");
}

#[test]
fn renders_output_schemas_shaped_like_an_mcp_call_tool_result_as_plain_types() {
    let tool = CodemodeToolDeclaration {
        input_schema: Some(json!({ "type": "object", "properties": {} })),
        output_schema: Some(json!({
            "type": "object",
            "properties": {
                "content": { "type": "array", "items": { "type": "object" } },
                "isError": { "type": "boolean" },
                "_meta": { "type": "object" },
            },
            "required": ["content"],
        })),
        ..decl("plain")
    };
    assert_eq!(
        render_tool_signature(&tool, None),
        "plain(args: { [key: string]: unknown; }): Promise<{ _meta?: { [key: string]: unknown; }; content: Array<{ [key: string]: unknown; }>; isError?: boolean; }>;"
    );
}

#[test]
fn renders_the_per_tool_sample() {
    let tool = CodemodeToolDeclaration {
        description: Some("bar".into()),
        input_schema: Some(json!({ "type": "string" })),
        ..decl("foo")
    };
    assert_eq!(
        render_tool_sample(&tool, None),
        "bar\n\ncodemode tool declaration:\n```ts\ndeclare const tools: { foo(args: string): Promise<unknown>; };\n```"
    );
}

#[test]
fn renders_tools_and_globals() {
    let tools = vec![
        CodemodeToolDeclaration {
            description: Some("Read a file.\nSecond line.".into()),
            input_schema: Some(json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] })),
            output_schema: Some(json!({ "type": "string" })),
            ..decl("read")
        },
        decl("remote-api"),
    ];
    let globals = vec![CodemodeToolDeclaration {
        description: Some("Attach it.".into()),
        input_schema: Some(json!({ "type": "string" })),
        ..decl("attach")
    }];
    let text = render_declarations(RenderDeclarationsOptions { tools: &tools, globals: &globals });
    assert_eq!(
        text,
        [
            "declare const tools: {",
            "  /**",
            "   * Read a file.",
            "   * Second line.",
            "   */",
            "  read(args: { path: string; }): Promise<string>;",
            "  remote_api(args: unknown): Promise<unknown>;",
            "};",
            "",
            "/** Attach it. */",
            "declare function attach(args: string): Promise<unknown>;",
        ]
        .join("\n")
    );
}

#[test]
fn renders_namespaced_globals_and_explicit_signatures() {
    let globals = vec![
        CodemodeToolDeclaration {
            description: Some("List models.".into()),
            signature: Some("(type: string): Promise<string[]>".into()),
            ..decl("models.list")
        },
        CodemodeToolDeclaration { input_schema: Some(json!({ "type": "string" })), ..decl("models.get") },
        CodemodeToolDeclaration { signature: Some("(): void".into()), ..decl("plain") },
    ];
    let tools: Vec<CodemodeToolDeclaration> = Vec::new();
    let text = render_declarations(RenderDeclarationsOptions { tools: &tools, globals: &globals });
    assert_eq!(
        text,
        [
            "declare function plain(): void;",
            "",
            "declare const models: {",
            "  /** List models. */",
            "  list(type: string): Promise<string[]>;",
            "  get(args: string): Promise<unknown>;",
            "};",
        ]
        .join("\n")
    );
}

#[test]
fn escapes_comment_terminators_in_descriptions() {
    let tools = vec![CodemodeToolDeclaration { description: Some("a */ b".into()), ..decl("x") }];
    let globals: Vec<CodemodeToolDeclaration> = Vec::new();
    let text = render_declarations(RenderDeclarationsOptions { tools: &tools, globals: &globals });
    assert!(text.contains("/** a *\\/ b */"));
}
