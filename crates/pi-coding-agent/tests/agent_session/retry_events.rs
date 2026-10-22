//! Port of `test/suite/agent-session-retry-events.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool};
use pi_ai::providers::faux::FauxResponseStep;
use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session::{AgentSessionEvent, AgentSessionEventListener};
use pi_coding_agent::core::extensions::types::{MessageEndEvent, MessageStartEvent, on_event};
use serde_json::{Value, json};

use crate::common::*;
use crate::support::*;

pub fn error_response(error: &str) -> FauxResponseStep {
    faux_assistant_message(
        vec![faux_text("")],
        FauxMessageOptions {
            stop_reason: Some(StopReason::Error),
            error_message: Some(error.into()),
            ..Default::default()
        },
    )
    .into()
}

pub fn retry_settings(max_retries: u32, base_delay_ms: u64) -> Option<Value> {
    Some(json!({"retry": {"enabled": true, "maxRetries": max_retries, "baseDelayMs": base_delay_ms}}))
}

fn normalize_event_order(events: &[Value]) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for event in events {
        let ty = event["type"].as_str().unwrap_or_default();
        let label = match ty {
            "message_start" | "message_end" => {
                format!("{ty}:{}", event["message"]["role"].as_str().unwrap_or_default())
            }
            "tool_execution_start" | "tool_execution_end" => {
                format!("{ty}:{}", event["toolName"].as_str().unwrap_or_default())
            }
            _ => ty.to_string(),
        };
        if label == "message_update" && normalized.last().map(String::as_str) == Some("message_update") {
            continue;
        }
        normalized.push(label);
    }
    normalized
}

fn all_events(harness: &Harness) -> Vec<Value> {
    harness
        .events()
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

fn record_retry_events(harness: &Harness) -> Arc<Mutex<Vec<String>>> {
    let retry_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = retry_events.clone();
    let listener: AgentSessionEventListener = Arc::new(move |event| match &event {
        AgentSessionEvent::AutoRetryStart { attempt, .. } => r.lock().push(format!("start:{attempt}")),
        AgentSessionEvent::AutoRetryEnd { success, .. } => r.lock().push(format!("end:{success}")),
        _ => {}
    });
    harness.session.subscribe(&listener);
    retry_events
}

fn will_retry(harness: &Harness) -> Vec<bool> {
    harness
        .events_of_type("agent_end")
        .iter()
        .map(|e| e["willRetry"].as_bool().unwrap())
        .collect()
}

fn echo_tool(runs: Arc<Mutex<Vec<String>>>) -> pi_agent::types::AgentToolRef {
    FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        move |_id, params: Value, _signal, _update| {
            let runs = runs.clone();
            async move {
                let text = params["text"].as_str().unwrap_or_default().to_string();
                runs.lock().push(text.clone());
                Ok(AgentToolResult::text(format!("echo:{text}"), Some(json!({"text": text}))))
            }
        },
    )
    .into_ref()
}

#[tokio::test]
async fn retries_after_a_transient_error_and_succeeds() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(3, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    let retry_events = record_retry_events(&harness);
    harness.set_responses(vec![error_response("overloaded_error"), text("recovered")]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(*retry_events.lock(), vec!["start:1", "end:true"]);
    assert_eq!(will_retry(&harness), vec![true, false]);
    assert_eq!(harness.faux.state().call_count, 2);
    assert!(!harness.session.is_retrying());
    harness.cleanup();
}

#[tokio::test]
async fn retries_multiple_transient_failures_and_succeeds_on_the_final_attempt() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(3, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    let retry_events = record_retry_events(&harness);
    harness.set_responses(vec![
        error_response("overloaded_error"),
        error_response("overloaded_error"),
        text("success"),
    ]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(*retry_events.lock(), vec!["start:1", "start:2", "end:true"]);
    assert_eq!(harness.faux.state().call_count, 3);
    harness.cleanup();
}

// Regression test for #9340.
#[tokio::test]
async fn finalizes_retry_state_when_abort_is_requested_after_a_retry_attempt_fails() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(3, 0),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        error_response("overloaded_error"),
        error_response("overloaded_error"),
    ]);
    let error_count = Arc::new(Mutex::new(0));
    let weak = Arc::downgrade(&harness.session);
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if let AgentSessionEvent::MessageEnd {
            message: AgentMessage::Assistant(message),
        } = &event
            && message.stop_reason == StopReason::Error
        {
            let mut count = error_count.lock();
            *count += 1;
            if *count == 2
                && let Some(session) = weak.upgrade()
            {
                // TS `void session.abort()`: run its synchronous part now.
                use futures::FutureExt;
                let _ = session.abort().now_or_never();
            }
        }
    });
    harness.session.subscribe(&listener);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(harness.session.retry_attempt(), 0);
    assert_eq!(will_retry(&harness).last(), Some(&false));
    let end = harness.events_of_type("auto_retry_end").last().cloned().unwrap();
    assert_eq!(end["success"], json!(false));
    assert_eq!(end["attempt"], json!(1));
    assert_eq!(end["finalError"], json!("Retry cancelled"));
    harness.cleanup();
}

#[tokio::test]
async fn exhausts_max_retries_and_emits_a_failure_event() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(2, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    let retry_events = record_retry_events(&harness);
    harness.set_responses(vec![
        error_response("overloaded_error"),
        error_response("overloaded_error"),
        error_response("overloaded_error"),
    ]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(*retry_events.lock(), vec!["start:1", "start:2", "end:false"]);
    assert_eq!(will_retry(&harness), vec![true, true, false]);
    assert_eq!(harness.faux.state().call_count, 3);
    assert!(!harness.session.is_retrying());
    harness.cleanup();
}

#[tokio::test]
async fn prompt_waits_for_retry_completion_even_when_assistant_message_end_handling_is_delayed() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(3, 1),
        extension_factories: vec![ext(|pi| {
            on_event::<MessageEndEvent, _, _>(&*pi, |event, _ctx| async move {
                if matches!(event.message, AgentMessage::Assistant(_)) {
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                }
                Ok(None)
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![error_response("overloaded_error"), text("recovered")]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 2);
    assert!(!harness.session.is_retrying());
    harness.cleanup();
}

#[tokio::test]
async fn does_not_retry_when_retry_is_disabled() {
    let harness = create_harness(HarnessOptions {
        settings: Some(json!({"retry": {"enabled": false}})),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![error_response("overloaded_error")]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 1);
    assert!(harness.events_of_type("auto_retry_start").is_empty());
    harness.cleanup();
}

#[tokio::test]
async fn does_not_retry_non_retryable_errors() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(3, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![error_response("invalid_api_key")]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 1);
    assert!(harness.events_of_type("auto_retry_start").is_empty());
    harness.cleanup();
}

#[tokio::test]
async fn cancels_retry_sleep_when_abort_retry_is_called() {
    let harness = Arc::new(
        create_harness(HarnessOptions {
            settings: retry_settings(3, 100),
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    harness.set_responses(vec![error_response("overloaded_error")]);
    let saw_retry_start =
        wait_for_event(&harness, |e| matches!(e, AgentSessionEvent::AutoRetryStart { .. }));

    let session = harness.session.clone();
    let run = tokio::spawn(async move { session.prompt("test", None).await });
    saw_retry_start.await.unwrap();
    harness.session.abort_retry();
    run.await.unwrap().unwrap();

    assert!(!harness.session.is_retrying());
    assert!(
        harness
            .events_of_type("auto_retry_end")
            .iter()
            .any(|e| e["finalError"] == json!("Retry cancelled"))
    );
    assert_eq!(harness.faux.state().call_count, 1);
    harness.cleanup();
}

#[tokio::test]
async fn waits_for_the_full_loop_when_retry_recovery_produces_tool_calls() {
    let runs = Arc::new(Mutex::new(Vec::<String>::new()));
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo_tool(runs.clone())]),
        settings: retry_settings(3, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        error_response("overloaded_error"),
        message(
            vec![tool_call("echo", json!({"text": "hello"}))],
            Some(StopReason::ToolUse),
        )
        .into(),
        text("final answer"),
    ]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 3);
    assert_eq!(*runs.lock(), vec!["hello"]);
    assert!(!harness.session.is_streaming());
    harness.append_responses(vec![text("again")]);
    harness.session.prompt("follow-up", None).await.unwrap();
    assert_eq!(harness.faux.state().call_count, 4);
    harness.cleanup();
}

#[tokio::test]
async fn emits_extension_events_before_public_event_subscribers() {
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let o = order.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let o1 = o.clone();
            on_event::<MessageStartEvent, _, _>(&*pi, move |event, _ctx| {
                let o = o1.clone();
                async move {
                    o.lock().push(format!("extension:message_start:{}", event.message.role()));
                    Ok(None)
                }
            })?;
            let o2 = o.clone();
            on_event::<MessageEndEvent, _, _>(&*pi, move |event, _ctx| {
                let o = o2.clone();
                async move {
                    o.lock().push(format!("extension:message_end:{}", event.message.role()));
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let o = order.clone();
    let listener: AgentSessionEventListener = Arc::new(move |event| match &event {
        AgentSessionEvent::MessageStart { message } => {
            o.lock().push(format!("public:message_start:{}", message.role()))
        }
        AgentSessionEvent::MessageEnd { message } => {
            o.lock().push(format!("public:message_end:{}", message.role()))
        }
        _ => {}
    });
    harness.session.subscribe(&listener);
    harness.set_responses(vec![text("done")]);

    harness.session.prompt("hi", None).await.unwrap();

    let mut expected = Vec::new();
    for role in ["system", "user", "assistant"] {
        for ty in ["message_start", "message_end"] {
            expected.push(format!("extension:{ty}:{role}"));
            expected.push(format!("public:{ty}:{role}"));
        }
    }
    assert_eq!(*order.lock(), expected);
    harness.cleanup();
}

#[tokio::test]
async fn emits_the_expected_event_order_for_a_single_prompt() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.set_responses(vec![text("hello")]);

    harness.session.prompt("hi", None).await.unwrap();

    assert_eq!(
        normalize_event_order(&all_events(&harness)),
        [
            "agent_start",
            "turn_start",
            "message_start:system",
            "message_end:system",
            "message_start:user",
            "message_end:user",
            "message_start:assistant",
            "message_update",
            "message_end:assistant",
            "turn_end",
            "agent_end",
            "agent_settled",
        ]
    );
    harness.cleanup();
}

#[tokio::test]
async fn emits_the_expected_event_order_for_a_tool_call_turn() {
    let runs = Arc::new(Mutex::new(Vec::<String>::new()));
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo_tool(runs.clone())]),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(
            vec![tool_call("echo", json!({"text": "hello"}))],
            Some(StopReason::ToolUse),
        )
        .into(),
        text("done"),
    ]);

    harness.session.prompt("hi", None).await.unwrap();

    assert_eq!(*runs.lock(), vec!["hello"]);
    assert_eq!(
        normalize_event_order(&all_events(&harness)),
        [
            "agent_start",
            "turn_start",
            "message_start:system",
            "message_end:system",
            "message_start:user",
            "message_end:user",
            "message_start:assistant",
            "message_update",
            "message_end:assistant",
            "tool_execution_start:echo",
            "tool_execution_end:echo",
            "message_start:toolResult",
            "message_end:toolResult",
            "turn_end",
            "turn_start",
            "message_start:assistant",
            "message_update",
            "message_end:assistant",
            "turn_end",
            "agent_end",
            "agent_settled",
        ]
    );
    harness.cleanup();
}

#[tokio::test]
async fn emits_streaming_deltas_for_text_thinking_and_tool_calls_in_message_update_events() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.set_responses(vec![
        message(
            vec![
                faux_thinking("plan"),
                faux_text("answer"),
                tool_call("echo", json!({"text": "hello"})),
            ],
            Some(StopReason::ToolUse),
        )
        .into(),
    ]);

    let _ = harness.session.prompt("hi", None).await;

    let update_types: Vec<String> = harness
        .events_of_type("message_update")
        .iter()
        .map(|e| e["assistantMessageEvent"]["type"].as_str().unwrap().to_string())
        .collect();
    for ty in ["thinking_delta", "text_delta", "toolcall_delta"] {
        assert!(update_types.iter().any(|t| t == ty), "{ty} missing in {update_types:?}");
    }
    harness.cleanup();
}

#[tokio::test]
async fn emits_agent_end_for_error_responses() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.set_responses(vec![error_response("broken")]);

    harness.session.prompt("hi", None).await.unwrap();

    assert_eq!(harness.events_of_type("agent_end").len(), 1);
    assert_eq!(harness.event_types().last().map(String::as_str), Some("agent_settled"));
    harness.cleanup();
}

#[tokio::test]
async fn emits_agent_end_for_aborted_runs_and_persists_the_aborted_assistant_message() {
    let harness = Arc::new(create_harness(HarnessOptions::default()).await.unwrap());
    harness.set_responses(vec![text(&"x".repeat(20_000))]);
    let saw_update = wait_for_event(&harness, |e| matches!(e, AgentSessionEvent::MessageUpdate { .. }));

    let session = harness.session.clone();
    let run = tokio::spawn(async move { session.prompt("hi", None).await });
    saw_update.await.unwrap();
    harness.session.abort().await.unwrap();
    run.await.unwrap().unwrap();

    assert_eq!(harness.events_of_type("agent_end").len(), 1);
    assert_eq!(harness.event_types().last().map(String::as_str), Some("agent_settled"));
    let messages = harness.session.messages();
    match messages.last() {
        Some(AgentMessage::Assistant(m)) => assert_eq!(m.stop_reason, StopReason::Aborted),
        other => panic!("expected an aborted assistant message, got {other:?}"),
    }
    harness.cleanup();
}
