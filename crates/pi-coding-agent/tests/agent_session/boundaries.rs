//! Port of `test/suite/agent-session-boundaries.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool};
use pi_ai::types::{StopReason, UserContent};
use pi_coding_agent::core::agent_session::{AgentSessionEvent, AgentSessionEventListener};
use pi_coding_agent::core::compaction::CompactionResult;
use pi_coding_agent::core::extensions::types::{
    AgentBeforeSettleEvent, AgentEndEvent, AgentSettledEvent, AgentStartEvent, BoundaryResult,
    CustomMessageInput, ExtensionContext, MessageEndEvent, MessageEndEventResult,
    SendMessageOptions, SendUserMessageOptions, SessionBeforeCompactEvent,
    SessionBeforeCompactResult, StreamingBehavior, TurnEndEvent, on_event,
};
use pi_coding_agent::core::session_manager::{ContextEditEntryReplacement, SessionEntry};
use serde_json::{Value, json};

use crate::common::*;
use crate::retry_events::{error_response, retry_settings};
use crate::support::*;

fn boundary(value: Value) -> BoundaryResult {
    serde_json::from_value(value).expect("valid boundary result")
}

fn entries_json(harness: &Harness) -> Vec<Value> {
    harness
        .session_manager
        .get_entries()
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

fn has_entry(harness: &Harness, ty: &str, custom_type: &str) -> bool {
    entries_json(harness)
        .iter()
        .any(|e| e["type"] == json!(ty) && e["customType"] == json!(custom_type))
}

fn record_request(requests: &Arc<Mutex<Vec<String>>>, reply: &'static str) -> pi_ai::providers::faux::FauxResponseStep {
    let r = requests.clone();
    respond_with(move |context| {
        r.lock().push(serde_json::to_string(&context.messages).unwrap());
        faux_text_message(reply)
    })
}

fn last_user_entry_id(ctx: &ExtensionContext) -> anyhow::Result<String> {
    ctx.session_manager()?
        .get_branch(None)
        .iter()
        .rev()
        .find_map(|e| match e {
            SessionEntry::Message(m) if matches!(m.message, AgentMessage::User(_)) => Some(m.id.clone()),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("missing user entry"))
}

fn summary_compactor(summary: &'static str) -> impl Fn(pi_coding_agent::core::extensions::types::ExtensionAPIRef) -> anyhow::Result<()> + Clone {
    move |pi| {
        on_event::<SessionBeforeCompactEvent, _, _>(&*pi, move |event, _ctx| async move {
            Ok(Some(SessionBeforeCompactResult {
                cancel: None,
                compaction: Some(CompactionResult {
                    summary: summary.into(),
                    first_kept_entry_id: event.preparation.first_kept_entry_id.clone(),
                    tokens_before: event.preparation.tokens_before,
                    estimated_tokens_after: None,
                    usage: None,
                    details: None,
                }),
            }))
        })?;
        Ok(())
    }
}

fn faux_model(window: u64) -> Vec<pi_ai::providers::faux::FauxModelDefinition> {
    vec![pi_ai::providers::faux::FauxModelDefinition {
        id: "faux-1".into(),
        context_window: Some(window),
        max_tokens: Some(100),
        ..Default::default()
    }]
}

fn send_user(pi: &pi_coding_agent::core::extensions::types::ExtensionAPIRef, text: &str, deliver_as: StreamingBehavior) {
    pi.send_user_message(
        UserContent::Text(text.into()),
        SendUserMessageOptions {
            deliver_as: Some(deliver_as),
            expand_prompt_templates: None,
        },
    )
    .unwrap();
}

fn send_custom(
    pi: &pi_coding_agent::core::extensions::types::ExtensionAPIRef,
    custom_type: &str,
    content: &str,
    trigger_turn: bool,
) {
    pi.send_message(
        CustomMessageInput {
            custom_type: custom_type.into(),
            content: UserContent::Text(content.into()),
            display: false,
            details: None,
        },
        SendMessageOptions {
            trigger_turn: Some(trigger_turn),
            deliver_as: None,
        },
    )
    .unwrap();
}

fn noop_tool() -> pi_agent::types::AgentToolRef {
    FnAgentTool::new(
        "noop",
        "Noop",
        "Noop",
        json!({"type": "object", "properties": {}}),
        |_id, _p, _s, _u| async { Ok(AgentToolResult::text("done", Some(json!({})))) },
    )
    .into_ref()
}

#[tokio::test]
async fn commits_a_retain_none_turn_end_compaction_and_explicitly_continues_once() {
    let handled = Arc::new(Mutex::new(false));
    let observed = Arc::new(Mutex::new(Vec::<String>::new()));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let (h, o) = (handled.clone(), observed.clone());
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let (h, o) = (h.clone(), o.clone());
            on_event::<TurnEndEvent, _, _>(&*pi, move |event, _ctx| {
                let (h, o) = (h.clone(), o.clone());
                async move {
                    o.lock().push(event.message_entry_id.clone());
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    Ok(Some(boundary(json!({
                        "entries": [{"type": "compaction", "summary": "exact handoff", "firstKeptEntryId": null, "details": {"source": "test"}}],
                        "continue": true
                    }))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        text("discarded response"),
        record_request(&requests, "continued from handoff"),
    ]);

    harness.session.prompt("discarded prompt", None).await.unwrap();

    let compaction = harness
        .session_manager
        .get_entries()
        .into_iter()
        .find_map(|e| match e {
            SessionEntry::Compaction(c) => Some(c),
            _ => None,
        })
        .expect("expected compaction");
    assert_eq!(compaction.summary, "exact handoff");
    assert_eq!(compaction.first_kept_entry_id, compaction.id);
    let requests = requests.lock();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("exact handoff"));
    assert!(!requests[0].contains("discarded prompt"));
    assert!(!requests[0].contains("discarded response"));
    assert_eq!(observed.lock().len(), 2);
    assert_eq!(harness.events_of_type("agent_settled").len(), 1);
    harness.cleanup();
}

#[derive(Clone, Copy, PartialEq)]
enum QueueKind {
    Steering,
    FollowUp,
    Both,
}

async fn preserves_queue_scheduling_around_a_turn_end_handoff(kind: QueueKind) {
    let handled = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let h = handled.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let h = h.clone();
            let api = pi.clone();
            on_event::<TurnEndEvent, _, _>(&*pi, move |_event, _ctx| {
                let h = h.clone();
                let api = api.clone();
                async move {
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    if matches!(kind, QueueKind::Steering | QueueKind::Both) {
                        send_user(&api, "queued steering", StreamingBehavior::Steer);
                    }
                    if matches!(kind, QueueKind::FollowUp | QueueKind::Both) {
                        send_user(&api, "queued follow-up", StreamingBehavior::FollowUp);
                    }
                    Ok(Some(boundary(json!({
                        "entries": [{"type": "compaction", "summary": "exact handoff", "firstKeptEntryId": null}],
                        "continue": true
                    }))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        text("first"),
        record_request(&requests, "second"),
        record_request(&requests, "third"),
    ]);

    harness.session.prompt("start", None).await.unwrap();

    let requests = requests.lock();
    assert!(requests[0].contains("exact handoff"));
    match kind {
        QueueKind::Steering => {
            assert_eq!(harness.faux.state().call_count, 2);
            assert!(requests[0].contains("queued steering"));
            assert!(!requests[0].contains("queued follow-up"));
        }
        QueueKind::FollowUp => {
            assert_eq!(harness.faux.state().call_count, 2);
            assert!(requests[0].contains("queued follow-up"));
        }
        QueueKind::Both => {
            assert_eq!(harness.faux.state().call_count, 3);
            assert!(requests[0].contains("queued steering"));
            assert!(!requests[0].contains("queued follow-up"));
            assert!(requests[1].contains("queued follow-up"));
        }
    }
    harness.cleanup();
}

#[tokio::test]
async fn preserves_steering_queue_scheduling_around_a_turn_end_handoff() {
    preserves_queue_scheduling_around_a_turn_end_handoff(QueueKind::Steering).await;
}

#[tokio::test]
async fn preserves_follow_up_queue_scheduling_around_a_turn_end_handoff() {
    preserves_queue_scheduling_around_a_turn_end_handoff(QueueKind::FollowUp).await;
}

#[tokio::test]
async fn preserves_both_queue_scheduling_around_a_turn_end_handoff() {
    preserves_queue_scheduling_around_a_turn_end_handoff(QueueKind::Both).await;
}

#[tokio::test]
async fn keeps_a_boundary_replacement_verbatim_through_threshold_compaction() {
    let handled = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let instruction = "EXACT-REPLACEMENT-INSTRUCTION ".repeat(100);
    let (h, ins) = (handled.clone(), instruction.clone());
    let compact = summary_compactor("older history summary");
    let harness = create_harness(HarnessOptions {
        models: faux_model(2_000),
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 0}})),
        extension_factories: vec![ext(move |pi| {
            let (h, ins) = (h.clone(), ins.clone());
            on_event::<TurnEndEvent, _, _>(&*pi, move |event, ctx| {
                let (h, ins) = (h.clone(), ins.clone());
                async move {
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    let user = last_user_entry_id(&ctx)?;
                    Ok(Some(boundary(json!({
                        "entries": [
                            {"type": "context_edit", "targetId": user, "replacement": {"content": ins}},
                            {"type": "context_edit", "targetId": event.message_entry_id, "replacement": null},
                            {"type": "custom", "customType": "bookkeeping", "data": {"source": "test"}}
                        ],
                        "continue": true
                    }))))
                }
            })?;
            compact(pi)
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness
        .session_manager
        .append_message(agent_message(json!({"role": "user", "content": "older input", "timestamp": now_ms() - 2})))
        .unwrap();
    let mut older = faux_text_message("older answer");
    older.timestamp = now_ms() - 1;
    harness.session_manager.append_message(AgentMessage::Assistant(older)).unwrap();
    harness.session.refresh_context();
    harness.set_responses(vec![
        text("answered original input"),
        record_request(&requests, "answered replacement"),
    ]);

    harness.session.prompt("original input", None).await.unwrap();

    assert!(!harness.events_of_type("compaction_start").is_empty());
    let requests = requests.lock();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("EXACT-REPLACEMENT-INSTRUCTION"));
    harness.cleanup();
}

#[tokio::test]
async fn keeps_boundary_input_verbatim_through_threshold_compaction_when_metadata_follows_it() {
    let handled = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let instruction = "EXACT-UNSENT-INSTRUCTION ".repeat(100);
    let (h, ins) = (handled.clone(), instruction.clone());
    let compact = summary_compactor("older history summary");
    let harness = create_harness(HarnessOptions {
        models: faux_model(2_000),
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 0}})),
        extension_factories: vec![ext(move |pi| {
            let (h, ins) = (h.clone(), ins.clone());
            on_event::<TurnEndEvent, _, _>(&*pi, move |_event, _ctx| {
                let (h, ins) = (h.clone(), ins.clone());
                async move {
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    Ok(Some(boundary(json!({
                        "entries": [
                            {"type": "custom_message", "customType": "next-work", "content": ins, "display": false},
                            {"type": "custom", "customType": "bookkeeping", "data": {"source": "test"}}
                        ],
                        "continue": true
                    }))))
                }
            })?;
            compact(pi)
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("first"), record_request(&requests, "second")]);

    harness.session.prompt(&"old input ".repeat(500), None).await.unwrap();

    assert!(!harness.events_of_type("compaction_start").is_empty());
    let requests = requests.lock();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("EXACT-UNSENT-INSTRUCTION"));
    harness.cleanup();
}

#[tokio::test]
async fn refreshes_canonical_context_before_publishing_boundary_entry_notifications() {
    let handled = Arc::new(Mutex::new(false));
    let h = handled.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let h = h.clone();
            on_event::<TurnEndEvent, _, _>(&*pi, move |_event, _ctx| {
                let h = h.clone();
                async move {
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    Ok(Some(boundary(json!({
                        "entries": [
                            {"type": "custom", "customType": "metadata", "data": true},
                            {"type": "custom_message", "customType": "visible-context", "content": "committed context", "display": true}
                        ]
                    }))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let snapshots = Arc::new(Mutex::new(Vec::<String>::new()));
    let (s, weak) = (snapshots.clone(), Arc::downgrade(&harness.session));
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if matches!(event, AgentSessionEvent::EntryAppended { .. })
            && let Some(session) = weak.upgrade()
        {
            s.lock().push(serde_json::to_string(&session.messages()).unwrap());
        }
    });
    harness.session.subscribe(&listener);
    harness.set_responses(vec![text("done")]);

    harness.session.prompt("start", None).await.unwrap();

    let snapshots = snapshots.lock();
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots.iter().all(|s| s.contains("committed context")));
    harness.cleanup();
}

#[tokio::test]
async fn continues_from_an_agent_before_settle_custom_message_before_final_settlement() {
    let requested = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = requested.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let r = r.clone();
            on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |_event, _ctx| {
                let r = r.clone();
                async move {
                    if std::mem::replace(&mut *r.lock(), true) {
                        return Ok(None);
                    }
                    Ok(Some(boundary(json!({
                        "entries": [{"type": "custom_message", "customType": "test-continuation", "content": "continue now", "display": false}],
                        "continue": true
                    }))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("first"), record_request(&requests, "second")]);

    harness.session.prompt("start", None).await.unwrap();

    assert!(requests.lock()[0].contains("continue now"));
    assert!(entries_json(&harness).iter().any(|e| e["type"] == json!("custom_message")
        && e["customType"] == json!("test-continuation")
        && e["display"] == json!(false)));
    assert_eq!(harness.events_of_type("agent_start").len(), 2);
    assert_eq!(harness.events_of_type("agent_settled").len(), 1);
    harness.cleanup();
}

#[tokio::test]
async fn persists_custom_context_queued_by_agent_end_before_pre_settlement_continuation() {
    let first_run = Arc::new(Mutex::new(true));
    let continued = Arc::new(Mutex::new(false));
    let observed = Arc::new(Mutex::new(None::<(String, String)>));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let (f, c, o) = (first_run.clone(), continued.clone(), observed.clone());
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let f = f.clone();
            let api = pi.clone();
            on_event::<AgentEndEvent, _, _>(&*pi, move |_event, _ctx| {
                let f = f.clone();
                let api = api.clone();
                async move {
                    if !std::mem::replace(&mut *f.lock(), false) {
                        return Ok(None);
                    }
                    send_custom(&api, "agent-end-context", "queued after agent end", false);
                    Ok(None)
                }
            })?;
            let (c, o) = (c.clone(), o.clone());
            on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |event, _ctx| {
                let (c, o) = (c.clone(), o.clone());
                async move {
                    if std::mem::replace(&mut *c.lock(), true) {
                        return Ok(None);
                    }
                    *o.lock() = Some((
                        serde_json::to_string(&event.state.context.pending_messages).unwrap(),
                        serde_json::to_string(&event.state.context.context_messages).unwrap(),
                    ));
                    Ok(Some(boundary(json!({"continue": true}))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("first"), record_request(&requests, "second")]);

    harness.session.prompt("start", None).await.unwrap();

    let (pending, context) = observed.lock().clone().unwrap();
    assert!(pending.contains("queued after agent end"));
    assert!(!context.contains("queued after agent end"));
    assert!(requests.lock()[0].contains("queued after agent end"));
    assert!(has_entry(&harness, "custom_message", "agent-end-context"));
    harness.cleanup();
}

#[tokio::test]
async fn keeps_a_pre_settlement_follow_up_deferred_until_the_explicit_continuation_would_stop() {
    let handled = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let h = handled.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let h = h.clone();
            let api = pi.clone();
            on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |_event, _ctx| {
                let h = h.clone();
                let api = api.clone();
                async move {
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    send_user(&api, "queued follow-up", StreamingBehavior::FollowUp);
                    Ok(Some(boundary(json!({
                        "entries": [{"type": "custom_message", "customType": "boundary", "content": "boundary context", "display": false}],
                        "continue": true
                    }))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        text("first"),
        record_request(&requests, "second"),
        record_request(&requests, "follow-up response"),
    ]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 3);
    let requests = requests.lock();
    assert!(requests[0].contains("boundary context"));
    assert!(!requests[0].contains("queued follow-up"));
    assert!(requests[1].contains("queued follow-up"));
    harness.cleanup();
}

#[tokio::test]
async fn defers_runs_started_by_agent_settled_handlers_until_every_settled_handler_completes() {
    let triggered = Arc::new(Mutex::new(false));
    let lifecycle = Arc::new(Mutex::new(Vec::<String>::new()));
    let (t, l) = (triggered.clone(), lifecycle.clone());
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let l1 = l.clone();
            on_event::<AgentStartEvent, _, _>(&*pi, move |_event, _ctx| {
                let l = l1.clone();
                async move {
                    l.lock().push("start".into());
                    Ok(None)
                }
            })?;
            let (l2, t) = (l.clone(), t.clone());
            let api = pi.clone();
            on_event::<AgentSettledEvent, _, _>(&*pi, move |_event, ctx| {
                let (l, t, api) = (l2.clone(), t.clone(), api.clone());
                async move {
                    l.lock().push(format!("settled-first:{}", ctx.is_idle()?));
                    if std::mem::replace(&mut *t.lock(), true) {
                        return Ok(None);
                    }
                    send_custom(&api, "settled-trigger", "start later", true);
                    Ok(None)
                }
            })?;
            let l3 = l.clone();
            on_event::<AgentSettledEvent, _, _>(&*pi, move |_event, ctx| {
                let l = l3.clone();
                async move {
                    l.lock().push(format!("settled-second:{}", ctx.is_idle()?));
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("first"), text("second")]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(
        *lifecycle.lock(),
        vec![
            "start",
            "settled-first:true",
            "settled-second:true",
            "start",
            "settled-first:true",
            "settled-second:true",
        ]
    );
    harness.cleanup();
}

#[tokio::test]
async fn does_not_let_an_invalid_explicit_continuation_suppress_natural_tool_continuation() {
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![noop_tool()]),
        extension_factories: vec![ext(|pi| {
            on_event::<TurnEndEvent, _, _>(&*pi, |event, ctx| async move {
                let user = last_user_entry_id(&ctx)?;
                let mut targets = vec![user, event.message_entry_id.clone()];
                targets.extend(event.tool_result_entry_ids.iter().cloned());
                let entries: Vec<Value> = targets
                    .into_iter()
                    .map(|id| json!({"type": "context_edit", "targetId": id, "replacement": null}))
                    .collect();
                Ok(Some(boundary(json!({"entries": entries, "continue": true}))))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![tool_use("noop", json!({})), text("must not run")]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 2);
    harness.cleanup();
}

#[tokio::test]
async fn dispatches_actionable_turn_end_for_synthetic_run_failures() {
    let turn_ends = Arc::new(Mutex::new(Vec::<String>::new()));
    let t = turn_ends.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let t = t.clone();
            on_event::<TurnEndEvent, _, _>(&*pi, move |event, _ctx| {
                let t = t.clone();
                async move {
                    t.lock()
                        .push(serde_json::to_value(event.state.outcome).unwrap().as_str().unwrap().to_string());
                    Ok(Some(boundary(json!({
                        "entries": [{"type": "custom", "customType": "failure-boundary", "data": true}]
                    }))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.session.agent().set_prepare_request(Some(Arc::new(|_request, _signal| {
        Box::pin(async { Err(anyhow::anyhow!("request preparation failed")) })
    })));

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(*turn_ends.lock(), vec!["error"]);
    assert_eq!(harness.faux.state().call_count, 0);
    assert!(has_entry(&harness, "custom", "failure-boundary"));
    harness.cleanup();
}

fn inflate_assistant_usage(input: u64) -> TestExtension {
    ext(move |pi| {
        on_event::<MessageEndEvent, _, _>(&*pi, move |event, _ctx| async move {
            let AgentMessage::Assistant(mut message) = event.message else {
                return Ok(None);
            };
            message.usage.input = input;
            message.usage.output = 1;
            message.usage.total_tokens = input + 1;
            Ok(Some(MessageEndEventResult {
                message: Some(AgentMessage::Assistant(message)),
            }))
        })?;
        Ok(())
    })
}

#[tokio::test]
async fn does_not_compact_from_usage_belonging_to_a_boundary_omitted_assistant() {
    let handled = Arc::new(Mutex::new(false));
    let h = handled.clone();
    let harness = create_harness(HarnessOptions {
        models: faux_model(10_000),
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 300}})),
        extension_factories: vec![
            inflate_assistant_usage(9_800),
            ext(move |pi| {
                let h = h.clone();
                on_event::<TurnEndEvent, _, _>(&*pi, move |event, _ctx| {
                    let h = h.clone();
                    async move {
                        if std::mem::replace(&mut *h.lock(), true) {
                            return Ok(None);
                        }
                        Ok(Some(boundary(json!({
                            "entries": [{"type": "context_edit", "targetId": event.message_entry_id, "replacement": null}]
                        }))))
                    }
                })?;
                Ok(())
            }),
        ],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("short response")]);

    harness.session.prompt("small prompt", None).await.unwrap();

    assert!(harness.events_of_type("compaction_start").is_empty());
    assert!(harness.session.get_context_usage().unwrap().tokens.unwrap() < 2_000);
    harness.cleanup();
}

#[tokio::test]
async fn does_not_trigger_successful_response_overflow_from_usage_captured_before_a_boundary_edit() {
    let handled = Arc::new(Mutex::new(false));
    let h = handled.clone();
    let harness = create_harness(HarnessOptions {
        models: faux_model(5_000),
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 0}})),
        extension_factories: vec![
            inflate_assistant_usage(5_100),
            ext(move |pi| {
                let h = h.clone();
                on_event::<TurnEndEvent, _, _>(&*pi, move |_event, ctx| {
                    let h = h.clone();
                    async move {
                        if std::mem::replace(&mut *h.lock(), true) {
                            return Ok(None);
                        }
                        let user = last_user_entry_id(&ctx)?;
                        Ok(Some(boundary(json!({
                            "entries": [{"type": "context_edit", "targetId": user, "replacement": null}]
                        }))))
                    }
                })?;
                Ok(())
            }),
        ],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("done")]);

    harness.session.prompt("large input that is later omitted", None).await.unwrap();

    assert!(harness.events_of_type("compaction_start").is_empty());
    assert!(harness.session.get_context_usage().unwrap().tokens.unwrap() < 2_000);
    harness.cleanup();
}

#[tokio::test]
async fn does_not_trigger_threshold_compaction_from_post_edit_usage_captured_before_a_later_compaction() {
    let harness = create_harness(HarnessOptions {
        models: faux_model(10_000),
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 0}})),
        ..Default::default()
    })
    .await
    .unwrap();
    let sm = &harness.session_manager;
    let user_id = sm
        .append_message(agent_message(json!({"role": "user", "content": "small input", "timestamp": now_ms() - 3})))
        .unwrap();
    let replacement: ContextEditEntryReplacement =
        serde_json::from_value(json!({"content": "edited input"})).unwrap();
    sm.append_context_edit(&user_id, Some(&replacement)).unwrap();
    let mut response = faux_text_message("answer");
    response.timestamp = now_ms() - 2;
    response.usage.input = 50_000;
    response.usage.output = 1;
    response.usage.total_tokens = 50_001;
    sm.append_message(AgentMessage::Assistant(response)).unwrap();
    sm.append_compaction("small summary", Some(&user_id), 50_001, None, None, None).unwrap();
    harness.session.refresh_context();
    let calls = Arc::new(Mutex::new(0));
    let c = calls.clone();
    harness.session.set_run_auto_compaction_override(Some(Arc::new(move |_, _| {
        *c.lock() += 1;
        false
    })));
    let mut error = faux_assistant_message(
        vec![faux_text("")],
        FauxMessageOptions {
            stop_reason: Some(StopReason::Error),
            error_message: Some("invalid_api_key".into()),
            ..Default::default()
        },
    );
    error.timestamp = now_ms() + 1_000;

    harness.session.check_compaction_for_test(&error, true).await;

    assert_eq!(*calls.lock(), 0);
    harness.cleanup();
}

#[tokio::test]
async fn does_not_treat_retained_pre_compaction_assistant_usage_as_post_compaction_usage() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let mut retained = faux_text_message("retained");
    retained.usage.input = 10_000;
    retained.usage.total_tokens = 10_001;
    let retained_id = harness
        .session_manager
        .append_message(AgentMessage::Assistant(retained))
        .unwrap();
    harness
        .session_manager
        .append_compaction("summary", Some(&retained_id), 10_001, None, None, None)
        .unwrap();
    harness.session.refresh_context();

    assert_eq!(harness.session.get_context_usage().unwrap().tokens, None);
    harness.cleanup();
}

#[tokio::test]
async fn persists_custom_context_sent_during_pre_settlement_before_continuing() {
    let handled = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let h = handled.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let h = h.clone();
            let api = pi.clone();
            on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |_event, _ctx| {
                let (h, api) = (h.clone(), api.clone());
                async move {
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    send_custom(&api, "pending-boundary", "persist before continue", false);
                    Ok(Some(boundary(json!({"continue": true}))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("first"), record_request(&requests, "second")]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 2);
    assert!(requests.lock()[0].contains("persist before continue"));
    assert!(has_entry(&harness, "custom_message", "pending-boundary"));
    harness.cleanup();
}

#[tokio::test]
async fn does_not_consume_queued_input_when_pre_settlement_drafts_leave_system_only_context() {
    let handled = Arc::new(Mutex::new(false));
    let h = handled.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let h = h.clone();
            let api = pi.clone();
            on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |_event, ctx| {
                let (h, api) = (h.clone(), api.clone());
                async move {
                    if std::mem::replace(&mut *h.lock(), true) {
                        return Ok(None);
                    }
                    send_user(&api, "still queued", StreamingBehavior::FollowUp);
                    let entries: Vec<Value> = ctx
                        .session_manager()?
                        .get_branch(None)
                        .iter()
                        .filter_map(|e| match e {
                            SessionEntry::Message(m)
                                if matches!(
                                    m.message,
                                    AgentMessage::User(_)
                                        | AgentMessage::Assistant(_)
                                        | AgentMessage::ToolResult(_)
                                ) =>
                            {
                                Some(json!({"type": "context_edit", "targetId": m.id, "replacement": null}))
                            }
                            _ => None,
                        })
                        .collect();
                    Ok(Some(boundary(json!({"entries": entries, "continue": true}))))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("first"), text("must not run")]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 1);
    assert_eq!(harness.session.pending_message_count(), 1);
    harness.cleanup();
}

#[tokio::test]
async fn commits_pre_settlement_drafts_but_suppresses_continuation_when_aborted_during_the_hook() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let release = Arc::new(tokio::sync::Notify::new());
    let rel = release.clone();
    let harness = Arc::new(
        create_harness(HarnessOptions {
            extension_factories: vec![ext(move |pi| {
                let (tx, rel) = (started_tx.clone(), rel.clone());
                on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |_event, _ctx| {
                    let (tx, rel) = (tx.clone(), rel.clone());
                    async move {
                        if let Some(tx) = tx.lock().take() {
                            let _ = tx.send(());
                        }
                        rel.notified().await;
                        Ok(Some(boundary(json!({
                            "entries": [{"type": "custom", "customType": "committed-after-abort", "data": true}],
                            "continue": true
                        }))))
                    }
                })?;
                Ok(())
            })],
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    harness.set_responses(vec![text("first"), text("must not run")]);

    let session = harness.session.clone();
    let prompt = tokio::spawn(async move { session.prompt("start", None).await });
    started_rx.await.unwrap();
    let session = harness.session.clone();
    let abort = tokio::spawn(async move { session.abort().await });
    tick().await;
    release.notify_one();
    prompt.await.unwrap().unwrap();
    abort.await.unwrap().unwrap();

    assert_eq!(harness.faux.state().call_count, 1);
    assert!(entries_json(&harness).iter().any(|e| e["type"] == json!("custom")
        && e["customType"] == json!("committed-after-abort")
        && e["data"] == json!(true)));
    // #10607: settlement reports the abort although the last response succeeded.
    assert_eq!(
        harness.events_of_type("agent_settled"),
        vec![json!({"type": "agent_settled", "aborted": true})]
    );
    harness.cleanup();
}

// ---------------------------------------------------------------------------
// durable length recovery
// ---------------------------------------------------------------------------

fn length_ids_and_omissions(harness: &Harness) -> (Vec<String>, Vec<String>) {
    let entries = harness.session_manager.get_entries();
    let length_ids = entries
        .iter()
        .filter_map(|e| match e {
            SessionEntry::Message(m) => match &m.message {
                AgentMessage::Assistant(a) if a.stop_reason == StopReason::Length => Some(m.id.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let omitted = entries
        .iter()
        .filter_map(|e| match e {
            SessionEntry::ContextEdit(edit) => Some(edit.target_id.clone()),
            _ => None,
        })
        .collect();
    (length_ids, omitted)
}

#[tokio::test]
async fn keeps_truncated_tool_attempts_in_context_for_the_natural_next_turn() {
    let executed = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let e = executed.clone();
    let tool = FnAgentTool::new(
        "unsafe_truncated_tool",
        "Unsafe truncated tool",
        "Must not execute from a length response",
        json!({"type": "object", "properties": {"value": {"type": "string"}}, "required": ["value"]}),
        move |_id, _p, _s, _u| {
            let e = e.clone();
            async move {
                *e.lock() = true;
                Ok(AgentToolResult::text("executed", Some(json!({}))))
            }
        },
    )
    .into_ref();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![tool]),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(
            vec![tool_call("unsafe_truncated_tool", json!({"value": "partial"}))],
            Some(StopReason::Length),
        )
        .into(),
        record_request(&requests, "completed natural continuation"),
    ]);

    harness.session.prompt("start", None).await.unwrap();

    assert!(!*executed.lock());
    assert_eq!(harness.faux.state().call_count, 2);
    assert!(requests.lock()[0].contains("may be truncated"));
    assert!(
        !harness
            .session_manager
            .get_entries()
            .iter()
            .any(|e| matches!(e, SessionEntry::ContextEdit(_)))
    );
    harness.cleanup();
}

#[tokio::test]
async fn resets_length_recovery_after_a_successful_intermediate_assistant_turn() {
    let harness = create_harness(HarnessOptions {
        models: faux_model(1000),
        settings: Some(json!({"compaction": {"keepRecentTokens": 1, "reserveTokens": 0}})),
        tools: Some(vec![noop_tool()]),
        extension_factories: vec![ext(summary_compactor("recovered input"))],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(vec![faux_text("first partial")], Some(StopReason::Length)).into(),
        tool_use("noop", json!({})),
        respond_with(|_| {
            let mut m = message(vec![faux_text("second partial")], Some(StopReason::Length));
            m.timestamp = now_ms() + 1_000;
            m
        }),
        respond_with(|_| {
            let mut m = faux_text_message("completed second recovery");
            m.timestamp = now_ms() + 2_000;
            m
        }),
    ]);

    harness.session.prompt(&"x".repeat(5000), None).await.unwrap();

    let (length_ids, omitted) = length_ids_and_omissions(&harness);
    assert_eq!(length_ids.len(), 2);
    assert!(length_ids.iter().all(|id| omitted.contains(id)));
    assert_eq!(harness.faux.state().call_count, 3);
    harness.cleanup();
}

#[tokio::test]
async fn gives_a_distinct_queued_follow_up_its_own_length_recovery_budget() {
    let queued = Arc::new(Mutex::new(false));
    let q = queued.clone();
    let compact = summary_compactor("recovered input");
    let harness = create_harness(HarnessOptions {
        models: faux_model(1000),
        settings: Some(json!({"compaction": {"keepRecentTokens": 1, "reserveTokens": 0}})),
        extension_factories: vec![ext(move |pi| {
            compact(pi.clone())?;
            let q = q.clone();
            let api = pi.clone();
            on_event::<AgentEndEvent, _, _>(&*pi, move |event, _ctx| {
                let (q, api) = (q.clone(), api.clone());
                async move {
                    if *q.lock() || !event.messages.iter().any(|m| get_message_text(m) == "first recovered") {
                        return Ok(None);
                    }
                    *q.lock() = true;
                    send_user(&api, "distinct follow-up", StreamingBehavior::FollowUp);
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(vec![faux_text("first partial")], Some(StopReason::Length)).into(),
        text("first recovered"),
        respond_with(|_| {
            let mut m = message(vec![faux_text("follow-up partial")], Some(StopReason::Length));
            m.timestamp = now_ms() + 1_000;
            m
        }),
        respond_with(|_| {
            let mut m = faux_text_message("follow-up recovered");
            m.timestamp = now_ms() + 2_000;
            m
        }),
    ]);

    harness.session.prompt(&"x".repeat(5000), None).await.unwrap();

    let (length_ids, omitted) = length_ids_and_omissions(&harness);
    assert_eq!(length_ids.len(), 2);
    assert!(length_ids.iter().all(|id| omitted.contains(id)));
    assert_eq!(harness.faux.state().call_count, 4);
    harness.cleanup();
}

#[tokio::test]
async fn finishes_retry_bookkeeping_when_a_retry_receives_a_nonretryable_error() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(2, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![error_response("overloaded_error"), error_response("invalid_api_key")]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 2);
    assert!(harness.events_of_type("auto_retry_end").iter().any(|e| e["success"] == json!(false)
        && e["attempt"] == json!(1)
        && e["finalError"] == json!("invalid_api_key")));
    harness.cleanup();
}

#[tokio::test]
async fn omits_a_recoverable_projected_replacement_by_its_source_entry_id() {
    let harness = create_harness(HarnessOptions {
        models: faux_model(1_000),
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 0}})),
        extension_factories: vec![ext(|pi| {
            on_event::<SessionBeforeCompactEvent, _, _>(&*pi, |_event, _ctx| async move {
                Ok(Some(SessionBeforeCompactResult {
                    cancel: Some(true),
                    compaction: None,
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let sm = &harness.session_manager;
    sm.append_message(agent_message(json!({"role": "user", "content": "x".repeat(5_000), "timestamp": now_ms() - 2})))
        .unwrap();
    let mut partial = message(vec![faux_text("original partial")], Some(StopReason::Length));
    partial.timestamp = now_ms() - 1;
    let partial_id = sm.append_message(AgentMessage::Assistant(partial)).unwrap();
    let replacement: ContextEditEntryReplacement =
        serde_json::from_value(json!({"content": [{"type": "text", "text": "edited partial"}]})).unwrap();
    sm.append_context_edit(&partial_id, Some(&replacement)).unwrap();
    harness.session.refresh_context();
    harness.set_responses(vec![text("new answer")]);

    harness.session.prompt("next prompt", None).await.unwrap();

    let last_edit = harness
        .session_manager
        .get_entries()
        .into_iter()
        .filter_map(|e| match e {
            SessionEntry::ContextEdit(edit) if edit.target_id == partial_id => Some(edit),
            _ => None,
        })
        .last()
        .unwrap();
    assert!(last_edit.replacement.is_none());
    assert!(
        !harness
            .session_manager
            .build_session_projection()
            .messages
            .iter()
            .any(|m| get_message_text(m) == "edited partial")
    );
    harness.cleanup();
}

#[tokio::test]
async fn recovers_an_explicit_overflow_error_after_a_retained_boundary_replacement() {
    let replaced = Arc::new(Mutex::new(false));
    let overflow_id = Arc::new(Mutex::new(None::<String>));
    let (r, o) = (replaced.clone(), overflow_id.clone());
    let compact = summary_compactor("recovered overflow");
    let harness = create_harness(HarnessOptions {
        models: faux_model(1_000),
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 0}})),
        extension_factories: vec![ext(move |pi| {
            let (r, o) = (r.clone(), o.clone());
            on_event::<TurnEndEvent, _, _>(&*pi, move |event, _ctx| {
                let (r, o) = (r.clone(), o.clone());
                async move {
                    let outcome = serde_json::to_value(event.state.outcome).unwrap();
                    if *r.lock() || outcome != json!("error") {
                        return Ok(None);
                    }
                    *r.lock() = true;
                    *o.lock() = Some(event.message_entry_id.clone());
                    Ok(Some(boundary(json!({
                        "entries": [{"type": "context_edit", "targetId": event.message_entry_id,
                            "replacement": {"content": [{"type": "text", "text": "retained error"}]}}]
                    }))))
                }
            })?;
            compact(pi)
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        faux_assistant_message(
            vec![faux_text("retained error")],
            FauxMessageOptions {
                stop_reason: Some(StopReason::Error),
                error_message: Some("prompt is too long".into()),
                ..Default::default()
            },
        )
        .into(),
        text("recovered"),
    ]);

    harness.session.prompt(&"x".repeat(5_000), None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 2);
    let overflow_id = overflow_id.lock().clone().expect("overflow id");
    let last_edit = harness
        .session_manager
        .get_entries()
        .into_iter()
        .filter_map(|e| match e {
            SessionEntry::ContextEdit(edit) if edit.target_id == overflow_id => Some(edit),
            _ => None,
        })
        .last()
        .unwrap();
    assert!(last_edit.replacement.is_none());
    harness.cleanup();
}

#[tokio::test]
async fn keeps_follow_up_work_behind_an_automatic_error_retry() {
    let queued = Arc::new(Mutex::new(false));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let q = queued.clone();
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(2, 1),
        extension_factories: vec![ext(move |pi| {
            let q = q.clone();
            let api = pi.clone();
            on_event::<TurnEndEvent, _, _>(&*pi, move |event, _ctx| {
                let (q, api) = (q.clone(), api.clone());
                async move {
                    let outcome = serde_json::to_value(event.state.outcome).unwrap();
                    if *q.lock() || outcome != json!("error") {
                        return Ok(None);
                    }
                    *q.lock() = true;
                    send_user(&api, "queued follow-up", StreamingBehavior::FollowUp);
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let lifecycle = Arc::new(Mutex::new(Vec::<String>::new()));
    let l = lifecycle.clone();
    let listener: AgentSessionEventListener = Arc::new(move |event| match &event {
        AgentSessionEvent::AgentEnd { .. } => l.lock().push("agent_end".into()),
        AgentSessionEvent::AutoRetryStart { .. } => l.lock().push("auto_retry_start".into()),
        _ => {}
    });
    harness.session.subscribe(&listener);
    harness.set_responses(vec![
        error_response("overloaded_error"),
        record_request(&requests, "retry recovered"),
        record_request(&requests, "follow-up completed"),
    ]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 3);
    let requests = requests.lock();
    assert!(!requests[0].contains("queued follow-up"));
    assert!(requests[1].contains("queued follow-up"));
    assert_eq!(lifecycle.lock()[..2], ["agent_end".to_string(), "auto_retry_start".to_string()]);
    harness.cleanup();
}

#[tokio::test]
async fn marks_the_exhausted_retry_run_as_final() {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(1, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![error_response("overloaded_error"), error_response("overloaded_error")]);

    harness.session.prompt("start", None).await.unwrap();

    let will_retry: Vec<_> = harness
        .events_of_type("agent_end")
        .iter()
        .map(|e| e["willRetry"].as_bool().unwrap())
        .collect();
    assert_eq!(will_retry, vec![true, false]);
    assert!(
        harness
            .events_of_type("auto_retry_end")
            .iter()
            .any(|e| e["success"] == json!(false) && e["attempt"] == json!(1))
    );
    harness.cleanup();
}

#[tokio::test]
async fn keeps_omissions_and_does_not_retry_when_recovery_compaction_fails() {
    let harness = create_harness(HarnessOptions {
        models: faux_model(1000),
        settings: Some(json!({
            "compaction": {"keepRecentTokens": 1, "reserveTokens": 0},
            "retry": {"enabled": false, "maxRetries": 0, "baseDelayMs": 1}
        })),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(vec![faux_text("partial response")], Some(StopReason::Length)).into(),
        faux_assistant_message(
            vec![faux_text("summary failed")],
            FauxMessageOptions {
                stop_reason: Some(StopReason::Error),
                error_message: Some("summary failed".into()),
                ..Default::default()
            },
        )
        .into(),
        text("must not retry"),
    ]);

    harness.session.prompt(&"x".repeat(5000), None).await.unwrap();

    let entries = harness.session_manager.get_entries();
    assert!(entries.iter().any(|e| matches!(e, SessionEntry::ContextEdit(_))));
    assert!(!entries.iter().any(|e| matches!(e, SessionEntry::Compaction(_))));
    assert!(entries.iter().any(|e| matches!(
        e,
        SessionEntry::Message(m) if get_message_text(&m.message) == "partial response"
    )));
    assert!(
        !harness
            .session_manager
            .build_session_projection()
            .messages
            .iter()
            .any(|m| get_message_text(m) == "partial response")
    );
    assert_eq!(harness.faux.state().call_count, 2);
    harness.cleanup();
}
