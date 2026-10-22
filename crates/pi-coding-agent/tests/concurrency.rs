//! Concurrency regressions for the session engine: extension handlers and session listeners
//! re-entering the session while it emits, listeners unsubscribing during emit, and sessions not
//! leaking through `session -> runner -> extension closures -> session` reference cycles.

#[path = "support/mod.rs"]
mod support;

#[path = "agent_session/common.rs"]
mod common;

use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;
use pi_agent::types::{AgentToolResult, FnAgentTool};
use pi_coding_agent::core::agent_session::{AgentSession, AgentSessionEvent, AgentSessionEventListener};
use pi_coding_agent::core::extensions::types::{ExtensionAPI, ExtensionContext, ExtensionEvent, ExtensionHandler};
use serde_json::json;

use crate::common::*;
use crate::support::*;

const EVENTS: &[&str] = &[
    "before_agent_start",
    "agent_start",
    "turn_start",
    "context",
    "message_start",
    "message_update",
    "message_end",
    "tool_call",
    "tool_execution_start",
    "tool_execution_update",
    "tool_execution_end",
    "tool_result",
    "turn_end",
    "agent_end",
];

fn echo_tool() -> pi_agent::types::AgentToolRef {
    FnAgentTool::new(
        "echo",
        "Echo",
        "Echo the input",
        json!({"type": "object", "properties": {}}),
        |_id, _params, _signal, _update| async move { Ok(AgentToolResult::text("echoed", Some(json!({})))) },
    )
    .into_ref()
}

/// Call back into the session through the extension API and the handler context.
fn probe(pi: &Weak<dyn ExtensionAPI>, ctx: &ExtensionContext) {
    if let Some(pi) = pi.upgrade() {
        let active = pi.get_active_tools().unwrap_or_default();
        let _ = pi.set_active_tools(active);
        let _ = pi.get_thinking_level();
        let _ = pi.get_session_name();
        let _ = pi.set_session_name("reentrant");
        let _ = pi.append_entry("probe", Some(json!({"ok": true})));
        let _ = pi.get_all_tools();
        let _ = pi.get_commands();
        let _ = pi.get_settings();
    }
    let _ = ctx.model();
    let _ = ctx.is_idle();
    let _ = ctx.has_pending_messages();
    let _ = ctx.get_context_usage();
    let _ = ctx.get_system_prompt();
    let _ = ctx.session_manager().map(|sm| sm.get_entries().len());
}

#[tokio::test]
async fn extension_handlers_and_listeners_can_reenter_the_session_during_events() {
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let c = calls.clone();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo_tool()]),
        extension_factories: vec![ext(move |pi| {
            let weak_pi: Weak<dyn ExtensionAPI> = Arc::downgrade(&pi);
            for event in EVENTS {
                let (weak_pi, calls) = (weak_pi.clone(), c.clone());
                let name = event.to_string();
                let handler: ExtensionHandler = Arc::new(move |_event: ExtensionEvent, ctx: ExtensionContext| {
                    probe(&weak_pi, &ctx);
                    calls.lock().push(name.clone());
                    let weak_pi = weak_pi.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        probe(&weak_pi, &ctx);
                        Ok(None)
                    })
                });
                pi.on(event, handler)?;
            }
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();

    // A session listener that reads session state synchronously during every event.
    let weak_session: Weak<AgentSession> = Arc::downgrade(&harness.session);
    let listener_events = Arc::new(Mutex::new(0usize));
    let le = listener_events.clone();
    let listener: AgentSessionEventListener = Arc::new(move |_event: AgentSessionEvent| {
        if let Some(session) = weak_session.upgrade() {
            let _ = session.messages();
            let _ = session.is_streaming();
            let _ = session.get_active_tool_names();
            let _ = session.model();
            let _ = session.get_context_usage();
            let _ = session.system_prompt();
        }
        *le.lock() += 1;
    });
    let _unsubscribe = harness.session.subscribe(&listener);

    harness.set_responses(vec![tool_use("echo", json!({})), text("done")]);
    tokio::time::timeout(Duration::from_secs(30), harness.session.prompt("hello", None))
        .await
        .expect("prompt deadlocked while handlers re-entered the session")
        .unwrap();

    let calls = calls.lock().clone();
    for required in ["agent_start", "message_end", "tool_execution_end", "turn_end", "agent_end"] {
        assert!(calls.iter().any(|c| c == required), "{required} handler ran: {calls:?}");
    }
    assert!(*listener_events.lock() > 0);
    assert_eq!(harness.assistant_texts().last().map(String::as_str), Some("done"));
    harness.cleanup();
}

#[tokio::test]
async fn session_listener_can_unsubscribe_itself_and_subscribe_another_during_emit() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let first_events = Arc::new(Mutex::new(0usize));
    let second_events = Arc::new(Mutex::new(0usize));
    let own_unsubscribe: Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>> = Arc::new(Mutex::new(None));

    let weak_session: Weak<AgentSession> = Arc::downgrade(&harness.session);
    let (fe, se, own) = (first_events.clone(), second_events.clone(), own_unsubscribe.clone());
    let first: AgentSessionEventListener = Arc::new(move |_event| {
        *fe.lock() += 1;
        // Unsubscribe while being emitted to, and subscribe a replacement.
        let unsubscribe = own.lock().take();
        if let Some(unsubscribe) = unsubscribe {
            unsubscribe();
            if let Some(session) = weak_session.upgrade() {
                let se = se.clone();
                let second: AgentSessionEventListener = Arc::new(move |_event| *se.lock() += 1);
                let _ = session.subscribe(&second);
            }
        }
    });
    *own_unsubscribe.lock() = Some(harness.session.subscribe(&first));

    harness.set_responses(vec![text("one"), text("two")]);
    tokio::time::timeout(Duration::from_secs(30), async {
        harness.session.prompt("first", None).await.unwrap();
        harness.session.prompt("second", None).await.unwrap();
    })
    .await
    .expect("emit deadlocked when a listener unsubscribed itself");

    assert_eq!(*first_events.lock(), 1, "the first listener is gone after unsubscribing");
    assert!(*second_events.lock() > 0, "the replacement listener receives later events");
    harness.cleanup();
}

#[tokio::test]
async fn session_with_extensions_is_freed_after_dispose() {
    let weak_session: Weak<AgentSession>;
    {
        let harness = create_harness(HarnessOptions {
            tools: Some(vec![echo_tool()]),
            extension_factories: vec![ext(|pi| {
                for event in EVENTS {
                    let handler: ExtensionHandler = Arc::new(move |_event: ExtensionEvent, ctx: ExtensionContext| {
                        // Handlers keep their context (which reaches the session) alive while running.
                        Box::pin(async move {
                            let _ = ctx.is_idle();
                            Ok(None)
                        })
                    });
                    pi.on(event, handler)?;
                }
                Ok(())
            })],
            ..Default::default()
        })
        .await
        .unwrap();
        let listener: AgentSessionEventListener = Arc::new(|_event| {});
        let _unsubscribe = harness.session.subscribe(&listener);
        harness.set_responses(vec![tool_use("echo", json!({})), text("done")]);
        harness.session.prompt("hello", None).await.unwrap();
        weak_session = Arc::downgrade(&harness.session);
        harness.cleanup();
    }
    // Background tasks (event queues, warmers) may need a moment to observe the disposal.
    for _ in 0..100 {
        if weak_session.upgrade().is_none() {
            return;
        }
        tick().await;
    }
    panic!(
        "AgentSession leaked: {} strong references remain after dispose and drop",
        weak_session.strong_count()
    );
}
