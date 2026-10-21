use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::error::{Error, McpError};

/// A JSON-RPC request ID: a string or a finite number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JsonRpcId {
    Number(serde_json::Number),
    String(String),
}

impl JsonRpcId {
    /// `isJsonRpcId`: strings and finite numbers. Integral floats are normalized so `1.0` equals `1`.
    pub fn from_value(value: &Value) -> Option<JsonRpcId> {
        match value {
            Value::String(id) => Some(JsonRpcId::String(id.clone())),
            Value::Number(number) => {
                if number.is_i64() || number.is_u64() {
                    return Some(JsonRpcId::Number(number.clone()));
                }
                let float = number.as_f64()?;
                if !float.is_finite() {
                    return None;
                }
                if float.fract() == 0.0 && float.abs() < 9_007_199_254_740_992.0 {
                    return Some(JsonRpcId::Number(serde_json::Number::from(float as i64)));
                }
                Some(JsonRpcId::Number(number.clone()))
            }
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            JsonRpcId::Number(number) => Value::Number(number.clone()),
            JsonRpcId::String(id) => Value::String(id.clone()),
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonRpcId::Number(number) => number.as_i64(),
            JsonRpcId::String(_) => None,
        }
    }
}

impl From<i64> for JsonRpcId {
    fn from(id: i64) -> Self {
        JsonRpcId::Number(id.into())
    }
}

impl From<&str> for JsonRpcId {
    fn from(id: &str) -> Self {
        JsonRpcId::String(id.to_string())
    }
}

impl From<String> for JsonRpcId {
    fn from(id: String) -> Self {
        JsonRpcId::String(id)
    }
}

impl std::fmt::Display for JsonRpcId {
    /// `String(id)`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonRpcId::Number(number) => write!(f, "{number}"),
            JsonRpcId::String(id) => f.write_str(id),
        }
    }
}

impl Serialize for JsonRpcId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonRpcId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        JsonRpcId::from_value(&value).ok_or_else(|| serde::de::Error::custom("invalid JSON-RPC id"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonRpcRequest {
    pub id: JsonRpcId,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonRpcNotification {
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsonRpcResponse {
    Success { id: JsonRpcId, result: Value },
    Error { id: JsonRpcId, error: JsonRpcErrorObject },
}

impl JsonRpcResponse {
    pub fn id(&self) -> &JsonRpcId {
        match self {
            JsonRpcResponse::Success { id, .. } | JsonRpcResponse::Error { id, .. } => id,
        }
    }
}

/// A JSON-RPC 2.0 message.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
}

impl JsonRpcMessage {
    pub fn request(id: impl Into<JsonRpcId>, method: impl Into<String>, params: Option<Value>) -> Self {
        JsonRpcMessage::Request(JsonRpcRequest { id: id.into(), method: method.into(), params })
    }

    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        JsonRpcMessage::Notification(JsonRpcNotification { method: method.into(), params })
    }

    pub fn result(id: impl Into<JsonRpcId>, result: Value) -> Self {
        JsonRpcMessage::Response(JsonRpcResponse::Success { id: id.into(), result })
    }

    pub fn error(id: impl Into<JsonRpcId>, code: i64, message: impl Into<String>, data: Option<Value>) -> Self {
        JsonRpcMessage::Response(JsonRpcResponse::Error {
            id: id.into(),
            error: JsonRpcErrorObject { code, message: message.into(), data },
        })
    }

    /// The method of a request or notification.
    pub fn method(&self) -> Option<&str> {
        match self {
            JsonRpcMessage::Request(request) => Some(&request.method),
            JsonRpcMessage::Notification(notification) => Some(&notification.method),
            JsonRpcMessage::Response(_) => None,
        }
    }

    /// The ID of a request or response.
    pub fn id(&self) -> Option<&JsonRpcId> {
        match self {
            JsonRpcMessage::Request(request) => Some(&request.id),
            JsonRpcMessage::Response(response) => Some(response.id()),
            JsonRpcMessage::Notification(_) => None,
        }
    }

    pub fn is_request(&self) -> bool {
        matches!(self, JsonRpcMessage::Request(_))
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

impl Serialize for JsonRpcMessage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("jsonrpc", "2.0")?;
        match self {
            JsonRpcMessage::Request(request) => {
                map.serialize_entry("id", &request.id)?;
                map.serialize_entry("method", &request.method)?;
                if let Some(params) = &request.params {
                    map.serialize_entry("params", params)?;
                }
            }
            JsonRpcMessage::Notification(notification) => {
                map.serialize_entry("method", &notification.method)?;
                if let Some(params) = &notification.params {
                    map.serialize_entry("params", params)?;
                }
            }
            JsonRpcMessage::Response(JsonRpcResponse::Success { id, result }) => {
                map.serialize_entry("id", id)?;
                map.serialize_entry("result", result)?;
            }
            JsonRpcMessage::Response(JsonRpcResponse::Error { id, error }) => {
                map.serialize_entry("id", id)?;
                map.serialize_entry("error", error)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for JsonRpcMessage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        parse_json_rpc_message(value).map_err(|error| serde::de::Error::custom(error.to_string()))
    }
}

pub struct JsonRpcErrorCodes {
    pub parse_error: i64,
    pub invalid_request: i64,
    pub method_not_found: i64,
    pub invalid_params: i64,
    pub internal_error: i64,
}

pub const JSON_RPC_ERROR_CODES: JsonRpcErrorCodes = JsonRpcErrorCodes {
    parse_error: -32700,
    invalid_request: -32600,
    method_not_found: -32601,
    invalid_params: -32602,
    internal_error: -32603,
};

pub fn is_object(value: &Value) -> bool {
    value.is_object()
}

fn jsonrpc_object(value: &Value) -> Option<&Map<String, Value>> {
    let object = value.as_object()?;
    (object.get("jsonrpc") == Some(&Value::String("2.0".into()))).then_some(object)
}

pub fn is_json_rpc_id(value: &Value) -> bool {
    JsonRpcId::from_value(value).is_some()
}

pub fn is_json_rpc_request(message: &Value) -> bool {
    jsonrpc_object(message).is_some_and(|object| {
        object.get("id").is_some_and(is_json_rpc_id) && object.get("method").is_some_and(Value::is_string)
    })
}

pub fn is_json_rpc_notification(message: &Value) -> bool {
    jsonrpc_object(message)
        .is_some_and(|object| !object.contains_key("id") && object.get("method").is_some_and(Value::is_string))
}

pub fn is_json_rpc_response(message: &Value) -> bool {
    let Some(object) = jsonrpc_object(message) else {
        return false;
    };
    if !object.get("id").is_some_and(is_json_rpc_id) {
        return false;
    }
    if object.contains_key("result") {
        return !object.contains_key("error");
    }
    let Some(error) = object.get("error").and_then(Value::as_object) else {
        return false;
    };
    error.get("code").is_some_and(Value::is_number) && error.get("message").is_some_and(Value::is_string)
}

/// Validate a parsed JSON value as a JSON-RPC message.
pub fn parse_json_rpc_message(value: Value) -> Result<JsonRpcMessage, Error> {
    let invalid = || Error::Rpc(McpError::new(JSON_RPC_ERROR_CODES.invalid_request, "Invalid JSON-RPC message"));
    if is_json_rpc_request(&value) {
        let Value::Object(mut object) = value else { return Err(invalid()) };
        let id = object.get("id").and_then(JsonRpcId::from_value).ok_or_else(invalid)?;
        let Some(Value::String(method)) = object.remove("method") else { return Err(invalid()) };
        return Ok(JsonRpcMessage::Request(JsonRpcRequest { id, method, params: object.remove("params") }));
    }
    if is_json_rpc_notification(&value) {
        let Value::Object(mut object) = value else { return Err(invalid()) };
        let Some(Value::String(method)) = object.remove("method") else { return Err(invalid()) };
        return Ok(JsonRpcMessage::Notification(JsonRpcNotification { method, params: object.remove("params") }));
    }
    if is_json_rpc_response(&value) {
        let Value::Object(mut object) = value else { return Err(invalid()) };
        let id = object.get("id").and_then(JsonRpcId::from_value).ok_or_else(invalid)?;
        if let Some(result) = object.remove("result") {
            return Ok(JsonRpcMessage::Response(JsonRpcResponse::Success { id, result }));
        }
        let error = object.remove("error").ok_or_else(invalid)?;
        let code = error.get("code").and_then(Value::as_f64).ok_or_else(invalid)?;
        let message = error.get("message").and_then(Value::as_str).ok_or_else(invalid)?.to_string();
        let data = error.get("data").cloned();
        return Ok(JsonRpcMessage::Response(JsonRpcResponse::Error {
            id,
            error: JsonRpcErrorObject { code: code as i64, message, data },
        }));
    }
    Err(invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_messages() {
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
        assert!(matches!(parse_json_rpc_message(request.clone()).unwrap(), JsonRpcMessage::Request(_)));
        assert_eq!(serde_json::to_value(parse_json_rpc_message(request.clone()).unwrap()).unwrap(), request);
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(matches!(parse_json_rpc_message(notification).unwrap(), JsonRpcMessage::Notification(_)));
        let response = json!({ "jsonrpc": "2.0", "id": "a", "result": null });
        assert_eq!(serde_json::to_value(parse_json_rpc_message(response.clone()).unwrap()).unwrap(), response);
        let error = json!({ "jsonrpc": "2.0", "id": 2, "error": { "code": -32601, "message": "nope" } });
        assert_eq!(serde_json::to_value(parse_json_rpc_message(error.clone()).unwrap()).unwrap(), error);
        for invalid in [
            json!({ "jsonrpc": "1.0", "id": 1, "method": "x" }),
            json!({ "jsonrpc": "2.0", "id": null, "method": "x" }),
            json!({ "jsonrpc": "2.0", "id": 1, "result": 1, "error": { "code": 1, "message": "x" } }),
            json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": "1", "message": "x" } }),
            json!([1]),
        ] {
            let error = parse_json_rpc_message(invalid).unwrap_err();
            assert_eq!(error.to_string(), "Invalid JSON-RPC message");
            assert_eq!(error.rpc_code(), Some(-32600));
        }
        assert_eq!(JsonRpcId::from_value(&json!(1.0)), Some(JsonRpcId::from(1)));
    }
}
