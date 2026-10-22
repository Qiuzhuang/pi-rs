//! Shared helpers for the TypeScript test ports in this binary.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use pi_ai::auth::AuthContext;
use pi_ai::*;
use serde_json::{Value, json};

/// A model from a TypeScript-style JSON literal.
pub fn model(value: Value) -> Model {
    serde_json::from_value(value).expect("model literal")
}

/// A context from a TypeScript-style JSON literal (`timestamp` defaults to 1).
pub fn ctx(value: Value) -> Context {
    let mut value = value;
    if let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if message.get("timestamp").is_none() {
                message["timestamp"] = json!(1);
            }
            if message["role"] == "assistant" {
                let defaults = assistant_defaults();
                for (key, val) in defaults.as_object().unwrap() {
                    if message.get(key).is_none() {
                        message[key] = val.clone();
                    }
                }
            }
        }
    }
    serde_json::from_value(value).expect("context literal")
}

fn assistant_defaults() -> Value {
    json!({
        "api": "anthropic-messages",
        "provider": "anthropic",
        "model": "claude-test",
        "usage": empty_usage(),
        "stopReason": "stop",
    })
}

pub fn empty_usage() -> Value {
    json!({
        "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}
    })
}

pub fn user_ctx(text: &str) -> Context {
    ctx(json!({"messages": [{"role": "user", "content": text}]}))
}

pub fn tool(value: Value) -> Tool {
    serde_json::from_value(value).expect("tool literal")
}

/// Capture the provider payload through `on_payload`; the request then fails against a closed port.
pub async fn capture_payload_with(
    run: impl FnOnce(StreamOptions) -> AssistantMessageEventStream,
    options: StreamOptions,
) -> (Value, AssistantMessage) {
    let captured = Arc::new(Mutex::new(None::<Value>));
    let sink = captured.clone();
    let options = StreamOptions {
        on_payload: Some(Arc::new(move |payload: Value, _model: Model| {
            let mut slot = sink.lock();
            if slot.is_none() {
                *slot = Some(payload);
            }
            Box::pin(async { None })
        })),
        ..options
    };
    let message = run(options).result().await;
    let payload = captured.lock().take().unwrap_or_else(|| panic!("payload captured: {:?}", message.error_message));
    (payload, message)
}

/// `compat::stream_simple` payload capture with an api key.
pub async fn simple_payload(model: &Model, context: &Context, options: StreamOptions) -> Value {
    let mut model = model.clone();
    model.base_url = dead_base_url(&model.base_url);
    let options = StreamOptions { api_key: options.api_key.clone().or(Some("fake-key".into())), ..options };
    capture_payload_with(|o| compat::stream_simple(&model, context, o), options).await.0
}

/// `compat::stream` payload capture with an api key.
pub async fn stream_payload(model: &Model, context: &Context, options: StreamOptions) -> Value {
    let mut model = model.clone();
    model.base_url = dead_base_url(&model.base_url);
    let options = StreamOptions { api_key: options.api_key.clone().or(Some("fake-key".into())), ..options };
    capture_payload_with(|o| compat::stream(&model, context, o), options).await.0
}

/// Keep the path of `base_url` but point it at a closed local port.
pub fn dead_base_url(base_url: &str) -> String {
    match url::Url::parse(base_url) {
        Ok(url) if url.host_str() != Some("127.0.0.1") => {
            let path = url.path().trim_end_matches('/');
            format!("http://127.0.0.1:9{path}")
        }
        _ => base_url.to_string(),
    }
}

/// An auth context backed by a fixed environment map.
pub struct MapAuthContext {
    pub env: HashMap<String, String>,
    pub files: Vec<String>,
}

impl MapAuthContext {
    pub fn new(pairs: &[(&str, &str)]) -> Arc<dyn AuthContext> {
        Arc::new(MapAuthContext {
            env: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            files: vec![],
        })
    }
}

#[async_trait]
impl AuthContext for MapAuthContext {
    async fn env(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }
    async fn file_exists(&self, path: &str) -> bool {
        self.files.iter().any(|f| f == path)
    }
}

/// Anthropic SSE body from event objects (`event:` is the object's `type`).
pub fn anthropic_sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|event| format!("event: {}\ndata: {}\n\n", event["type"].as_str().unwrap_or("message"), event))
        .collect()
}

/// OpenAI-style `data:` SSE body plus `[DONE]`.
pub fn openai_sse(chunks: &[Value]) -> String {
    let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    body.push_str("data: [DONE]\n\n");
    body
}

/// A minimal successful Anthropic stream.
pub fn anthropic_ok_sse() -> String {
    anthropic_sse(&[
        json!({"type": "message_start", "message": {"id": "msg_test", "usage": {"input_tokens": 1, "output_tokens": 0}}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ])
}

/// Process-wide environment mutations are serialized through this lock.
pub static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Sets environment variables for the lifetime of the guard.
pub struct EnvGuard {
    saved: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    pub fn set(pairs: &[(&str, Option<&str>)]) -> Self {
        let mut saved = Vec::new();
        for (name, value) in pairs {
            saved.push((name.to_string(), std::env::var(name).ok()));
            // SAFETY: tests that touch the environment hold ENV_LOCK.
            unsafe {
                match value {
                    Some(v) => std::env::set_var(name, v),
                    None => std::env::remove_var(name),
                }
            }
        }
        EnvGuard { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.saved.drain(..) {
            unsafe {
                match value {
                    Some(v) => std::env::set_var(&name, v),
                    None => std::env::remove_var(&name),
                }
            }
        }
    }
}

/// Environment variable for live (credentialed) tests.
pub fn live_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// vitest `toMatchObject`: objects match by subset, arrays element-wise with equal length.
pub fn json_matches(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            expected.iter().all(|(key, value)| actual.get(key).is_some_and(|a| json_matches(a, value)))
        }
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len() && actual.iter().zip(expected).all(|(a, e)| json_matches(a, e))
        }
        _ => actual == expected,
    }
}

#[track_caller]
pub fn assert_json_matches(actual: &Value, expected: Value) {
    assert!(json_matches(actual, &expected), "expected match\n  actual: {actual:#}\nexpected: {expected:#}");
}

/// Capture the provider payload and cancel the request from the payload hook, so a real base URL
/// (whose host the adapter inspects) is never contacted.
pub async fn capture_payload_offline(
    run: impl FnOnce(StreamOptions) -> AssistantMessageEventStream,
    options: StreamOptions,
) -> Value {
    let signal = tokio_util::sync::CancellationToken::new();
    let cancel = signal.clone();
    let captured = Arc::new(Mutex::new(None::<Value>));
    let sink = captured.clone();
    let options = StreamOptions {
        signal: Some(signal),
        on_payload: Some(Arc::new(move |payload: Value, _model: Model| {
            sink.lock().get_or_insert(payload);
            cancel.cancel();
            Box::pin(async { None })
        })),
        ..options
    };
    let message = run(options).result().await;
    captured.lock().take().unwrap_or_else(|| panic!("payload captured: {:?}", message.error_message))
}
