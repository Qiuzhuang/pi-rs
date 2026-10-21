//! Offline tests for the pi-messages API (port of `pi-messages.test.ts`).

mod responses_mock;

use std::sync::Arc;

use parking_lot::Mutex;
use pi_ai::api::pi_messages;
use pi_ai::*;
use responses_mock::*;
use serde_json::{Value, json};

fn model(base_url: &str) -> Model {
    serde_json::from_value(json!({
        "id": "auto", "name": "Radius Auto", "api": "pi-messages", "provider": "radius", "baseUrl": base_url,
        "reasoning": false, "input": ["text"], "cost": { "input": 1, "output": 2, "cacheRead": 0.1, "cacheWrite": 0.2 },
        "contextWindow": 128000, "maxTokens": 16384
    }))
    .unwrap()
}

fn context() -> Context {
    Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage::new("Hello", 1))],
        tools: None,
    }
}

fn usage() -> Value {
    json!({ "input": 10, "output": 5, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 15, "cost": { "input": 0.1, "output": 0.2, "cacheRead": 0, "cacheWrite": 0, "total": 0.3 } })
}

async fn sse_server(events: Vec<Value>) -> MockHttp {
    serve_sequence(vec![Reply::sse(data_sse(&events))]).await
}

fn key(key: &str) -> StreamOptions {
    StreamOptions {
        api_key: Some(key.into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn streams_text_and_tool_calls() {
    let server = sse_server(vec![
        json!({ "type": "start" }),
        json!({ "type": "text_start", "contentIndex": 0 }),
        json!({ "type": "text_delta", "contentIndex": 0, "delta": "Hel" }),
        json!({ "type": "text_delta", "contentIndex": 0, "delta": "lo" }),
        json!({ "type": "text_end", "contentIndex": 0, "content": "Hello" }),
        json!({ "type": "toolcall_start", "contentIndex": 1, "id": "call_1", "toolName": "read" }),
        json!({ "type": "toolcall_delta", "contentIndex": 1, "delta": "{\"path\":" }),
        json!({ "type": "toolcall_delta", "contentIndex": 1, "delta": "\"a.txt\"}" }),
        json!({ "type": "toolcall_end", "contentIndex": 1, "toolCall": { "type": "toolCall", "id": "call_1", "name": "read", "arguments": { "path": "a.txt" } } }),
        json!({ "type": "done", "reason": "toolUse", "usage": usage(), "responseId": "resp_1", "providerThinkingLevel": "high" }),
    ])
    .await;
    let model = model(&format!("{}/v1", server.base_url));
    let mut options = key("test-key").with_extra("toolChoice", "auto");
    options.session_id = Some("session-1".into());
    options.max_tokens = Some(100);
    let mut headers = ProviderHeaders::new();
    headers.insert("x-custom".into(), Some("1".into()));
    options.headers = Some(headers);
    let (events, message) = pi_messages::stream(&model, &normalize_context(&context()), options)
        .collect_all()
        .await;

    assert_eq!(events[0].message().stop_reason, StopReason::Pending);
    assert_eq!(message.stop_reason, StopReason::ToolUse);
    assert_eq!(serde_json::to_value(&message.usage).unwrap(), usage());
    assert_eq!(message.response_id.as_deref(), Some("resp_1"));
    assert_eq!(message.provider_thinking_level.as_deref(), Some("high"));
    assert_eq!(
        (message.model.as_str(), message.provider.as_str()),
        ("auto", "radius")
    );
    assert_eq!(
        serde_json::to_value(&message.content).unwrap(),
        json!([
            { "type": "text", "text": "Hello" },
            { "type": "toolCall", "id": "call_1", "name": "read", "arguments": { "path": "a.txt" } }
        ])
    );
    assert!(events.iter().any(|e| e.type_name() == "text_delta"));
    assert_eq!(
        events
            .iter()
            .filter(|e| e.type_name() == "toolcall_end")
            .count(),
        1
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/messages");
    assert_eq!(requests[0].header("authorization"), Some("Bearer test-key"));
    assert_eq!(requests[0].header("x-custom"), Some("1"));
    assert_eq!(
        requests[0].json(),
        json!({
            "model": "auto",
            "context": serde_json::to_value(&context().messages).map(|m| json!({ "messages": m })).unwrap(),
            "options": { "maxTokens": 100, "sessionId": "session-1", "toolChoice": "auto" }
        })
    );
}

#[tokio::test]
async fn forwards_wire_events_in_order() {
    let wire = vec![
        json!({ "type": "start" }),
        json!({ "type": "text_start", "contentIndex": 0 }),
        json!({ "type": "text_delta", "contentIndex": 0, "delta": "Hello", "gatewayField": "upstream-value" }),
        json!({ "type": "text_end", "contentIndex": 0, "content": "Hello" }),
        json!({ "type": "done", "reason": "stop", "usage": usage(), "responseId": "resp_1" }),
    ];
    let server = sse_server(wire.clone()).await;
    let model = model(&format!("{}/v1", server.base_url));
    let received: Arc<Mutex<Vec<Value>>> = Arc::default();
    let sink = received.clone();
    let mut options = key("test-key");
    options.on_provider_stream_event = Some(Arc::new(move |event: &Value, _m: &Model| {
        sink.lock().push(event.clone())
    }));
    let message = pi_messages::stream_simple(&model, &normalize_context(&context()), options)
        .result()
        .await;
    assert_eq!(*received.lock(), wire);
    assert_eq!(message.stop_reason, StopReason::Stop);
    assert_eq!(
        serde_json::to_value(&message.content).unwrap(),
        json!([{ "type": "text", "text": "Hello" }])
    );
}

#[tokio::test]
async fn debug_query_and_on_response_headers() {
    let server = serve_sequence(vec![
        Reply::sse(data_sse(&[
            json!({ "type": "done", "reason": "stop", "usage": usage() }),
        ]))
        .header("x-pi-gateway-upstream-provider", "anthropic"),
    ])
    .await;
    let model = model(&format!("{}/v1", server.base_url));
    let observed: Arc<Mutex<Option<ProviderResponse>>> = Arc::default();
    let sink = observed.clone();
    let mut options = key("test-key").with_extra("debug", true);
    options.on_response = Some(Arc::new(move |response, _model| {
        *sink.lock() = Some(response);
        Box::pin(async {})
    }));
    let message = pi_messages::stream_simple(&model, &normalize_context(&context()), options)
        .result()
        .await;
    assert_eq!(message.stop_reason, StopReason::Stop);
    assert_eq!(server.requests()[0].path, "/v1/messages?debug=1");
    let observed = observed.lock().clone().unwrap();
    assert_eq!(
        observed
            .headers
            .get("x-pi-gateway-upstream-provider")
            .map(String::as_str),
        Some("anthropic")
    );
}

#[tokio::test]
async fn backend_errors_carry_diagnostics() {
    let server = serve_sequence(vec![Reply::json(
        401,
        json!({ "error": { "message": "Token expired", "code": "unauthorized" } }).to_string(),
    )])
    .await;
    let model = model(&format!("{}/v1", server.base_url));
    let message = pi_messages::stream(&model, &normalize_context(&context()), key("stale"))
        .result()
        .await;
    assert_eq!(message.stop_reason, StopReason::Error);
    let error = message.error_message.clone().unwrap();
    assert!(
        error.contains("401") && error.contains("Token expired") && error.contains("unauthorized"),
        "{error}"
    );
    let diagnostic = &message.diagnostics.unwrap()[0];
    assert_eq!(diagnostic.diagnostic_type, "pi_messages_response_failure");
    assert_eq!(diagnostic.details.as_ref().unwrap()["status"], 401);
}

#[tokio::test]
async fn server_error_events_missing_key_and_truncated_streams() {
    let server = sse_server(vec![
        json!({ "type": "start" }),
        json!({ "type": "error", "reason": "error", "usage": usage(), "errorMessage": "Upstream failed" }),
    ])
    .await;
    let model_a = model(&format!("{}/v1", server.base_url));
    let message = pi_messages::stream(&model_a, &normalize_context(&context()), key("test-key"))
        .result()
        .await;
    assert_eq!(message.stop_reason, StopReason::Error);
    assert_eq!(message.error_message.as_deref(), Some("Upstream failed"));
    assert_eq!(serde_json::to_value(&message.usage).unwrap(), usage());

    let message = pi_messages::stream(
        &model("http://127.0.0.1:1/v1"),
        &normalize_context(&context()),
        StreamOptions::default(),
    )
    .result()
    .await;
    assert!(
        message
            .error_message
            .unwrap()
            .contains("No API key provided")
    );

    let server = sse_server(vec![
        json!({ "type": "start" }),
        json!({ "type": "text_start", "contentIndex": 0 }),
        json!({ "type": "text_delta", "contentIndex": 0, "delta": "partial" }),
    ])
    .await;
    let model_b = model(&format!("{}/v1", server.base_url));
    let message = pi_messages::stream(&model_b, &normalize_context(&context()), key("test-key"))
        .result()
        .await;
    assert_eq!(message.stop_reason, StopReason::Error);
    assert!(
        message
            .error_message
            .unwrap()
            .contains("stream ended without a terminal event")
    );
}

#[test]
fn registered_as_builtin_api() {
    assert!(pi_ai::api::registry::resolve_builtin_api("pi-messages").is_some());
    for api in [
        "openai-responses",
        "azure-openai-responses",
        "openai-codex-responses",
    ] {
        assert!(
            pi_ai::api::registry::resolve_builtin_api(api).is_some(),
            "{api}"
        );
    }
}
