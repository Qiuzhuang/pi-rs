//! Serde helpers that keep JSON output byte-compatible with the TypeScript
//! implementation (JavaScript number formatting, lax `null` handling).

use serde::{Deserialize, Deserializer, Serializer};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Returns true when `value` would be printed by JavaScript without a fractional part.
pub fn is_js_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER
}

/// Serialize an `f64` the way `JSON.stringify` prints numbers: integral values have no `.0`.
pub fn serialize_js_f64<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if is_js_integer(*value) {
        if *value == 0.0 {
            // -0 prints as 0 in JSON.stringify
            return serializer.serialize_i64(0);
        }
        serializer.serialize_i64(*value as i64)
    } else if value.is_finite() {
        serializer.serialize_f64(*value)
    } else {
        // JSON.stringify(NaN|Infinity) === "null"
        serializer.serialize_none()
    }
}

/// Convert an f64 to a `serde_json::Value` with JavaScript number semantics.
pub fn js_number(value: f64) -> serde_json::Value {
    if is_js_integer(value) {
        serde_json::Value::from(value as i64)
    } else if value.is_finite() {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    }
}

/// JavaScript truthiness of a JSON value.
pub fn js_truthy(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// JavaScript `String(value)` of a JSON value.
pub fn js_string(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if is_js_integer(f) => (f as i64).to_string(),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        // Array.prototype.join: null and undefined elements print as "".
        Value::Array(items) => items
            .iter()
            .map(|item| if item.is_null() { String::new() } else { js_string(item) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

pub mod js_f64 {
    use super::*;
    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_js_f64(value, serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        f64::deserialize(deserializer)
    }
}

pub mod opt_js_f64 {
    use super::*;
    pub fn serialize<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serialize_js_f64(v, serializer),
            None => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
        Option::<f64>::deserialize(deserializer)
    }
}

/// Integer-like counters that tolerate float encodings (`12.0`) on input.
pub mod lax_u64 {
    use super::*;
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(*value)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(value_to_u64(&value))
    }
}

pub mod opt_lax_u64 {
    use super::*;
    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.serialize_u64(*v),
            None => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        Ok(match value {
            None | Some(serde_json::Value::Null) => None,
            Some(v) => Some(value_to_u64(&v)),
        })
    }
}

/// Timestamps in milliseconds (JS numbers); tolerate floats on input.
pub mod lax_i64 {
    use super::*;
    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(*value)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(match value {
            serde_json::Value::Number(n) => n
                .as_i64()
                .or_else(|| n.as_f64().map(|f| f.round() as i64))
                .unwrap_or(0),
            _ => 0,
        })
    }
}

fn value_to_u64(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| if f.is_finite() && f > 0.0 { f.round() as u64 } else { 0 }))
            .unwrap_or(0),
        _ => 0,
    }
}

/// Deserialize `null` as the type's default value (lax message content handling).
pub fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// For `Option<serde_json::Value>` fields where an explicit `null` must be preserved
/// (distinct from an absent field). Use with `#[serde(default)]`.
pub fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

pub fn is_false(value: &bool) -> bool {
    !*value
}
