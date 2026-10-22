//! Port of the `ModelRuntime virtual models` and `getBranchSelection` parts of
//! `test/virtual-models.test.ts`.

use std::sync::Arc;

use futures::FutureExt;
use parking_lot::Mutex;
use pi_ai::models::get_supported_thinking_levels;
use pi_ai::models::ModelsRequestOptions;
use pi_ai::providers::faux::{
    FauxModelDefinition, FauxResponseStep, RegisterFauxProviderOptions, faux_provider, faux_text_message,
};
use pi_ai::providers::model_schema::{ModelInputModality, ModelThinkingLevel, ThinkingLevel};
use pi_ai::types::{AssistantMessage, Context, Message, Model, StopReason, StreamOptions, UserMessage};
use pi_coding_agent::core::auth_storage::AuthStorage;
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime, ModelRuntimeResolveModelOptions};
use pi_coding_agent::core::models_store::InMemoryCodingAgentModelsStore;
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::core::virtual_models::{
    GetBranchSelectionResult, ModelRoute, ModelRouteReason, ModelRouteRequest, VirtualModelDefinition,
    get_branch_selection,
};
use serde_json::json;

struct Setup {
    runtime: Arc<ModelRuntime>,
    faux: pi_ai::providers::faux::FauxProviderHandle,
    definition: VirtualModelDefinition,
    virtual_model: Model,
    requests: Arc<Mutex<Vec<ModelRouteRequest>>>,
}

async fn create_runtime() -> Setup {
    let runtime = ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(AuthStorage::in_memory(None)),
        models_store: Some(Arc::new(InMemoryCodingAgentModelsStore::new())),
        models_path: Some(String::new()),
        allow_model_network: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap();
    let faux = faux_provider(RegisterFauxProviderOptions {
        models: vec![
            FauxModelDefinition {
                id: "small".into(),
                context_window: Some(1000),
                max_tokens: Some(100),
                input: Some(vec![ModelInputModality::Text]),
                ..Default::default()
            },
            FauxModelDefinition {
                id: "large".into(),
                context_window: Some(50_000),
                max_tokens: Some(5000),
                input: Some(vec![ModelInputModality::Text, ModelInputModality::Image]),
                reasoning: Some(true),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    runtime.register_native_provider(faux.provider.clone()).unwrap();
    let requests: Arc<Mutex<Vec<ModelRouteRequest>>> = Arc::default();
    let weak = Arc::downgrade(&runtime);
    let recorded = requests.clone();
    let definition = VirtualModelDefinition {
        provider: "router".into(),
        id: "auto".into(),
        name: "Auto".into(),
        thinking_levels: Some(vec![ModelThinkingLevel::Low, ModelThinkingLevel::High]),
        context_window: None,
        max_tokens: None,
        input: None,
        route: Arc::new(move |request: ModelRouteRequest| {
            recorded.lock().push(request.clone());
            let runtime = weak.upgrade().unwrap();
            let id = if request.thinking_level == ModelThinkingLevel::High { "large" } else { "small" };
            let model = runtime.get_model("faux", id).unwrap();
            async move { Ok(ModelRoute { model, thinking_level: ModelThinkingLevel::High, state: None }) }.boxed()
        }),
    };
    runtime.register_virtual_model(&definition).unwrap();
    runtime.refresh(None).await.unwrap();
    assert_eq!(runtime.get_error(), None);
    let virtual_model = runtime.get_model("router", "auto").unwrap();
    Setup { runtime, faux, definition, virtual_model, requests }
}

fn assistant_from(model: &Model, text: &str) -> AssistantMessage {
    AssistantMessage {
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        ..faux_text_message(text)
    }
}

fn user(text: &str) -> Message {
    Message::User(UserMessage::new(text, 1))
}

fn options(reason: ModelRouteReason, level: ModelThinkingLevel, failed: Option<AssistantMessage>) -> ModelRuntimeResolveModelOptions {
    ModelRuntimeResolveModelOptions { reason, thinking_level: level, signal: None, failed, state: None }
}

#[tokio::test]
async fn lists_a_virtual_model_and_routes_it_with_a_clamped_thinking_level() {
    let setup = create_runtime().await;
    let virtual_model = &setup.virtual_model;
    assert_eq!(virtual_model.context_window, 0);
    assert_eq!(virtual_model.max_tokens, 0);
    assert_eq!(virtual_model.input, vec![ModelInputModality::Text, ModelInputModality::Image]);
    assert_eq!(get_supported_thinking_levels(virtual_model), vec![ModelThinkingLevel::Low, ModelThinkingLevel::High]);
    let large = setup.runtime.get_model("faux", "large").unwrap();
    let mut answer = assistant_from(&large, "answer");
    answer.thinking_level = Some(ModelThinkingLevel::Medium);
    let messages = vec![user("first"), Message::Assistant(answer), user("second")];

    let low = setup
        .runtime
        .resolve_model(virtual_model.clone(), &messages, &options(ModelRouteReason::User, ModelThinkingLevel::Low, None))
        .await
        .unwrap();
    assert_eq!(low.model.id, "small");
    // The router asked for "high", but the small model does not reason.
    assert_eq!(low.thinking_level, ModelThinkingLevel::Off);
    let previous = setup.requests.lock()[0].previous.clone().unwrap();
    assert_eq!(previous.model, large);
    assert_eq!(previous.thinking_level, Some(ModelThinkingLevel::Medium));

    let high = setup
        .runtime
        .resolve_model(virtual_model.clone(), &messages, &options(ModelRouteReason::User, ModelThinkingLevel::High, None))
        .await
        .unwrap();
    assert_eq!(high.model, large);
    assert_eq!(high.thinking_level, ModelThinkingLevel::High);
}

#[tokio::test]
async fn reports_the_failed_request_of_a_retry_separately() {
    let setup = create_runtime().await;
    let small = setup.runtime.get_model("faux", "small").unwrap();
    let large = setup.runtime.get_model("faux", "large").unwrap();
    let mut failed = assistant_from(&large, "");
    failed.thinking_level = Some(ModelThinkingLevel::High);
    failed.stop_reason = StopReason::Error;
    failed.error_message = Some("overloaded_error".into());
    let messages = vec![user("first"), Message::Assistant(assistant_from(&small, "answer")), user("second")];
    setup
        .runtime
        .resolve_model(setup.virtual_model.clone(), &messages, &options(ModelRouteReason::Retry, ModelThinkingLevel::Low, Some(failed.clone())))
        .await
        .unwrap();
    let mut failed_route = assistant_from(&setup.virtual_model, "");
    failed_route.stop_reason = StopReason::Error;
    setup
        .runtime
        .resolve_model(setup.virtual_model.clone(), &messages, &options(ModelRouteReason::Retry, ModelThinkingLevel::Low, Some(failed_route)))
        .await
        .unwrap();
    let requests = setup.requests.lock();
    assert_eq!(requests[0].previous.as_ref().unwrap().model, small);
    let failed_request = requests[0].failed.as_ref().unwrap();
    assert_eq!(failed_request.model, large);
    assert_eq!(failed_request.thinking_level, Some(ModelThinkingLevel::High));
    assert_eq!(failed_request.message.error_message.as_deref(), Some("overloaded_error"));
    assert!(requests[1].failed.is_none());
}

#[tokio::test]
async fn lists_several_virtual_models_under_a_provider_with_physical_models() {
    let setup = create_runtime().await;
    let runtime = setup.runtime.clone();
    let weak = Arc::downgrade(&runtime);
    let route: pi_coding_agent::core::virtual_models::VirtualModelRouteFn = Arc::new(move |_| {
        let model = weak.upgrade().unwrap().get_model("faux", "small").unwrap();
        async move { Ok(ModelRoute { model, thinking_level: ModelThinkingLevel::Off, state: None }) }.boxed()
    });
    let mut def = setup.definition.clone();
    def.provider = "faux".into();
    def.route = route.clone();
    runtime.register_virtual_model(&def).unwrap();
    runtime
        .register_virtual_model(&VirtualModelDefinition { id: "fast".into(), name: "Fast".into(), ..def.clone() })
        .unwrap();
    runtime
        .register_virtual_model(&VirtualModelDefinition { id: "second".into(), name: "Second".into(), ..setup.definition.clone() })
        .unwrap();
    let ids = |provider: &str| runtime.get_models(Some(provider)).into_iter().map(|m| m.id).collect::<Vec<_>>();
    assert_eq!(ids("faux"), ["small", "large", "auto", "fast"]);
    assert_eq!(ids("router"), ["auto", "second"]);
    let available = runtime.get_available(None, None).await.unwrap();
    assert_eq!(
        available.iter().filter(|m| m.provider == "faux").map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["small", "large", "auto", "fast"]
    );
    let fast = runtime.get_model("faux", "fast").unwrap();
    let route = runtime
        .resolve_model(fast, &[], &options(ModelRouteReason::User, ModelThinkingLevel::Off, None))
        .await
        .unwrap();
    assert_eq!((route.model.provider.as_str(), route.model.id.as_str()), ("faux", "small"));

    let conflict = runtime
        .register_virtual_model(&VirtualModelDefinition { id: "large".into(), ..def.clone() })
        .unwrap_err();
    assert!(conflict.to_string().contains("conflicts with a physical model"));
    runtime.unregister_virtual_model("faux", "fast");
    runtime.unregister_virtual_model("router", "auto");
    assert_eq!(ids("faux"), ["small", "large", "auto"]);
    assert_eq!(ids("router"), ["second"]);
}

#[tokio::test]
async fn rejects_routes_to_virtual_or_unknown_models() {
    let setup = create_runtime().await;
    let mut unknown = setup.virtual_model.clone();
    unknown.provider = "faux".into();
    unknown.id = "missing".into();
    for model in [setup.virtual_model.clone(), unknown] {
        let def = VirtualModelDefinition {
            route: Arc::new(move |_| {
                let model = model.clone();
                async move { Ok(ModelRoute { model, thinking_level: ModelThinkingLevel::Off, state: None }) }.boxed()
            }),
            ..setup.definition.clone()
        };
        setup.runtime.register_virtual_model(&def).unwrap();
        let error = setup
            .runtime
            .resolve_model(setup.virtual_model.clone(), &[], &options(ModelRouteReason::User, ModelThinkingLevel::Low, None))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("which is not a physical model"), "{error}");
    }
}

#[tokio::test]
async fn routes_direct_stream_simple_calls_within_the_routed_models_limits() {
    let setup = create_runtime().await;
    let max_tokens: Arc<Mutex<Option<u64>>> = Arc::default();
    let seen = max_tokens.clone();
    setup.faux.set_responses(vec![FauxResponseStep::factory(move |_, options, _, _| {
        *seen.lock() = options.max_tokens;
        faux_text_message("hello")
    })]);
    let context = Context { system_prompt: None, messages: vec![user("hi")], tools: None };
    let message = setup
        .runtime
        .complete_simple(
            setup.virtual_model.clone(),
            context,
            Some(ModelsRequestOptions::from(StreamOptions {
                reasoning: Some(ThinkingLevel::High),
                max_tokens: Some(20_000),
                ..Default::default()
            })),
        )
        .await;
    assert_eq!(setup.requests.lock().iter().map(|r| r.reason).collect::<Vec<_>>(), [ModelRouteReason::Direct]);
    assert_eq!(message.provider, "faux");
    assert_eq!(message.model, "large");
    assert_eq!(message.stop_reason, StopReason::Stop);
    assert_eq!(*max_tokens.lock(), Some(5000));
}

#[tokio::test]
async fn fails_unrouted_stream_calls_on_virtual_models() {
    let setup = create_runtime().await;
    let context = Context { system_prompt: None, messages: vec![user("hi")], tools: None };
    let message = setup.runtime.complete(setup.virtual_model.clone(), context, None).await;
    assert_eq!(message.stop_reason, StopReason::Error);
    assert!(message.error_message.unwrap().contains("must be routed before streaming"));
}

fn entries(values: Vec<serde_json::Value>) -> Vec<SessionEntry> {
    values.into_iter().map(|v| serde_json::from_value(v).unwrap()).collect()
}

fn model_change(provider: &str, id: &str) -> serde_json::Value {
    json!({ "type": "model_change", "id": "x", "parentId": null, "timestamp": "2024-01-01T00:00:00.000Z", "provider": provider, "modelId": id })
}

fn message(model: &Model, stop: &str) -> serde_json::Value {
    let mut message = serde_json::to_value(assistant_from(model, "ok")).unwrap();
    message["stopReason"] = json!(stop);
    message["role"] = json!("assistant");
    json!({ "type": "message", "id": "m", "parentId": null, "timestamp": "2024-01-01T00:00:00.000Z", "message": message })
}

#[tokio::test]
async fn branch_selection_looks_up_only_the_last_model_change() {
    let setup = create_runtime().await;
    let small = setup.runtime.get_model("faux", "small").unwrap();
    let large = setup.runtime.get_model("faux", "large").unwrap();
    let lookups: Arc<Mutex<Vec<String>>> = Arc::default();
    let runtime = setup.runtime.clone();
    let recorded = lookups.clone();
    let get_model: Arc<dyn Fn(String, String) -> Option<Model> + Send + Sync> = Arc::new(move |provider, id| {
        recorded.lock().push(format!("{provider}/{id}"));
        runtime.get_model(&provider, &id)
    });

    let mut physical = vec![model_change("faux", "small")];
    physical.extend((0..100).map(|_| message(&large, "stop")));
    assert_eq!(
        get_branch_selection(&entries(physical), get_model.clone()),
        Some(GetBranchSelectionResult { provider: "faux".into(), model_id: "large".into() })
    );
    assert_eq!(*lookups.lock(), ["faux/small"]);
    lookups.lock().clear();

    let mut routed = vec![model_change("faux", "small"), message(&small, "stop"), model_change("router", "auto")];
    routed.extend((0..100).map(|_| message(&large, "stop")));
    assert_eq!(
        get_branch_selection(&entries(routed), get_model.clone()),
        Some(GetBranchSelectionResult { provider: "router".into(), model_id: "auto".into() })
    );
    assert_eq!(*lookups.lock(), ["router/auto"]);
    lookups.lock().clear();

    let trailing = vec![model_change("router", "auto"), message(&small, "stop"), model_change("faux", "small")];
    assert_eq!(
        get_branch_selection(&entries(trailing), get_model.clone()),
        Some(GetBranchSelectionResult { provider: "faux".into(), model_id: "small".into() })
    );
    let no_change = vec![message(&small, "stop"), message(&large, "stop"), message(&setup.virtual_model, "error")];
    assert_eq!(
        get_branch_selection(&entries(no_change), get_model),
        Some(GetBranchSelectionResult { provider: "faux".into(), model_id: "large".into() })
    );
    assert!(lookups.lock().is_empty());
}
