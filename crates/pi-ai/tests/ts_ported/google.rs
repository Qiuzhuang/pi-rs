//! Cases of the Google tests not covered by `tests/google.rs`: `google-shared-retry.test.ts`,
//! `google-shared-convert-tools.test.ts`, `google-shared-gemini3-unsigned-tool-call.test.ts` and
//! `google-vertex-api-key-resolution.test.ts`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pi_ai::api::google_shared::{convert_messages, convert_tools, google_api_error, retry_google_request};
use pi_ai::api::google_vertex;
use pi_ai::providers::all::get_builtin_model;
use pi_ai::*;
use serde_json::{Value, json};

use crate::support::*;

// ---------------------------------------------------------------------------
// Port of google-shared-retry.test.ts
// ---------------------------------------------------------------------------

async fn retry_with(statuses: Vec<u16>, max_retries: Option<u32>) -> (anyhow::Result<&'static str>, usize) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let statuses = Arc::new(statuses);
    let options = StreamOptions { max_retries, max_retry_delay_ms: Some(60_000), ..Default::default() };
    let result = retry_google_request(
        move || {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            let statuses = statuses.clone();
            async move {
                match statuses.get(attempt) {
                    Some(status) => Err(anyhow::Error::new(google_api_error(*status, format!("got status: {status}")))),
                    None => Ok("ok"),
                }
            }
        },
        &options,
    )
    .await;
    (result, calls.load(Ordering::SeqCst))
}

#[tokio::test(start_paused = true)]
async fn google_requests_retry_only_retryable_statuses_when_enabled() {
    let (result, calls) = retry_with(vec![429], Some(1)).await;
    assert_eq!(result.unwrap(), "ok");
    assert_eq!(calls, 2);

    let (result, calls) = retry_with(vec![429, 429], None).await;
    assert!(result.unwrap_err().to_string().contains("got status: 429"));
    assert_eq!(calls, 1);

    let (result, calls) = retry_with(vec![400, 400], Some(2)).await;
    assert!(result.unwrap_err().to_string().contains("got status: 400"));
    assert_eq!(calls, 1);
}

// ---------------------------------------------------------------------------
// Port of google-shared-convert-tools.test.ts (schemas without $schema)
// ---------------------------------------------------------------------------

#[test]
fn convert_tools_handles_schemas_without_meta_keys() {
    let parameters = json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]});
    let tools = vec![Tool::new("test_tool", "A test tool", parameters.clone())];
    let result = convert_tools(&tools, true, true).unwrap().unwrap();
    assert_eq!(result[0]["functionDeclarations"][0]["parameters"], parameters);
    // $ref survives while the other meta keys are stripped; the original schema is untouched.
    let with_ref = json!({"$schema": "x", "type": "object", "properties": {"a": {"$ref": "#/$defs/a"}}, "$defs": {"a": {"type": "string"}}});
    let tools = vec![Tool::new("test_tool", "A test tool", with_ref.clone())];
    let result = convert_tools(&tools, true, true).unwrap().unwrap();
    assert_eq!(result[0]["functionDeclarations"][0]["parameters"], json!({"type": "object", "properties": {"a": {"$ref": "#/$defs/a"}}}));
    assert_eq!(tools[0].parameters, with_ref);
}

// ---------------------------------------------------------------------------
// Port of google-shared-gemini3-unsigned-tool-call.test.ts (non-Gemini-3 signatures)
// ---------------------------------------------------------------------------

#[test]
fn non_gemini3_models_get_no_thought_signature_or_ids() {
    let model = model(json!({"id": "gemini-2.5-flash", "name": "g", "api": "google-generative-ai", "provider": "google",
        "baseUrl": "https://example.com", "reasoning": true, "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 128000, "maxTokens": 8192}));
    let context = normalize_context(&ctx(json!({"messages": [
        {"role": "user", "content": "Hi"},
        {"role": "assistant", "api": "google-generative-ai", "provider": "google", "model": "other-model", "stopReason": "toolUse", "content": [
            {"type": "toolCall", "id": "call_1", "name": "bash", "arguments": {"command": "echo hi"}},
            {"type": "toolCall", "id": "call_2", "name": "bash", "arguments": {"command": "ls -la"}}
        ]},
        {"role": "toolResult", "toolCallId": "call_1", "toolName": "bash", "isError": false, "content": [{"type": "text", "text": "hi"}]},
        {"role": "toolResult", "toolCallId": "call_2", "toolName": "bash", "isError": false, "content": [{"type": "text", "text": "files"}]}
    ]})));
    let contents = convert_messages(&model, &context);
    let turn = contents.iter().find(|c| c["role"] == "model").unwrap();
    let calls: Vec<&Value> = turn["parts"].as_array().unwrap().iter().filter(|p| p.get("functionCall").is_some()).collect();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|p| p.get("thoughtSignature").is_none() && p["functionCall"].get("id").is_none()));
}

// ---------------------------------------------------------------------------
// Port of google-vertex-api-key-resolution.test.ts (GOOGLE_CLOUD_API_KEY placeholders)
// ---------------------------------------------------------------------------

#[test]
fn vertex_falls_back_to_adc_for_placeholder_env_api_keys() {
    let model = get_builtin_model("google-vertex", "gemini-3-flash-preview").unwrap();
    let mut options = StreamOptions::default();
    options.extra.insert("project".into(), json!("test-project"));
    options.extra.insert("location".into(), json!("us-central1"));
    options.env = Some([("GOOGLE_CLOUD_API_KEY".to_string(), "<authenticated>".to_string())].into_iter().collect());
    let client = google_vertex::client_options(&model, &options).unwrap();
    assert!(client.vertexai);
    assert!(client.api_key.is_none());
    assert_eq!(client.project.as_deref(), Some("test-project"));
    assert_eq!(client.location.as_deref(), Some("us-central1"));
    assert_eq!(client.api_version.as_deref(), Some("v1"));
    assert!(client.base_url.is_none());

    // Like TS, the stream only reads `options.apiKey`; env keys arrive through provider auth.
    options.api_key = Some("AIzaSyExampleRealisticLookingApiKey123456".into());
    let client = google_vertex::client_options(&model, &options).unwrap();
    assert_eq!(client.api_key.as_deref(), Some("AIzaSyExampleRealisticLookingApiKey123456"));
    assert!(client.project.is_none());
}
