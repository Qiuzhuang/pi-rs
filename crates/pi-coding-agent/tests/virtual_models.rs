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

#[tokio::test]
async fn does_not_forward_caller_credentials_to_a_routed_model_of_another_provider() {
    let setup = create_runtime().await;
    type Seen = Vec<(Option<String>, Option<pi_ai::types::ProviderHeaders>)>;
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let step = |seen: Arc<Mutex<Seen>>| {
        FauxResponseStep::factory(move |_, options, _, _| {
            seen.lock().push((options.api_key.clone(), options.headers.clone()));
            faux_text_message("hello")
        })
    };
    setup.faux.set_responses(vec![step(seen.clone()), step(seen.clone())]);
    let context = Context { system_prompt: None, messages: vec![user("hi")], tools: None };
    let options = || {
        Some(ModelsRequestOptions::from(StreamOptions {
            api_key: Some("caller-key".into()),
            headers: Some([("x-caller".to_string(), Some("1".to_string()))].into_iter().collect()),
            ..Default::default()
        }))
    };
    setup
        .runtime
        .register_virtual_model(&VirtualModelDefinition { provider: "faux".into(), id: "auto".into(), ..setup.definition.clone() })
        .unwrap();

    setup.runtime.complete_simple(setup.virtual_model.clone(), context.clone(), options()).await;
    let faux_auto = setup.runtime.get_model("faux", "auto").unwrap();
    setup.runtime.complete_simple(faux_auto, context, options()).await;

    // The router provider's credentials stay with it; the faux provider's own virtual model keeps them.
    let seen = seen.lock();
    assert_eq!(seen.len(), 2);
    assert_ne!(seen[0].0.as_deref(), Some("caller-key"));
    assert!(seen[0].1.as_ref().is_none_or(|h| !h.contains_key("x-caller")));
    assert_eq!(seen[1].0.as_deref(), Some("caller-key"));
    assert_eq!(seen[1].1.as_ref().and_then(|h| h.get("x-caller").cloned()), Some(Some("1".to_string())));
}

// ---------------------------------------------------------------------------
// createAgentSession with virtual models
// ---------------------------------------------------------------------------

mod support;

use pi_coding_agent::core::agent_session::AgentSession;
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, CreateAgentSessionResult, create_agent_session};
use pi_coding_agent::core::session_manager::SessionManager;

async fn open_session(
    runtime: &Arc<ModelRuntime>,
    dir: &std::path::Path,
    session_manager: Arc<SessionManager>,
    model: Option<Model>,
) -> CreateAgentSessionResult {
    let dir = dir.to_string_lossy().into_owned();
    create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(dir.clone()),
        agent_dir: Some(dir),
        model_runtime: Some(runtime.clone()),
        session_manager: Some(session_manager),
        resource_loader: Some(support::TestResourceLoader::new(None)),
        model,
        ..Default::default()
    }))
    .await
    .unwrap()
}

/// Resume a transcript where the virtual model was selected and the large model answered.
async fn resume(runtime: &Arc<ModelRuntime>, dir: &std::path::Path, model: Option<Model>) -> (Arc<AgentSession>, Arc<SessionManager>) {
    let session_manager = SessionManager::in_memory(Some(&dir.to_string_lossy()), None, None).unwrap();
    session_manager.append_model_change("router", "auto").unwrap();
    session_manager.append_message(UserMessage::new("hi", 1)).unwrap();
    session_manager.append_message(assistant_from(&runtime.get_model("faux", "large").unwrap(), "hello")).unwrap();
    let result = open_session(runtime, dir, session_manager.clone(), model).await;
    (result.session, session_manager)
}

#[tokio::test]
async fn restores_the_virtual_selection_instead_of_the_physical_model_that_answered() {
    let setup = create_runtime().await;
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = resume(&setup.runtime, dir.path(), None).await;
    let model = session.model().unwrap();
    assert_eq!((model.provider.as_str(), model.id.as_str()), ("router", "auto"));
    let routed = session.routed_model().unwrap().model;
    assert_eq!((routed.provider.as_str(), routed.id.as_str()), ("faux", "large"));
    session.dispose();
}

#[tokio::test]
async fn restores_a_virtual_selection_registered_right_before_the_session_opens() {
    let setup = create_runtime().await;
    let dir = tempfile::tempdir().unwrap();
    // Extensions register while the session is created, without waiting for the availability refresh.
    setup
        .runtime
        .register_virtual_model(&VirtualModelDefinition { provider: "late".into(), ..setup.definition.clone() })
        .unwrap();
    let session_manager = SessionManager::in_memory(Some(&dir.path().to_string_lossy()), None, None).unwrap();
    session_manager.append_model_change("late", "auto").unwrap();
    session_manager.append_message(UserMessage::new("hi", 1)).unwrap();
    session_manager.append_message(assistant_from(&setup.runtime.get_model("faux", "large").unwrap(), "hello")).unwrap();

    let result = open_session(&setup.runtime, dir.path(), session_manager, None).await;
    assert_eq!(result.model_fallback_message, None);
    let model = result.session.model().unwrap();
    assert_eq!((model.provider.as_str(), model.id.as_str()), ("late", "auto"));
    result.session.dispose();
}

#[tokio::test]
async fn falls_back_to_the_physical_model_when_the_virtual_model_is_not_registered() {
    let setup = create_runtime().await;
    let dir = tempfile::tempdir().unwrap();
    setup.runtime.unregister_virtual_model("router", "auto");
    let (session, _) = resume(&setup.runtime, dir.path(), None).await;
    let model = session.model().unwrap();
    assert_eq!((model.provider.as_str(), model.id.as_str()), ("faux", "large"));
    assert!(session.routed_model().is_none());
    session.dispose();
}

#[tokio::test]
async fn falls_back_to_the_last_physical_response_when_the_transcript_ends_with_a_routing_failure() {
    let setup = create_runtime().await;
    let dir = tempfile::tempdir().unwrap();
    let session_manager = SessionManager::in_memory(Some(&dir.path().to_string_lossy()), None, None).unwrap();
    session_manager.append_model_change("router", "auto").unwrap();
    session_manager.append_message(UserMessage::new("hi", 1)).unwrap();
    session_manager.append_message(assistant_from(&setup.runtime.get_model("faux", "large").unwrap(), "hello")).unwrap();
    session_manager.append_message(UserMessage::new("again", 2)).unwrap();
    session_manager
        .append_message(AssistantMessage {
            stop_reason: StopReason::Error,
            error_message: Some("router failed".into()),
            ..assistant_from(&setup.virtual_model, "")
        })
        .unwrap();
    setup.runtime.unregister_virtual_model("router", "auto");

    let result = open_session(&setup.runtime, dir.path(), session_manager, None).await;
    let model = result.session.model().unwrap();
    assert_eq!((model.provider.as_str(), model.id.as_str()), ("faux", "large"));
    assert_eq!(result.model_fallback_message, None);
    result.session.dispose();
}

#[tokio::test]
async fn resumes_the_selection_made_before_tree_navigation_left_its_model_change_on_another_branch() {
    let setup = create_runtime().await;
    let dir = tempfile::tempdir().unwrap();
    setup.faux.set_responses((0..6).map(|_| FauxResponseStep::from(faux_text_message("ok"))).collect());
    let large = setup.runtime.get_model("faux", "large").unwrap();
    for (before, after) in [(setup.virtual_model.clone(), large.clone()), (large.clone(), setup.virtual_model.clone())] {
        let session_manager = SessionManager::in_memory(Some(&dir.path().to_string_lossy()), None, None).unwrap();
        let session = open_session(&setup.runtime, dir.path(), session_manager.clone(), Some(before)).await.session;
        session.prompt("one", None).await.unwrap();
        let first_answer = session_manager.get_leaf_id().unwrap();
        session.set_model(after.clone(), None).await.unwrap();
        session.prompt("two", None).await.unwrap();
        // Navigating back to before the switch keeps `after` selected, but its model_change is on the old branch.
        session.navigate_tree(&first_answer, None).await.unwrap();
        session.prompt("three", None).await.unwrap();
        session.dispose();

        let resumed = open_session(&setup.runtime, dir.path(), session_manager, None).await.session;
        let model = resumed.model().unwrap();
        assert_eq!((model.provider.as_str(), model.id.as_str()), (after.provider.as_str(), after.id.as_str()));
        resumed.dispose();
    }
}

fn model_changes(session_manager: &SessionManager) -> Vec<(String, String)> {
    session_manager
        .get_branch(None)
        .into_iter()
        .filter_map(|entry| {
            let value = serde_json::to_value(&entry).unwrap();
            (value["type"] == "model_change").then(|| {
                (value["provider"].as_str().unwrap().to_string(), value["modelId"].as_str().unwrap().to_string())
            })
        })
        .collect()
}

#[tokio::test]
async fn does_not_record_a_physical_selection_on_every_prompt_while_requests_are_redirected() {
    let setup = create_runtime().await;
    let dir = tempfile::tempdir().unwrap();
    setup.faux.set_responses(vec![faux_text_message("ok").into(), faux_text_message("ok").into()]);
    let small = setup.runtime.get_model("faux", "small").unwrap();
    let session_manager = SessionManager::in_memory(Some(&dir.path().to_string_lossy()), None, None).unwrap();
    let session = open_session(&setup.runtime, dir.path(), session_manager.clone(), setup.runtime.get_model("faux", "large"))
        .await
        .session;
    let agent = session.agent();
    let prepare_request = agent.prepare_request().expect("session installs prepareRequest");
    agent.set_prepare_request(Some(Arc::new(move |request, signal| {
        let prepare_request = prepare_request.clone();
        let small = small.clone();
        async move {
            let mut update = prepare_request(request, signal).await?.unwrap_or_default();
            update.model = Some(small);
            Ok(Some(update))
        }
        .boxed()
    })));
    let initial = model_changes(&session_manager).len();

    session.prompt("one", None).await.unwrap();
    session.prompt("two", None).await.unwrap();

    let models: Vec<String> = session
        .messages()
        .iter()
        .filter_map(|m| match m {
            pi_agent::types::AgentMessage::Assistant(a) => Some(a.model.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(models, vec!["small", "small"]);
    assert_eq!(model_changes(&session_manager).len(), initial);
    session.dispose();
}

#[tokio::test]
async fn records_an_explicit_model_override_on_resume_with_the_next_prompt() {
    let setup = create_runtime().await;
    let dir = tempfile::tempdir().unwrap();
    setup.faux.set_responses(vec![faux_text_message("ok").into()]);
    let (session, session_manager) = resume(&setup.runtime, dir.path(), setup.runtime.get_model("faux", "small")).await;
    // Opening the session does not write to it.
    assert_eq!(model_changes(&session_manager).last().cloned(), Some(("router".to_string(), "auto".to_string())));

    session.prompt("again", None).await.unwrap();
    assert_eq!(model_changes(&session_manager).last().cloned(), Some(("faux".to_string(), "small".to_string())));
    session.dispose();
}
