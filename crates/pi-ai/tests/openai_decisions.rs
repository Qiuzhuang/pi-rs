//! Offline tests for OpenAI Decisions (port of `openai-decisions.test.ts`).

mod responses_mock;

use pi_ai::api::openai_decisions::classify;
use pi_ai::*;
use responses_mock::*;
use serde_json::{Value, json};

fn model(base_url: &str) -> ClassifierModel {
    serde_json::from_value(json!({
        "type": "classifier", "id": "gpt-6-luna", "name": "GPT-6 Luna", "api": "openai-decisions", "provider": "openai",
        "baseUrl": base_url, "input": ["text", "image"],
        "cost": { "input": 0.1, "output": 0, "cacheRead": 0, "cacheWrite": 0,
                  "tiers": [{ "inputTokensAbove": 272000, "input": 0.2, "output": 0, "cacheRead": 0, "cacheWrite": 0 }] },
        "contextWindow": 922000
    }))
    .unwrap()
}

fn context() -> ClassifierContext {
    serde_json::from_value(json!({
        "state": { "text": "The deployment succeeded, thank you." },
        "questions": {
            "category": { "type": "choice", "instructions": "Classify the message", "criteria": { "success": "Successful", "failure": "" } },
            "satisfaction": { "type": "score", "instructions": "Score satisfaction", "criteria": ["low", "neutral", "high"] },
            "approved": { "type": "bool", "instructions": "Does the user approve?", "criteria": { "true": "Approval", "false": "No approval" } }
        }
    }))
    .unwrap()
}

fn wire_answers() -> Vec<Value> {
    vec![
        json!({ "type": "choice", "name": "category", "choice": "success", "probabilities": [{ "value": "success", "probability": 0.9 }, { "value": "failure", "probability": 0.1 }], "confidence": 0.8 }),
        json!({ "type": "score", "name": "satisfaction", "score": 1.8, "probabilities": [{ "value": 0, "label": "low", "probability": 0.05 }], "confidence": 0.7 }),
        json!({ "type": "predicate", "name": "approved", "probability": 0.95 }),
    ]
}

fn wire_usage() -> Value {
    json!({ "input_tokens": 164, "input_tokens_details": { "cached_tokens": 0, "cache_write_tokens": 0 }, "output_tokens": 0, "output_tokens_details": { "reasoning_tokens": 0 }, "total_tokens": 164 })
}

fn key() -> StreamOptions {
    StreamOptions { api_key: Some("secret".into()), ..Default::default() }
}

#[tokio::test]
async fn maps_questions_and_answers_by_name() {
    let mut reversed = wire_answers();
    reversed.reverse();
    let server = serve_sequence(vec![Reply::json(200, json!({ "model": "gpt-6-luna", "answers": reversed, "usage": wire_usage() }).to_string())]).await;
    let mut options = key();
    options.temperature = Some(1.5);
    let result = classify(&model(&format!("{}/v1", server.base_url)), &context(), options).await;
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/decisions");
    assert_eq!(requests[0].header("authorization"), Some("Bearer secret"));
    assert_eq!(
        requests[0].json(),
        json!({
            "model": "gpt-6-luna",
            "input": Value::Object(context().state).to_string(),
            "questions": [
                { "type": "choice", "name": "category", "instructions": "Classify the message", "choices": [{ "value": "success", "description": "Successful" }, { "value": "failure" }] },
                { "type": "score", "name": "satisfaction", "instructions": "Score satisfaction", "levels": [{ "label": "low" }, { "label": "neutral" }, { "label": "high" }] },
                { "type": "predicate", "name": "approved", "instructions": "Does the user approve?\n\nTrue means: Approval\nFalse means: No approval" }
            ]
        })
    );
    assert_eq!(result.stop_reason, ClassifierStopReason::Stop, "{:?}", result.error_message);
    assert_eq!(
        serde_json::to_value(&result.answers).unwrap(),
        json!({
            "category": { "type": "choice", "choice": "success", "probabilities": { "success": 0.9, "failure": 0.1 }, "confidence": 0.8 },
            "satisfaction": { "type": "score", "score": 1.8, "confidence": 0.7 },
            "approved": { "type": "bool", "probability": 0.95 }
        })
    );
    let usage = result.usage.unwrap();
    assert_eq!((usage.input, usage.output, usage.cache_read, usage.total_tokens), (164, 0, 0, 164));
    assert!((usage.cost.total - 0.0000164).abs() < 1e-12);
}

#[tokio::test]
async fn long_context_pricing_and_images() {
    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": wire_answers(), "usage": { "input_tokens": 300000, "output_tokens": 0 } }).to_string())]).await;
    let result = classify(&model(&server.base_url), &context(), key()).await;
    assert!((result.usage.unwrap().cost.total - 0.06).abs() < 1e-12);

    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": wire_answers() }).to_string())]).await;
    let mut with_images = context();
    let image = ImageContent { data: "aW1hZ2U=".into(), mime_type: "image/png".into() };
    with_images.images = Some(vec![image.clone(), ImageContent { mime_type: "image/jpeg".into(), ..image.clone() }]);
    let result = classify(&model(&server.base_url), &with_images, key()).await;
    assert_eq!(result.stop_reason, ClassifierStopReason::Stop);
    assert_eq!(
        server.requests()[0].json()["input"],
        json!([{ "role": "user", "content": [
            { "type": "input_text", "text": Value::Object(context().state).to_string() },
            { "type": "input_image", "image_url": "data:image/png;base64,aW1hZ2U=" },
            { "type": "input_image", "image_url": "data:image/jpeg;base64,aW1hZ2U=" }
        ] }])
    );

    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": wire_answers() }).to_string())]).await;
    let mut too_many = context();
    too_many.images = Some(vec![image; 129]);
    let result = classify(&model(&server.base_url), &too_many, key()).await;
    assert!(server.requests().is_empty());
    assert_eq!(result.stop_reason, ClassifierStopReason::Error);
    assert!(result.error_message.unwrap().contains("at most 128 images, got 129"));
}

#[tokio::test]
async fn refused_missing_and_mistyped_answers() {
    let mut answers = wire_answers();
    answers[2] = json!({ "type": "refusal", "name": "approved" });
    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": answers, "usage": wire_usage() }).to_string())]).await;
    let result = classify(&model(&server.base_url), &context(), key()).await;
    assert_eq!(result.stop_reason, ClassifierStopReason::Error);
    assert!(result.answers.is_empty());
    assert_eq!(result.error_message.as_deref(), Some("OpenAI Decisions refused to answer approved"));
    assert_eq!(result.usage.unwrap().input, 164);

    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": wire_answers()[..2] }).to_string())]).await;
    let missing = classify(&model(&server.base_url), &context(), key()).await;
    assert!(missing.error_message.unwrap().contains("did not return an answer for approved"));

    let mut answers = wire_answers();
    answers[2] = json!({ "type": "score", "name": "approved" });
    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": answers }).to_string())]).await;
    let mistyped = classify(&model(&server.base_url), &context(), key()).await;
    assert!(mistyped.error_message.unwrap().contains("did not return a predicate answer for approved"));

    let proto: ClassifierContext = serde_json::from_value(json!({
        "state": {},
        "questions": { "__proto__": { "type": "bool", "instructions": "Is this true?", "criteria": { "true": "Yes", "false": "No" } } }
    }))
    .unwrap();
    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": [{ "type": "predicate", "name": "__proto__", "probability": 0.75 }] }).to_string())]).await;
    let result = classify(&model(&server.base_url), &proto, key()).await;
    assert_eq!(result.stop_reason, ClassifierStopReason::Stop);
    assert_eq!(serde_json::to_value(&result.answers).unwrap(), json!({ "__proto__": { "type": "bool", "probability": 0.75 } }));
}

#[tokio::test]
async fn gateway_timeouts_retries_and_http_errors() {
    let server = serve_sequence(vec![Reply::text(504, "<!DOCTYPE html><html>Gateway time-out</html>").header("retry-after-ms", "0")]).await;
    let result = classify(&model(&server.base_url), &context(), key()).await;
    assert_eq!(server.requests().len(), 1);
    let message = result.error_message.unwrap();
    assert!(message.contains("OpenAI Decisions error (504): the request timed out at the gateway"));
    assert!(!message.contains("<html>"));

    let server = serve_sequence(vec![
        Reply::text(503, "busy").header("retry-after-ms", "0"),
        Reply::json(200, json!({ "answers": wire_answers() }).to_string()),
    ])
    .await;
    let result = classify(&model(&server.base_url), &context(), key()).await;
    assert_eq!(server.requests().len(), 2);
    assert_eq!(result.stop_reason, ClassifierStopReason::Stop);

    let server = serve_sequence(vec![Reply::json(400, json!({ "error": { "message": "Decision input exceeds the token limit.", "type": "invalid_request_error" } }).to_string())]).await;
    let mut options = key();
    options.max_retries = Some(0);
    let result = classify(&model(&server.base_url), &context(), options).await;
    let message = result.error_message.unwrap();
    assert!(message.contains("OpenAI Decisions error (400)"), "{message}");
    assert!(message.contains("Decision input exceeds the token limit."));
}

#[tokio::test]
async fn rejects_other_apis_and_missing_keys() {
    let server = serve_sequence(vec![Reply::json(200, json!({ "answers": wire_answers() }).to_string())]).await;
    let mut other = model(&server.base_url);
    other.api = "typesafe-system-one".into();
    let result = classify(&other, &context(), key()).await;
    assert!(result.error_message.unwrap().contains("Unsupported classifier API: typesafe-system-one"));
    let result = classify(&model(&server.base_url), &context(), StreamOptions::default()).await;
    assert!(result.error_message.unwrap().contains("No API key for provider: openai"));
    assert!(server.requests().is_empty());
    assert!(pi_ai::api::registry::resolve_builtin_classifier_api("openai-decisions").is_some());
}
