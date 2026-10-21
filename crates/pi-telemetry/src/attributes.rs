use indexmap::IndexMap;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::js::{JsNumber, number_to_json};

/// A span, event, link, or measurement attribute value.
///
/// TypeScript attributes may also be `undefined`, which every recorder ignores; in Rust such
/// attributes are simply left out of the map (see [`attributes!`](crate::attributes)).
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    String(String),
    Number(f64),
    Bool(bool),
    StringArray(Vec<String>),
    NumberArray(Vec<f64>),
    BoolArray(Vec<bool>),
}

/// Attributes in insertion order, like a JavaScript object.
pub type SpanAttributes = IndexMap<String, AttributeValue>;

/// Opaque, JSON-serializable adapter-owned propagation data.
pub type TelemetryCarrier = IndexMap<String, String>;

impl AttributeValue {
    /// `JSON.stringify` of the value.
    pub fn to_json_string(&self) -> String {
        fn string(value: &str) -> String {
            serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
        }
        match self {
            AttributeValue::String(value) => string(value),
            AttributeValue::Number(value) => number_to_json(*value),
            AttributeValue::Bool(value) => value.to_string(),
            AttributeValue::StringArray(values) => {
                format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|value| string(value))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
            AttributeValue::NumberArray(values) => {
                format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|value| number_to_json(*value))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
            AttributeValue::BoolArray(values) => {
                format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }

    /// Strict (`===`) equality: arrays are never equal, as in JavaScript.
    pub fn strict_equals(&self, other: &AttributeValue) -> bool {
        match (self, other) {
            (AttributeValue::String(a), AttributeValue::String(b)) => a == b,
            (AttributeValue::Number(a), AttributeValue::Number(b)) => a == b,
            (AttributeValue::Bool(a), AttributeValue::Bool(b)) => a == b,
            _ => false,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttributeValue::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            AttributeValue::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            AttributeValue::Bool(value) => Some(*value),
            _ => None,
        }
    }
}

impl Serialize for AttributeValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AttributeValue::String(value) => serializer.serialize_str(value),
            AttributeValue::Number(value) => JsNumber(*value).serialize(serializer),
            AttributeValue::Bool(value) => serializer.serialize_bool(*value),
            AttributeValue::StringArray(values) => values.serialize(serializer),
            AttributeValue::NumberArray(values) => values
                .iter()
                .map(|value| JsNumber(*value))
                .collect::<Vec<_>>()
                .serialize(serializer),
            AttributeValue::BoolArray(values) => values.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AttributeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        AttributeValue::try_from(value).map_err(D::Error::custom)
    }
}

impl TryFrom<serde_json::Value> for AttributeValue {
    type Error = String;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        use serde_json::Value;
        match value {
            Value::String(value) => Ok(AttributeValue::String(value)),
            Value::Number(value) => Ok(AttributeValue::Number(value.as_f64().unwrap_or(f64::NAN))),
            Value::Bool(value) => Ok(AttributeValue::Bool(value)),
            Value::Array(items) => {
                if items.iter().all(Value::is_string) {
                    Ok(AttributeValue::StringArray(
                        items
                            .into_iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect(),
                    ))
                } else if items.iter().all(Value::is_number) {
                    Ok(AttributeValue::NumberArray(
                        items.iter().filter_map(Value::as_f64).collect(),
                    ))
                } else if items.iter().all(Value::is_boolean) {
                    Ok(AttributeValue::BoolArray(
                        items.iter().filter_map(Value::as_bool).collect(),
                    ))
                } else {
                    Err("attribute arrays must hold only strings, numbers, or booleans".to_string())
                }
            }
            other => Err(format!("invalid attribute value {other}")),
        }
    }
}

/// Conversion into an optional attribute value; `None` leaves the attribute out, like `undefined`.
pub trait IntoAttributeValue {
    fn into_attribute_value(self) -> Option<AttributeValue>;
}

impl IntoAttributeValue for AttributeValue {
    fn into_attribute_value(self) -> Option<AttributeValue> {
        Some(self)
    }
}

impl<T: IntoAttributeValue> IntoAttributeValue for Option<T> {
    fn into_attribute_value(self) -> Option<AttributeValue> {
        self.and_then(IntoAttributeValue::into_attribute_value)
    }
}

macro_rules! impl_attribute {
    ($($ty:ty => |$v:ident| $body:expr),* $(,)?) => {
        $(
            impl From<$ty> for AttributeValue {
                fn from($v: $ty) -> Self {
                    $body
                }
            }
            impl IntoAttributeValue for $ty {
                fn into_attribute_value(self) -> Option<AttributeValue> {
                    Some(AttributeValue::from(self))
                }
            }
        )*
    };
}

impl_attribute! {
    String => |v| AttributeValue::String(v),
    &str => |v| AttributeValue::String(v.to_string()),
    &String => |v| AttributeValue::String(v.clone()),
    f64 => |v| AttributeValue::Number(v),
    f32 => |v| AttributeValue::Number(v as f64),
    i32 => |v| AttributeValue::Number(v as f64),
    i64 => |v| AttributeValue::Number(v as f64),
    u32 => |v| AttributeValue::Number(v as f64),
    u64 => |v| AttributeValue::Number(v as f64),
    usize => |v| AttributeValue::Number(v as f64),
    bool => |v| AttributeValue::Bool(v),
    Vec<String> => |v| AttributeValue::StringArray(v),
    Vec<&str> => |v| AttributeValue::StringArray(v.into_iter().map(str::to_string).collect()),
    &[&str] => |v| AttributeValue::StringArray(v.iter().map(|s| s.to_string()).collect()),
    Vec<f64> => |v| AttributeValue::NumberArray(v),
    Vec<i64> => |v| AttributeValue::NumberArray(v.into_iter().map(|n| n as f64).collect()),
    Vec<bool> => |v| AttributeValue::BoolArray(v),
}

impl<const N: usize> From<[&str; N]> for AttributeValue {
    fn from(value: [&str; N]) -> Self {
        AttributeValue::StringArray(value.iter().map(|s| s.to_string()).collect())
    }
}

impl<const N: usize> IntoAttributeValue for [&str; N] {
    fn into_attribute_value(self) -> Option<AttributeValue> {
        Some(self.into())
    }
}

/// Build [`SpanAttributes`] in order, skipping `None` values:
/// `attributes! { "provider" => "openai", "attempt" => 2, "reasoning" => None::<&str> }`.
#[macro_export]
macro_rules! attributes {
    () => { $crate::SpanAttributes::new() };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut attributes = $crate::SpanAttributes::new();
        $(
            if let Some(value) = $crate::IntoAttributeValue::into_attribute_value($value) {
                attributes.insert(::std::string::String::from($key), value);
            }
        )+
        attributes
    }};
}
