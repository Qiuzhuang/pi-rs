//! Cases of the Bedrock tests not covered by `tests/bedrock.rs`: `bedrock-custom-headers.test.ts`,
//! `bedrock-models.test.ts`, `bedrock-raw-stop-reason.test.ts`, `bedrock-error-metadata.test.ts`,
//! `bedrock-endpoint-resolution.test.ts` and `bedrock-thinking-payload.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_ai::api::aws_eventstream::{HeaderValue, encode_message};
use pi_ai::api::bedrock_converse_stream::{self, resolve_client_config};
use pi_ai::providers::all::{get_builtin_model, get_builtin_models};
use pi_ai::*;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::mock_http::{MockServer, Response};
use crate::support::*;

fn frame(event_type: &str, payload: &Value) -> Vec<u8> {
    encode_message(
        &[
            (":message-type", HeaderValue::String("event".into())),
            (":event-type", HeaderValue::String(event_type.into())),
            (":content-type", HeaderValue::String("application/json".into())),
        ],
        payload.to_string().as_bytes(),
    )
}

fn items(items: &[Value]) -> Vec<u8> {
    items
        .iter()
        .flat_map(|item| {
            let (name, payload) = item.as_object().unwrap().iter().next().unwrap();
            frame(name, payload)
        })
        .collect()
}

fn eventstream(body: Vec<u8>) -> Response {
    Response::new(200, "application/vnd.amazon.eventstream", body).with_header("x-amzn-requestid", "req-123")
}

fn env(pairs: &[(&str, &str)]) -> ProviderEnv {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn skip_auth() -> StreamOptions {
    StreamOptions {
        cache_retention: Some(CacheRetention::None),
        env: Some(env(&[("AWS_BEDROCK_SKIP_AUTH", "1"), ("AWS_REGION", "us-east-1")])),
        ..Default::default()
    }
}

fn opus_at(url: &str) -> Model {
    let mut model = get_builtin_model("amazon-bedrock", "us.anthropic.claude-opus-4-8").unwrap();
    model.base_url = url.to_string();
    model
}

fn hello() -> TranscriptContext {
    normalize_context(&user_ctx("hello"))
}

fn ok_stream() -> Response {
    eventstream(items(&[json!({"messageStart": {"role": "assistant"}}), json!({"messageStop": {"stopReason": "end_turn"}})]))
}

// ---------------------------------------------------------------------------
// Port of bedrock-custom-headers.test.ts (VC3, VC4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn custom_headers_are_optional_and_forwarded_by_stream_simple() {
    let server = MockServer::start(|_, _| ok_stream()).await;
    let model = opus_at(&server.url);
    bedrock_converse_stream::stream(&model, &hello(), skip_auth()).result().await;
    let options = StreamOptions { headers: Some(ProviderHeaders::new()), ..skip_auth() };
    bedrock_converse_stream::stream(&model, &hello(), options).result().await;
    let mut headers = ProviderHeaders::new();
    headers.insert("x-custom".into(), Some("v".into()));
    let options = StreamOptions { headers: Some(headers), ..skip_auth() };
    let message = bedrock_converse_stream::stream_simple(&model, &hello(), options).result().await;
    assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].header("x-custom"), None);
    assert_eq!(requests[1].header("x-custom"), None);
    assert_eq!(requests[2].header("x-custom"), Some("v"));
    assert_eq!(requests[0].header_count("content-type"), 1);
}

// ---------------------------------------------------------------------------
// Port of bedrock-models.test.ts (catalog cases)
// ---------------------------------------------------------------------------

#[test]
fn bedrock_catalog_exposes_opus_5_through_an_inference_profile_only() {
    let models = get_builtin_models("amazon-bedrock");
    assert!(!models.is_empty());
    assert!(models.iter().any(|m| m.id == "global.anthropic.claude-opus-5"));
    assert!(!models.iter().any(|m| m.id == "anthropic.claude-opus-5"));
}

// ---------------------------------------------------------------------------
// Port of bedrock-raw-stop-reason.test.ts (provider events, error items)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn forwards_stream_items_and_error_items_with_the_request_model() {
    let stream_items = vec![
        json!({"messageStart": {"role": "assistant"}}),
        json!({"contentBlockDelta": {"contentBlockIndex": 0, "delta": {"text": "hello"}}}),
        json!({"messageStop": {"stopReason": "end_turn", "additionalModelResponseFields": {"source": "test"}}}),
        json!({"metadata": {"usage": {"inputTokens": 1, "outputTokens": 1, "totalTokens": 2}}}),
    ];
    let body = items(&stream_items);
    let server = MockServer::start(move |_, _| eventstream(body.clone())).await;
    let model = opus_at(&server.url);
    let received = Arc::new(Mutex::new(Vec::<(Value, Model)>::new()));
    let sink = received.clone();
    let options = StreamOptions {
        on_provider_stream_event: Some(Arc::new(move |item: Value, event_model: Model| {
            let sink = sink.clone();
            Box::pin(async move {
                tokio::task::yield_now().await;
                sink.lock().push((item, event_model));
            })
        })),
        ..skip_auth()
    };
    let result = bedrock_converse_stream::stream(&model, &hello(), options).result().await;
    assert_eq!(result.stop_reason, StopReason::Stop, "{:?}", result.error_message);
    assert_eq!(serde_json::to_value(&result.content).unwrap(), json!([{"type": "text", "text": "hello"}]));
    let received = received.lock().clone();
    assert_eq!(received.iter().map(|(i, _)| i.clone()).collect::<Vec<_>>(), stream_items);
    assert!(received.iter().all(|(_, m)| *m == model));

    // An exception frame fails the stream after the items before it were forwarded. (The TS test
    // yields the exception as a stream item; the real SDK, which the Rust decoder follows,
    // throws exception frames instead of yielding them, so they never reach the observer.)
    let mut body = frame("messageStart", &json!({"role": "assistant"}));
    body.extend(encode_message(
        &[(":message-type", HeaderValue::String("exception".into())), (":exception-type", HeaderValue::String("internalServerException".into()))],
        br#"{"message":"bedrock stream failed"}"#,
    ));
    let server = MockServer::start(move |_, _| eventstream(body.clone())).await;
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sink = received.clone();
    let options = StreamOptions {
        on_provider_stream_event: Some(Arc::new(move |item: Value, _m: Model| {
            sink.lock().push(item);
            Box::pin(async {})
        })),
        ..skip_auth()
    };
    let result = bedrock_converse_stream::stream(&opus_at(&server.url), &hello(), options).result().await;
    assert_eq!(result.stop_reason, StopReason::Error);
    let received = received.lock().clone();
    assert_eq!(received, vec![json!({"messageStart": {"role": "assistant"}})]);
    assert_eq!(result.error_message.as_deref(), Some(r#"{"message":"bedrock stream failed"}"#));
}

// ---------------------------------------------------------------------------
// Port of bedrock-error-metadata.test.ts (transport failure after messageStart)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn transport_failures_mid_stream_report_only_the_request_id() {
    const REQUEST_ID: &str = "11111111-2222-3333-4444-555555555555";
    let start = frame("messageStart", &json!({"role": "assistant"}));
    let partial = frame("contentBlockDelta", &json!({"contentBlockIndex": 0, "delta": {"text": "hello"}}));
    // The second chunk is a truncated frame and the connection then closes mid-body.
    let server = MockServer::start(move |_, _| {
        let mut response = Response::chunked(200, "application/vnd.amazon.eventstream", vec![start.clone(), partial[..partial.len() / 2].to_vec()]);
        response.headers.push(("x-amzn-requestid".into(), REQUEST_ID.into()));
        response
    })
    .await;
    let result = bedrock_converse_stream::stream(&opus_at(&server.url), &hello(), skip_auth()).result().await;
    assert_eq!(result.stop_reason, StopReason::Error);
    let diagnostic = result
        .diagnostics
        .as_ref()
        .and_then(|d| d.iter().find(|d| d.diagnostic_type == "bedrock_response_failure"))
        .map(|d| serde_json::to_value(d).unwrap());
    assert_eq!(diagnostic.map(|d| d["details"].clone()), Some(json!({"requestId": REQUEST_ID})), "{:?}", result.error_message);
}

// ---------------------------------------------------------------------------
// Port of bedrock-endpoint-resolution.test.ts (ambient profiles, compat dispatch)
// ---------------------------------------------------------------------------

fn clear_aws_env() -> Vec<(&'static str, Option<&'static str>)> {
    [
        "AWS_REGION", "AWS_DEFAULT_REGION", "AWS_PROFILE", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN",
        "AWS_BEARER_TOKEN_BEDROCK", "AWS_CONFIG_FILE", "AWS_SHARED_CREDENTIALS_FILE", "AWS_BEDROCK_SKIP_AUTH",
        "AWS_WEB_IDENTITY_TOKEN_FILE", "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI", "AWS_CONTAINER_CREDENTIALS_FULL_URI",
    ]
    .into_iter()
    .map(|k| (k, None))
    .collect()
}

#[tokio::test]
async fn ambient_profiles_leave_region_and_endpoint_to_the_sdk() {
    let _lock = ENV_LOCK.lock().await;
    let mut vars = clear_aws_env();
    vars.push(("AWS_PROFILE", Some("ambient-bedrock-profile")));
    let _guard = EnvGuard::set(&vars);
    let model = get_builtin_model("amazon-bedrock", "eu.anthropic.claude-sonnet-4-5-20250929-v1:0").unwrap();
    let config = resolve_client_config(&model, &StreamOptions::default());
    assert_eq!(config.profile.as_deref(), Some("ambient-bedrock-profile"));
    assert_eq!(config.endpoint, None);
    assert_eq!(config.region, None);
}

#[tokio::test]
async fn compat_dispatch_keeps_ambient_profile_auth_for_custom_model_ids() {
    let _lock = ENV_LOCK.lock().await;
    let dir = std::env::temp_dir().join(format!("pi-ai-ts-bedrock-profile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_file = dir.join("config");
    let credentials_file = dir.join("credentials");
    std::fs::write(&config_file, "[profile bedrock-profile]\nregion = us-east-1\n").unwrap();
    std::fs::write(&credentials_file, "[bedrock-profile]\naws_access_key_id = AKIDPROFILE\naws_secret_access_key = profile-secret\n").unwrap();
    let config_path = config_file.to_string_lossy().into_owned();
    let credentials_path = credentials_file.to_string_lossy().into_owned();
    let mut vars = clear_aws_env();
    vars.push(("AWS_PROFILE", Some("bedrock-profile")));
    vars.push(("AWS_CONFIG_FILE", Some(&config_path)));
    vars.push(("AWS_SHARED_CREDENTIALS_FILE", Some(&credentials_path)));
    let _guard = EnvGuard::set(&vars);

    let server = MockServer::start(|_, _| ok_stream()).await;
    let mut model = opus_at(&server.url);
    model.id = "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/example".into();
    let config = resolve_client_config(&model, &StreamOptions::default());
    assert_eq!(config.profile.as_deref(), Some("bedrock-profile"));
    assert!(config.bearer_token.is_none());

    let context = user_ctx("hello");
    let message = compat::stream(&model, &context, StreamOptions { cache_retention: Some(CacheRetention::None), ..Default::default() }).result().await;
    assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
    let authorization = server.request(0).header("authorization").unwrap().to_string();
    assert!(authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDPROFILE/"), "{authorization}");
}

// ---------------------------------------------------------------------------
// Port of bedrock-thinking-payload.test.ts (non-adaptive Claude via model.name)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn application_profiles_fall_back_to_budget_thinking_for_non_adaptive_claude() {
    let mut model = get_builtin_model("amazon-bedrock", "us.anthropic.claude-sonnet-4-5-20250929-v1:0").unwrap();
    model.id = "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/my-profile".into();
    model.name = "Claude Sonnet 4.5".into();
    let signal = CancellationToken::new();
    signal.cancel();
    let options = StreamOptions { reasoning: Some(ThinkingLevel::High), signal: Some(signal), cache_retention: Some(CacheRetention::None), ..Default::default() };
    let (payload, _) = capture_payload_with(|o| bedrock_converse_stream::stream(&model, &hello(), o), options).await;
    let fields = &payload["additionalModelRequestFields"];
    assert_eq!(fields["thinking"]["type"], "enabled");
    assert!(fields["thinking"]["budget_tokens"].is_number());
    assert_eq!(fields["anthropic_beta"], json!(["interleaved-thinking-2025-05-14"]));
}
