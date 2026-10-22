//! Offline tests for the Anthropic Messages API against a local mock server
//! (ports of `anthropic-sse-parsing.test.ts` and request-shape tests).

mod common;

use common::*;
use pi_ai::api::anthropic_messages;
use pi_ai::providers::all::get_builtin_model;
use pi_ai::*;
use serde_json::json;

fn minimal_events() -> String {
    sse_body(&[
        ("message_start", json!({"type": "message_start", "message": {"id": "msg_test", "usage": {"input_tokens": 12, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}})),
        ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})),
        ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello"}})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
        ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"input_tokens": 12, "output_tokens": 5, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}})),
        ("message_stop", json!({"type": "message_stop"})),
    ])
}

fn model_at(base_url: &str, id: &str) -> Model {
    let mut model = get_builtin_model("anthropic", id).unwrap();
    model.base_url = base_url.to_string();
    model
}

fn context(text: &str) -> TranscriptContext {
    normalize_context(&Context {
        system_prompt: Some("You are helpful.".into()),
        messages: vec![Message::User(UserMessage::new(text, 1))],
        tools: Some(vec![Tool::new("read", "Read a file", json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}))]),
    })
}

#[tokio::test]
async fn streams_text_and_sends_sdk_shaped_request() {
    let server = serve(vec![MockResponse::sse(minimal_events())]).await;
    let model = model_at(&server.base_url, "claude-haiku-4-5");
    let observed = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let sink = observed.clone();
    let options = StreamOptions {
        api_key: Some("sk-ant-test".into()),
        on_provider_stream_event: Some(std::sync::Arc::new(move |event: serde_json::Value, _m: Model| {
            sink.lock().push(event["type"].as_str().unwrap_or_default().to_string());
            Box::pin(async {})
        })),
        ..Default::default()
    };
    let (events, result) = anthropic_messages::stream(&model, &context("Hello"), options).collect_all().await;
    assert_eq!(result.stop_reason, StopReason::Stop, "{:?}", result.error_message);
    assert_eq!(result.text(), "Hello");
    assert_eq!(result.response_id.as_deref(), Some("msg_test"));
    assert_eq!(result.usage.input, 12);
    assert_eq!(result.usage.output, 5);
    assert_eq!(result.usage.total_tokens, 17);
    assert!(result.duration_ms.is_some());
    assert_eq!(
        events.iter().map(|e| e.type_name()).collect::<Vec<_>>(),
        vec!["start", "text_start", "text_delta", "text_end", "done"]
    );
    assert_eq!(
        observed.lock().clone(),
        vec!["message_start", "content_block_start", "content_block_delta", "content_block_stop", "message_delta", "message_stop"]
    );

    let request = server.requests.lock()[0].clone();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/messages?beta=true");
    assert_eq!(request.header("x-api-key"), Some("sk-ant-test"));
    assert_eq!(request.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(request.header("anthropic-dangerous-direct-browser-access"), Some("true"));
    assert!(request.header("user-agent").unwrap().starts_with("pi/"));
    let body = request.json();
    assert_eq!(body["model"], "claude-haiku-4-5");
    assert_eq!(body["stream"], true);
    assert_eq!(body["system"], json!([{"type": "text", "text": "You are helpful.", "cache_control": {"type": "ephemeral"}}]));
    assert_eq!(body["messages"], json!([{"role": "user", "content": [{"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}]}]));
    let tool = &body["tools"][0];
    assert_eq!(tool["name"], "read");
    assert_eq!(tool["eager_input_streaming"], true);
    assert_eq!(tool["input_schema"], json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}));
    assert_eq!(tool["cache_control"], json!({"type": "ephemeral"}));
    assert!(body.get("betas").is_none());
}

#[tokio::test]
async fn tool_calls_thinking_and_oauth_headers() {
    let events = sse_body(&[
        ("message_start", json!({"type": "message_start", "message": {"id": "msg_1", "model": "claude-haiku-4-5", "usage": {"input_tokens": 5, "output_tokens": 0}}})),
        ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}})),
        ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "hmm"}})),
        ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig"}})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
        ("content_block_start", json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {}}})),
        ("content_block_delta", json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"path\": \"a."}})),
        ("content_block_delta", json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "ts\"}"}})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 1})),
        ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 9}})),
        ("message_stop", json!({"type": "message_stop"})),
    ]);
    let server = serve(vec![MockResponse::sse(events)]).await;
    let model = model_at(&server.base_url, "claude-haiku-4-5");
    let options = StreamOptions { api_key: Some("sk-ant-oat01-token".into()), ..Default::default() };
    let (events, result) = anthropic_messages::stream(&model, &context("Read it"), options).collect_all().await;
    assert_eq!(result.stop_reason, StopReason::ToolUse, "{:?}", result.error_message);
    let call = result.tool_calls().next().unwrap();
    assert_eq!(call.name, "read", "Claude Code names map back to the declared tool");
    assert_eq!(serde_json::Value::Object(call.arguments.clone()), json!({"path": "a.ts"}));
    let thinking = result.content[0].as_thinking().unwrap();
    assert_eq!(thinking.thinking, "hmm");
    assert_eq!(thinking.thinking_signature.as_deref(), Some("sig"));
    assert!(events.iter().any(|e| matches!(e, AssistantMessageEvent::ToolCallDelta { .. })));

    let request = server.requests.lock()[0].clone();
    assert_eq!(request.header("authorization"), Some("Bearer sk-ant-oat01-token"));
    assert!(request.header("x-api-key").is_none());
    assert_eq!(request.header("user-agent"), Some("claude-cli/2.1.280"));
    assert_eq!(request.header("anthropic-beta"), Some("claude-code-20250219,oauth-2025-04-20"));
    let body = request.json();
    assert_eq!(body["system"][0]["text"], "You are Claude Code, Anthropic's official CLI for Claude.");
    assert_eq!(body["tools"][0]["name"], "Read");
}

#[tokio::test]
async fn http_errors_and_missing_stop() {
    let server = serve(vec![MockResponse::json(
        400,
        json!({"type": "error", "error": {"type": "invalid_request_error", "message": "prompt is too long: 213462 tokens > 200000 maximum"}}).to_string(),
    )])
    .await;
    let model = model_at(&server.base_url, "claude-haiku-4-5");
    let result =
        anthropic_messages::stream(&model, &context("x"), StreamOptions { api_key: Some("k".into()), ..Default::default() })
            .result()
            .await;
    assert_eq!(result.stop_reason, StopReason::Error);
    let message = result.error_message.clone().unwrap();
    assert!(message.starts_with("400 {"), "{message}");
    assert!(is_context_overflow(&result, None));

    let truncated = sse_body(&[(
        "message_start",
        json!({"type": "message_start", "message": {"id": "m", "usage": {"input_tokens": 1}}}),
    )]);
    let server = serve(vec![MockResponse::sse(truncated)]).await;
    let model = model_at(&server.base_url, "claude-haiku-4-5");
    let result =
        anthropic_messages::stream(&model, &context("x"), StreamOptions { api_key: Some("k".into()), ..Default::default() })
            .result()
            .await;
    assert_eq!(result.error_message.as_deref(), Some("Anthropic stream ended before message_stop"));
    assert!(is_retryable_assistant_error(&result));
}

#[tokio::test]
async fn simple_options_map_reasoning() {
    let server = serve(vec![MockResponse::sse(minimal_events()), MockResponse::sse(minimal_events())]).await;
    let adaptive = model_at(&server.base_url, "claude-fable-5");
    let result = anthropic_messages::stream_simple(
        &adaptive,
        &context("x"),
        StreamOptions { api_key: Some("k".into()), reasoning: Some(ThinkingLevel::Xhigh), ..Default::default() },
    )
    .result()
    .await;
    assert_eq!(result.stop_reason, StopReason::Stop, "{:?}", result.error_message);
    let body = server.requests.lock()[0].json();
    assert_eq!(body["thinking"], json!({"type": "adaptive", "display": "summarized"}));
    assert_eq!(body["output_config"], json!({"effort": "xhigh"}));

    let mut budget = model_at(&server.base_url, "claude-haiku-4-5");
    budget.compat.get_or_insert_with(Default::default).force_adaptive_thinking = Some(false);
    budget.reasoning = true;
    anthropic_messages::stream_simple(
        &budget,
        &context("x"),
        StreamOptions { api_key: Some("k".into()), reasoning: Some(ThinkingLevel::Low), ..Default::default() },
    )
    .result()
    .await;
    let request = server.requests.lock()[1].clone();
    let body = request.json();
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 2048);
    assert!(request.header("anthropic-beta").unwrap().contains("interleaved-thinking-2025-05-14"));
}

#[tokio::test]
async fn missing_api_key_errors() {
    let model = model_at("http://127.0.0.1:9", "claude-haiku-4-5");
    let result = anthropic_messages::stream_simple(&model, &context("x"), StreamOptions::default()).result().await;
    assert_eq!(result.error_message.as_deref(), Some("No API key for provider: anthropic"));
}
