//! Offline tests for image generation (ports of `openrouter-images.test.ts` and the registry
//! dispatch of `images.ts`).

mod mock_http;

use mock_http::{MockServer, Response};
use pi_ai::images::generate_images;
use pi_ai::images_api_registry::{ImagesApiProvider, get_images_api_provider, register_images_api_provider};
use pi_ai::*;
use serde_json::{Value, json};

fn image_model(base_url: &str, id: &str, output: &[&str]) -> ImageModel {
    serde_json::from_value(json!({
        "type": "image", "id": id, "name": id, "api": "openrouter-images", "provider": "openrouter", "baseUrl": base_url,
        "input": ["text", "image"], "output": output, "cost": { "input": 0.015, "output": 0.03, "cacheRead": 0, "cacheWrite": 0 },
        "headers": { "HTTP-Referer": "https://example.com" }
    }))
    .unwrap()
}

fn dog() -> ImagesContext {
    ImagesContext { input: vec![UserContentBlock::text("Generate a dog")] }
}

fn response_body() -> Value {
    json!({
        "id": "img-1",
        "usage": { "prompt_tokens": 12, "completion_tokens": 34, "prompt_tokens_details": { "cached_tokens": 0 } },
        "choices": [{ "message": { "content": "Here is your image.", "images": [{ "image_url": "data:image/png;base64,ZmFrZS1wbmc=" }, { "image_url": { "url": "data:image/jpeg;base64,anBn" } }, { "image_url": "https://example.com/not-inline.png" }] } }]
    })
}

#[tokio::test]
async fn returns_text_plus_images() {
    let server = MockServer::start(|_, _| Response::json(200, response_body())).await;
    let model = image_model(&server.url, "google/gemini-3.1-flash-image-preview", &["text", "image"]);
    let output = generate_images(&model, &dog(), StreamOptions { api_key: Some("test".into()), ..Default::default() }).await.unwrap();
    assert_eq!(output.stop_reason, ImagesStopReason::Stop, "{:?}", output.error_message);
    assert_eq!(output.response_id.as_deref(), Some("img-1"));
    assert_eq!(output.output[0], UserContentBlock::text("Here is your image."));
    assert_eq!(output.output[1], UserContentBlock::Image(ImageContent { mime_type: "image/png".into(), data: "ZmFrZS1wbmc=".into() }));
    assert_eq!(output.output[2], UserContentBlock::Image(ImageContent { mime_type: "image/jpeg".into(), data: "anBn".into() }));
    assert_eq!(output.output.len(), 3);
    let usage = output.usage.unwrap();
    assert_eq!((usage.input, usage.output, usage.total_tokens), (12, 34, 46));

    let request = server.request(0);
    assert_eq!(request.path, "/chat/completions");
    assert_eq!(request.header("authorization"), Some("Bearer test"));
    assert_eq!(request.header("http-referer"), Some("https://example.com"));
    let params = request.json();
    assert_eq!(params["stream"], false);
    assert_eq!(params["modalities"], json!(["image", "text"]));
    assert_eq!(params["messages"][0]["content"][0], json!({ "type": "text", "text": "Generate a dog" }));

    let image_only = image_model(&server.url, "black-forest-labs/flux.2-pro", &["image"]);
    let output = generate_images(&image_only, &dog(), StreamOptions { api_key: Some("test".into()), ..Default::default() }).await.unwrap();
    assert!(output.output.iter().any(|o| matches!(o, UserContentBlock::Image(_))));
    assert_eq!(server.request(1).json()["modalities"], json!(["image"]));
}

#[tokio::test]
async fn aborted_and_failed_requests_become_results() {
    let server = MockServer::start(|_, _| Response::json(400, json!({ "error": { "message": "Bad prompt", "code": 400 } }))).await;
    let model = image_model(&server.url, "black-forest-labs/flux.2-pro", &["image"]);
    let signal = tokio_util::sync::CancellationToken::new();
    signal.cancel();
    let output = generate_images(&model, &dog(), StreamOptions { api_key: Some("test".into()), signal: Some(signal), ..Default::default() }).await.unwrap();
    assert_eq!(output.stop_reason, ImagesStopReason::Aborted);
    assert_eq!(output.error_message.as_deref(), Some("Request aborted"));

    let output = generate_images(&model, &dog(), StreamOptions { api_key: Some("test".into()), ..Default::default() }).await.unwrap();
    assert_eq!(output.stop_reason, ImagesStopReason::Error);
    assert_eq!(output.error_message.as_deref(), Some(r#"400: {"message":"Bad prompt","code":400}"#));

    let output = generate_images(&model, &dog(), StreamOptions::default()).await.unwrap();
    assert_eq!(output.error_message.as_deref(), Some("No API key for provider: openrouter"));
}

#[tokio::test]
async fn registry_dispatch() {
    let mut model = image_model("http://127.0.0.1:9", "m", &["image"]);
    model.api = "unknown-images".into();
    let error = generate_images(&model, &dog(), StreamOptions::default()).await.unwrap_err();
    assert_eq!(error.to_string(), "No API provider registered for api: unknown-images");

    let builtin = get_images_api_provider("openrouter-images").unwrap();
    register_images_api_provider(ImagesApiProvider { api: "alias-images".into(), implementation: builtin.implementation }, Some("test"));
    model.api = "alias-images".into();
    assert!(get_images_api_provider("alias-images").is_some());
    let result = generate_images(&model, &dog(), StreamOptions::default()).await.unwrap();
    assert_eq!(result.error_message.as_deref(), Some("No API key for provider: openrouter"));
    pi_ai::images_api_registry::unregister_images_api_providers("test");
    assert!(get_images_api_provider("alias-images").is_none());
}
