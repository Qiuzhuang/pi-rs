//! Offline tests for the OpenAI Chat Completions API against a local mock server.

mod common;

use common::*;
use pi_ai::api::openai_completions;
use pi_ai::providers::all::get_builtin_model;
use pi_ai::*;
use serde_json::json;

fn model_at(base_url: &str, provider: &str, id: &str) -> Model {
    let mut model = get_builtin_model(provider, id).unwrap_or_else(|| panic!("{provider}/{id}"));
    model.base_url = base_url.to_string();
    model
}

fn custom_model(base_url: &str) -> Model {
    Model {
        id: "local-model".into(),
        name: "Local".into(),
        api: "openai-completions".into(),
        provider: "local".into(),
        base_url: base_url.into(),
        reasoning: true,
        input: vec![ModelInputModality::Text],
        context_window: 32000,
        max_tokens: 4096,
        ..Default::default()
    }
}

fn context() -> TranscriptContext {
    normalize_context(&Context {
        system_prompt: Some("Be brief.".into()),
        messages: vec![Message::User(UserMessage::new("hi", 1))],
        tools: Some(vec![Tool::new("echo", "Echo", json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}))]),
    })
}

fn chunk(delta: serde_json::Value, finish: Option<&str>) -> serde_json::Value {
    json!({"id": "chatcmpl-1", "model": "local-model", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
}

#[tokio::test]
async fn streams_reasoning_text_and_tool_calls() {
    let body = openai_sse_body(&[
        chunk(json!({"role": "assistant", "reasoning_content": "think "}), None),
        chunk(json!({"reasoning_content": "more"}), None),
        chunk(json!({"content": "Hello"}), None),
        chunk(json!({"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "echo", "arguments": "{\"te"}}]}), None),
        chunk(json!({"tool_calls": [{"index": 0, "function": {"arguments": "xt\": \"hi\"}"}}]}), None),
        chunk(json!({}), Some("tool_calls")),
        json!({"id": "chatcmpl-1", "choices": [], "usage": {"prompt_tokens": 20, "completion_tokens": 7, "prompt_tokens_details": {"cached_tokens": 5}, "completion_tokens_details": {"reasoning_tokens": 3}}}),
    ]);
    let server = serve(vec![MockResponse::sse(body)]).await;
    let model = custom_model(&server.base_url);
    let options = StreamOptions { api_key: Some("sk-local".into()), reasoning: Some(ThinkingLevel::High), ..Default::default() };
    let (events, result) = openai_completions::stream_simple(&model, &context(), options).collect_all().await;
    assert_eq!(result.stop_reason, StopReason::ToolUse, "{:?}", result.error_message);
    assert_eq!(result.response_id.as_deref(), Some("chatcmpl-1"));
    let thinking = result.content[0].as_thinking().unwrap();
    assert_eq!(thinking.thinking, "think more");
    assert_eq!(thinking.thinking_signature.as_deref(), Some("reasoning_content"));
    assert_eq!(result.content[1].as_text().unwrap().text, "Hello");
    let call = result.tool_calls().next().unwrap();
    assert_eq!(call.id, "call_1");
    assert_eq!(serde_json::Value::Object(call.arguments.clone()), json!({"text": "hi"}));
    assert_eq!(result.usage.input, 15);
    assert_eq!(result.usage.cache_read, 5);
    assert_eq!(result.usage.output, 7);
    assert_eq!(result.usage.reasoning, Some(3));
    let types: Vec<&str> = events.iter().map(|e| e.type_name()).collect();
    assert_eq!(types.first(), Some(&"start"));
    assert_eq!(types.last(), Some(&"done"));
    assert!(types.contains(&"toolcall_end"));

    let request = server.requests.lock()[0].clone();
    assert_eq!(request.path, "/chat/completions");
    assert_eq!(request.header("authorization"), Some("Bearer sk-local"));
    let body = request.json();
    assert_eq!(body["model"], "local-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"], json!({"include_usage": true}));
    assert_eq!(body["store"], false);
    assert_eq!(body["reasoning_effort"], "high");
    assert_eq!(body["messages"][0], json!({"role": "developer", "content": "Be brief."}));
    assert_eq!(body["messages"][1], json!({"role": "user", "content": "hi"}));
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "echo");
    assert!(body["tools"][0]["function"].get("strict").is_none());
    assert!(body["max_completion_tokens"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn converts_history_with_tool_results_and_images() {
    let server = serve(vec![MockResponse::sse(openai_sse_body(&[chunk(json!({"content": "ok"}), Some("stop"))]))]).await;
    let mut model = custom_model(&server.base_url);
    model.input = vec![ModelInputModality::Text, ModelInputModality::Image];
    let assistant = AssistantMessage {
        content: vec![
            AssistantContent::Text(TextContent::new("calling")),
            AssistantContent::ToolCall(ToolCall {
                id: "call|item".into(),
                name: "echo".into(),
                arguments: json!({"text": "x"}).as_object().unwrap().clone(),
                ..Default::default()
            }),
        ],
        api: "openai-responses".into(),
        provider: "openai".into(),
        model: "gpt".into(),
        stop_reason: StopReason::ToolUse,
        timestamp: 2,
        ..Default::default()
    };
    let tool_result = ToolResultMessage {
        tool_call_id: "call|item".into(),
        tool_name: "echo".into(),
        content: vec![UserContentBlock::Image(ImageContent { data: "AAAA".into(), mime_type: "image/png".into() })],
        timestamp: 3,
        ..Default::default()
    };
    let ctx = normalize_context(&Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage::new("hi", 1)), assistant.into(), tool_result.into()],
        tools: None,
    });
    let result = openai_completions::stream(&model, &ctx, StreamOptions { api_key: Some("k".into()), ..Default::default() }).result().await;
    assert_eq!(result.stop_reason, StopReason::Stop, "{:?}", result.error_message);
    let body = server.requests.lock()[0].json();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[1]["content"], "calling");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_item");
    assert_eq!(messages[1]["tool_calls"][0]["function"]["arguments"], "{\"text\":\"x\"}");
    assert_eq!(messages[2], json!({"role": "tool", "content": "(see attached image)", "tool_call_id": "call_item"}));
    assert_eq!(messages[3]["role"], "user");
    assert_eq!(messages[3]["content"][1]["image_url"]["url"], "data:image/png;base64,AAAA");
    assert_eq!(body["tools"], json!([]));
}

#[tokio::test]
async fn errors_and_missing_finish_reason() {
    let server = serve(vec![MockResponse::json(429, json!({"error": {"message": "Rate limit reached", "type": "rate_limit"}}).to_string())]).await;
    let model = custom_model(&server.base_url);
    let result = openai_completions::stream(&model, &context(), StreamOptions { api_key: Some("k".into()), ..Default::default() }).result().await;
    assert_eq!(result.stop_reason, StopReason::Error);
    assert_eq!(result.error_message.as_deref(), Some(r#"429: {"message":"Rate limit reached","type":"rate_limit"}"#));
    assert!(is_retryable_assistant_error(&result));

    let server = serve(vec![MockResponse::sse(openai_sse_body(&[chunk(json!({"content": "partial"}), None)]))]).await;
    let model = custom_model(&server.base_url);
    let result = openai_completions::stream(&model, &context(), StreamOptions { api_key: Some("k".into()), ..Default::default() }).result().await;
    assert_eq!(result.error_message.as_deref(), Some("Stream ended without finish_reason"));

    let server = serve(vec![MockResponse::sse(
        "data: {\"error\":{\"message\":\"Provider returned error\",\"metadata\":{\"raw\":\"upstream boom\"}}}\n\n".to_string(),
    )])
    .await;
    let model = custom_model(&server.base_url);
    let result = openai_completions::stream(&model, &context(), StreamOptions { api_key: Some("k".into()), ..Default::default() }).result().await;
    assert_eq!(result.error_message.as_deref(), Some("Provider returned error\nupstream boom"));
}

#[tokio::test]
async fn provider_compat_detection_shapes_requests() {
    let server = serve(vec![
        MockResponse::sse(openai_sse_body(&[chunk(json!({"content": "ok"}), Some("stop"))])),
        MockResponse::sse(openai_sse_body(&[chunk(json!({"content": "ok"}), Some("stop"))])),
    ])
    .await;
    let deepseek = model_at(&server.base_url, "deepseek", get_builtin_models("deepseek")[0].id.as_str());
    openai_completions::stream_simple(&deepseek, &context(), StreamOptions { api_key: Some("k".into()), reasoning: Some(ThinkingLevel::High), ..Default::default() })
        .result()
        .await;
    let body = server.requests.lock()[0].json();
    if deepseek.reasoning {
        assert_eq!(body["thinking"], json!({"type": "enabled"}));
    }
    assert!(body.get("max_tokens").is_some());
    assert!(body.get("store").is_none());

    let mut openrouter = model_at(&server.base_url, "openrouter", "anthropic/claude-haiku-4.5");
    openrouter.base_url = server.base_url.clone();
    openai_completions::stream(&openrouter, &context(), StreamOptions { api_key: Some("k".into()), session_id: Some("sess".into()), ..Default::default() })
        .result()
        .await;
    let request = server.requests.lock()[1].clone();
    assert_eq!(request.header("x-session-id"), Some("sess"));
    let body = request.json();
    assert_eq!(body["messages"][0]["content"][0]["cache_control"], json!({"type": "ephemeral"}));
    assert_eq!(body["tools"][0]["cache_control"], json!({"type": "ephemeral"}));
}

use pi_ai::providers::all::get_builtin_models;
