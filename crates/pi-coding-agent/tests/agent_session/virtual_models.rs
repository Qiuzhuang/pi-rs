//! Port of `test/suite/virtual-models.test.ts` (AgentSession routing of virtual models).
//!
//! Not ported: "projects the session once per request under a virtual selection" spies on
//! `SessionManager.buildSessionProjection`, which Rust cannot intercept.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool, ThinkingLevel};
use pi_ai::providers::faux::{FauxModelDefinition, FauxResponseStep};
use pi_ai::types::{Context, StopReason, UserContent};
use pi_coding_agent::core::compaction::CompactionResult;
use pi_coding_agent::core::extensions::types::{
    AgentBeforeSettleEvent, BeforeAgentStartEvent, BeforeAgentStartEventResult, BoundaryResult,
    CustomMessageInput, ExtensionContext, ExtensionVirtualModel, SessionBeforeCompactEvent,
    SessionBeforeCompactResult, on_event,
};
use pi_ai::models::ModelsRequestOptions;
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::core::virtual_models::{
    ModelRoute, ModelRouteReason, ModelRouteRequest, VIRTUAL_MODEL_STATE_ENTRY,
};
use serde_json::{Value, json};

use crate::common::*;
use crate::retry_events::{error_response, retry_settings};
use crate::support::*;

type Route = Arc<dyn Fn(&ModelRouteRequest, &ExtensionContext) -> anyhow::Result<ModelRoute> + Send + Sync>;

#[derive(Clone, Debug)]
struct Seen {
    reason: ModelRouteReason,
    failed: Option<(String, Option<String>, StopReason)>,
    previous: Option<String>,
    state: Option<Value>,
}

fn find(ctx: &ExtensionContext, id: &str) -> pi_ai::types::Model {
    ctx.model_registry().unwrap().find("faux", id).unwrap()
}

fn route_to(model: pi_ai::types::Model, level: ThinkingLevel) -> ModelRoute {
    ModelRoute {
        model,
        thinking_level: level,
        state: None,
    }
}

/// New user turns pick by thinking level, everything else stays put.
fn default_route(request: &ModelRouteRequest, ctx: &ExtensionContext) -> anyhow::Result<ModelRoute> {
    if request.reason == ModelRouteReason::Direct {
        return Ok(route_to(find(ctx, "large"), ThinkingLevel::Low));
    }
    let sticky = request
        .failed
        .as_ref()
        .map(|f| (f.model.clone(), f.thinking_level))
        .or_else(|| request.previous.as_ref().map(|p| (p.model.clone(), p.thinking_level)));
    if request.reason != ModelRouteReason::User
        && let Some((model, level)) = sticky
    {
        return Ok(route_to(model, level.unwrap_or(ThinkingLevel::High)));
    }
    Ok(if request.thinking_level == ThinkingLevel::High {
        route_to(find(ctx, "large"), ThinkingLevel::High)
    } else {
        route_to(find(ctx, "small"), ThinkingLevel::Off)
    })
}

struct Routed {
    harness: Harness,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Routed {
    fn reasons(&self) -> Vec<ModelRouteReason> {
        self.seen.lock().iter().map(|s| s.reason).collect()
    }

    /// Physical model and thinking level recorded on each response.
    fn dispatched(&self) -> Vec<String> {
        self.harness
            .session
            .messages()
            .iter()
            .filter_map(|m| match m {
                AgentMessage::Assistant(a) => Some(format!(
                    "{}/{}:{}",
                    a.provider,
                    a.model,
                    a.thinking_level.map(|l| l.as_str().to_string()).unwrap_or_default()
                )),
                _ => None,
            })
            .collect()
    }
}

async fn create_routed_harness(route: Route, mut options: HarnessOptions) -> Routed {
    let seen = Arc::new(Mutex::new(Vec::<Seen>::new()));
    let s = seen.clone();
    options.models = vec![
        FauxModelDefinition {
            id: "small".into(),
            context_window: Some(1000),
            ..Default::default()
        },
        FauxModelDefinition {
            id: "large".into(),
            context_window: Some(50_000),
            max_tokens: Some(4000),
            reasoning: Some(true),
            ..Default::default()
        },
    ];
    options.tools = Some(vec![
        FnAgentTool::new(
            "echo",
            "Echo",
            "Echo text back",
            json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
            |_id, _p, _s, _u| async { Ok(AgentToolResult::text("echoed", Some(json!({})))) },
        )
        .into_ref(),
    ]);
    options.extension_factories.push(ext(move |pi| {
        let s = s.clone();
        let route = route.clone();
        pi.register_virtual_model(ExtensionVirtualModel {
            provider: "router".into(),
            id: "auto".into(),
            name: "Auto".into(),
            thinking_levels: Some(vec![ThinkingLevel::Low, ThinkingLevel::High]),
            context_window: Some(1000),
            max_tokens: None,
            input: None,
            route: Arc::new(move |request, ctx| {
                s.lock().push(Seen {
                    reason: request.reason,
                    failed: request.failed.as_ref().map(|f| {
                        (f.model.id.clone(), f.message.error_message.clone(), f.message.stop_reason)
                    }),
                    previous: request.previous.as_ref().map(|p| p.model.id.clone()),
                    state: request.state.clone(),
                });
                let result = route(&request, &ctx);
                Box::pin(async move { result })
            }),
        })
    }));
    let harness = create_harness(options).await.unwrap();
    // Stream through the runtime, which records the thinking level on responses.
    let runtime = harness.model_runtime.clone();
    harness.session.agent().set_stream_fn(Some(Arc::new(move |model, context, options| {
        let stream = runtime.stream_simple(
            model,
            Context {
                system_prompt: None,
                messages: context.messages,
                tools: None,
            },
            Some(ModelsRequestOptions {
                options,
                transform_headers: None,
            }),
        );
        Box::pin(async move { Ok(stream) })
    })));
    let auto = harness.model_runtime.get_model("router", "auto").unwrap();
    harness.session.set_model(auto, None).await.unwrap();
    harness.session.set_thinking_level(ThinkingLevel::High, None);
    Routed { harness, seen }
}

fn default() -> Route {
    Arc::new(default_route)
}

fn compactor(summary: &'static str) -> TestExtension {
    ext(move |pi| {
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
    })
}

#[tokio::test]
async fn routes_each_request_including_retries_while_the_selection_stays_virtual() {
    let r = create_routed_harness(
        default(),
        HarnessOptions {
            settings: retry_settings(3, 1),
            ..Default::default()
        },
    )
    .await;
    r.harness.set_responses(vec![
        error_response("overloaded_error"),
        tool_use("echo", json!({"text": "hi"})),
        text("done"),
    ]);

    r.harness.session.prompt("hello", None).await.unwrap();

    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, Retry, Continuation]);
    let seen = r.seen.lock().clone();
    assert_eq!(seen[1].failed.as_ref().unwrap().1.as_deref(), Some("overloaded_error"));
    assert_eq!(seen[2].previous.as_deref(), Some("large"));
    assert_eq!(r.dispatched(), vec!["faux/large:high", "faux/large:high"]);
    let model = r.harness.session.model().unwrap();
    assert_eq!((model.provider.as_str(), model.id.as_str()), ("router", "auto"));
    assert_eq!(r.harness.session.thinking_level(), ThinkingLevel::High);
    // Limits come from the physical model that produced the latest response.
    assert_eq!(r.harness.session.get_context_usage().unwrap().context_window, 50_000);
    r.harness.cleanup();
}

#[tokio::test]
async fn retries_the_first_request_of_a_turn_on_the_model_routed_for_that_turn() {
    let r = create_routed_harness(
        default(),
        HarnessOptions {
            settings: retry_settings(3, 1),
            ..Default::default()
        },
    )
    .await;
    r.harness.session.set_thinking_level(ThinkingLevel::Low, None);
    r.harness.set_responses(vec![
        text("easy answer"),
        error_response("overloaded_error"),
        text("hard answer"),
    ]);
    r.harness.session.prompt("easy", None).await.unwrap();
    r.harness.session.set_thinking_level(ThinkingLevel::High, None);

    r.harness.session.prompt("hard", None).await.unwrap();

    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, User, Retry]);
    let seen = r.seen.lock().clone();
    assert_eq!(seen[2].failed.as_ref().unwrap().0, "large");
    assert_eq!(seen[2].previous.as_deref(), Some("small"));
    assert_eq!(r.dispatched(), vec!["faux/small:off", "faux/large:high"]);
    r.harness.cleanup();
}

#[tokio::test]
async fn routes_the_compact_and_retry_after_a_truncated_response_as_a_retry() {
    let r = create_routed_harness(
        default(),
        HarnessOptions {
            settings: Some(json!({"compaction": {"keepRecentTokens": 1, "reserveTokens": 0}})),
            extension_factories: vec![compactor("overflow compacted")],
            ..Default::default()
        },
    )
    .await;
    r.harness.set_responses(vec![
        respond_with(|_| {
            let mut m = message(vec![faux_text("x".repeat(64))], Some(StopReason::Length));
            m.timestamp = now_ms() + 10_000;
            m
        }),
        text("done"),
    ]);

    r.harness.session.prompt(&"x".repeat(5000), None).await.unwrap();

    let reasons: Vec<Value> = r.harness.events_of_type("compaction_start").iter().map(|e| e["reason"].clone()).collect();
    assert_eq!(reasons, vec![json!("overflow")]);
    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, Retry]);
    let seen = r.seen.lock().clone();
    let failed = seen[1].failed.clone().unwrap();
    assert_eq!(failed.0, "large");
    assert_eq!(failed.2, StopReason::Length);
    r.harness.cleanup();
}

#[tokio::test]
async fn routes_requests_after_extension_messages_as_continuations() {
    let continued = Arc::new(Mutex::new(false));
    let r = create_routed_harness(
        default(),
        HarnessOptions {
            extension_factories: vec![ext(move |pi| {
                let continued = continued.clone();
                on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |_event, _ctx| {
                    let continued = continued.clone();
                    async move {
                        if std::mem::replace(&mut *continued.lock(), true) {
                            return Ok(None);
                        }
                        let result: BoundaryResult = serde_json::from_value(json!({
                            "entries": [{"type": "custom_message", "customType": "nudge", "content": "Keep going.", "display": false}],
                            "continue": true
                        }))?;
                        Ok(Some(result))
                    }
                })?;
                Ok(())
            })],
            ..Default::default()
        },
    )
    .await;
    r.harness.set_responses(vec![text("first"), text("second")]);

    r.harness.session.prompt("hello", None).await.unwrap();

    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, Continuation]);
    r.harness.cleanup();
}

#[tokio::test]
async fn routes_the_first_request_of_a_prompt_as_a_user_turn_when_extension_messages_follow_the_prompt() {
    let r = create_routed_harness(
        default(),
        HarnessOptions {
            extension_factories: vec![ext(|pi| {
                on_event::<BeforeAgentStartEvent, _, _>(&*pi, |_event, _ctx| async move {
                    Ok(Some(BeforeAgentStartEventResult {
                        message: Some(CustomMessageInput {
                            custom_type: "context".into(),
                            content: UserContent::Text("Extra context.".into()),
                            display: false,
                            details: None,
                        }),
                        ..Default::default()
                    }))
                })?;
                Ok(())
            })],
            ..Default::default()
        },
    )
    .await;
    r.harness.set_responses(vec![text("first"), text("second")]);

    r.harness.session.prompt("hello", None).await.unwrap();
    r.harness.session.prompt("again", None).await.unwrap();

    let messages = r.harness.session.messages();
    assert_eq!(messages[messages.len() - 2].role(), "custom");
    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, User]);
    r.harness.cleanup();
}

#[tokio::test]
async fn ends_the_run_with_an_error_response_when_routing_fails_and_keeps_the_last_physical_limits() {
    let fail = Arc::new(Mutex::new(false));
    let f = fail.clone();
    let route: Route = Arc::new(move |request, ctx| {
        if *f.lock() {
            anyhow::bail!("router unavailable");
        }
        default_route(request, ctx)
    });
    let r = create_routed_harness(route, HarnessOptions::default()).await;
    r.harness.set_responses(vec![text("answer")]);
    r.harness.session.prompt("hello", None).await.unwrap();

    *fail.lock() = true;
    r.harness.session.prompt("again", None).await.unwrap();

    match r.harness.session.messages().last() {
        Some(AgentMessage::Assistant(a)) => {
            assert_eq!(a.provider, "router");
            assert_eq!(a.model, "auto");
            assert_eq!(a.stop_reason, StopReason::Error);
            assert!(a.error_message.as_deref().unwrap_or_default().contains("router unavailable"));
        }
        other => panic!("expected an error response, got {other:?}"),
    }
    assert_eq!(r.harness.faux.state().call_count, 1);
    // The failed attempt names the virtual model (1k window); the large model's 50k applies.
    assert_eq!(r.harness.session.get_context_usage().unwrap().context_window, 50_000);
    r.harness.cleanup();
}

#[tokio::test]
async fn checks_compaction_against_the_physical_model_that_produced_the_response() {
    let r = create_routed_harness(default(), HarnessOptions::default()).await;
    r.harness.set_responses(vec![text("short answer"), text("long answer")]);
    r.harness.session.prompt("hello", None).await.unwrap();

    // About 20k tokens exceed the virtual model's declared 1k window but fit the large model's 50k.
    r.harness.session.prompt(&"x".repeat(80_000), None).await.unwrap();

    assert!(r.harness.events_of_type("compaction_start").is_empty());
    r.harness.cleanup();
}

#[tokio::test]
async fn compacts_before_a_request_routed_to_a_model_with_a_smaller_window() {
    let compacted_before_small = Arc::new(Mutex::new(false));
    let r = create_routed_harness(
        default(),
        HarnessOptions {
            settings: Some(json!({"compaction": {"keepRecentTokens": 1, "reserveTokens": 0}})),
            extension_factories: vec![compactor("compacted")],
            ..Default::default()
        },
    )
    .await;
    let events = r.harness.events.clone();
    let c = compacted_before_small.clone();
    r.harness.set_responses(vec![
        text(&"y".repeat(8000)),
        respond_with(move |_| {
            let ends = events
                .lock()
                .iter()
                .filter(|e| matches!(e, pi_coding_agent::core::agent_session::AgentSessionEvent::CompactionEnd { .. }))
                .count();
            *c.lock() = ends == 1;
            faux_text_message("small answer")
        }),
    ]);
    r.harness.session.prompt("hello", None).await.unwrap();
    assert!(r.harness.events_of_type("compaction_start").is_empty());

    // About 2k tokens fit the large model that answered last, but not the small model's 1k window.
    r.harness.session.set_thinking_level(ThinkingLevel::Low, None);
    r.harness.session.prompt("next", None).await.unwrap();

    let reasons: Vec<Value> = r.harness.events_of_type("compaction_start").iter().map(|e| e["reason"].clone()).collect();
    assert_eq!(reasons, vec![json!("threshold")]);
    assert!(*compacted_before_small.lock());
    assert_eq!(r.dispatched().last().map(String::as_str), Some("faux/small:off"));
    r.harness.cleanup();
}

#[tokio::test]
async fn compacts_between_turns_of_a_run_when_the_next_request_is_routed_to_a_smaller_window() {
    let compacted_before_small = Arc::new(Mutex::new(false));
    let route: Route = Arc::new(|request, ctx| {
        if request.reason == ModelRouteReason::Continuation {
            return Ok(route_to(find(ctx, "small"), ThinkingLevel::Off));
        }
        default_route(request, ctx)
    });
    let r = create_routed_harness(
        route,
        HarnessOptions {
            settings: Some(json!({"compaction": {"keepRecentTokens": 1, "reserveTokens": 0}})),
            extension_factories: vec![compactor("compacted")],
            ..Default::default()
        },
    )
    .await;
    let events = r.harness.events.clone();
    let c = compacted_before_small.clone();
    r.harness.set_responses(vec![
        tool_use("echo", json!({"text": "hi"})),
        respond_with(move |_| {
            let ends = events
                .lock()
                .iter()
                .filter(|e| matches!(e, pi_coding_agent::core::agent_session::AgentSessionEvent::CompactionEnd { .. }))
                .count();
            *c.lock() = ends == 1;
            faux_text_message("small answer")
        }),
    ]);

    // About 2k tokens fit the large model of the first turn, but not the small model of the second.
    r.harness.session.prompt(&"x".repeat(8000), None).await.unwrap();

    let reasons: Vec<Value> = r.harness.events_of_type("compaction_start").iter().map(|e| e["reason"].clone()).collect();
    assert_eq!(reasons, vec![json!("threshold")]);
    assert!(*compacted_before_small.lock());
    assert_eq!(r.dispatched(), vec!["faux/large:high", "faux/small:off"]);
    r.harness.cleanup();
}

#[tokio::test]
async fn stores_router_state_on_the_branch_and_passes_it_to_later_requests() {
    let route: Route = Arc::new(|request, ctx| {
        let turns = request
            .state
            .as_ref()
            .and_then(|s| s["turns"].as_u64())
            .unwrap_or(0);
        let mut route = default_route(request, ctx)?;
        // Returning request.state keeps it without storing it again. Direct requests neither get nor store state.
        route.state = if request.reason == ModelRouteReason::Continuation {
            request.state.clone()
        } else {
            Some(json!({"turns": turns + 1}))
        };
        Ok(route)
    });
    let r = create_routed_harness(
        route,
        HarnessOptions {
            settings: Some(json!({"compaction": {"keepRecentTokens": 1}})),
            ..Default::default()
        },
    )
    .await;
    r.harness.set_responses(vec![
        tool_use("echo", json!({"text": "hi"})),
        text("first"),
        text("second"),
        text("summary"),
        text("summary"),
    ]);

    r.harness.session.prompt("one", None).await.unwrap();
    r.harness.session.prompt("two", None).await.unwrap();
    let stored = |h: &Harness| -> Vec<Value> {
        h.session_manager
            .get_branch(None)
            .into_iter()
            .filter_map(|e| match e {
                SessionEntry::Custom(c) if c.custom_type == VIRTUAL_MODEL_STATE_ENTRY => c.data,
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        stored(&r.harness),
        vec![
            json!({"provider": "router", "modelId": "auto", "state": {"turns": 1}}),
            json!({"provider": "router", "modelId": "auto", "state": {"turns": 2}}),
        ]
    );

    r.harness.session.compact(None).await.unwrap();

    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, Continuation, User, Direct]);
    let states: Vec<Option<Value>> = r.seen.lock().iter().map(|s| s.state.clone()).collect();
    assert_eq!(
        states,
        vec![None, Some(json!({"turns": 1})), Some(json!({"turns": 1})), None]
    );
    assert_eq!(stored(&r.harness).len(), 2);
    r.harness.cleanup();
}

#[tokio::test]
async fn does_not_route_compactions_that_an_extension_supplies() {
    let route: Route = Arc::new(|request, ctx| {
        if request.reason == ModelRouteReason::Direct {
            anyhow::bail!("router unavailable");
        }
        default_route(request, ctx)
    });
    let r = create_routed_harness(
        route,
        HarnessOptions {
            settings: Some(json!({"compaction": {"keepRecentTokens": 1}})),
            extension_factories: vec![compactor("extension summary")],
            ..Default::default()
        },
    )
    .await;
    r.harness.set_responses(vec![text("first answer"), text("second answer")]);
    r.harness.session.prompt("first", None).await.unwrap();
    r.harness.session.prompt("second", None).await.unwrap();

    let result = r.harness.session.compact(None).await.unwrap();

    assert_eq!(result.summary, "extension summary");
    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, User]);
    r.harness.cleanup();
}

#[tokio::test]
async fn routes_compaction_summaries_to_the_resolved_model() {
    let summaries = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = create_routed_harness(
        default(),
        HarnessOptions {
            settings: Some(json!({"compaction": {"keepRecentTokens": 1}})),
            ..Default::default()
        },
    )
    .await;
    let summary = |summaries: Arc<Mutex<Vec<String>>>| {
        FauxResponseStep::factory(move |_c, options, _s, model| {
            let reasoning = options
                .reasoning
                .map(|r| serde_json::to_value(r).unwrap().as_str().unwrap().to_string())
                .unwrap_or_else(|| "off".into());
            summaries.lock().push(format!("{}:{reasoning}", model.id));
            faux_text_message("summary")
        })
    };
    // Compaction summarizes the history and the split turn prefix with one routed model.
    r.harness.set_responses(vec![
        text("first answer"),
        text("second answer"),
        summary(summaries.clone()),
        summary(summaries.clone()),
    ]);
    r.harness.session.prompt("first", None).await.unwrap();
    r.harness.session.prompt("second", None).await.unwrap();

    let result = r.harness.session.compact(None).await.unwrap();

    assert!(result.summary.contains("summary"));
    use ModelRouteReason::*;
    assert_eq!(r.reasons(), vec![User, User, Direct]);
    // The router's model and thinking level apply.
    assert_eq!(*summaries.lock(), vec!["large:low", "large:low"]);
    r.harness.cleanup();
}
