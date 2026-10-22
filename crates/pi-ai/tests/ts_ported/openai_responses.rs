//! Cases of the OpenAI Responses tests not covered by `tests/openai_responses.rs`:
//! `openai-responses-compat.test.ts`, `openai-responses-terminal-event.test.ts`,
//! `openai-responses-namespace.test.ts` and `openai-responses-mid-convo-effort.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_ai::api::openai_responses;
use pi_ai::api::openai_responses_shared::{convert_responses_messages, process_responses_stream};
use pi_ai::providers::all::get_builtin_model;
use pi_ai::*;
use serde_json::{Value, json};

use crate::mock_http::{MockServer, Request, Response};
use crate::support::*;

fn builtin(provider: &str, id: &str) -> Model {
    get_builtin_model(provider, id).unwrap_or_else(|| panic!("{provider}/{id}"))
}

fn rehost(base_url: &str, server: &str) -> String {
    let stripped = base_url.split_once("://").map(|(_, rest)| rest).unwrap_or(base_url);
    format!("{server}/{}", stripped.trim_end_matches('/'))
}

fn sys_hi() -> TranscriptContext {
    normalize_context(&ctx(json!({"systemPrompt": "sys", "messages": [{"role": "user", "content": "hi"}]})))
}

struct Run {
    request: Request,
    payload: Value,
    message: AssistantMessage,
}

async fn run(model: &Model, context: &TranscriptContext, options: StreamOptions, body: String) -> Run {
    let server = MockServer::start(move |_, _| Response::sse(&body)).await;
    let mut model = model.clone();
    model.base_url = rehost(&model.base_url, &server.url);
    let options = StreamOptions { api_key: options.api_key.clone().or(Some("sk-test-key".into())), ..options };
    let message = openai_responses::stream(&model, context, options).result().await;
    let request = server.request(0);
    let payload = request.json();
    Run { request, payload, message }
}

async fn done(model: &Model, options: StreamOptions) -> Run {
    run(model, &sys_hi(), options, "data: [DONE]\n\n".into()).await
}

fn session(id: &str) -> StreamOptions {
    StreamOptions { session_id: Some(id.into()), ..Default::default() }
}

// ---------------------------------------------------------------------------
// Port of openai-responses-compat.test.ts (model lists, proxy and OpenCode affinity, total cost)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reasoning_effort_none_for_every_model_that_supports_off() {
    for id in [
        "gpt-5.1", "gpt-5.2", "gpt-5.3-codex", "gpt-5.4", "gpt-5.4-mini", "gpt-5.4-nano", "gpt-5.5", "gpt-5.6-sol",
        "gpt-5.6-terra", "gpt-5.6-luna", "gpt-6-sol", "gpt-6-luna",
    ] {
        let run = done(&builtin("openai", id), StreamOptions::default()).await;
        assert_eq!(run.payload["reasoning"], json!({"effort": "none"}), "{id}");
    }
    for id in ["gpt-5", "gpt-5-mini", "gpt-5-nano", "gpt-5-pro", "gpt-5.2-pro", "gpt-5.4-pro", "gpt-5.5-pro"] {
        let run = done(&builtin("openai", id), StreamOptions::default()).await;
        assert!(run.payload.get("reasoning").is_none(), "{id}");
    }
}

#[tokio::test]
async fn proxy_and_opencode_cache_affinity() {
    let mut proxy = builtin("openai", "gpt-5.4");
    proxy.provider = "opencode".into();
    proxy.base_url = "https://proxy.example.com/v1".into();
    let run1 = done(&proxy, session("session-123")).await;
    assert_eq!(run1.request.header("session_id"), Some("session-123"));
    assert_eq!(run1.request.header("x-client-request-id"), Some("session-123"));

    let mut openrouter = builtin("openai", "gpt-5.4");
    openrouter.provider = "openrouter".into();
    openrouter.base_url = "https://openrouter.ai/api/v1".into();
    let run2 = done(&openrouter, session("session-openrouter")).await;
    assert_eq!(run2.request.header("session_id"), None);
    assert_eq!(run2.request.header("x-client-request-id"), None);
    assert_eq!(run2.request.header("x-session-id"), Some("session-openrouter"));
    assert!(run2.payload.get("session_id").is_none());
    assert_eq!(run2.payload["prompt_cache_key"], "session-openrouter");

    let mut configured = builtin("openai", "gpt-5.4");
    configured.provider = "proxy".into();
    configured.base_url = "https://proxy.example.com/v1".into();
    configured.compat = Some(serde_json::from_value(json!({"sessionAffinityFormat": "openrouter"})).unwrap());
    let run3 = done(&configured, session("session-proxy")).await;
    assert_eq!(run3.request.header("session_id"), None);
    assert_eq!(run3.request.header("x-client-request-id"), None);
    assert_eq!(run3.request.header("x-session-id"), Some("session-proxy"));
    assert_eq!(run3.payload["prompt_cache_key"], "session-proxy");

    let opencode = builtin("opencode", "gpt-5.4");
    assert_eq!(serde_json::to_value(opencode.compat()).unwrap()["sessionAffinityFormat"], "openai-nosession");
    let run4 = done(&opencode, session("session-opencode")).await;
    assert_eq!(run4.request.header("session_id"), None);
    assert_eq!(run4.request.header("x-client-request-id"), Some("session-opencode"));
    assert_eq!(run4.request.header("x-session-id"), None);
    assert_eq!(run4.payload["prompt_cache_key"], "session-opencode");

    let mut nosession = proxy.clone();
    nosession.compat = Some(serde_json::from_value(json!({"sessionAffinityFormat": "openai-nosession"})).unwrap());
    let run5 = done(&nosession, session("session-123")).await;
    assert_eq!(run5.request.header("session_id"), None);
    assert_eq!(run5.request.header("x-client-request-id"), Some("session-123"));
    assert_eq!(run5.payload["prompt_cache_key"], "session-123");
}

#[tokio::test]
async fn service_tier_multipliers_apply_to_the_total() {
    for (id, tier, response_tier, multiplier) in [
        ("gpt-5.4", "priority", "priority", 2.0),
        ("gpt-5.5", "priority", "priority", 2.5),
        ("gpt-5.5", "flex", "flex", 0.5),
        ("gpt-6-luna", "priority", "fast", 2.0),
        ("gpt-6-luna", "fast", "fast", 2.0),
    ] {
        let model = builtin("openai", id);
        let tokens = 100_000u64;
        let scale = tokens as f64 / 1_000_000.0;
        let body = format!(
            "data: {}\n\n",
            json!({"type": "response.completed", "response": {"status": "completed", "service_tier": response_tier,
                "usage": {"input_tokens": tokens, "output_tokens": tokens, "total_tokens": tokens * 2, "input_tokens_details": {"cached_tokens": 0}}}})
        );
        let run = run(&model, &sys_hi(), StreamOptions::default().with_extra("serviceTier", tier), body).await;
        let rates = model.cost.rates();
        let expected = (rates.input + rates.output) * multiplier * scale;
        assert!((run.message.usage.cost.total - expected).abs() < 1e-9, "{id} {tier}: {} vs {expected}", run.message.usage.cost.total);
    }
}

// ---------------------------------------------------------------------------
// Port of openai-responses-terminal-event.test.ts (provider event models, incomplete details)
// ---------------------------------------------------------------------------

fn gpt5_mini() -> Model {
    model(json!({"id": "gpt-5-mini", "name": "GPT-5 Mini", "api": "openai-responses", "provider": "openai",
        "baseUrl": "https://api.openai.com/v1", "reasoning": true, "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 400000, "maxTokens": 128000}))
}

async fn process(model: &Model, events: Vec<Value>) -> (anyhow::Result<()>, AssistantMessage, Vec<AssistantMessageEvent>) {
    let mut output = AssistantMessage { stop_reason: StopReason::Pending, ..AssistantMessage::empty_for(model, 1) };
    let (sender, mut stream) = AssistantMessageEventStream::new();
    let result = process_responses_stream(futures::stream::iter(events.into_iter().map(Ok)), &mut output, &sender, model, Default::default()).await;
    sender.end(None);
    let mut pushed = Vec::new();
    while let Some(event) = stream.next_event().await {
        pushed.push(event);
    }
    (result, output, pushed)
}

#[tokio::test]
async fn provider_events_carry_the_request_model() {
    let model = gpt5_mini();
    let body: String = [
        json!({"type": "response.created", "sequence_number": 0, "response": {"id": "resp_wrapper_early_eof"}}),
        json!({"type": "response.output_item.added", "sequence_number": 1, "output_index": 0, "item": {"type": "reasoning", "id": "rs", "summary": []}}),
        json!({"type": "response.reasoning_text.delta", "sequence_number": 2, "output_index": 0, "content_index": 0, "item_id": "rs", "delta": "partial"}),
    ]
    .iter()
    .map(|e| format!("data: {e}\n\n"))
    .collect();
    let seen = Arc::new(Mutex::new(Vec::<Model>::new()));
    let sink = seen.clone();
    let options = StreamOptions {
        on_provider_stream_event: Some(Arc::new(move |_event: Value, event_model: Model| {
            let sink = sink.clone();
            Box::pin(async move {
                tokio::task::yield_now().await;
                sink.lock().push(event_model);
            })
        })),
        ..Default::default()
    };
    let server = MockServer::start(move |_, _| Response::sse(&body)).await;
    let mut request_model = model.clone();
    request_model.base_url = rehost(&model.base_url, &server.url);
    let result = openai_responses::stream(&request_model, &sys_hi(), StreamOptions { api_key: Some("test".into()), ..options }).result().await;
    assert_eq!(result.stop_reason, StopReason::Error);
    let seen = seen.lock().clone();
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|m| *m == request_model));
}

#[tokio::test]
async fn incomplete_terminal_events_keep_the_response_id_and_usage() {
    let model = gpt5_mini();
    let (result, output, _) = process(&model, vec![json!({"type": "response.incomplete", "sequence_number": 0, "response": {
        "id": "resp_incomplete", "status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"},
        "usage": {"input_tokens": 30, "output_tokens": 12, "total_tokens": 42, "input_tokens_details": {"cached_tokens": 5}}}})])
    .await;
    result.unwrap();
    assert_eq!(output.response_id.as_deref(), Some("resp_incomplete"));
    assert_eq!(output.stop_reason, StopReason::Length);
    let u = &output.usage;
    assert_eq!((u.input, u.output, u.cache_read, u.cache_write, u.total_tokens), (25, 12, 5, 0, 42));

    // A provisional final-answer stop is replaced by the incomplete terminal reason.
    let phased = vec![
        json!({"type": "response.output_item.added", "output_index": 0, "item": {"type": "message", "id": "msg_phase", "role": "assistant", "status": "in_progress", "content": [], "phase": "final_answer"}}),
        json!({"type": "response.output_item.done", "output_index": 0, "item": {"type": "message", "id": "msg_phase", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "answer", "annotations": []}], "phase": "final_answer"}}),
        json!({"type": "response.incomplete", "response": {"id": "resp_phase", "status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"}}}),
    ];
    let (result, output, events) = process(&model, phased).await;
    result.unwrap();
    let observed: Vec<StopReason> = events.iter().filter(|e| !e.is_terminal()).map(|e| e.message().stop_reason).collect();
    assert_eq!(observed, vec![StopReason::Stop, StopReason::Stop]);
    assert_eq!(output.stop_reason, StopReason::Length);
}

// ---------------------------------------------------------------------------
// Port of openai-responses-namespace.test.ts (ordinary calls)
// ---------------------------------------------------------------------------

#[test]
fn ordinary_function_calls_get_no_namespace() {
    let model = model(json!({"id": "gpt-5.4", "name": "GPT-5.4", "api": "openai-responses", "provider": "openai",
        "baseUrl": "https://api.openai.com/v1", "reasoning": true, "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 400000, "maxTokens": 128000}));
    let context = normalize_context(&ctx(json!({"messages": [{"role": "assistant", "api": "openai-responses", "provider": "openai", "model": "gpt-5.4",
        "stopReason": "pending", "content": [{"type": "toolCall", "id": "call_test|fc_test", "name": "lookup", "arguments": {"value": "hello"}}]}]})));
    let items = convert_responses_messages(&model, &context, &["openai"], &Default::default()).unwrap();
    let call = items.iter().find(|i| i["type"] == "function_call").unwrap();
    assert!(call.get("namespace").is_none());
}

// ---------------------------------------------------------------------------
// Port of openai-responses-mid-convo-effort.test.ts ("adds items only before replies recorded by this model")
// ---------------------------------------------------------------------------

fn luna(compat: Value) -> Model {
    model(json!({
        "id": "gpt-6-luna", "name": "GPT-6 Luna", "api": "openai-responses", "provider": "openai",
        "baseUrl": "https://api.openai.com/v1", "reasoning": true,
        "thinkingLevelMap": {"off": "none", "minimal": null, "low": "low", "medium": "medium", "high": "high", "xhigh": "xhigh", "max": "max"},
        "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": 272000, "maxTokens": 128000, "compat": compat
    }))
}

fn reply_events() -> String {
    let item = json!({"type": "message", "id": "msg_resp_1", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "answer", "annotations": []}]});
    [
        json!({"type": "response.created", "response": {"id": "resp_1"}}),
        json!({"type": "response.output_item.added", "output_index": 0, "item": item}),
        json!({"type": "response.output_item.done", "output_index": 0, "item": item}),
        json!({"type": "response.completed", "response": {"id": "resp_1", "status": "completed"}}),
    ]
    .iter()
    .map(|e| format!("data: {e}\n\n"))
    .collect()
}

#[tokio::test]
async fn configuration_updates_only_precede_replies_recorded_by_this_model() {
    let unstamped_run = run(&luna(json!({})), &normalize_context(&user_ctx("one")), StreamOptions::default().with_extra("reasoningEffort", "high"), reply_events()).await;
    let mut unstamped = unstamped_run.message.clone();
    assert_ne!(unstamped.stop_reason, StopReason::Error, "{:?}", unstamped.error_message);
    assert_eq!(unstamped.provider_thinking_level, None);
    unstamped.provider = "openai".into();
    let mut other_provider = unstamped.clone();
    other_provider.provider = "opencode".into();
    other_provider.provider_thinking_level = Some("high".into());
    let mut other_model = unstamped.clone();
    other_model.model = "gpt-6-sol".into();
    other_model.provider_thinking_level = Some("high".into());
    let mut recorded = unstamped.clone();
    recorded.provider_thinking_level = Some("high".into());
    let u = |t: &str| Message::User(UserMessage::new(t, 0));
    let messages = vec![
        u("one"),
        Message::Assistant(unstamped),
        u("two"),
        Message::Assistant(other_provider),
        u("three"),
        Message::Assistant(other_model),
        u("four"),
        Message::Assistant(recorded),
        u("five"),
    ];
    let context = normalize_context(&Context { system_prompt: None, messages, tools: None });
    let run = run(&luna(json!({"supportsMidConvoEffort": true})), &context, StreamOptions::default().with_extra("reasoningEffort", "low"), reply_events()).await;
    assert_ne!(run.message.stop_reason, StopReason::Error, "{:?}", run.message.error_message);
    let kinds: Vec<String> = run.payload["input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.get("type").or_else(|| i.get("role")).and_then(Value::as_str).unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        vec!["user", "message", "user", "message", "user", "message", "user", "configuration_update", "message", "user", "configuration_update"]
    );
    let updates: Vec<Value> = run.payload["input"].as_array().unwrap().iter().filter(|i| i["type"] == "configuration_update").cloned().collect();
    assert_eq!(
        updates,
        vec![
            json!({"type": "configuration_update", "reasoning": {"effort": "high"}}),
            json!({"type": "configuration_update", "reasoning": {"effort": "low"}})
        ]
    );
}
