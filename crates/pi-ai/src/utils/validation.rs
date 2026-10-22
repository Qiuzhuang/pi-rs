//! Tool argument validation and coercion (port of `utils/validation.ts`).
//!
//! TypeBox schemas are JSON Schema. This module contains a JSON Schema validator that
//! mirrors TypeBox's checker (keyword order, English error messages and instance paths),
//! plus the AJV-compatible primitive coercion pi applies to plain JSON schemas.
//!
//! In Rust every schema is plain JSON, so the plain-schema path of `validateToolArguments`
//! (`normalizeOptionalNulls` + `coerceWithJsonSchema`) applies to all tools.

use std::collections::HashSet;

use regex::Regex;
use serde_json::{Map, Value};

use crate::types::{Tool, ToolCall};

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

fn js_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| js_equal(a, b)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| js_equal(v, w)))
        }
        _ => a == b,
    }
}

fn is_integer(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.is_finite() && f.fract() == 0.0),
        _ => false,
    }
}

fn js_string_length(text: &str) -> usize {
    text.encode_utf16().count()
}

fn number_to_string(value: f64) -> String {
    crate::serde_util::js_number(value).to_string()
}

/// `JSON.stringify(value)` rendering of a number for error messages.
fn render_number(value: &Value) -> String {
    match value {
        Value::Number(n) => match n.as_f64() {
            Some(f) => number_to_string(f),
            None => n.to_string(),
        },
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Validator
// ---------------------------------------------------------------------------

/// A validation error in TypeBox's shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub keyword: String,
    pub schema_path: String,
    pub instance_path: String,
    /// For `required`: the missing properties.
    pub required_properties: Vec<String>,
    pub message: String,
}

/// A compiled JSON Schema checker (TypeBox's `Compile(schema)`).
pub struct SchemaValidator<'a> {
    root: &'a Value,
}

impl<'a> SchemaValidator<'a> {
    pub fn new(root: &'a Value) -> Self {
        Self { root }
    }

    /// `Check(value)`.
    pub fn check(&self, value: &Value) -> bool {
        let mut errors = ErrorSink { errors: None };
        self.validate(self.root, value, "#", "", &mut errors, 0)
    }

    /// `Errors(value)`.
    pub fn errors(&self, value: &Value) -> Vec<ValidationError> {
        let mut sink = ErrorSink { errors: Some(Vec::new()) };
        self.validate(self.root, value, "#", "", &mut sink, 0);
        sink.errors.unwrap_or_default()
    }

    fn resolve_ref(&self, reference: &str) -> Option<&'a Value> {
        let pointer = reference.strip_prefix('#')?;
        if pointer.is_empty() {
            return Some(self.root);
        }
        let decoded: String = pointer
            .split('/')
            .map(|part| {
                let part = percent_decode(part);
                part.replace("~1", "/").replace("~0", "~")
            })
            .collect::<Vec<_>>()
            .join("/");
        // Re-encode to a JSON pointer for serde_json.
        let encoded: String = decoded
            .split('/')
            .map(|p| p.replace('~', "~0").replace('/', "~1"))
            .collect::<Vec<_>>()
            .join("/");
        self.root.pointer(&encoded).or_else(|| self.find_by_id(self.root, reference))
    }

    fn find_by_id(&self, schema: &'a Value, id: &str) -> Option<&'a Value> {
        match schema {
            Value::Object(map) => {
                if map.get("$id").and_then(Value::as_str) == Some(id) {
                    return Some(schema);
                }
                map.values().find_map(|v| self.find_by_id(v, id))
            }
            Value::Array(items) => items.iter().find_map(|v| self.find_by_id(v, id)),
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate(
        &self,
        schema: &'a Value,
        value: &Value,
        schema_path: &str,
        instance_path: &str,
        sink: &mut ErrorSink,
        depth: usize,
    ) -> bool {
        if depth > 256 {
            return false;
        }
        let schema_obj = match schema {
            Value::Bool(true) => return true,
            Value::Bool(false) => {
                sink.add("boolean", schema_path, instance_path, Vec::new(), "schema is false".to_string());
                return false;
            }
            Value::Object(map) => map,
            _ => return true,
        };
        let mut ok = true;

        // type
        if let Some(type_value) = schema_obj.get("type")
            && !check_type(type_value, value) {
                ok = false;
                let message = match type_value {
                    Value::Array(types) => format!(
                        "must be either {}",
                        types.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(" or ")
                    ),
                    Value::String(t) => format!("must be {t}"),
                    other => format!("must be {other}"),
                };
                sink.add("type", schema_path, instance_path, Vec::new(), message);
            }

        // object keywords
        if let Value::Object(object) = value {
            if let Some(Value::Array(required)) = schema_obj.get("required") {
                let missing: Vec<String> = required
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|key| !object.contains_key(*key))
                    .map(str::to_string)
                    .collect();
                if !missing.is_empty() {
                    ok = false;
                    let message = format!("must have required properties {}", missing.join(", "));
                    sink.add("required", schema_path, instance_path, missing, message);
                }
            }
            if let Some(additional) = schema_obj.get("additionalProperties") {
                let properties = schema_obj.get("properties").and_then(Value::as_object);
                let patterns: Vec<Regex> = schema_obj
                    .get("patternProperties")
                    .and_then(Value::as_object)
                    .map(|p| p.keys().filter_map(|k| Regex::new(k).ok()).collect())
                    .unwrap_or_default();
                let extra_keys: Vec<&String> = object
                    .keys()
                    .filter(|key| {
                        !properties.is_some_and(|p| p.contains_key(*key)) && !patterns.iter().any(|r| r.is_match(key))
                    })
                    .collect();
                // Like TypeBox: each extra property is checked against the `additionalProperties`
                // schema (`false` reports "schema is false" at the property), then one
                // `additionalProperties` error at the object lists the properties that failed.
                if matches!(additional, Value::Bool(false) | Value::Object(_)) {
                    let mut failed = false;
                    for key in extra_keys {
                        let child_path = format!("{instance_path}/{}", escape_pointer(key));
                        if !self.validate(
                            additional,
                            &object[key.as_str()],
                            &format!("{schema_path}/additionalProperties"),
                            &child_path,
                            sink,
                            depth + 1,
                        ) {
                            failed = true;
                        }
                    }
                    if failed {
                        ok = false;
                        sink.add(
                            "additionalProperties",
                            schema_path,
                            instance_path,
                            Vec::new(),
                            "must not have additional properties".to_string(),
                        );
                    }
                }
            }
            if let Some(Value::Object(dependent)) = schema_obj.get("dependentRequired") {
                for (property, deps) in dependent {
                    if !object.contains_key(property) {
                        continue;
                    }
                    let deps: Vec<String> = deps.as_array().map(|d| d.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
                    if deps.iter().any(|d| !object.contains_key(d)) {
                        ok = false;
                        sink.add(
                            "dependentRequired",
                            schema_path,
                            instance_path,
                            Vec::new(),
                            format!("must have properties {} when property {property} is present", deps.join(", ")),
                        );
                    }
                }
            }
            if let Some(Value::Object(dependent)) = schema_obj.get("dependentSchemas") {
                for (property, dep_schema) in dependent {
                    if object.contains_key(property)
                        && !self.validate(dep_schema, value, &format!("{schema_path}/dependentSchemas/{property}"), instance_path, sink, depth + 1)
                    {
                        ok = false;
                    }
                }
            }
            if let Some(Value::Object(patterns)) = schema_obj.get("patternProperties") {
                for (pattern, pattern_schema) in patterns {
                    let Ok(regex) = Regex::new(pattern) else { continue };
                    for (key, item) in object {
                        if regex.is_match(key)
                            && !self.validate(
                                pattern_schema,
                                item,
                                &format!("{schema_path}/patternProperties/{}", escape_pointer(pattern)),
                                &format!("{instance_path}/{}", escape_pointer(key)),
                                sink,
                                depth + 1,
                            )
                        {
                            ok = false;
                        }
                    }
                }
            }
            if let Some(Value::Object(properties)) = schema_obj.get("properties") {
                for (key, property_schema) in properties {
                    let Some(item) = object.get(key) else { continue };
                    if !self.validate(
                        property_schema,
                        item,
                        &format!("{schema_path}/properties/{}", escape_pointer(key)),
                        &format!("{instance_path}/{}", escape_pointer(key)),
                        sink,
                        depth + 1,
                    ) {
                        ok = false;
                    }
                }
            }
            if let Some(names_schema) = schema_obj.get("propertyNames") {
                let invalid: Vec<String> = object
                    .keys()
                    .filter(|key| !self.check_sub(names_schema, &Value::String((*key).clone()), depth))
                    .cloned()
                    .collect();
                if !invalid.is_empty() {
                    ok = false;
                    sink.add(
                        "propertyNames",
                        schema_path,
                        instance_path,
                        Vec::new(),
                        format!("property names {} are invalid", invalid.join(", ")),
                    );
                }
            }
            if let Some(limit) = schema_obj.get("minProperties").and_then(Value::as_f64)
                && (object.len() as f64) < limit {
                    ok = false;
                    sink.add("minProperties", schema_path, instance_path, Vec::new(), format!("must not have fewer than {} properties", number_to_string(limit)));
                }
            if let Some(limit) = schema_obj.get("maxProperties").and_then(Value::as_f64)
                && (object.len() as f64) > limit {
                    ok = false;
                    sink.add("maxProperties", schema_path, instance_path, Vec::new(), format!("must not have more than {} properties", number_to_string(limit)));
                }
        }

        // array keywords
        if let Value::Array(items) = value {
            if let Some(contains) = schema_obj.get("contains") {
                let count = items.iter().filter(|item| self.check_sub(contains, item, depth)).count();
                let min_contains = schema_obj.get("minContains").and_then(Value::as_f64).unwrap_or(1.0);
                if (count as f64) < min_contains {
                    ok = false;
                    sink.add("contains", schema_path, instance_path, Vec::new(), "must contain at least 1 valid item".to_string());
                }
                if let Some(max) = schema_obj.get("maxContains").and_then(Value::as_f64)
                    && count as f64 > max {
                        ok = false;
                        sink.add("maxContains", schema_path, instance_path, Vec::new(), format!("must not contain more than {} matching items", number_to_string(max)));
                    }
            }
            let prefix_len = match schema_obj.get("prefixItems") {
                Some(Value::Array(prefix)) => {
                    for (index, (item_schema, item)) in prefix.iter().zip(items.iter()).enumerate() {
                        if !self.validate(item_schema, item, &format!("{schema_path}/prefixItems/{index}"), &format!("{instance_path}/{index}"), sink, depth + 1) {
                            ok = false;
                        }
                    }
                    prefix.len()
                }
                _ => 0,
            };
            match schema_obj.get("items") {
                Some(Value::Array(tuple)) => {
                    for (index, (item_schema, item)) in tuple.iter().zip(items.iter()).enumerate() {
                        if !self.validate(item_schema, item, &format!("{schema_path}/items/{index}"), &format!("{instance_path}/{index}"), sink, depth + 1) {
                            ok = false;
                        }
                    }
                    if let Some(additional) = schema_obj.get("additionalItems") {
                        for (index, item) in items.iter().enumerate().skip(tuple.len()) {
                            if !self.validate(additional, item, &format!("{schema_path}/additionalItems"), &format!("{instance_path}/{index}"), sink, depth + 1) {
                                ok = false;
                            }
                        }
                    }
                }
                Some(item_schema) => {
                    for (index, item) in items.iter().enumerate().skip(prefix_len) {
                        if !self.validate(item_schema, item, &format!("{schema_path}/items"), &format!("{instance_path}/{index}"), sink, depth + 1) {
                            ok = false;
                        }
                    }
                }
                None => {}
            }
            if let Some(limit) = schema_obj.get("maxItems").and_then(Value::as_f64)
                && items.len() as f64 > limit {
                    ok = false;
                    sink.add("maxItems", schema_path, instance_path, Vec::new(), format!("must not have more than {} items", number_to_string(limit)));
                }
            if let Some(limit) = schema_obj.get("minItems").and_then(Value::as_f64)
                && (items.len() as f64) < limit {
                    ok = false;
                    sink.add("minItems", schema_path, instance_path, Vec::new(), format!("must not have fewer than {} items", number_to_string(limit)));
                }
            if schema_obj.get("uniqueItems") == Some(&Value::Bool(true)) {
                let duplicate = items.iter().enumerate().any(|(i, a)| items.iter().skip(i + 1).any(|b| js_equal(a, b)));
                if duplicate {
                    ok = false;
                    sink.add("uniqueItems", schema_path, instance_path, Vec::new(), "must not have duplicate items".to_string());
                }
            }
        }

        // string keywords
        if let Value::String(text) = value {
            let length = js_string_length(text) as f64;
            if let Some(limit) = schema_obj.get("maxLength").and_then(Value::as_f64)
                && length > limit {
                    ok = false;
                    sink.add("maxLength", schema_path, instance_path, Vec::new(), format!("must not have more than {} characters", number_to_string(limit)));
                }
            if let Some(limit) = schema_obj.get("minLength").and_then(Value::as_f64)
                && length < limit {
                    ok = false;
                    sink.add("minLength", schema_path, instance_path, Vec::new(), format!("must not have fewer than {} characters", number_to_string(limit)));
                }
            if let Some(pattern) = schema_obj.get("pattern").and_then(Value::as_str)
                && let Ok(regex) = Regex::new(pattern)
                    && !regex.is_match(text) {
                        ok = false;
                        sink.add("pattern", schema_path, instance_path, Vec::new(), format!("must match pattern \"{pattern}\""));
                    }
        }

        // number keywords
        if let Some(number) = value.as_f64().filter(|_| value.is_number()) {
            type Check = (&'static str, &'static str, fn(f64, f64) -> bool);
            let checks: [Check; 4] = [
                ("exclusiveMaximum", "<", |v, l| v < l),
                ("exclusiveMinimum", ">", |v, l| v > l),
                ("maximum", "<=", |v, l| v <= l),
                ("minimum", ">=", |v, l| v >= l),
            ];
            for (keyword, comparison, predicate) in checks {
                if let Some(limit) = schema_obj.get(keyword).filter(|l| l.is_number()) {
                    let limit_f = limit.as_f64().unwrap_or(0.0);
                    if !predicate(number, limit_f) {
                        ok = false;
                        sink.add(keyword, schema_path, instance_path, Vec::new(), format!("must be {comparison} {}", render_number(limit)));
                    }
                }
            }
            if let Some(multiple) = schema_obj.get("multipleOf").and_then(Value::as_f64)
                && multiple != 0.0 {
                    let quotient = number / multiple;
                    if (quotient - quotient.round()).abs() > 1e-9 {
                        ok = false;
                        sink.add("multipleOf", schema_path, instance_path, Vec::new(), format!("must be multiple of {}", number_to_string(multiple)));
                    }
                }
        }

        // $ref
        if let Some(reference) = schema_obj.get("$ref").and_then(Value::as_str) {
            match self.resolve_ref(reference) {
                Some(target) => {
                    if !self.validate(target, value, &format!("{schema_path}/$ref"), instance_path, sink, depth + 1) {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }

        if let Some(constant) = schema_obj.get("const")
            && !js_equal(constant, value) {
                ok = false;
                sink.add("const", schema_path, instance_path, Vec::new(), "must be equal to constant".to_string());
            }
        if let Some(Value::Array(options)) = schema_obj.get("enum")
            && !options.iter().any(|o| js_equal(o, value)) {
                ok = false;
                sink.add("enum", schema_path, instance_path, Vec::new(), "must be equal to one of the allowed values".to_string());
            }
        if let Some(if_schema) = schema_obj.get("if") {
            let (branch, keyword) = if self.check_sub(if_schema, value, depth) {
                (schema_obj.get("then"), "then")
            } else {
                (schema_obj.get("else"), "else")
            };
            if let Some(branch) = branch
                && !self.check_sub(branch, value, depth) {
                    ok = false;
                    sink.add("if", schema_path, instance_path, Vec::new(), format!("must match \"{keyword}\" schema"));
                }
        }
        if let Some(not_schema) = schema_obj.get("not")
            && self.check_sub(not_schema, value, depth) {
                ok = false;
                sink.add("not", schema_path, instance_path, Vec::new(), "must not be valid".to_string());
            }
        if let Some(Value::Array(all)) = schema_obj.get("allOf") {
            for (index, sub) in all.iter().enumerate() {
                if !self.validate(sub, value, &format!("{schema_path}/allOf/{index}"), instance_path, sink, depth + 1) {
                    ok = false;
                }
            }
        }
        if let Some(Value::Array(any)) = schema_obj.get("anyOf") {
            let mut failed: Vec<Vec<ValidationError>> = Vec::new();
            let mut matched = false;
            for (index, sub) in any.iter().enumerate() {
                let mut nested = ErrorSink { errors: sink.errors.as_ref().map(|_| Vec::new()) };
                if self.validate(sub, value, &format!("{schema_path}/anyOf/{index}"), instance_path, &mut nested, depth + 1) {
                    matched = true;
                    break;
                }
                failed.push(nested.errors.unwrap_or_default());
            }
            if !matched {
                ok = false;
                for errors in failed {
                    for error in errors {
                        sink.push(error);
                    }
                }
                sink.add("anyOf", schema_path, instance_path, Vec::new(), "must match a schema in anyOf".to_string());
            }
        }
        if let Some(Value::Array(one)) = schema_obj.get("oneOf") {
            let mut failed: Vec<Vec<ValidationError>> = Vec::new();
            let mut passing = 0;
            for (index, sub) in one.iter().enumerate() {
                let mut nested = ErrorSink { errors: sink.errors.as_ref().map(|_| Vec::new()) };
                if self.validate(sub, value, &format!("{schema_path}/oneOf/{index}"), instance_path, &mut nested, depth + 1) {
                    passing += 1;
                } else {
                    failed.push(nested.errors.unwrap_or_default());
                }
            }
            if passing != 1 {
                ok = false;
                if passing == 0 {
                    for errors in failed {
                        for error in errors {
                            sink.push(error);
                        }
                    }
                }
                sink.add("oneOf", schema_path, instance_path, Vec::new(), "must match exactly one schema in oneOf".to_string());
            }
        }
        ok
    }

    fn check_sub(&self, schema: &'a Value, value: &Value, depth: usize) -> bool {
        let mut sink = ErrorSink { errors: None };
        self.validate(schema, value, "#", "", &mut sink, depth + 1)
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

struct ErrorSink {
    errors: Option<Vec<ValidationError>>,
}

impl ErrorSink {
    fn add(&mut self, keyword: &str, schema_path: &str, instance_path: &str, required: Vec<String>, message: String) {
        if let Some(errors) = &mut self.errors {
            errors.push(ValidationError {
                keyword: keyword.to_string(),
                schema_path: schema_path.to_string(),
                instance_path: instance_path.to_string(),
                required_properties: required,
                message,
            });
        }
    }

    fn push(&mut self, error: ValidationError) {
        if let Some(errors) = &mut self.errors {
            errors.push(error);
        }
    }
}

fn check_type(type_value: &Value, value: &Value) -> bool {
    match type_value {
        Value::String(t) => matches_json_type(value, t),
        Value::Array(types) => types.iter().filter_map(Value::as_str).any(|t| matches_json_type(value, t)),
        _ => true,
    }
}

fn matches_json_type(value: &Value, type_name: &str) -> bool {
    match type_name {
        "number" => value.is_number(),
        "integer" => is_integer(value),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "null" => value.is_null(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    }
}

/// Validate `value` against `schema` (TypeBox `Compile(schema).Check(value)`).
pub fn check_schema(schema: &Value, value: &Value) -> bool {
    SchemaValidator::new(schema).check(value)
}

// ---------------------------------------------------------------------------
// TypeBox `Value.Convert` (typebox 1.3.27, `build/value/convert`)
// ---------------------------------------------------------------------------

/// The TypeBox kind of a schema, inferred from the JSON TypeBox emits for it (the `~kind`
/// marker itself is non-enumerable and does not survive JSON).
enum TypeBoxKind<'a> {
    Array(&'a Value),
    Tuple(&'a [Value]),
    Boolean,
    Integer,
    Number,
    String,
    Null,
    Literal(&'a Value),
    Enum(&'a [Value]),
    Object,
    Record(&'a Map<String, Value>),
    Union(&'a [Value]),
    Intersect(&'a [Value]),
    Ref(&'a str),
    Cyclic(&'a Map<String, Value>, &'a str),
    Other,
}

fn typebox_kind(schema: &Value) -> TypeBoxKind<'_> {
    let Some(map) = schema.as_object() else { return TypeBoxKind::Other };
    if let (Some(Value::Object(defs)), Some(Value::String(reference))) = (map.get("$defs"), map.get("$ref")) {
        return TypeBoxKind::Cyclic(defs, reference);
    }
    if let Some(Value::String(reference)) = map.get("$ref") {
        return TypeBoxKind::Ref(reference);
    }
    if let Some(Value::Array(members)) = map.get("anyOf") {
        return TypeBoxKind::Union(members);
    }
    if let Some(Value::Array(members)) = map.get("allOf") {
        return TypeBoxKind::Intersect(members);
    }
    if let Some(constant) = map.get("const") {
        return TypeBoxKind::Literal(constant);
    }
    if let (None, Some(Value::Array(values))) = (map.get("type"), map.get("enum")) {
        return TypeBoxKind::Enum(values);
    }
    match map.get("type").and_then(Value::as_str) {
        Some("array") => match map.get("items") {
            Some(Value::Array(items)) => TypeBoxKind::Tuple(items),
            Some(items @ Value::Object(_)) => TypeBoxKind::Array(items),
            _ => TypeBoxKind::Other,
        },
        Some("boolean") => TypeBoxKind::Boolean,
        Some("integer") => TypeBoxKind::Integer,
        Some("number") => TypeBoxKind::Number,
        Some("string") => TypeBoxKind::String,
        Some("null") => TypeBoxKind::Null,
        Some("object") => match map.get("patternProperties") {
            Some(Value::Object(patterns)) if !map.contains_key("properties") => TypeBoxKind::Record(patterns),
            _ => TypeBoxKind::Object,
        },
        _ => TypeBoxKind::Other,
    }
}

/// `Guard.IsNumber`: a finite number.
fn finite_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}

/// `TryNumber(value)`.
fn try_number(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Number(_) => finite_number(value),
        Value::Null => Some(0.0),
        Value::String(text) => {
            // `+value`
            if let Some(n) = js_number_from_string(text).filter(|n| n.is_finite()) {
                return Some(n);
            }
            match text.to_lowercase().as_str() {
                "false" => return Some(0.0),
                "true" => return Some(1.0),
                _ => {}
            }
            // TryBigInt: `-?(0|[1-9]\d*)n`, `-?(0|[1-9]\d*)\.\d+`, `-?(0|[1-9]\d*)`.
            let digits = text.strip_suffix('n').unwrap_or(text);
            let (negative, unsigned) = match digits.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, digits),
            };
            let integer_part = if text.ends_with('n') {
                unsigned
            } else {
                unsigned.split_once('.').map(|(i, f)| if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) { i } else { "" }).unwrap_or(unsigned)
            };
            let canonical = integer_part == "0"
                || (!integer_part.is_empty() && !integer_part.starts_with('0') && integer_part.bytes().all(|b| b.is_ascii_digit()));
            if !canonical {
                return None;
            }
            let magnitude: f64 = integer_part.parse().ok()?;
            // Number(bigint) only within the safe integer range.
            (magnitude <= 9_007_199_254_740_991.0).then_some(if negative { -magnitude } else { magnitude })
        }
        _ => None,
    }
}

/// `TryBoolean(value)`.
fn try_boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_f64() {
            Some(0.0) => Some(false),
            Some(1.0) => Some(true),
            _ => None,
        },
        Value::Null => Some(false),
        Value::String(text) => match text.to_lowercase().as_str() {
            "false" => Some(false),
            "true" => Some(true),
            _ if text == "0" => Some(false),
            _ if text == "1" => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// `TryString(value)`.
fn try_string(value: &Value) -> Option<String> {
    match value {
        Value::Bool(_) | Value::Number(_) => Some(js_to_string(value)),
        Value::Null => Some("null".to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// `TryNull(value)`.
fn try_null(value: &Value) -> bool {
    match value {
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::Null => true,
        Value::String(text) => {
            let lower = text.to_lowercase();
            lower == "undefined" || lower == "null" || text.is_empty() || text == "0"
        }
        _ => false,
    }
}

/// `new RegExp(`^${key}$`).test(candidate)` as TypeBox's `EntriesRegExp` matches property keys.
fn key_pattern_matches(pattern: &str, candidate: &str) -> bool {
    // Property names are almost always plain; those with regex syntax go through `regex`.
    if !pattern.contains(|c: char| "\\^$.|?*+()[]{}".contains(c)) {
        return pattern == candidate;
    }
    match regex::Regex::new(&format!("^(?:{pattern})$")) {
        Ok(re) => re.is_match(candidate),
        Err(_) => pattern == candidate,
    }
}

/// `Value.Convert(schema, value)` of TypeBox 1.3.27 for a schema built with TypeBox.
pub fn typebox_convert(schema: &Value, value: Value) -> Value {
    typebox_convert_in(&Map::new(), schema, value)
}

fn typebox_convert_in(context: &Map<String, Value>, schema: &Value, value: Value) -> Value {
    match typebox_kind(schema) {
        TypeBoxKind::Array(items) => {
            let elements = match value {
                Value::Array(elements) => elements,
                other => vec![other],
            };
            Value::Array(elements.into_iter().map(|e| typebox_convert_in(context, items, e)).collect())
        }
        TypeBoxKind::Tuple(items) => match value {
            Value::Array(mut elements) => {
                for (index, item) in items.iter().enumerate().take(elements.len()) {
                    let current = std::mem::take(&mut elements[index]);
                    elements[index] = typebox_convert_in(context, item, current);
                }
                Value::Array(elements)
            }
            other => other,
        },
        TypeBoxKind::Boolean => try_boolean(&value).map(Value::Bool).unwrap_or(value),
        TypeBoxKind::Integer => try_number(&value).map(|n| number_value(n.trunc())).unwrap_or(value),
        TypeBoxKind::Number => try_number(&value).map(number_value).unwrap_or(value),
        TypeBoxKind::String => try_string(&value).map(Value::String).unwrap_or(value),
        TypeBoxKind::Null => {
            if try_null(&value) {
                Value::Null
            } else {
                value
            }
        }
        TypeBoxKind::Literal(constant) => convert_literal(constant, value),
        TypeBoxKind::Enum(values) => {
            // Evaluate(Enum) is a union of literals.
            let members: Vec<Value> = values.iter().map(|v| serde_json::json!({ "const": v })).collect();
            convert_union(context, &members, &serde_json::json!({ "anyOf": members }), value)
        }
        TypeBoxKind::Object => match value {
            Value::Object(mut object) => {
                let map = schema.as_object().expect("object kind");
                let properties = map.get("properties").and_then(Value::as_object);
                let keys: Vec<String> = object.keys().cloned().collect();
                if let Some(properties) = properties {
                    for (pattern, property) in properties {
                        for key in &keys {
                            if key_pattern_matches(pattern, key)
                                && let Some(current) = object.get(key).cloned()
                            {
                                object.insert(key.clone(), typebox_convert_in(context, property, current));
                            }
                        }
                    }
                }
                if let Some(additional @ Value::Object(_)) = map.get("additionalProperties") {
                    let patterns: Vec<&String> = properties.map(|p| p.keys().collect()).unwrap_or_default();
                    convert_additional(context, &patterns, additional, &mut object, &keys);
                }
                Value::Object(object)
            }
            other => other,
        },
        TypeBoxKind::Record(patterns) => match value {
            Value::Object(mut object) => {
                let keys: Vec<String> = object.keys().cloned().collect();
                for (pattern, property) in patterns {
                    for key in &keys {
                        if key_pattern_matches(pattern, key)
                            && let Some(current) = object.get(key).cloned()
                        {
                            object.insert(key.clone(), typebox_convert_in(context, property, current));
                        }
                    }
                }
                if let Some(additional @ Value::Object(_)) = schema.get("additionalProperties") {
                    let pattern_keys: Vec<&String> = patterns.keys().collect();
                    convert_additional(context, &pattern_keys, additional, &mut object, &keys);
                }
                Value::Object(object)
            }
            other => other,
        },
        TypeBoxKind::Union(members) => convert_union(context, members, schema, value),
        TypeBoxKind::Intersect(members) => {
            // Evaluate(Intersect) of objects: one object with the members' properties.
            let mut properties = Map::new();
            for member in members {
                match member.get("properties").and_then(Value::as_object) {
                    Some(member_properties) if member.get("type").and_then(Value::as_str) == Some("object") => {
                        for (key, property) in member_properties {
                            properties.insert(key.clone(), property.clone());
                        }
                    }
                    _ => return value,
                }
            }
            typebox_convert_in(context, &serde_json::json!({ "type": "object", "properties": properties }), value)
        }
        TypeBoxKind::Ref(reference) => match context.get(reference) {
            Some(target) => typebox_convert_in(context, target, value),
            None => value,
        },
        TypeBoxKind::Cyclic(defs, reference) => {
            let mut merged = context.clone();
            for (key, def) in defs {
                merged.insert(key.clone(), def.clone());
            }
            match merged.get(reference).cloned() {
                Some(target) => typebox_convert_in(&merged, &target, value),
                None => value,
            }
        }
        TypeBoxKind::Other => value,
    }
}

fn convert_additional(
    context: &Map<String, Value>,
    patterns: &[&String],
    additional: &Value,
    object: &mut Map<String, Value>,
    keys: &[String],
) {
    for pattern in patterns {
        for key in keys {
            if !key_pattern_matches(pattern, key)
                && let Some(current) = object.get(key).cloned()
            {
                object.insert(key.clone(), typebox_convert_in(context, additional, current));
            }
        }
    }
}

/// `FromLiteral`: convert to the literal's type when the result equals the constant.
fn convert_literal(constant: &Value, value: Value) -> Value {
    if js_equal(constant, &value) {
        return value;
    }
    let converted = match constant {
        Value::Bool(_) => try_boolean(&value).map(Value::Bool),
        Value::Number(_) => try_number(&value).map(number_value),
        Value::String(_) => try_string(&value).map(Value::String),
        _ => None,
    };
    match converted {
        Some(converted) if js_equal(constant, &converted) => converted,
        _ => value,
    }
}

/// `FromUnion`: keep a value a member accepts, else the first member conversion the union accepts.
fn convert_union(context: &Map<String, Value>, members: &[Value], union: &Value, value: Value) -> Value {
    if members.iter().any(|member| check_schema(member, &value)) {
        return value;
    }
    for member in members {
        let candidate = typebox_convert_in(context, member, value.clone());
        if check_schema(union, &candidate) {
            return candidate;
        }
    }
    value
}

// ---------------------------------------------------------------------------
// Coercion (plain JSON schemas)
// ---------------------------------------------------------------------------

fn get_schema_types(schema: &Map<String, Value>) -> Vec<String> {
    match schema.get("type") {
        Some(Value::String(t)) => vec![t.clone()],
        Some(Value::Array(types)) => types.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

/// JavaScript `Number(text)` for strings.
fn js_number_from_string(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok().map(|v| v as f64);
    }
    if let Some(bin) = trimmed.strip_prefix("0b").or_else(|| trimmed.strip_prefix("0B")) {
        return u64::from_str_radix(bin, 2).ok().map(|v| v as f64);
    }
    if let Some(oct) = trimmed.strip_prefix("0o").or_else(|| trimmed.strip_prefix("0O")) {
        return u64::from_str_radix(oct, 8).ok().map(|v| v as f64);
    }
    match trimmed {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    let valid = trimmed.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
    if !valid {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

fn number_value(value: f64) -> Value {
    crate::serde_util::js_number(value)
}

fn js_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => number_to_string(f),
            _ => n.to_string(),
        },
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn coerce_primitive_by_type(value: &Value, type_name: &str) -> Value {
    match type_name {
        "number" => match value {
            Value::Null => number_value(0.0),
            Value::String(s) if !s.trim().is_empty() => match js_number_from_string(s) {
                Some(parsed) if parsed.is_finite() => number_value(parsed),
                _ => value.clone(),
            },
            Value::Bool(b) => number_value(if *b { 1.0 } else { 0.0 }),
            _ => value.clone(),
        },
        "integer" => match value {
            Value::Null => number_value(0.0),
            Value::String(s) if !s.trim().is_empty() => match js_number_from_string(s) {
                Some(parsed) if parsed.is_finite() && parsed.fract() == 0.0 => number_value(parsed),
                _ => value.clone(),
            },
            Value::Bool(b) => number_value(if *b { 1.0 } else { 0.0 }),
            _ => value.clone(),
        },
        "boolean" => match value {
            Value::Null => Value::Bool(false),
            Value::String(s) if s == "true" => Value::Bool(true),
            Value::String(s) if s == "false" => Value::Bool(false),
            Value::Number(n) if n.as_f64() == Some(1.0) => Value::Bool(true),
            Value::Number(n) if n.as_f64() == Some(0.0) => Value::Bool(false),
            _ => value.clone(),
        },
        "string" => match value {
            Value::Null => Value::String(String::new()),
            Value::Number(_) | Value::Bool(_) => Value::String(js_to_string(value)),
            _ => value.clone(),
        },
        "null" => match value {
            Value::String(s) if s.is_empty() => Value::Null,
            Value::Number(n) if n.as_f64() == Some(0.0) => Value::Null,
            Value::Bool(false) => Value::Null,
            _ => value.clone(),
        },
        _ => value.clone(),
    }
}

/// Identity comparison for coercion: JS `candidate !== nextValue` for primitives.
fn same_primitive(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Object(_), Value::Object(_)) | (Value::Array(_), Value::Array(_)) => true,
        _ => a == b,
    }
}

fn apply_schema_object_coercion(value: &mut Map<String, Value>, schema: &Map<String, Value>) {
    let properties = schema.get("properties").and_then(Value::as_object);
    let defined: HashSet<&String> = properties.map(|p| p.keys().collect()).unwrap_or_default();
    if let Some(properties) = properties {
        for (key, property_schema) in properties {
            if let Some(current) = value.get(key).cloned() {
                value.insert(key.clone(), coerce_with_json_schema(current, property_schema));
            }
        }
    }
    if let Some(additional @ Value::Object(_)) = schema.get("additionalProperties") {
        let keys: Vec<String> = value.keys().filter(|k| !defined.contains(k)).cloned().collect();
        for key in keys {
            let current = value[&key].clone();
            value.insert(key, coerce_with_json_schema(current, additional));
        }
    }
}

fn apply_schema_array_coercion(value: &mut [Value], schema: &Map<String, Value>) {
    match schema.get("items") {
        Some(Value::Array(item_schemas)) => {
            for (index, item) in value.iter_mut().enumerate() {
                if let Some(item_schema) = item_schemas.get(index) {
                    *item = coerce_with_json_schema(item.take(), item_schema);
                }
            }
        }
        Some(item_schema @ Value::Object(_)) => {
            for item in value.iter_mut() {
                *item = coerce_with_json_schema(item.take(), item_schema);
            }
        }
        _ => {}
    }
}

fn coerce_with_union_schema(value: Value, schemas: &[Value]) -> Value {
    for schema in schemas {
        if check_schema(schema, &value) {
            return value;
        }
    }
    for schema in schemas {
        let coerced = coerce_with_json_schema(value.clone(), schema);
        if check_schema(schema, &coerced) {
            return coerced;
        }
    }
    value
}

/// Coerce a value toward a plain JSON schema using AJV-compatible primitive rules.
pub fn coerce_with_json_schema(value: Value, schema: &Value) -> Value {
    let Some(schema) = schema.as_object() else { return value };
    let mut next = value;
    if let Some(Value::Array(all)) = schema.get("allOf") {
        for nested in all {
            next = coerce_with_json_schema(next, nested);
        }
    }
    if let Some(Value::Array(any)) = schema.get("anyOf") {
        next = coerce_with_union_schema(next, any);
    }
    if let Some(Value::Array(one)) = schema.get("oneOf") {
        next = coerce_with_union_schema(next, one);
    }
    let types = get_schema_types(schema);
    let matches_union_member = types.len() > 1 && types.iter().any(|t| matches_json_type(&next, t));
    if !types.is_empty() && !matches_union_member {
        for type_name in &types {
            let candidate = coerce_primitive_by_type(&next, type_name);
            if !same_primitive(&candidate, &next) {
                next = candidate;
                break;
            }
        }
    }
    if types.iter().any(|t| t == "object")
        && let Value::Object(map) = &mut next {
            apply_schema_object_coercion(map, schema);
        }
    if types.iter().any(|t| t == "array")
        && let Value::Array(items) = &mut next {
            apply_schema_array_coercion(items, schema);
        }
    next
}

fn normalize_optional_nulls(value: &mut Value, schema: &Value) {
    let Some(schema_obj) = schema.as_object() else { return };
    if let Value::Array(items) = value {
        match schema_obj.get("items") {
            Some(Value::Array(item_schemas)) => {
                for (index, item) in items.iter_mut().enumerate() {
                    if let Some(item_schema) = item_schemas.get(index) {
                        normalize_optional_nulls(item, item_schema);
                    }
                }
            }
            Some(item_schema) => {
                for item in items.iter_mut() {
                    normalize_optional_nulls(item, item_schema);
                }
            }
            None => {}
        }
        return;
    }
    let Value::Object(object) = value else { return };
    let Some(Value::Object(properties)) = schema_obj.get("properties") else { return };
    let required: HashSet<&str> = schema_obj
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for (key, property_schema) in properties {
        let Some(current) = object.get_mut(key) else { continue };
        let is_ref = property_schema.get("$ref").is_some_and(Value::is_string);
        if current.is_null() && !required.contains(key.as_str()) && !is_ref && !check_schema(property_schema, &Value::Null) {
            object.shift_remove(key);
        } else {
            normalize_optional_nulls(current, property_schema);
        }
    }
}

fn format_validation_path(error: &ValidationError) -> String {
    let base_path = error.instance_path.strip_prefix('/').unwrap_or(&error.instance_path).replace('/', ".");
    if error.keyword == "required"
        && let Some(required) = error.required_properties.first() {
            return if base_path.is_empty() { required.clone() } else { format!("{base_path}.{required}") };
        }
    if base_path.is_empty() { "root".to_string() } else { base_path }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct ToolValidationError(pub String);

/// Finds a tool by name and validates the tool call arguments against its schema.
pub fn validate_tool_call(tools: &[Tool], tool_call: &ToolCall) -> Result<Value, ToolValidationError> {
    let tool = tools
        .iter()
        .find(|t| t.name == tool_call.name)
        .ok_or_else(|| ToolValidationError(format!("Tool \"{}\" not found", tool_call.name)))?;
    validate_tool_arguments(tool, tool_call)
}

/// Validates tool call arguments against the tool's schema, returning the coerced arguments.
pub fn validate_tool_arguments(tool: &Tool, tool_call: &ToolCall) -> Result<Value, ToolValidationError> {
    if let Some(arguments_error) = &tool_call.arguments_error {
        return Err(ToolValidationError(format!(
            "Invalid arguments for tool \"{}\": {arguments_error}",
            tool_call.name
        )));
    }
    let mut args = Value::Object(tool_call.arguments.clone());
    normalize_optional_nulls(&mut args, &tool.parameters);
    // TS: `Value.Convert(tool.parameters, args)` converts schemas built with TypeBox (it does nothing
    // for plain JSON schemas, which carry no `~kind`); the JSON Schema coercion follows for every
    // schema without the legacy TypeBox 0.x `Kind` symbol.
    if tool.typebox {
        args = typebox_convert(&tool.parameters, args);
    }
    let args = coerce_with_json_schema(args, &tool.parameters);

    let validator = SchemaValidator::new(&tool.parameters);
    if validator.check(&args) {
        return Ok(args);
    }
    let errors = validator
        .errors(&args)
        .iter()
        .map(|error| format!("  - {}: {}", format_validation_path(error), error.message))
        .collect::<Vec<_>>()
        .join("\n");
    let errors = if errors.is_empty() { "Unknown validation error".to_string() } else { errors };
    let received = serde_json::to_string_pretty(&tool_call.arguments).unwrap_or_default();
    Err(ToolValidationError(format!(
        "Validation failed for tool \"{}\":\n{errors}\n\nReceived arguments:\n{received}",
        tool_call.name
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plain(schema: Value, value: Value) -> (Tool, ToolCall) {
        let tool = Tool::new(
            "echo",
            "Echo tool",
            json!({"type": "object", "properties": {"value": schema}, "required": ["value"]}),
        );
        let mut arguments = Map::new();
        arguments.insert("value".into(), value);
        let call = ToolCall { id: "tool-1".into(), name: "echo".into(), arguments, ..Default::default() };
        (tool, call)
    }

    #[test]
    fn coerces_string_to_number_for_typebox_like_schema() {
        let tool = Tool::new(
            "echo",
            "Echo tool",
            json!({"type": "object", "properties": {"count": {"type": "number"}}, "required": ["count"]}),
        );
        let mut arguments = Map::new();
        arguments.insert("count".into(), json!("42"));
        let call = ToolCall { id: "t".into(), name: "echo".into(), arguments, ..Default::default() };
        assert_eq!(validate_tool_arguments(&tool, &call).unwrap(), json!({"count": 42}));
    }

    #[test]
    fn coerces_with_ajv_compatible_rules() {
        let cases = vec![
            (json!({"type": "number"}), json!("42"), json!(42)),
            (json!({"type": "number"}), json!(true), json!(1)),
            (json!({"type": "number"}), json!(null), json!(0)),
            (json!({"type": "integer"}), json!("42"), json!(42)),
            (json!({"type": "boolean"}), json!("true"), json!(true)),
            (json!({"type": "boolean"}), json!("false"), json!(false)),
            (json!({"type": "boolean"}), json!(1), json!(true)),
            (json!({"type": "boolean"}), json!(0), json!(false)),
            (json!({"type": "string"}), json!(null), json!("")),
            (json!({"type": "string"}), json!(true), json!("true")),
            (json!({"type": "null"}), json!(""), json!(null)),
            (json!({"type": "null"}), json!(0), json!(null)),
            (json!({"type": "null"}), json!(false), json!(null)),
            (json!({"type": ["number", "string"]}), json!("1"), json!("1")),
            (json!({"type": ["boolean", "number"]}), json!("1"), json!(1)),
        ];
        for (schema, input, expected) in cases {
            let (tool, call) = plain(schema.clone(), input.clone());
            assert_eq!(validate_tool_arguments(&tool, &call).unwrap(), json!({"value": expected}), "{schema} {input}");
        }
    }

    #[test]
    fn treats_null_as_omission_for_optional_non_nullable_properties() {
        let tool = Tool::new(
            "echo",
            "Echo tool",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "offset": {"type": "number"},
                    "nullable": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                    "metadata": {"type": "object", "properties": {"enabled": {"type": "boolean"}}}
                },
                "required": ["path", "metadata"]
            }),
        );
        let arguments = json!({"path": "file.txt", "offset": null, "nullable": null, "metadata": {"enabled": null}});
        let call = ToolCall {
            id: "t".into(),
            name: "echo".into(),
            arguments: arguments.as_object().unwrap().clone(),
            ..Default::default()
        };
        assert_eq!(
            validate_tool_arguments(&tool, &call).unwrap(),
            json!({"path": "file.txt", "nullable": null, "metadata": {}})
        );
    }

    #[test]
    fn preserves_optional_nulls_whose_referenced_schema_is_nullable() {
        let tool = Tool::new(
            "echo",
            "Echo tool",
            json!({
                "type": "object",
                "properties": {"value": {"$ref": "#/$defs/value"}},
                "$defs": {"value": {"anyOf": [{"type": "number"}, {"type": "null"}]}}
            }),
        );
        let call = ToolCall {
            id: "t".into(),
            name: "echo".into(),
            arguments: json!({"value": null}).as_object().unwrap().clone(),
            ..Default::default()
        };
        assert_eq!(validate_tool_arguments(&tool, &call).unwrap(), json!({"value": null}));
    }

    #[test]
    fn nullable_unions() {
        let (tool, call) = plain(json!({"anyOf": [{"type": "number"}, {"type": "null"}]}), json!(null));
        assert_eq!(validate_tool_arguments(&tool, &call).unwrap(), json!({"value": null}));
        let (tool, call) = plain(json!({"oneOf": [{"type": "number"}, {"type": "null"}]}), json!(null));
        assert_eq!(validate_tool_arguments(&tool, &call).unwrap(), json!({"value": null}));
        let (tool, call) = plain(json!({"anyOf": [{"type": "number"}, {"type": "null"}]}), json!("42"));
        assert_eq!(validate_tool_arguments(&tool, &call).unwrap(), json!({"value": 42}));
        let (tool, call) = plain(json!({"type": ["array", "null"], "items": {"type": "string"}}), json!(null));
        assert_eq!(validate_tool_arguments(&tool, &call).unwrap(), json!({"value": null}));
    }

    #[test]
    fn rejects_invalid_coercions() {
        let cases = vec![
            (json!({"type": "boolean"}), json!("1")),
            (json!({"type": "boolean"}), json!("0")),
            (json!({"type": "null"}), json!("null")),
            (json!({"type": "integer"}), json!("42.1")),
        ];
        for (schema, input) in cases {
            let (tool, call) = plain(schema, input);
            let err = validate_tool_arguments(&tool, &call).unwrap_err();
            assert!(err.0.contains("Validation failed"), "{}", err.0);
        }
    }

    #[test]
    fn reports_malformed_arguments() {
        let tool = Tool::new("echo", "Echo tool", json!({"type": "object", "properties": {"value": {"type": "string"}}}));
        let call = ToolCall {
            id: "t".into(),
            name: "echo".into(),
            arguments_error: Some("Unexpected token".into()),
            ..Default::default()
        };
        assert_eq!(
            validate_tool_arguments(&tool, &call).unwrap_err().0,
            "Invalid arguments for tool \"echo\": Unexpected token"
        );
    }

    #[test]
    fn formats_errors_like_typebox() {
        let tool = Tool::new(
            "read",
            "Read",
            json!({"type": "object", "properties": {"path": {"type": "string"}, "limit": {"type": "number", "minimum": 1}}, "required": ["path"]}),
        );
        let call = ToolCall {
            id: "t".into(),
            name: "read".into(),
            arguments: json!({"limit": 0}).as_object().unwrap().clone(),
            ..Default::default()
        };
        let err = validate_tool_arguments(&tool, &call).unwrap_err().0;
        assert!(err.starts_with("Validation failed for tool \"read\":\n  - path: must have required properties path\n  - limit: must be >= 1"), "{err}");
        assert!(err.ends_with("Received arguments:\n{\n  \"limit\": 0\n}"));
    }

    // Expected errors are the output of TypeBox 1.3.27 `Compile(schema).Errors(value)`.
    #[test]
    fn reports_additional_properties_like_typebox() {
        let schema = json!({
            "type": "object",
            "properties": { "name": { "type": "string" }, "nested": { "type": "object", "properties": {}, "additionalProperties": false } },
            "additionalProperties": false
        });
        let errors: Vec<(String, String, String, String)> = SchemaValidator::new(&schema)
            .errors(&json!({ "name": "x", "unsupported": true, "nested": { "a": 1 } }))
            .into_iter()
            .map(|e| (e.keyword, e.schema_path, e.instance_path, e.message))
            .collect();
        let expected = [
            ("boolean", "#/additionalProperties", "/unsupported", "schema is false"),
            ("additionalProperties", "#", "", "must not have additional properties"),
            ("boolean", "#/properties/nested/additionalProperties", "/nested/a", "schema is false"),
            ("additionalProperties", "#/properties/nested", "/nested", "must not have additional properties"),
        ];
        assert_eq!(
            errors,
            expected.iter().map(|(a, b, c, d)| (a.to_string(), b.to_string(), c.to_string(), d.to_string())).collect::<Vec<_>>()
        );

        let schema = json!({ "type": "object", "properties": { "name": { "type": "string" } }, "additionalProperties": { "type": "number" } });
        let errors: Vec<(String, String, String)> = SchemaValidator::new(&schema)
            .errors(&json!({ "name": "x", "extra": "s" }))
            .into_iter()
            .map(|e| (e.keyword, e.schema_path, e.instance_path))
            .collect();
        assert_eq!(
            errors,
            vec![
                ("type".to_string(), "#/additionalProperties".to_string(), "/extra".to_string()),
                ("additionalProperties".to_string(), "#".to_string(), String::new()),
            ]
        );
    }
}
