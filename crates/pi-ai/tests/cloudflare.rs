//! Offline tests for Cloudflare stream wrapping and binding-style auth headers (ports of
//! `cloudflare-stream.test.ts` and `cloudflare-ai-binding.test.ts`).

mod mock_http;

use std::sync::Arc;

use mock_http::{MockServer, Response};
use parking_lot::Mutex;
use pi_ai::api::cloudflare::{CLOUDFLARE_GATEWAY_BINDING_AUTH_SENTINEL, ai_binding_auth_headers, ai_binding_gateway_base_url};
use pi_ai::api::openai_completions;
use pi_ai::providers::cloudflare::CloudflareStreams;
use pi_ai::*;
use serde_json::json;

fn gateway_model() -> Model {
    serde_json::from_value(json!({
        "id": "model", "name": "model", "api": "openai-completions", "provider": "cloudflare-ai-gateway",
        "baseUrl": "https://gateway.ai.cloudflare.com/v1/{CLOUDFLARE_ACCOUNT_ID}/{CLOUDFLARE_GATEWAY_ID}/openai",
        "reasoning": false, "input": ["text"], "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
        "contextWindow": 1000, "maxTokens": 100
    }))
    .unwrap()
}

struct Capture(Arc<Mutex<Vec<String>>>);

impl ProviderStreams for Capture {
    fn stream(&self, model: &Model, _: &TranscriptContext, _: StreamOptions) -> AssistantMessageEventStream {
        self.0.lock().push(model.base_url.clone());
        AssistantMessageEventStream::new().1
    }
    fn stream_simple(&self, model: &Model, _: &TranscriptContext, _: StreamOptions) -> AssistantMessageEventStream {
        self.0.lock().push(model.base_url.clone());
        AssistantMessageEventStream::new().1
    }
}

#[tokio::test]
async fn materializes_endpoints_before_dispatch() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let streams = CloudflareStreams(Arc::new(Capture(captured.clone())));
    let context = normalize_context(&Context::default());
    let mut env = ProviderEnv::new();
    env.insert("CLOUDFLARE_ACCOUNT_ID".into(), "account".into());
    env.insert("CLOUDFLARE_GATEWAY_ID".into(), "gateway".into());
    let options = StreamOptions { env: Some(env), ..Default::default() };
    streams.stream(&gateway_model(), &context, options.clone());
    streams.stream_simple(&gateway_model(), &context, options);
    assert_eq!(
        *captured.lock(),
        ["https://gateway.ai.cloudflare.com/v1/account/gateway/openai", "https://gateway.ai.cloudflare.com/v1/account/gateway/openai"]
    );
    captured.lock().clear();
    streams.stream_simple(&gateway_model(), &context, StreamOptions::default());
    assert_eq!(*captured.lock(), [gateway_model().base_url]);
}

#[tokio::test]
async fn binding_auth_headers_keep_placeholder_auth_off_the_wire() {
    assert_eq!(
        ai_binding_gateway_base_url("my-gateway", "openai"),
        "https://workers-binding.ai/ai-gateway/gateways/my-gateway/openai"
    );
    let server = MockServer::sequence(vec![Response::json(400, json!({ "error": { "type": "bad_request", "message": "stubbed" } }))]).await;
    let mut model = gateway_model();
    model.provider = "openai".into();
    model.base_url = format!("{}/openai", server.url);
    let context = normalize_context(&Context { system_prompt: None, tools: None, messages: vec![Message::User(UserMessage::new("hello", 1))] });
    let options = StreamOptions { headers: Some(ai_binding_auth_headers()), max_retries: Some(0), ..Default::default() };
    let result = openai_completions::stream(&model, &context, options).result().await;
    assert_eq!(result.stop_reason, StopReason::Error);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/openai/chat/completions");
    assert!(requests[0].header("authorization").is_none());
    assert!(requests[0].header("x-api-key").is_none());
    assert_eq!(requests[0].header("cf-aig-authorization"), Some(format!("Bearer {CLOUDFLARE_GATEWAY_BINDING_AUTH_SENTINEL}").as_str()));
}
