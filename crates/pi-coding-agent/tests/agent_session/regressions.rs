//! Ports of the session regressions in `test/suite/regressions/` (one test per file unless noted).

use std::sync::Arc;

use futures::FutureExt;
use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, FnAgentTool};
use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session::AgentSessionNavigateTreeOptions;
use pi_coding_agent::core::extensions::types::{
    ExtensionAPIRef, ExtensionEventResult, MessageEndEvent, MessageEndEventResult, ToolCallEvent,
    ToolCallEventResult, on_event,
};
use pi_coding_agent::core::session_manager::SessionEntry;
use serde_json::{Value, json};

use crate::common::*;
use crate::retry_events::{error_response, retry_settings};
use crate::support::*;

async fn retries_once(error_message: &str, reply: &str) {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(3, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![error_response(error_message), text(reply)]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 2);
    let starts: Vec<Value> = harness
        .events_of_type("auto_retry_start")
        .iter()
        .map(|e| e["errorMessage"].clone())
        .collect();
    assert_eq!(starts, vec![json!(error_message)]);
    let ends: Vec<Value> = harness.events_of_type("auto_retry_end").iter().map(|e| e["success"].clone()).collect();
    assert_eq!(ends, vec![json!(true)]);
    assert!(harness.assistant_texts().contains(&reply.to_string()));
    harness.cleanup();
}

// #6904
#[tokio::test]
async fn retries_a_transient_dns_lookup_failure() {
    retries_once(
        "The pending stream has been canceled (caused by: getaddrinfo ENOTFOUND bedrock-runtime.us-east-1.amazonaws.com)",
        "recovered after DNS retry",
    )
    .await;
}

// #6019
#[tokio::test]
async fn retries_openai_explicit_retry_guidance() {
    retries_once(
        "An error occurred while processing your request. You can retry your request, or contact us through our help center at help.openai.com if the error persists. Please include the request ID req_******** in your message.",
        "recovered",
    )
    .await;
}

// #6019
#[tokio::test]
async fn retries_bedrock_explicit_retry_guidance() {
    retries_once(
        r#"{"message":"The system encountered an unexpected error during processing. Try your request again."}"#,
        "recovered",
    )
    .await;
}

// #3317
#[tokio::test]
async fn retries_transient_network_connection_lost_failures() {
    retries_once("Network connection lost.", "recovered after reconnect").await;
}

// tree-during-streaming
#[tokio::test]
async fn rejects_tree_navigation_during_an_active_response_without_changing_the_leaf() {
    let harness = Arc::new(create_harness(HarnessOptions::default()).await.unwrap());
    let target = harness.session_manager.append_message(user_msg("first")).unwrap();
    let observed = Arc::new(Mutex::new(None::<(Option<String>, bool)>));
    let (o, weak, t) = (observed.clone(), Arc::downgrade(&harness), target.clone());
    harness.set_responses(vec![respond_with(move |_| {
        let harness = weak.upgrade().unwrap();
        let active_leaf = harness.session_manager.get_leaf_id();
        // Navigate from inside the response, while the run is active.
        let result = harness
            .session
            .navigate_tree(
                &t,
                Some(&AgentSessionNavigateTreeOptions {
                    summarize: Some(false),
                    ..Default::default()
                }),
            )
            .now_or_never()
            .expect("navigation answers immediately while streaming");
        let unchanged =
            active_leaf.as_deref() != Some(t.as_str()) && harness.session_manager.get_leaf_id() == active_leaf;
        *o.lock() = Some((result.err().map(|e| e.to_string()), unchanged));
        faux_text_message("response")
    })]);

    harness.session.prompt("second", None).await.unwrap();

    let (error, unchanged) = observed.lock().clone().unwrap();
    assert_eq!(
        error.as_deref(),
        Some("Wait for the current response to finish before navigating the session tree.")
    );
    assert!(unchanged);
    harness.cleanup();
}

// #3688
#[tokio::test]
async fn clears_branch_summary_state_when_session_before_tree_cancels_navigation() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            pi.on(
                "session_before_tree",
                Arc::new(|_event, _ctx| {
                    Box::pin(async { Ok(Some(ExtensionEventResult::Json(json!({"cancel": true})))) })
                }),
            )?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let sm = &harness.session_manager;
    let target = sm.append_message(user_msg("first")).unwrap();
    sm.append_message(assistant_msg("reply")).unwrap();
    let current = sm.append_message(user_msg("second")).unwrap();
    assert_eq!(sm.get_leaf_id().as_deref(), Some(current.as_str()));

    let result = harness
        .session
        .navigate_tree(
            &target,
            Some(&AgentSessionNavigateTreeOptions {
                summarize: Some(false),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    assert_eq!(serde_json::to_value(&result).unwrap(), json!({"cancelled": true}));
    assert!(!harness.session.is_compacting());
    assert_eq!(sm.get_leaf_id().as_deref(), Some(current.as_str()));
    harness.cleanup();
}

// #5996
#[tokio::test]
async fn filters_newlines_from_session_names() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.session.set_session_name("hello\nworld\r\nagain");
    assert_eq!(harness.session_manager.get_session_name().as_deref(), Some("hello world again"));
    let names: Vec<Value> = harness.events_of_type("session_info_changed").iter().map(|e| e["name"].clone()).collect();
    assert_eq!(names, vec![json!("hello world again")]);
    harness.cleanup();

    let api = Arc::new(Mutex::new(None::<ExtensionAPIRef>));
    let a = api.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            *a.lock() = Some(pi);
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    api.lock().clone().unwrap().set_session_name("from\nextension").unwrap();
    assert_eq!(harness.session_manager.get_session_name().as_deref(), Some("from extension"));
    let names: Vec<Value> = harness.events_of_type("session_info_changed").iter().map(|e| e["name"].clone()).collect();
    assert_eq!(names, vec![json!("from extension")]);
    harness.cleanup();
}

// #7048
#[tokio::test]
async fn does_not_persist_a_length_limited_compaction_summary() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    seed_compactable_session(&harness);
    harness.set_responses(vec![
        message(vec![faux_text("partial summar")], Some(StopReason::Length)).into(),
    ]);

    let error = harness.session.compact(None).await.unwrap_err();
    assert!(error.to_string().contains("generation hit the token cap"), "{error}");
    assert!(
        !harness
            .session_manager
            .get_entries()
            .iter()
            .any(|e| matches!(e, SessionEntry::Compaction(_)))
    );
    harness.cleanup();
}

// #5998
#[tokio::test]
async fn lets_a_tool_call_handler_terminate_the_run_after_blocking_execution() {
    let echo = FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        |_id, _p, _s, _u| async { anyhow::bail!("tool should have been blocked") },
    )
    .into_ref();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo]),
        extension_factories: vec![ext(|pi| {
            on_event::<ToolCallEvent, _, _>(&*pi, |_event, _ctx| async move {
                Ok(Some(ToolCallEventResult {
                    block: Some(true),
                    reason: Some("Blocked by terminating policy".into()),
                    terminate: Some(true),
                    input: None,
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![tool_use("echo", json!({"text": "hello"})), text("should not run")]);

    harness.session.prompt("hi", None).await.unwrap();

    assert_eq!(harness.get_pending_response_count(), 1);
    assert!(!harness.assistant_texts().contains(&"should not run".to_string()));
    assert_eq!(harness.events_of_type("tool_execution_end")[0]["result"]["terminate"], json!(true));
    assert!(
        harness
            .session
            .messages()
            .iter()
            .any(|m| matches!(m, AgentMessage::ToolResult(r) if r.is_error))
    );
    harness.cleanup();
}

// #3982
#[tokio::test]
async fn allows_extensions_to_replace_finalized_assistant_usage_cost() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            on_event::<MessageEndEvent, _, _>(&*pi, |event, _ctx| async move {
                let AgentMessage::Assistant(mut message) = event.message else {
                    return Ok(None);
                };
                message.usage.cost.total = 0.123;
                Ok(Some(MessageEndEventResult {
                    message: Some(AgentMessage::Assistant(message)),
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("hello")]);

    harness.session.prompt("hi", None).await.unwrap();

    let assistant = harness
        .session
        .messages()
        .into_iter()
        .find_map(|m| match m {
            AgentMessage::Assistant(a) => Some(a),
            _ => None,
        })
        .expect("missing assistant message");
    assert_eq!(assistant.usage.cost.total, 0.123);
    let end = harness
        .events_of_type("message_end")
        .into_iter()
        .find(|e| e["message"]["role"] == json!("assistant"))
        .expect("missing assistant message_end event");
    assert_eq!(end["message"]["usage"]["cost"]["total"], json!(0.123));
    harness.cleanup();
}
