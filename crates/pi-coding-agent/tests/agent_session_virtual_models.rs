//! Port of `test/suite/virtual-models.test.ts` (AgentSession with an extension-registered virtual
//! model).
//!
//! Not ported: "projects the session once per request under a virtual selection" spies on
//! `SessionManager.buildSessionProjection`, which Rust cannot intercept.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::FutureExt;
use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool};
use pi_ai::providers::faux::{FauxModelDefinition, FauxResponseStep};
use pi_ai::providers::model_schema::ModelThinkingLevel;
use pi_ai::types::{Context, Model, StopReason};
use pi_coding_agent::core::compaction::compaction::CompactionResult;
use pi_coding_agent::core::extensions::types::{
    AgentBeforeSettleEvent, BeforeAgentStartEvent, BeforeAgentStartEventResult, BoundaryResult, CustomMessageInput,
    ExtensionAPIRef, ExtensionContext, ExtensionFactory, ExtensionVirtualModel, SessionBeforeCompactEvent,
    SessionBeforeCompactResult, on_event,
};
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::core::virtual_models::{
    ModelRoute, ModelRouteReason, ModelRouteRequest, VIRTUAL_MODEL_STATE_ENTRY,
};
use serde_json::{Value, json};
use support::*;

type Route = Arc<dyn Fn(&ModelRouteRequest, &ExtensionContext) -> anyhow::Result<ModelRoute> + Send + Sync>;

fn find(ctx: &ExtensionContext, id: &str) -> Model {
    ctx.model_registry().unwrap().find("faux", id).unwrap()
}

/// Router used by these tests: new user turns pick by thinking level, everything else stays put.
fn default_route(request: &ModelRouteRequest, ctx: &ExtensionContext) -> anyhow::Result<ModelRoute> {
    if request.reason == ModelRouteReason::Direct {
        return Ok(ModelRoute { model: find(ctx, "large"), thinking_level: ModelThinkingLevel::Low, state: None });
    }
    let sticky = request
        .failed
        .as_ref()
        .map(|f| (f.model.clone(), f.thinking_level))
        .or_else(|| request.previous.as_ref().map(|p| (p.model.clone(), p.thinking_level)));
    if request.reason != ModelRouteReason::User
        && let Some((model, level)) = sticky
    {
        return Ok(ModelRoute { model, thinking_level: level.unwrap_or(ModelThinkingLevel::High), state: None });
    }
    Ok(if request.thinking_level == ModelThinkingLevel::High {
        ModelRoute { model: find(ctx, "large"), thinking_level: ModelThinkingLevel::High, state: None }
    } else {
        ModelRoute { model: find(ctx, "small"), thinking_level: ModelThinkingLevel::Off, state: None }
    })
}

fn factory<F, Fut>(f: F) -> TestExtension
where
    F: Fn(ExtensionAPIRef) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let f: ExtensionFactory = Arc::new(move |pi| Box::pin(f(pi)));
    TestExtension::new(f)
}

fn echo_tool() -> pi_agent::types::AgentToolRef {
    Arc::new(FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] }),
        |_id, _params, _signal, _update| async move { Ok(AgentToolResult::text("echoed", Some(json!({})))) }.boxed(),
    ))
}

/// An extension answering `session_before_compact` with a fixed summary.
fn compaction_summary_extension(summary: &'static str) -> TestExtension {
    factory(move |pi| async move {
        on_event::<SessionBeforeCompactEvent, _, _>(&*pi, move |event, _ctx| async move {
            Ok(Some(SessionBeforeCompactResult {
                cancel: None,
                compaction: Some(CompactionResult {
                    summary: summary.to_string(),
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

struct Routed {
    harness: Harness,
    requests: Arc<Mutex<Vec<ModelRouteRequest>>>,
}

impl Routed {
    fn reasons(&self) -> Vec<ModelRouteReason> {
        self.requests.lock().iter().map(|r| r.reason).collect()
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
                    a.thinking_level.map(|l| l.as_str().to_string()).unwrap_or_else(|| "undefined".into())
                )),
                _ => None,
            })
            .collect()
    }

    fn compaction_reasons(&self) -> Vec<String> {
        self.harness
            .events_of_type("compaction_start")
            .iter()
            .map(|e| e["reason"].as_str().unwrap_or_default().to_string())
            .collect()
    }
}

impl Drop for Routed {
    fn drop(&mut self) {
        self.harness.cleanup();
    }
}

async fn create_routed_harness(route: Route, settings: Option<Value>, extensions: Vec<TestExtension>) -> Routed {
    let requests: Arc<Mutex<Vec<ModelRouteRequest>>> = Arc::default();
    let recorded = requests.clone();
    let mut extension_factories = extensions;
    extension_factories.push(factory(move |pi| {
        let recorded = recorded.clone();
        let route = route.clone();
        async move {
            pi.register_virtual_model(ExtensionVirtualModel {
                provider: "router".into(),
                id: "auto".into(),
                name: "Auto".into(),
                thinking_levels: Some(vec![ModelThinkingLevel::Low, ModelThinkingLevel::High]),
                context_window: Some(1000),
                max_tokens: None,
                input: None,
                route: Arc::new(move |request, ctx| {
                    recorded.lock().push(request.clone());
                    let result = route(&request, &ctx);
                    async move { result }.boxed()
                }),
            })
        }
    }));
    let harness = create_harness(HarnessOptions {
        models: vec![
            FauxModelDefinition { id: "small".into(), context_window: Some(1000), ..Default::default() },
            FauxModelDefinition {
                id: "large".into(),
                context_window: Some(50_000),
                max_tokens: Some(4000),
                reasoning: Some(true),
                ..Default::default()
            },
        ],
        settings,
        tools: Some(vec![echo_tool()]),
        extension_factories,
        ..Default::default()
    })
    .await
    .unwrap();
    // Stream through the runtime, which records the thinking level on responses.
    let runtime = harness.session.model_runtime();
    let stream_runtime = runtime.clone();
    harness.session.agent().set_stream_fn(Some(Arc::new(move |model, context, options| {
        let context = Context { system_prompt: None, messages: context.messages, tools: None };
        let stream = stream_runtime.stream_simple(model, context, Some(options.into()));
        async move { Ok(stream) }.boxed()
    })));
    harness.session.set_model(runtime.get_model("router", "auto").unwrap(), None).await.unwrap();
    harness.session.set_thinking_level(ModelThinkingLevel::High, None);
    Routed { harness, requests }
}

fn route_fn(f: impl Fn(&ModelRouteRequest, &ExtensionContext) -> anyhow::Result<ModelRoute> + Send + Sync + 'static) -> Route {
    Arc::new(f)
}

fn retry_settings() -> Option<Value> {
    Some(json!({ "retry": { "enabled": true, "maxRetries": 3, "baseDelayMs": 1 } }))
}

fn error_response(message: &str) -> FauxResponseStep {
    faux_assistant_message(
        vec![faux_text("")],
        FauxMessageOptions { stop_reason: Some(StopReason::Error), error_message: Some(message.into()), ..Default::default() },
    )
    .into()
}

fn tool_call_response() -> FauxResponseStep {
    let args = json!({ "text": "hi" }).as_object().cloned().unwrap();
    faux_assistant_message(
        vec![faux_tool_call("echo", args, None)],
        FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
    )
    .into()
}

fn text(text: &str) -> FauxResponseStep {
    faux_text_message(text).into()
}

#[tokio::test]
async fn routes_each_request_including_retries_while_the_selection_stays_virtual() {
    let routed = create_routed_harness(route_fn(default_route), retry_settings(), vec![]).await;
    routed.harness.set_responses(vec![error_response("overloaded_error"), tool_call_response(), text("done")]);

    routed.harness.session.prompt("hello", None).await.unwrap();

    assert_eq!(routed.reasons(), vec![ModelRouteReason::User, ModelRouteReason::Retry, ModelRouteReason::Continuation]);
    let requests = routed.requests.lock().clone();
    assert_eq!(requests[1].failed.as_ref().and_then(|f| f.message.error_message.clone()).as_deref(), Some("overloaded_error"));
    assert_eq!(requests[2].previous.as_ref().map(|p| p.model.id.as_str()), Some("large"));
    assert_eq!(routed.dispatched(), vec!["faux/large:high", "faux/large:high"]);
    let model = routed.harness.session.model().unwrap();
    assert_eq!((model.provider.as_str(), model.id.as_str()), ("router", "auto"));
    assert_eq!(routed.harness.session.thinking_level(), ModelThinkingLevel::High);
    // Limits come from the physical model that produced the latest response, not the virtual model.
    assert_eq!(routed.harness.session.get_context_usage().map(|u| u.context_window), Some(50_000));
}

#[tokio::test]
async fn retries_the_first_request_of_a_turn_on_the_model_routed_for_that_turn() {
    let routed = create_routed_harness(route_fn(default_route), retry_settings(), vec![]).await;
    routed.harness.session.set_thinking_level(ModelThinkingLevel::Low, None);
    routed.harness.set_responses(vec![text("easy answer"), error_response("overloaded_error"), text("hard answer")]);
    routed.harness.session.prompt("easy", None).await.unwrap();
    routed.harness.session.set_thinking_level(ModelThinkingLevel::High, None);

    routed.harness.session.prompt("hard", None).await.unwrap();

    assert_eq!(routed.reasons(), vec![ModelRouteReason::User, ModelRouteReason::User, ModelRouteReason::Retry]);
    // The retry reports the failed request on large next to the small response of the previous turn.
    let requests = routed.requests.lock().clone();
    assert_eq!(requests[2].failed.as_ref().map(|f| f.model.id.as_str()), Some("large"));
    assert_eq!(requests[2].previous.as_ref().map(|p| p.model.id.as_str()), Some("small"));
    assert_eq!(routed.dispatched(), vec!["faux/small:off", "faux/large:high"]);
}

#[tokio::test]
async fn routes_the_compact_and_retry_after_a_truncated_response_as_a_retry() {
    let routed = create_routed_harness(
        route_fn(default_route),
        Some(json!({ "compaction": { "keepRecentTokens": 1, "reserveTokens": 0 } })),
        vec![compaction_summary_extension("overflow compacted")],
    )
    .await;
    routed.harness.set_responses(vec![
        FauxResponseStep::factory(|_, _, _, _| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
            faux_assistant_message(
                vec![faux_text("x".repeat(64))],
                FauxMessageOptions { stop_reason: Some(StopReason::Length), timestamp: Some(now + 10_000), ..Default::default() },
            )
        }),
        text("done"),
    ]);

    routed.harness.session.prompt(&"x".repeat(5000), None).await.unwrap();

    assert_eq!(routed.compaction_reasons(), vec!["overflow"]);
    // Compaction may fold the prompt into the summary, so the retry is not a new user turn.
    assert_eq!(routed.reasons(), vec![ModelRouteReason::User, ModelRouteReason::Retry]);
    let requests = routed.requests.lock().clone();
    assert_eq!(requests[1].failed.as_ref().map(|f| f.model.id.as_str()), Some("large"));
    assert_eq!(requests[1].failed.as_ref().map(|f| f.message.stop_reason), Some(StopReason::Length));
}

#[tokio::test]
async fn routes_requests_after_extension_messages_as_continuations() {
    let nudge = factory(|pi| async move {
        let continued = Arc::new(AtomicBool::new(false));
        on_event::<AgentBeforeSettleEvent, _, _>(&*pi, move |_event, _ctx| {
            let first = !continued.swap(true, Ordering::SeqCst);
            async move {
                if !first {
                    return Ok(None);
                }
                Ok(Some(BoundaryResult {
                    entries: Some(vec![
                        serde_json::from_value(json!({
                            "type": "custom_message", "customType": "nudge", "content": "Keep going.", "display": false
                        }))
                        .unwrap(),
                    ]),
                    continue_: Some(true),
                }))
            }
        })?;
        Ok(())
    });
    let routed = create_routed_harness(route_fn(default_route), None, vec![nudge]).await;
    routed.harness.set_responses(vec![text("first"), text("second")]);

    routed.harness.session.prompt("hello", None).await.unwrap();

    // The hidden custom message becomes a user message for the model, but the user did not write it.
    assert_eq!(routed.reasons(), vec![ModelRouteReason::User, ModelRouteReason::Continuation]);
}

#[tokio::test]
async fn routes_the_first_request_of_a_prompt_as_a_user_turn_when_extension_messages_follow_the_prompt() {
    let context = factory(|pi| async move {
        on_event::<BeforeAgentStartEvent, _, _>(&*pi, |_event, _ctx| async move {
            Ok(Some(BeforeAgentStartEventResult {
                message: Some(CustomMessageInput {
                    custom_type: "context".into(),
                    content: pi_ai::types::UserContent::Text("Extra context.".into()),
                    display: false,
                    details: None,
                }),
                system_prompt: None,
                system_prompt_options: None,
            }))
        })?;
        Ok(())
    });
    let routed = create_routed_harness(route_fn(default_route), None, vec![context]).await;
    routed.harness.set_responses(vec![text("first"), text("second")]);

    routed.harness.session.prompt("hello", None).await.unwrap();
    routed.harness.session.prompt("again", None).await.unwrap();

    // The context ends with the extension message, but the request answers the user's prompt.
    let messages = routed.harness.session.messages();
    assert_eq!(messages[messages.len() - 2].role(), "custom");
    assert_eq!(routed.reasons(), vec![ModelRouteReason::User, ModelRouteReason::User]);
}

#[tokio::test]
async fn ends_the_run_with_an_error_response_when_routing_fails_and_keeps_the_last_physical_limits() {
    let fail = Arc::new(AtomicBool::new(false));
    let failing = fail.clone();
    let routed = create_routed_harness(
        route_fn(move |request, ctx| {
            if failing.load(Ordering::SeqCst) {
                anyhow::bail!("router unavailable");
            }
            default_route(request, ctx)
        }),
        None,
        vec![],
    )
    .await;
    routed.harness.set_responses(vec![text("answer")]);
    routed.harness.session.prompt("hello", None).await.unwrap();

    fail.store(true, Ordering::SeqCst);
    routed.harness.session.prompt("again", None).await.unwrap();

    let messages = routed.harness.session.messages();
    let AgentMessage::Assistant(last) = messages.last().unwrap() else { panic!("expected assistant message") };
    assert_eq!((last.provider.as_str(), last.model.as_str()), ("router", "auto"));
    assert_eq!(last.stop_reason, StopReason::Error);
    assert!(last.error_message.as_deref().unwrap_or_default().contains("router unavailable"), "{:?}", last.error_message);
    assert_eq!(routed.harness.faux.state().call_count, 1);
    // The failed attempt names the virtual model, whose declared window is 1k; the large model's 50k applies.
    assert_eq!(routed.harness.session.get_context_usage().map(|u| u.context_window), Some(50_000));
}

#[tokio::test]
async fn checks_compaction_against_the_physical_model_that_produced_the_response() {
    let routed = create_routed_harness(route_fn(default_route), None, vec![]).await;
    routed.harness.set_responses(vec![text("short answer"), text("long answer")]);
    routed.harness.session.prompt("hello", None).await.unwrap();

    // About 20k tokens exceed the virtual model's declared 1k window but fit the large model's 50k.
    routed.harness.session.prompt(&"x".repeat(80_000), None).await.unwrap();

    assert!(routed.harness.events_of_type("compaction_start").is_empty());
}

#[tokio::test]
async fn compacts_before_a_request_routed_to_a_model_with_a_smaller_window() {
    let routed = create_routed_harness(
        route_fn(default_route),
        Some(json!({ "compaction": { "keepRecentTokens": 1, "reserveTokens": 0 } })),
        vec![compaction_summary_extension("compacted")],
    )
    .await;
    let compacted_before_small = Arc::new(AtomicBool::new(false));
    let flag = compacted_before_small.clone();
    let events = routed.harness.events.clone();
    routed.harness.set_responses(vec![
        text(&"y".repeat(8000)),
        FauxResponseStep::factory(move |_, _, _, _| {
            let ends = events
                .lock()
                .iter()
                .filter(|e| serde_json::to_value(e).ok().is_some_and(|v| v["type"] == "compaction_end"))
                .count();
            flag.store(ends == 1, Ordering::SeqCst);
            faux_text_message("small answer")
        }),
    ]);
    routed.harness.session.prompt("hello", None).await.unwrap();
    assert!(routed.harness.events_of_type("compaction_start").is_empty());

    // About 2k tokens fit the large model that answered last, but not the small model's 1k window.
    routed.harness.session.set_thinking_level(ModelThinkingLevel::Low, None);
    routed.harness.session.prompt("next", None).await.unwrap();

    assert_eq!(routed.compaction_reasons(), vec!["threshold"]);
    assert!(compacted_before_small.load(Ordering::SeqCst));
    assert_eq!(routed.dispatched().last().map(String::as_str), Some("faux/small:off"));
}

#[tokio::test]
async fn compacts_between_turns_of_a_run_when_the_next_request_is_routed_to_a_smaller_window() {
    let routed = create_routed_harness(
        route_fn(|request, ctx| {
            if request.reason == ModelRouteReason::Continuation {
                return Ok(ModelRoute { model: find(ctx, "small"), thinking_level: ModelThinkingLevel::Off, state: None });
            }
            default_route(request, ctx)
        }),
        Some(json!({ "compaction": { "keepRecentTokens": 1, "reserveTokens": 0 } })),
        vec![compaction_summary_extension("compacted")],
    )
    .await;
    let compacted_before_small = Arc::new(AtomicBool::new(false));
    let flag = compacted_before_small.clone();
    let events = routed.harness.events.clone();
    routed.harness.set_responses(vec![
        tool_call_response(),
        FauxResponseStep::factory(move |_, _, _, _| {
            let ends = events
                .lock()
                .iter()
                .filter(|e| serde_json::to_value(e).ok().is_some_and(|v| v["type"] == "compaction_end"))
                .count();
            flag.store(ends == 1, Ordering::SeqCst);
            faux_text_message("small answer")
        }),
    ]);

    // About 2k tokens fit the large model of the first turn, but not the small model of the second.
    routed.harness.session.prompt(&"x".repeat(8000), None).await.unwrap();

    assert_eq!(routed.compaction_reasons(), vec!["threshold"]);
    assert!(compacted_before_small.load(Ordering::SeqCst));
    assert_eq!(routed.dispatched(), vec!["faux/large:high", "faux/small:off"]);
}

#[tokio::test]
async fn stores_router_state_on_the_branch_and_passes_it_to_later_requests() {
    let states: Arc<Mutex<Vec<Option<Value>>>> = Arc::default();
    let recorded = states.clone();
    let routed = create_routed_harness(
        route_fn(move |request, ctx| {
            recorded.lock().push(request.state.clone());
            let turns = request.state.as_ref().and_then(|s| s["turns"].as_u64()).unwrap_or(0);
            let route = default_route(request, ctx)?;
            // Returning request.state keeps it without storing it again. Direct requests neither get nor store state.
            if request.reason == ModelRouteReason::Continuation {
                return Ok(ModelRoute { state: request.state.clone(), ..route });
            }
            Ok(ModelRoute { state: Some(json!({ "turns": turns + 1 })), ..route })
        }),
        Some(json!({ "compaction": { "keepRecentTokens": 1 } })),
        vec![],
    )
    .await;
    routed.harness.set_responses(vec![tool_call_response(), text("first"), text("second"), text("summary"), text("summary")]);

    routed.harness.session.prompt("one", None).await.unwrap();
    routed.harness.session.prompt("two", None).await.unwrap();
    let stored = || -> Vec<Value> {
        routed
            .harness
            .session_manager
            .get_branch(None)
            .into_iter()
            .filter_map(|entry| match entry {
                SessionEntry::Custom(custom) if custom.custom_type == VIRTUAL_MODEL_STATE_ENTRY => custom.data.clone(),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        stored(),
        vec![
            json!({ "provider": "router", "modelId": "auto", "state": { "turns": 1 } }),
            json!({ "provider": "router", "modelId": "auto", "state": { "turns": 2 } }),
        ]
    );

    routed.harness.session.compact(None).await.unwrap();

    assert_eq!(
        routed.reasons(),
        vec![ModelRouteReason::User, ModelRouteReason::Continuation, ModelRouteReason::User, ModelRouteReason::Direct]
    );
    assert_eq!(*states.lock(), vec![None, Some(json!({ "turns": 1 })), Some(json!({ "turns": 1 })), None]);
    assert_eq!(stored().len(), 2);
}

#[tokio::test]
async fn does_not_route_compactions_that_an_extension_supplies() {
    let routed = create_routed_harness(
        route_fn(|request, ctx| {
            if request.reason == ModelRouteReason::Direct {
                anyhow::bail!("router unavailable");
            }
            default_route(request, ctx)
        }),
        Some(json!({ "compaction": { "keepRecentTokens": 1 } })),
        vec![compaction_summary_extension("extension summary")],
    )
    .await;
    routed.harness.set_responses(vec![text("first answer"), text("second answer")]);
    routed.harness.session.prompt("first", None).await.unwrap();
    routed.harness.session.prompt("second", None).await.unwrap();

    let result = routed.harness.session.compact(None).await.unwrap();

    assert_eq!(result.summary, "extension summary");
    assert_eq!(routed.reasons(), vec![ModelRouteReason::User, ModelRouteReason::User]);
}

#[tokio::test]
async fn routes_compaction_summaries_to_the_resolved_model() {
    let summaries: Arc<Mutex<Vec<String>>> = Arc::default();
    let routed = create_routed_harness(route_fn(default_route), Some(json!({ "compaction": { "keepRecentTokens": 1 } })), vec![])
        .await;
    let summary = |summaries: Arc<Mutex<Vec<String>>>| {
        FauxResponseStep::factory(move |_, options, _, model| {
            let level = options.reasoning.map(|l| l.as_str().to_string()).unwrap_or_else(|| "off".into());
            summaries.lock().push(format!("{}:{level}", model.id));
            faux_text_message("summary")
        })
    };
    // Compaction summarizes the history and the split turn prefix with one routed model.
    routed.harness.set_responses(vec![
        text("first answer"),
        text("second answer"),
        summary(summaries.clone()),
        summary(summaries.clone()),
    ]);
    routed.harness.session.prompt("first", None).await.unwrap();
    routed.harness.session.prompt("second", None).await.unwrap();

    let result = routed.harness.session.compact(None).await.unwrap();

    assert!(result.summary.contains("summary"));
    assert_eq!(routed.reasons(), vec![ModelRouteReason::User, ModelRouteReason::User, ModelRouteReason::Direct]);
    // The router's model and thinking level apply. The provider layer sizes output from that model.
    assert_eq!(*summaries.lock(), vec!["large:low", "large:low"]);
}
