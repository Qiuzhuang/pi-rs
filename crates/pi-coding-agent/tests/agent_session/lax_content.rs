//! Port of `test/suite/lax-message-content.test.ts` and `test/suite/agent-session-tool-result-images.test.ts`.
//!
//! Rust tools and messages are typed, so "untyped" input only arrives as JSON: from the JS bridge
//! (`ExtensionEventResult::Json`) and from session files. Those boundaries are exercised here.
//! The tool-result-images suite mocks `normalizeToolResultImages`; Rust cannot mock a module, so
//! the port checks the observable effect of `images.autoResize: false` instead.

use std::sync::Arc;

use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool};
use pi_coding_agent::core::extensions::types::ExtensionEventResult;
use pi_coding_agent::core::session_manager::{SessionEntry, session_entry_to_context_messages};
use serde_json::{Value, json};

use crate::common::*;
use crate::support::*;

fn message_entry(message: Value) -> SessionEntry {
    serde_json::from_value(json!({
        "type": "message",
        "id": "entry-1",
        "parentId": null,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "message": message,
    }))
    .expect("lenient message entry")
}

#[tokio::test]
async fn normalizes_null_content_in_message_end_extension_replacements() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            pi.on(
                "message_end",
                Arc::new(|event, _ctx| {
                    Box::pin(async move {
                        let value = serde_json::to_value(&event)?;
                        if value["message"]["role"] != json!("assistant") {
                            return Ok(None);
                        }
                        // An untyped JS extension replacing the message without content.
                        let mut message = value["message"].clone();
                        message["content"] = Value::Null;
                        Ok(Some(ExtensionEventResult::Json(json!({"message": message}))))
                    })
                }),
            )?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("hello")]);

    harness.session.prompt("hi", None).await.unwrap();

    let assistants: Vec<_> = harness
        .session
        .messages()
        .into_iter()
        .filter_map(|m| match m {
            AgentMessage::Assistant(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(assistants.len(), 1);
    assert!(assistants[0].content.is_empty());
    harness.cleanup();
}

#[test]
fn normalizes_null_or_missing_content_when_loading_session_message_entries() {
    let now = chrono::Utc::now().timestamp_millis();
    let bad_messages = [
        json!({"role": "user", "content": null, "timestamp": now}),
        json!({
            "role": "assistant", "content": null, "api": "openai-completions", "provider": "openai",
            "model": "test-model",
            "usage": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
                "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}},
            "stopReason": "stop", "timestamp": now
        }),
        json!({"role": "toolResult", "toolCallId": "call_1", "toolName": "web_search", "isError": false, "timestamp": now}),
    ];
    for bad in bad_messages {
        let role = bad["role"].clone();
        let messages = session_entry_to_context_messages(&message_entry(bad));
        let value = messages[0].to_value();
        assert_eq!(value["role"], role);
        assert_eq!(value["content"], json!([]), "{value}");
    }
}

#[test]
fn normalizes_null_content_when_loading_custom_message_entries() {
    let entry: SessionEntry = serde_json::from_value(json!({
        "type": "custom_message",
        "id": "entry-1",
        "parentId": null,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "customType": "test",
        "content": null,
        "display": false,
    }))
    .unwrap();
    let value = session_entry_to_context_messages(&entry)[0].to_value();
    assert_eq!(value["role"], json!("custom"));
    assert_eq!(value["content"], json!([]));
}

#[test]
fn keeps_valid_message_content_untouched_when_loading_session_entries() {
    let entry = message_entry(json!({"role": "user", "content": "hello", "timestamp": 1}));
    let value = session_entry_to_context_messages(&entry)[0].to_value();
    assert_eq!(value["role"], json!("user"));
    assert_eq!(value["content"], json!("hello"));
}

const TINY_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

#[tokio::test]
async fn passes_tool_result_images_through_when_auto_resize_is_disabled() {
    let screenshot = FnAgentTool::new(
        "screenshot",
        "Screenshot",
        "Return a screenshot",
        json!({"type": "object", "properties": {}}),
        |_id, _p, _s, _u| async {
            let mut result = AgentToolResult::text("captured", Some(json!({})));
            result.content.push(
                serde_json::from_value(json!({"type": "image", "data": TINY_PNG_BASE64, "mimeType": "image/png"}))
                    .unwrap(),
            );
            Ok(result)
        },
    )
    .into_ref();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![screenshot]),
        settings: Some(json!({"images": {"autoResize": false}})),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![tool_use("screenshot", json!({})), text("done")]);

    harness.session.prompt("take a screenshot", None).await.unwrap();

    let result = harness.tool_result("screenshot");
    let content = serde_json::to_value(&result.content).unwrap();
    assert_eq!(content[1]["data"], json!(TINY_PNG_BASE64));
    assert_eq!(content[1]["mimeType"], json!("image/png"));
    harness.cleanup();
}
