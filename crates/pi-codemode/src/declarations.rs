//! TypeScript declarations of the script-visible API, for model-facing tool descriptions.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::identifier::{is_identifier, to_codemode_identifier};
use crate::js;
use crate::types::{CodemodeJsonSchema, ToolDeclaration};

const INDENT: &str = "  ";
/// Largest rendered input type, in characters, before it becomes `unknown`.
pub const DEFAULT_INPUT_SCHEMA_MAX_CHARS: usize = 16_000;
/// Local `$ref` expansions per rendered schema, so shared definitions cannot blow up the output.
const MAX_REF_EXPANSIONS: usize = 32;
/// JSON Schema keywords with a number or string value shown as `@keyword value` property tags.
const TAG_KEYWORDS: [&str; 11] = [
    "minimum",
    "exclusiveMinimum",
    "maximum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "minItems",
    "maxItems",
];
/// Tag keywords whose values all hold at once, so only the largest (lower) or smallest (upper)
/// matters.
const LOWER_BOUNDS: [&str; 4] = ["minimum", "exclusiveMinimum", "minLength", "minItems"];
const UPPER_BOUNDS: [&str; 4] = ["maximum", "exclusiveMaximum", "maxLength", "maxItems"];
/// Longest property tag; longer ones (large defaults, long patterns) are left out.
const MAX_TAG_CHARS: usize = 80;

/// Characters that end a `//` comment.
fn is_line_terminator(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{2028}' | '\u{2029}')
}

/// What [`render_declarations`] renders.
pub struct RenderDeclarationsOptions<'a, T: ToolDeclaration = crate::types::CodemodeTool, G: ToolDeclaration = crate::types::CodemodeTool> {
    pub tools: &'a [T],
    pub globals: &'a [G],
}

/// Render TypeScript declarations for the script-visible API. Tools become members of
/// `declare const tools`, globals become `declare function` statements, and `ns.member` globals
/// members of `declare const ns`. Descriptions become doc comments; schemas become types.
pub fn render_declarations<T: ToolDeclaration, G: ToolDeclaration>(options: RenderDeclarationsOptions<'_, T, G>) -> String {
    let mut sections: Vec<String> = Vec::new();
    if !options.tools.is_empty() {
        let members: Vec<String> = options
            .tools
            .iter()
            .map(|tool| format!("{}{INDENT}{}", doc_comment(tool.description(), INDENT), render_tool_signature(tool, None)))
            .collect();
        sections.push(format!("declare const tools: {{\n{}\n}};", members.join("\n")));
    }
    let mut namespaces: Vec<(String, Vec<String>)> = Vec::new();
    for global in options.globals {
        let name = global.name();
        let Some(dot) = name.find('.') else {
            sections.push(render_global(&format!("declare function {name}"), global, ""));
            continue;
        };
        let namespace = &name[..dot];
        let member = render_global(&name[dot + 1..], global, INDENT);
        match namespaces.iter_mut().find(|(existing, _)| existing == namespace) {
            Some((_, members)) => members.push(member),
            None => namespaces.push((namespace.to_string(), vec![member])),
        }
    }
    for (namespace, members) in namespaces {
        sections.push(format!("declare const {namespace}: {{\n{}\n}};", members.join("\n")));
    }
    sections.join("\n\n")
}

/// One tool as a member of the `tools` object: `name(args: T): Promise<R>;` with the name as the
/// identifier scripts use. Input types longer than `input_max_chars` (default
/// [`DEFAULT_INPUT_SCHEMA_MAX_CHARS`]) render as `unknown`.
pub fn render_tool_signature(tool: &impl ToolDeclaration, input_max_chars: Option<usize>) -> String {
    let input = match tool.input_schema() {
        None => "unknown".to_string(),
        Some(schema) => schema_to_type(schema, Some(input_max_chars.unwrap_or(DEFAULT_INPUT_SCHEMA_MAX_CHARS))),
    };
    format!(
        "{}(args: {input}): Promise<{}>;",
        to_codemode_identifier(tool.name()),
        render_tool_output_type(tool.output_schema())
    )
}

/// A tool's sample: the description followed by the tool's declaration. Used for tool listings and
/// `ALL_TOOLS` entries.
pub fn render_tool_sample(tool: &impl ToolDeclaration, input_max_chars: Option<usize>) -> String {
    let declaration = format!("declare const tools: {{ {} }};", render_tool_signature(tool, input_max_chars));
    format!(
        "{}\n\ncodemode tool declaration:\n```ts\n{declaration}\n```",
        tool.description().map(js::trim).unwrap_or("")
    )
}

/// The type a tool call resolves to: the output schema's type, and `unknown` without a schema.
pub fn render_tool_output_type(schema: Option<&CodemodeJsonSchema>) -> String {
    match schema {
        None => "unknown".to_string(),
        Some(schema) => schema_to_type(schema, None),
    }
}

fn render_global(head: &str, global: &impl ToolDeclaration, indent: &str) -> String {
    let doc = doc_comment(global.description(), indent);
    if let Some(signature) = global.signature() {
        return format!("{doc}{indent}{head}{signature};");
    }
    let input = global.input_schema().map_or_else(|| "unknown".to_string(), |schema| schema_to_type(schema, None));
    let output = global.output_schema().map_or_else(|| "unknown".to_string(), |schema| schema_to_type(schema, None));
    format!("{doc}{indent}{head}(args: {input}): Promise<{output}>;")
}

fn doc_comment(description: Option<&str>, indent: &str) -> String {
    let text = description.map(js::trim).unwrap_or("");
    if text.is_empty() {
        return String::new();
    }
    let escaped = text.replace("*/", "*\\/");
    let lines: Vec<&str> = escaped.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    if lines.len() == 1 {
        return format!("{indent}/** {} */\n", lines[0]);
    }
    let body: Vec<String> = lines
        .iter()
        .map(|line| if line.is_empty() { format!("{indent} *") } else { format!("{indent} * {line}") })
        .collect();
    format!("{indent}/**\n{}\n{indent} */\n", body.join("\n"))
}

fn property_key(name: &str) -> String {
    if is_identifier(name) { name.to_string() } else { js::quote(name) }
}

fn union(types: Vec<String>) -> String {
    let mut unique: Vec<String> = Vec::new();
    for item in types {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    if unique.iter().any(|item| item == "unknown") {
        return "unknown".to_string();
    }
    if unique.is_empty() { "never".to_string() } else { unique.join(" | ") }
}

/// Convert a JSON Schema to a TypeScript type expression: objects on one line
/// (`{ a: string; b?: number; }`) with properties sorted by name, or one property per line with
/// `//` comments when a property has a description or constraints the type cannot express
/// (`// @integer @minimum 1 @default 20`); `Array<T>` for arrays. Local references
/// (`#/$defs/...`, `#/definitions/...`) resolve against `schema`; recursive and remote references
/// render as `unknown`. A result longer than `max_chars` is rendered again without the constraint
/// tags, and renders as `unknown` if it is still too long.
pub fn schema_to_type(schema: &CodemodeJsonSchema, max_chars: Option<usize>) -> String {
    let render = |tags: bool| {
        let mut context = SchemaContext { root: schema, resolving: Vec::new(), expansions: 0, tags };
        to_type(schema, &mut context)
    };
    let rendered = render(true);
    let Some(max_chars) = max_chars else { return rendered };
    if js::utf16_len(&rendered) <= max_chars {
        return rendered;
    }
    let untagged = render(false);
    if js::utf16_len(&untagged) <= max_chars { untagged } else { "unknown".to_string() }
}

struct SchemaContext<'a> {
    root: &'a Value,
    /// References being expanded on the current path, to stop at recursive types.
    resolving: Vec<String>,
    expansions: usize,
    /// Whether property comments include constraint tags.
    tags: bool,
}

/// `decodeURIComponent`, or `None` for malformed input.
fn decode_uri_component(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn resolve_ref<'a>(reference: &str, root: &'a Value) -> Option<&'a Value> {
    if reference != "#" && !reference.starts_with("#/") {
        return None;
    }
    let mut current = root;
    for segment in reference.get(2..).unwrap_or("").split('/').filter(|segment| !segment.is_empty()) {
        let key = decode_uri_component(segment)?.replace("~1", "/").replace("~0", "~");
        current = current.as_object()?.get(&key)?;
    }
    match current {
        Value::Bool(_) | Value::Object(_) => Some(current),
        _ => None,
    }
}

fn stringify_or_unknown(value: &Value) -> String {
    js::stringify(value)
}

fn to_type(schema: &Value, context: &mut SchemaContext<'_>) -> String {
    let object = match schema {
        Value::Bool(true) => return "unknown".to_string(),
        Value::Bool(false) => return "never".to_string(),
        Value::Object(object) => object,
        _ => return "unknown".to_string(),
    };
    if let Some(Value::String(reference)) = object.get("$ref") {
        if context.resolving.contains(reference) || context.expansions >= MAX_REF_EXPANSIONS {
            return "unknown".to_string();
        }
        let Some(target) = resolve_ref(reference, context.root) else { return "unknown".to_string() };
        context.expansions += 1;
        context.resolving.push(reference.clone());
        let rendered = to_type(target, context);
        context.resolving.retain(|existing| existing != reference);
        return rendered;
    }

    if let Some(constant) = object.get("const") {
        return stringify_or_unknown(constant);
    }
    if let Some(Value::Array(values)) = object.get("enum") {
        return union(values.iter().map(stringify_or_unknown).collect());
    }

    let variants = match (object.get("anyOf"), object.get("oneOf")) {
        (Some(Value::Array(variants)), _) => Some(variants),
        (_, Some(Value::Array(variants))) => Some(variants),
        _ => None,
    };
    if let Some(variants) = variants {
        return union(variants.iter().map(|variant| to_type(variant, context)).collect());
    }
    if let Some(Value::Array(parts)) = object.get("allOf") {
        let parts: Vec<String> =
            parts.iter().map(|part| to_type(part, context)).filter(|part| part != "unknown").collect();
        if parts.is_empty() {
            return "unknown".to_string();
        }
        return parts
            .iter()
            .map(|part| if part.contains(" | ") { format!("({part})") } else { part.clone() })
            .collect::<Vec<_>>()
            .join(" & ");
    }

    match object.get("type") {
        Some(Value::Array(types)) => union(
            types
                .iter()
                .map(|entry| {
                    let mut variant = object.clone();
                    variant.insert("type".to_string(), entry.clone());
                    to_type(&Value::Object(variant), context)
                })
                .collect(),
        ),
        Some(Value::String(kind)) => match kind.as_str() {
            "string" => "string".to_string(),
            "number" | "integer" => "number".to_string(),
            "boolean" => "boolean".to_string(),
            "null" => "null".to_string(),
            "array" => array_type(object, context),
            "object" => object_type(object, context),
            _ => "unknown".to_string(),
        },
        None => {
            if object.contains_key("properties") || object.contains_key("additionalProperties") || object.contains_key("required") {
                object_type(object, context)
            } else if object.contains_key("items") || object.contains_key("prefixItems") {
                array_type(object, context)
            } else {
                "unknown".to_string()
            }
        }
        Some(_) => "unknown".to_string(),
    }
}

fn array_type(schema: &Map<String, Value>, context: &mut SchemaContext<'_>) -> String {
    if let Some(items) = schema.get("items")
        && !items.is_array()
    {
        return format!("Array<{}>", to_type(items, context));
    }
    let tuple = match (schema.get("prefixItems"), schema.get("items")) {
        (Some(Value::Array(items)), _) => items.as_slice(),
        (_, Some(Value::Array(items))) => items.as_slice(),
        _ => &[],
    };
    if !tuple.is_empty() {
        let items: Vec<String> = tuple.iter().map(|item| to_type(item, context)).collect();
        return format!("[{}]", items.join(", "));
    }
    "unknown[]".to_string()
}

fn description_of(property: &Value) -> String {
    match property.get("description") {
        Some(Value::String(description)) => js::trim(description).to_string(),
        _ => String::new(),
    }
}

fn is_integer_type(kind: Option<&Value>) -> bool {
    match kind {
        Some(Value::String(kind)) => kind == "integer",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "integer"),
        _ => false,
    }
}

/// JavaScript `String(value)` for a number or string tag value.
fn tag_value(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(js::json_number_to_string(number)),
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

/// SameValueZero for the `Set` of tag values.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => a == b,
    }
}

/// Constraints of a property that its TypeScript type cannot express, as JSDoc-style tags:
/// `@integer @minimum 1 @maximum 100 @default 20`. Empty when there are none.
fn constraint_tags(schema: &Value) -> String {
    let Value::Object(property) = schema else { return String::new() };
    // Optional fields such as Pydantic's `int | None` keep their constraints in the non-null
    // branch. The branch's constraints apply together with the property's, so both are read.
    let variants = match property.get("anyOf") {
        Some(Value::Array(variants)) => Some(variants),
        _ => match property.get("oneOf") {
            Some(Value::Array(variants)) => Some(variants),
            _ => None,
        },
    };
    let non_null: Vec<&Value> = variants
        .map(|variants| variants.iter().filter(|variant| variant.get("type") != Some(&Value::String("null".into()))).collect())
        .unwrap_or_default();
    let mut branches: Vec<&Map<String, Value>> = vec![property];
    if non_null.len() == 1
        && let Value::Object(branch) = non_null[0]
    {
        branches.push(branch);
    }
    // Draft-04 and OpenAPI 3.0 make `minimum`/`maximum` exclusive with a boolean
    // `exclusiveMinimum`/`exclusiveMaximum`.
    let schemas: Vec<Map<String, Value>> = branches
        .iter()
        .map(|schema| {
            let mut schema = (*schema).clone();
            for (bound, exclusive) in [("minimum", "exclusiveMinimum"), ("maximum", "exclusiveMaximum")] {
                if schema.get(exclusive) == Some(&Value::Bool(true)) {
                    let value = schema.remove(bound);
                    match value {
                        Some(value) => schema.insert(exclusive.to_string(), value),
                        None => schema.remove(exclusive),
                    };
                }
            }
            schema
        })
        .collect();

    let mut tags: Vec<String> = Vec::new();
    if schemas.iter().any(|schema| is_integer_type(schema.get("type"))) {
        tags.push("@integer".to_string());
    }
    for keyword in TAG_KEYWORDS {
        let values: Vec<Option<&Value>> = schemas.iter().map(|schema| schema.get(keyword)).collect();
        let lower = LOWER_BOUNDS.contains(&keyword);
        if lower || UPPER_BOUNDS.contains(&keyword) {
            let numbers: Vec<f64> = values.iter().filter_map(|value| value.and_then(Value::as_f64)).collect();
            if numbers.is_empty() {
                continue;
            }
            let bound = if lower {
                numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            } else {
                numbers.iter().copied().fold(f64::INFINITY, f64::min)
            };
            tags.push(format!("@{keyword} {}", js::number_to_string(bound)));
            continue;
        }
        let mut unique: Vec<&Value> = Vec::new();
        for value in values.into_iter().flatten() {
            if !unique.iter().any(|existing| same_value(existing, value)) {
                unique.push(value);
            }
        }
        for value in unique {
            if let Some(text) = tag_value(value) {
                tags.push(format!("@{keyword} {text}"));
            }
        }
    }
    if schemas.iter().any(|schema| schema.get("uniqueItems") == Some(&Value::Bool(true))) {
        tags.push("@uniqueItems".to_string());
    }
    // Annotations, unlike constraints, come from the property itself.
    if let Some(default) = property.get("default")
        && !default.is_null()
    {
        tags.push(format!("@default {}", js::stringify(default)));
    }
    if property.get("deprecated") == Some(&Value::Bool(true)) {
        tags.push("@deprecated".to_string());
    }
    tags.into_iter()
        .filter(|tag| js::utf16_len(tag) <= MAX_TAG_CHARS)
        .map(|tag| {
            let mut escaped = String::with_capacity(tag.len());
            for c in tag.chars() {
                if is_line_terminator(c) {
                    escaped.push_str(&format!("\\u{:04x}", c as u32));
                } else {
                    escaped.push(c);
                }
            }
            escaped
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn object_type(schema: &Map<String, Value>, context: &mut SchemaContext<'_>) -> String {
    let empty = Map::new();
    let properties = match schema.get("properties") {
        Some(Value::Object(properties)) => properties,
        _ => &empty,
    };
    let required: HashSet<&str> = match schema.get("required") {
        Some(Value::Array(required)) => required.iter().filter_map(Value::as_str).collect(),
        _ => HashSet::new(),
    };
    let mut names: Vec<&String> = properties.keys().collect();
    names.sort_by(|a, b| js::utf16_cmp(a, b));
    let mut members: Vec<String> = names
        .iter()
        .map(|name| {
            let optional = if required.contains(name.as_str()) { "" } else { "?" };
            format!("{}{optional}: {};", property_key(name), to_type(&properties[name.as_str()], context))
        })
        .collect();
    match schema.get("additionalProperties") {
        Some(Value::Bool(false)) => {}
        Some(additional) => {
            let rendered = if additional == &Value::Bool(true) { "unknown".to_string() } else { to_type(additional, context) };
            members.push(format!("[key: string]: {rendered};"));
        }
        None if names.is_empty() => members.push("[key: string]: unknown;".to_string()),
        None => {}
    }
    if members.is_empty() {
        return "{}".to_string();
    }
    let comments: Vec<Vec<String>> = names
        .iter()
        .map(|name| {
            let property = &properties[name.as_str()];
            let mut lines: Vec<String> = description_of(property)
                .split(is_line_terminator)
                .map(|line| js::trim(line).to_string())
                .filter(|line| !line.is_empty())
                .collect();
            let tags = if context.tags { constraint_tags(property) } else { String::new() };
            if !tags.is_empty() {
                lines.push(tags);
            }
            lines
        })
        .collect();
    if !comments.iter().any(|comment| !comment.is_empty()) {
        return format!("{{ {} }}", members.join(" "));
    }

    let mut lines = vec!["{".to_string()];
    for (index, comment) in comments.iter().enumerate() {
        for line in comment {
            lines.push(format!("{INDENT}// {line}"));
        }
        lines.push(format!("{INDENT}{}", members[index].replace('\n', &format!("\n{INDENT}"))));
    }
    for member in &members[names.len()..] {
        lines.push(format!("{INDENT}{member}"));
    }
    lines.push("}".to_string());
    lines.join("\n")
}
