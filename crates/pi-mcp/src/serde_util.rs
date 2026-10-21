//! Serde helpers for wire compatibility with the TypeScript implementation.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;

/// Optional field that tolerates values of the wrong type (they become `None`), since the
/// TypeScript client passes server data through without validating optional fields.
pub fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

/// A present JSON value, including `null` (`Some(Value::Null)`), as opposed to an absent field.
pub fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// Serialize an `f64` like `JSON.stringify`: integral values without a fraction.
pub fn js_number<S: Serializer>(value: f64, serializer: S) -> Result<S::Ok, S::Error> {
    if !value.is_finite() {
        return serializer.serialize_unit();
    }
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        return serializer.serialize_i64(value as i64);
    }
    serializer.serialize_f64(value)
}

/// `Option<f64>` with [`js_number`] serialization and lenient deserialization.
pub mod opt_number {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => super::js_number(*value, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f64>, D::Error> {
        super::lenient(deserializer)
    }
}

/// `f64` with [`js_number`] serialization.
pub mod number {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        super::js_number(*value, serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        f64::deserialize(deserializer)
    }
}
