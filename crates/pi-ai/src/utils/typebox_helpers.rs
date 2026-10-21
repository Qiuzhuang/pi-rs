//! Schema helpers (port of `utils/typebox-helpers.ts`).

use serde_json::{Map, Value, json};

/// A string enum schema compatible with providers that don't support anyOf/const patterns.
pub fn string_enum(values: &[&str], description: Option<&str>, default: Option<&str>) -> Value {
    let mut schema = Map::new();
    schema.insert("type".into(), json!("string"));
    schema.insert("enum".into(), json!(values));
    if let Some(d) = description.filter(|d| !d.is_empty()) {
        schema.insert("description".into(), json!(d));
    }
    if let Some(d) = default.filter(|d| !d.is_empty()) {
        schema.insert("default".into(), json!(d));
    }
    Value::Object(schema)
}
