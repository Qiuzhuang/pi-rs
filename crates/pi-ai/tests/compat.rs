//! Port of compat-path tests (`faux-provider.test.ts` registration, `stream.test.ts` dispatch).

mod common;

use std::sync::Arc;

use common::*;
use pi_ai::compat;
use pi_ai::providers::faux::*;
use pi_ai::*;
use serde_json::json;

fn ctx(text: &str) -> Context {
    Context { messages: vec![Message::User(UserMessage::new(text, 1))], ..Default::default() }
}

#[tokio::test]
async fn faux_registration_and_unregister() {
    let registration = compat::register_faux_provider(Default::default());
    registration.set_responses(vec![faux_text_message("hello").into()]);
    let response = compat::complete(&registration.get_model(), &ctx("hi"), StreamOptions::default()).await;
    assert_eq!(response.text(), "hello");
    assert_eq!(registration.state().call_count, 1);
    registration.unregister();
    let after = compat::complete(&registration.get_model(), &ctx("hi"), StreamOptions::default()).await;
    assert_eq!(after.error_message, Some(format!("No API provider registered for api: {}", registration.api())));
}

#[tokio::test]
async fn aborting_before_first_chunk_via_compat() {
    let registration = compat::register_faux_provider(RegisterFauxProviderOptions { tokens_per_second: Some(50.0), ..Default::default() });
    registration.set_responses(vec![faux_text_message("abcdefghijklmnopqrstuvwxyz").into()]);
    let signal = tokio_util::sync::CancellationToken::new();
    signal.cancel();
    let (events, result) =
        compat::stream(&registration.get_model(), &ctx("hi"), StreamOptions { signal: Some(signal), ..Default::default() })
            .collect_all()
            .await;
    assert_eq!(events.len(), 1);
    assert_eq!(result.stop_reason, StopReason::Aborted);
    registration.unregister();
}

struct EchoApi;

impl ProviderStreams for EchoApi {
    fn stream(&self, model: &Model, context: &TranscriptContext, _options: StreamOptions) -> AssistantMessageEventStream {
        let (sender, stream) = AssistantMessageEventStream::new();
        let mut message = AssistantMessage::empty_for(model, utils::now_ms());
        let last = context.messages.last().and_then(Message::as_user).map(|u| match &u.content {
            UserContent::Text(t) => t.clone(),
            UserContent::Blocks(_) => String::new(),
        });
        message.content.push(AssistantContent::Text(TextContent::new(format!("echo: {}", last.unwrap_or_default()))));
        sender.push(AssistantMessageEvent::Start { partial: message.clone() });
        sender.push(AssistantMessageEvent::Done { reason: StopReason::Stop, message });
        stream
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.stream(model, context, options)
    }
}

#[tokio::test]
async fn custom_api_registration_and_mismatch() {
    compat::register_api_provider("echo-api", Arc::new(EchoApi), Some("echo-test"));
    let model = Model { id: "e".into(), api: "echo-api".into(), provider: "echo".into(), ..Default::default() };
    let result = compat::complete(&model, &ctx("ping"), StreamOptions::default()).await;
    assert_eq!(result.text(), "echo: ping");
    let provider = compat::get_api_provider("echo-api").unwrap();
    let wrong = Model { api: "other".into(), ..model.clone() };
    let mismatch = provider.streams.stream(&wrong, &normalize_context(&ctx("x")), StreamOptions::default()).result().await;
    assert_eq!(mismatch.error_message.as_deref(), Some("Mismatched api: other expected echo-api"));
    compat::unregister_api_providers("echo-test");
    assert!(compat::get_api_provider("echo-api").is_none());
    assert!(compat::get_api_provider("anthropic-messages").is_some());
}

#[tokio::test]
async fn builtin_dispatch_uses_provider_and_explicit_key() {
    let body = sse_body(&[
        ("message_start", json!({"type": "message_start", "message": {"id": "m", "usage": {"input_tokens": 1}}})),
        ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": "hi"}})),
        ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
        ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}})),
        ("message_stop", json!({"type": "message_stop"})),
    ]);
    let server = serve(vec![MockResponse::sse(body)]).await;
    let mut model = compat::get_model("anthropic", "claude-haiku-4-5").unwrap();
    model.base_url = server.base_url.clone();
    let result = compat::complete(&model, &ctx("x"), StreamOptions { api_key: Some("sk-ant-x".into()), ..Default::default() }).await;
    assert_eq!(result.stop_reason, StopReason::Stop, "{:?}", result.error_message);
    assert_eq!(result.text(), "hi");
    assert_eq!(server.requests.lock()[0].header("x-api-key"), Some("sk-ant-x"));
}

