//! Port of `test/faux-provider.test.ts` (using an explicit `Models` collection).

use pi_ai::providers::faux::*;
use pi_ai::*;
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn user(text: &str, timestamp: i64) -> Message {
    Message::User(UserMessage::new(text, timestamp))
}

fn ctx(messages: Vec<Message>) -> Context {
    Context { messages, ..Default::default() }
}

fn setup(options: RegisterFauxProviderOptions) -> (Models, FauxProviderHandle) {
    let faux = faux_provider(options);
    let models = create_models(CreateModelsOptions::default());
    models.set_provider(faux.provider.clone());
    (models, faux)
}

fn args(value: serde_json::Value) -> JsonObject {
    value.as_object().unwrap().clone()
}

async fn collect(stream: AssistantMessageEventStream) -> Vec<AssistantMessageEvent> {
    stream.collect_all().await.0
}

#[tokio::test]
async fn registers_and_estimates_usage() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![faux_text_message("hello world").into()]);
    let context = Context { system_prompt: Some("Be concise.".into()), messages: vec![user("hi there", 1)], tools: None };
    let response = models.complete(&faux.get_model(), &context, StreamOptions::default()).await;
    assert_eq!(serde_json::to_value(&response.content).unwrap(), json!([{"type": "text", "text": "hello world"}]));
    assert!(response.usage.input > 0);
    assert!(response.usage.output > 0);
    assert_eq!(response.usage.total_tokens, response.usage.input + response.usage.output);
    assert_eq!(faux.state().call_count, 1);
}

#[tokio::test]
async fn helper_blocks() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![
        faux_assistant_message(
            vec![faux_thinking("think"), faux_tool_call("echo", args(json!({"text": "hi"})), None), faux_text("done")],
            FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
        )
        .into(),
    ]);
    let response = models.complete(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default()).await;
    let content = serde_json::to_value(&response.content).unwrap();
    assert_eq!(content[0], json!({"type": "thinking", "thinking": "think"}));
    assert_eq!(content[1]["name"], "echo");
    assert_eq!(content[1]["arguments"], json!({"text": "hi"}));
    assert_eq!(content[2], json!({"type": "text", "text": "done"}));
    assert_eq!(response.stop_reason, StopReason::ToolUse);
}

#[tokio::test]
async fn multiple_models_and_factories() {
    let (models, faux) = setup(RegisterFauxProviderOptions {
        models: vec![
            FauxModelDefinition { id: "faux-fast".into(), name: Some("Faux Fast".into()), reasoning: Some(false), ..Default::default() },
            FauxModelDefinition { id: "faux-thinker".into(), name: Some("Faux Thinker".into()), reasoning: Some(true), ..Default::default() },
        ],
        ..Default::default()
    });
    let step = FauxResponseStep::factory(|_, _, _, model| faux_text_message(format!("{}:{}", model.id, model.reasoning)));
    faux.set_responses(vec![step.clone(), step]);
    assert_eq!(faux.models().iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["faux-fast", "faux-thinker"]);
    let fast = models.complete(&faux.get_model_by_id("faux-fast").unwrap(), &ctx(vec![user("hi", 1)]), StreamOptions::default()).await;
    let thinker =
        models.complete(&faux.get_model_by_id("faux-thinker").unwrap(), &ctx(vec![user("hi", 1)]), StreamOptions::default()).await;
    assert_eq!(fast.text(), "faux-fast:false");
    assert_eq!(thinker.text(), "faux-thinker:true");
}

#[tokio::test]
async fn rewrites_api_provider_model() {
    let (models, faux) = setup(RegisterFauxProviderOptions {
        api: Some("faux:test".into()),
        provider: Some("faux-provider".into()),
        models: vec![FauxModelDefinition { id: "faux-model".into(), ..Default::default() }],
        ..Default::default()
    });
    faux.set_responses(vec![faux_text_message("hello").into()]);
    let response = models.complete(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default()).await;
    assert_eq!(response.api, "faux:test");
    assert_eq!(response.provider, "faux-provider");
    assert_eq!(response.model, "faux-model");
}

#[tokio::test]
async fn queue_order_and_exhaustion() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![faux_text_message("first").into(), faux_text_message("second").into()]);
    let context = ctx(vec![user("hi", 1)]);
    let model = faux.get_model();
    assert_eq!(models.complete(&model, &context, StreamOptions::default()).await.text(), "first");
    assert_eq!(models.complete(&model, &context, StreamOptions::default()).await.text(), "second");
    let exhausted = models.complete(&model, &context, StreamOptions::default()).await;
    assert_eq!(exhausted.stop_reason, StopReason::Error);
    assert_eq!(exhausted.error_message.as_deref(), Some("No more faux responses queued"));
    assert_eq!(faux.get_pending_response_count(), 0);
    assert_eq!(faux.state().call_count, 3);
    faux.set_responses(vec![faux_text_message("a").into()]);
    faux.append_responses(vec![faux_text_message("b").into()]);
    assert_eq!(faux.get_pending_response_count(), 2);
}

#[tokio::test]
async fn factory_errors_become_error_events() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![FauxResponseStep::Factory(std::sync::Arc::new(|_, _, _, _| {
        Box::pin(async { Err(anyhow::anyhow!("boom")) })
    }))]);
    let events = collect(models.stream(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default())).await;
    assert_eq!(events.len(), 1);
    match &events[0] {
        AssistantMessageEvent::Error { error, .. } => {
            assert_eq!(error.stop_reason, StopReason::Error);
            assert_eq!(error.error_message.as_deref(), Some("boom"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn rejects_pending_stop_reason() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![
        faux_assistant_message(
            vec![faux_text("partial")],
            FauxMessageOptions { stop_reason: Some(StopReason::Pending), ..Default::default() },
        )
        .into(),
    ]);
    let events = collect(models.stream(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default())).await;
    assert!(!events.iter().any(|e| e.type_name() == "done"));
    match events.last().unwrap() {
        AssistantMessageEvent::Error { error, .. } => {
            assert_eq!(error.error_message.as_deref(), Some("Faux response ended without a stop reason"))
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn counts_cached_characters_up_to_first_difference() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![faux_text_message("a").into(), faux_text_message("b").into(), faux_text_message("c").into()]);
    let options = || StreamOptions {
        session_id: Some("session-1".into()),
        cache_retention: Some(CacheRetention::Short),
        ..Default::default()
    };
    let model = faux.get_model();
    models.complete(&model, &ctx(vec![user("hello world", 1)]), options()).await;
    let extended = models.complete(&model, &ctx(vec![user("hello world", 1), user("next", 1)]), options()).await;
    assert_eq!((extended.usage.input, extended.usage.cache_read, extended.usage.cache_write), (3, 4, 3));
    let edited = models.complete(&model, &ctx(vec![user("hello wide", 1), user("next", 1)]), options()).await;
    assert_eq!((edited.usage.input, edited.usage.cache_read, edited.usage.cache_write), (4, 3, 4));
}

#[tokio::test]
async fn no_cache_without_session_or_with_retention_none() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![faux_text_message("first").into(), faux_text_message("second").into()]);
    let model = faux.get_model();
    let opts = StreamOptions {
        session_id: Some("session-1".into()),
        cache_retention: Some(CacheRetention::None),
        ..Default::default()
    };
    models.complete(&model, &ctx(vec![user("hello", 1)]), opts.clone()).await;
    let second = models
        .complete(&model, &ctx(vec![user("hello", 1), Message::Assistant(faux_text_message("first")), user("follow up", 2)]), opts)
        .await;
    assert_eq!(second.usage.cache_read, 0);
    assert_eq!(second.usage.cache_write, 0);
}

#[tokio::test]
async fn exact_event_order_for_fixed_chunks() {
    let (models, faux) = setup(RegisterFauxProviderOptions { token_size_min: Some(1), token_size_max: Some(1), ..Default::default() });
    faux.set_responses(vec![
        faux_assistant_message(
            vec![faux_thinking("go"), faux_text("ok"), faux_tool_call("echo", JsonObject::new(), Some("tool-1"))],
            FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
        )
        .into(),
    ]);
    let events = collect(models.stream(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default())).await;
    assert_eq!(events[0].message().stop_reason, StopReason::Pending);
    assert_eq!(
        events.iter().map(|e| e.type_name()).collect::<Vec<_>>(),
        vec![
            "start",
            "thinking_start",
            "thinking_delta",
            "thinking_end",
            "text_start",
            "text_delta",
            "text_end",
            "toolcall_start",
            "toolcall_delta",
            "toolcall_end",
            "done"
        ]
    );
}

#[tokio::test]
async fn partial_tool_call_deltas_join_to_arguments() {
    let (models, faux) = setup(Default::default());
    faux.set_responses(vec![
        faux_assistant_message(
            vec![faux_tool_call("echo", args(json!({"text": "hi", "count": 12})), Some("tool-1"))],
            FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
        )
        .into(),
    ]);
    let events = collect(models.stream(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default())).await;
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AssistantMessageEvent::ToolCallDelta { delta, .. } => Some(delta.clone()),
            _ => None,
        })
        .collect();
    assert!(deltas.len() > 1);
    let joined: serde_json::Value = serde_json::from_str(&deltas.concat()).unwrap();
    assert_eq!(joined, json!({"text": "hi", "count": 12}));
}

#[tokio::test]
async fn explicit_error_and_aborted_messages() {
    for (reason, message) in [(StopReason::Error, "upstream failed"), (StopReason::Aborted, "Request was aborted")] {
        let (models, faux) = setup(RegisterFauxProviderOptions { token_size_min: Some(2), token_size_max: Some(2), ..Default::default() });
        faux.set_responses(vec![
            faux_assistant_message(
                vec![faux_text("partial")],
                FauxMessageOptions { stop_reason: Some(reason), error_message: Some(message.into()), ..Default::default() },
            )
            .into(),
        ]);
        let events = collect(models.stream(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default())).await;
        assert_eq!(events.iter().map(|e| e.type_name()).collect::<Vec<_>>(), vec!["start", "text_start", "text_delta", "text_end", "error"]);
        match events.last().unwrap() {
            AssistantMessageEvent::Error { reason: r, error } => {
                assert_eq!(*r, reason);
                assert_eq!(error.error_message.as_deref(), Some(message));
            }
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn aborting_before_first_chunk() {
    // Direct provider dispatch (the compat path): Models would fail auth resolution first.
    let (_models, faux) = setup(RegisterFauxProviderOptions { tokens_per_second: Some(50.0), ..Default::default() });
    faux.set_responses(vec![faux_text_message("abcdefghijklmnopqrstuvwxyz").into()]);
    let signal = CancellationToken::new();
    signal.cancel();
    let events = collect(faux.provider.stream(
        &faux.get_model(),
        &normalize_context(&ctx(vec![user("hi", 1)])),
        StreamOptions { signal: Some(signal), ..Default::default() },
    ))
    .await;
    assert_eq!(events.len(), 1);
    match &events[0] {
        AssistantMessageEvent::Error { reason, error } => {
            assert_eq!(*reason, StopReason::Aborted);
            assert_eq!(error.stop_reason, StopReason::Aborted);
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn aborting_mid_text_stream() {
    let (models, faux) = setup(RegisterFauxProviderOptions {
        tokens_per_second: Some(100.0),
        token_size_min: Some(3),
        token_size_max: Some(3),
        ..Default::default()
    });
    faux.set_responses(vec![faux_text_message("abcdefghijklmnopqrstuvwxyz").into()]);
    let signal = CancellationToken::new();
    let mut stream = models.stream(
        &faux.get_model(),
        &ctx(vec![user("hi", 1)]),
        StreamOptions { signal: Some(signal.clone()), ..Default::default() },
    );
    let mut types = vec![];
    let mut deltas = 0;
    while let Some(event) = stream.next_event().await {
        types.push(event.type_name());
        if event.type_name() == "text_delta" {
            deltas += 1;
            signal.cancel();
        }
    }
    assert_eq!(deltas, 1);
    assert!(types.contains(&"error"));
    assert!(!types.contains(&"text_end"));
}

#[tokio::test]
async fn deferred_responses() {
    let (models, faux) = setup(RegisterFauxProviderOptions {
        deferred: Some(FauxDeferredOptions { pending_fetches: Some(1), poll_after_ms: Some(10) }),
        ..Default::default()
    });
    faux.set_responses(vec![faux_text_message("later").into()]);
    let model = faux.get_model();
    let submitted = models
        .complete_simple(
            &model,
            &ctx(vec![user("hi", 1)]),
            StreamOptions { deferred: Some(DeferredOption::Enabled(true)), ..Default::default() },
        )
        .await;
    assert_eq!(submitted.stop_reason, StopReason::Deferred);
    let handle = submitted.deferred.clone().unwrap();
    assert_eq!(handle.poll_after_ms, Some(10));
    let pending = models.fetch_deferred(&model, &handle, StreamOptions::default()).await;
    assert_eq!(pending.stop_reason, StopReason::Deferred);
    let done = models.fetch_deferred(&model, &handle, StreamOptions::default()).await;
    assert_eq!(done.text(), "later");
    models.cancel_deferred(&model, &handle, StreamOptions::default()).await.unwrap();
    assert_eq!(faux.state().cancelled_deferred.len(), 1);
    let cancelled = models.fetch_deferred(&model, &handle, StreamOptions::default()).await;
    assert_eq!(cancelled.stop_reason, StopReason::Error);
}

#[tokio::test]
async fn unknown_provider_errors() {
    let models = create_models(CreateModelsOptions::default());
    let faux = faux_provider(Default::default());
    let result = models.complete(&faux.get_model(), &ctx(vec![user("hi", 1)]), StreamOptions::default()).await;
    assert_eq!(result.stop_reason, StopReason::Error);
    assert_eq!(result.error_message.as_deref(), Some("Unknown provider: faux"));
}
