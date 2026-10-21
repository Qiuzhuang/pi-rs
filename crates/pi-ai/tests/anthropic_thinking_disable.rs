//! Port of `anthropic-thinking-disable.test.ts` (payload-capture cases only).

use std::sync::Arc;

use parking_lot::Mutex;
use pi_ai::providers::all::get_builtin_model;
use pi_ai::*;
use serde_json::{Value, json};

async fn capture_payload(id: &str, reasoning: Option<ThinkingLevel>) -> Value {
    let mut model = get_builtin_model("anthropic", id).unwrap();
    model.base_url = "http://127.0.0.1:9".into();
    let captured = Arc::new(Mutex::new(None::<Value>));
    let sink = captured.clone();
    let options = StreamOptions {
        api_key: Some("fake-key".into()),
        reasoning,
        on_payload: Some(Arc::new(move |payload: Value, _model: Model| {
            *sink.lock() = Some(payload);
            Box::pin(async { None })
        })),
        ..Default::default()
    };
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage::new("Hello", 1))],
        tools: None,
    };
    let _ = compat::stream_simple(&model, &context, options).result().await;
    captured.lock().take().expect("payload captured before request failure")
}

#[tokio::test]
async fn disables_thinking_for_budget_and_adaptive_models_when_off() {
    for id in ["claude-sonnet-4-5", "claude-opus-4-6", "claude-opus-4-8"] {
        let payload = capture_payload(id, None).await;
        assert_eq!(payload["thinking"], json!({"type": "disabled"}), "{id}");
        assert!(payload.get("output_config").is_none(), "{id}");
    }
}

#[tokio::test]
async fn omits_disabled_thinking_for_fable_5() {
    let payload = capture_payload("claude-fable-5", None).await;
    assert!(payload.get("thinking").is_none());
    assert!(payload.get("output_config").is_none());
}

#[tokio::test]
async fn uses_adaptive_thinking_when_reasoning_enabled() {
    for (id, level, effort) in [
        ("claude-opus-4-8", ThinkingLevel::High, "high"),
        ("claude-sonnet-5", ThinkingLevel::High, "high"),
        ("claude-opus-4-8", ThinkingLevel::Xhigh, "xhigh"),
    ] {
        let payload = capture_payload(id, Some(level)).await;
        assert_eq!(payload["thinking"], json!({"type": "adaptive", "display": "summarized"}), "{id}");
        assert_eq!(payload["output_config"], json!({"effort": effort}), "{id}");
    }
}
