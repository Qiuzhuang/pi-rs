//! Port of `test/sdk-stream-options.test.ts`.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::auth::types::{ApiKeyCredential, Credential};
use pi_ai::types::{AssistantMessage, Context, Model, SimpleStreamOptions};
use pi_coding_agent::core::agent_session::AgentSession;
use pi_coding_agent::core::auth_storage::AuthStorage;
use pi_coding_agent::core::cache_warmer::CacheWarmingStatusState;
use pi_coding_agent::core::extensions::types::{
    BeforeProviderHeadersEvent, BeforeProviderHeadersEventResult, ExtensionAPIRef, ExtensionFactory,
    InlineExtension, ProviderConfig, ProviderStreamEvent, on_event,
};
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use pi_coding_agent::core::resource_loader::{
    DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader,
};
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, create_agent_session};
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::core::settings_manager::SettingsManager;
use serde_json::{Value, json};

struct Dirs {
    temp_dir: String,
    cwd: String,
    agent_dir: String,
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

fn dirs() -> Dirs {
    let temp_dir = std::env::temp_dir()
        .join(format!("pi-sdk-stream-options-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();
    let cwd = format!("{temp_dir}/project");
    let agent_dir = format!("{temp_dir}/agent");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&agent_dir).unwrap();
    Dirs { temp_dir, cwd, agent_dir }
}

fn create_model(api: &str) -> Model {
    serde_json::from_value(json!({
        "id": "capture-model",
        "name": "Capture Model",
        "api": api,
        "provider": "capture-provider",
        "baseUrl": "https://capture.invalid/v1",
        "reasoning": false,
        "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": 128000,
        "maxTokens": 4096,
        "headers": {"x-model": "model"}
    }))
    .unwrap()
}

fn done_message(api: &str, prompt_tokens: u64) -> AssistantMessage {
    serde_json::from_value(json!({
        "role": "assistant",
        "content": [{"type": "text", "text": "ok"}],
        "api": api,
        "provider": "capture-provider",
        "model": "capture-model",
        "usage": {"input": 0, "output": 0, "cacheRead": prompt_tokens, "cacheWrite": 0, "totalTokens": prompt_tokens,
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}},
        "stopReason": "stop",
        "timestamp": chrono::Utc::now().timestamp_millis()
    }))
    .unwrap()
}

fn done_stream(api: &str, prompt_tokens: u64) -> pi_ai::AssistantMessageEventStream {
    let (sender, stream) = pi_ai::AssistantMessageEventStream::new();
    sender.end(Some(done_message(api, prompt_tokens)));
    stream
}

async fn model_runtime(dirs: &Dirs, provider: &str) -> Arc<ModelRuntime> {
    let mut credentials: IndexMap<String, Credential> = IndexMap::new();
    credentials.insert(
        provider.into(),
        Credential::ApiKey(ApiKeyCredential {
            key: Some("test-api-key".into()),
            env: None,
        }),
    );
    ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(AuthStorage::in_memory(Some(&credentials))),
        models_path: Some(format!("{}/models.json", dirs.agent_dir)),
        allow_model_network: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap()
}

fn settings(value: Value) -> Arc<SettingsManager> {
    SettingsManager::in_memory_json(value.as_object().cloned().unwrap_or_default(), None)
}

async fn create_session(
    dirs: &Dirs,
    model: &Model,
    runtime: Arc<ModelRuntime>,
    settings_manager: Arc<SettingsManager>,
    extension: Option<Arc<dyn Fn(ExtensionAPIRef) -> anyhow::Result<()> + Send + Sync>>,
    session_manager: Option<Arc<SessionManager>>,
) -> Arc<AgentSession> {
    let mut extensions = Vec::new();
    if let Some(setup) = extension {
        let factory: ExtensionFactory = Arc::new(move |pi| {
            let setup = setup.clone();
            Box::pin(async move { setup(pi) })
        });
        extensions.push(InlineExtension::Factory(factory));
    }
    let resource_loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: dirs.cwd.clone(),
        agent_dir: dirs.agent_dir.clone(),
        settings_manager: Some(settings_manager.clone()),
        extension_factories: Some(extensions),
        ..Default::default()
    }));
    resource_loader.reload(None).await.unwrap();
    create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(dirs.cwd.clone()),
        agent_dir: Some(dirs.agent_dir.clone()),
        model: Some(model.clone()),
        model_runtime: Some(runtime),
        settings_manager: Some(settings_manager),
        session_manager: Some(
            session_manager.unwrap_or_else(|| SessionManager::in_memory(Some(&dirs.cwd), None, None).unwrap()),
        ),
        resource_loader: Some(resource_loader as Arc<dyn ResourceLoader>),
        ..Default::default()
    }))
    .await
    .unwrap()
    .session
}

/// TS `captureStreamOptions`.
async fn capture_stream_options(
    api: &'static str,
    settings_json: Value,
    request_options: SimpleStreamOptions,
    extension: Option<Arc<dyn Fn(ExtensionAPIRef) -> anyhow::Result<()> + Send + Sync>>,
    provider_event: Option<Value>,
) -> Option<SimpleStreamOptions> {
    let d = dirs();
    let model = create_model(api);
    let runtime = model_runtime(&d, &model.provider).await;
    let captured = Arc::new(Mutex::new(None::<SimpleStreamOptions>));
    let c = captured.clone();
    let event = provider_event.clone();
    let mut headers = IndexMap::new();
    headers.insert("x-provider".to_string(), "provider".to_string());
    runtime
        .register_provider(
            &model.provider,
            &ProviderConfig {
                api: Some(api.to_string()),
                headers: Some(headers),
                stream_simple: Some(Arc::new(move |request_model, _context, options| {
                    *c.lock() = options.clone();
                    let Some(event) = event.clone() else {
                        return done_stream(api, 0);
                    };
                    let (sender, stream) = pi_ai::AssistantMessageEventStream::new();
                    tokio::spawn(async move {
                        if let Some(observer) = options.as_ref().and_then(|o| o.on_provider_stream_event.clone()) {
                            observer(event, request_model).await;
                        }
                        sender.end(Some(done_message(api, 0)));
                    });
                    stream
                })),
                ..Default::default()
            },
        )
        .unwrap();
    let session = create_session(&d, &model, runtime.clone(), settings(settings_json), extension, None).await;

    if provider_event.is_none() {
        let stream_fn = session.agent().stream_fn().unwrap();
        let context = pi_ai::normalize_context(&Context {
            system_prompt: None,
            messages: Vec::new(),
            tools: None,
        });
        let stream = stream_fn(model.clone(), context, request_options).await.unwrap();
        let _ = stream.result().await;
    } else {
        session.prompt("test", None).await.unwrap();
    }
    session.dispose();
    runtime.unregister_provider(&model.provider);
    
    captured.lock().clone()
}

async fn create_cache_warming_session(
    d: &Dirs,
    populate: Option<Box<dyn Fn(&SessionManager, &Model)>>,
) -> (Arc<AgentSession>, Arc<Mutex<usize>>, Arc<ModelRuntime>) {
    let mut model = create_model("anthropic-messages");
    model.cost = serde_json::from_value(json!({"input": 10, "output": 50, "cacheRead": 0.25, "cacheWrite": 12.5})).unwrap();
    model.prompt_cache = Some(serde_json::from_value(json!({"short": 300})).unwrap());
    let runtime = model_runtime(d, &model.provider).await;
    let calls = Arc::new(Mutex::new(0usize));
    let c = calls.clone();
    runtime
        .register_provider(
            &model.provider,
            &ProviderConfig {
                api: Some(model.api.clone()),
                stream_simple: Some(Arc::new(move |_model, _context, _options| {
                    *c.lock() += 1;
                    done_stream("anthropic-messages", 100_000)
                })),
                ..Default::default()
            },
        )
        .unwrap();
    let session_manager = SessionManager::in_memory(Some(&d.cwd), None, None).unwrap();
    if let Some(populate) = populate {
        populate(&session_manager, &model);
    }
    let session = create_session(
        d,
        &model,
        runtime.clone(),
        settings(json!({"cacheWarming": "idle"})),
        None,
        Some(session_manager),
    )
    .await;
    (session, calls, runtime)
}

fn now_ms() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64
}

#[tokio::test]
async fn schedules_cache_warming_after_a_completed_session_request() {
    let d = dirs();
    let (session, _calls, runtime) = create_cache_warming_session(&d, None).await;

    session.prompt("test", None).await.unwrap();
    assert!(session.cache_warming_status().unwrap().next_warm_at.unwrap() > now_ms());

    // Equivalent copies remain current, but removing the request prefix does not.
    let agent = session.agent();
    agent.set_messages(agent.messages());
    agent.set_model(agent.model());
    assert!(session.cache_warming_status().unwrap().next_warm_at.unwrap() > now_ms());
    let rest: Vec<AgentMessage> = agent.messages().into_iter().skip(1).collect();
    agent.set_messages(rest);
    assert_eq!(
        session.cache_warming_status().unwrap().reason.as_deref(),
        Some("conversation context changed")
    );
    session.dispose();
    runtime.unregister_provider("capture-provider");
}

#[tokio::test]
async fn waits_for_the_next_request_instead_of_restoring_cache_warming() {
    let d = dirs();
    let (session, calls, runtime) = create_cache_warming_session(
        &d,
        Some(Box::new(|manager, model| {
            manager.append_model_change(&model.provider, &model.id).unwrap();
            manager.append_thinking_level_change("off").unwrap();
            manager
                .append_message(AgentMessage::from_value(json!({"role": "user", "content": "test", "timestamp": now_ms() as i64 - 60_000})).unwrap())
                .unwrap();
            let mut assistant = done_message(&model.api, 100_000);
            assistant.timestamp = now_ms() as i64 - 59_000;
            let usage = assistant.usage.clone();
            manager.append_message(AgentMessage::Assistant(assistant)).unwrap();
            manager.append_usage("cache_warm", &model.provider, &model.id, usage, None).unwrap();
        })),
    )
    .await;

    assert_eq!(*calls.lock(), 0);
    let status = session.cache_warming_status().unwrap();
    assert_eq!(status.state, CacheWarmingStatusState::Inactive);
    assert_eq!(status.reason.as_deref(), Some("waiting for first request"));
    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        json!({"state": "inactive", "reason": "waiting for first request"})
    );
    session.dispose();
    runtime.unregister_provider("capture-provider");
}

#[tokio::test]
async fn forwards_http_idle_timeout_ms_as_timeout_ms_for_openai_codex() {
    let options = capture_stream_options("openai-codex-responses", json!({"httpIdleTimeoutMs": 1234}), Default::default(), None, None).await;
    assert_eq!(options.unwrap().timeout_ms, Some(1234));
}

#[tokio::test]
async fn defaults_timeout_ms_from_http_idle_timeout_ms_for_all_providers() {
    let options = capture_stream_options("openai-completions", json!({"httpIdleTimeoutMs": 1234}), Default::default(), None, None).await;
    assert_eq!(options.unwrap().timeout_ms, Some(1234));
}

#[tokio::test]
async fn lets_request_timeout_ms_override_http_idle_timeout_ms_for_openai_codex() {
    let options = capture_stream_options(
        "openai-codex-responses",
        json!({"httpIdleTimeoutMs": 1234}),
        SimpleStreamOptions {
            timeout_ms: Some(0),
            ..Default::default()
        },
        None,
        None,
    )
    .await;
    assert_eq!(options.unwrap().timeout_ms, Some(0));
}

#[tokio::test]
async fn forwards_websocket_connect_timeout_ms_from_settings() {
    let options =
        capture_stream_options("openai-codex-responses", json!({"websocketConnectTimeoutMs": 1234}), Default::default(), None, None).await;
    assert_eq!(options.unwrap().websocket_connect_timeout_ms, Some(1234));
}

#[tokio::test]
async fn lets_request_websocket_connect_timeout_ms_override_settings() {
    let options = capture_stream_options(
        "openai-codex-responses",
        json!({"websocketConnectTimeoutMs": 1234}),
        SimpleStreamOptions {
            websocket_connect_timeout_ms: Some(0),
            ..Default::default()
        },
        None,
        None,
    )
    .await;
    assert_eq!(options.unwrap().websocket_connect_timeout_ms, Some(0));
}

#[tokio::test]
async fn forwards_provider_retry_settings() {
    let options = capture_stream_options(
        "openai-completions",
        json!({"retry": {"provider": {"maxRetries": 2, "maxRetryDelayMs": 3000}}}),
        Default::default(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(options.max_retries, Some(2));
    assert_eq!(options.max_retry_delay_ms, Some(3000));
}

// Regression test for #9784.
#[tokio::test]
async fn forwards_provider_stream_events_to_extensions() {
    let provider_event = json!({"openrouter_metadata": {"strategy": "direct"}});
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let e = events.clone();
    let options = capture_stream_options(
        "openai-completions",
        json!({}),
        Default::default(),
        Some(Arc::new(move |pi| {
            let e = e.clone();
            on_event::<ProviderStreamEvent, _, _>(&*pi, move |event, _ctx| {
                let e = e.clone();
                async move {
                    e.lock().push(serde_json::to_value(
                        pi_coding_agent::core::extensions::types::ExtensionEvent::ProviderStream(event),
                    )?);
                    Ok(None)
                }
            })?;
            Ok(())
        })),
        Some(provider_event.clone()),
    )
    .await
    .unwrap();

    assert!(options.on_provider_stream_event.is_some());
    assert_eq!(
        *events.lock(),
        vec![json!({
            "data": provider_event,
            "type": "provider_stream_event",
            "provider": "capture-provider",
            "api": "openai-completions",
            "model": "capture-model",
        })]
    );
}

#[tokio::test]
async fn runs_before_provider_headers_on_assembled_headers_without_forwarding_the_transform() {
    let mut explicit = pi_ai::types::ProviderHeaders::new();
    explicit.insert("x-explicit".into(), Some("explicit".into()));
    let options = capture_stream_options(
        "openai-completions",
        json!({}),
        SimpleStreamOptions {
            headers: Some(explicit),
            ..Default::default()
        },
        Some(Arc::new(|pi| {
            on_event::<BeforeProviderHeadersEvent, _, _>(&*pi, |event, _ctx| async move {
                let mut headers = event.headers.clone();
                let get = |key: &str| headers.get(key).cloned().flatten().unwrap_or_default();
                let joined = [get("x-provider"), get("x-model"), get("x-explicit")].join(":");
                headers.insert("x-hook".into(), Some(joined));
                Ok(Some(BeforeProviderHeadersEventResult { headers }))
            })?;
            Ok(())
        })),
        None,
    )
    .await
    .unwrap();

    let headers = serde_json::to_value(options.headers.clone().unwrap()).unwrap();
    assert_eq!(headers["x-provider"], json!("provider"));
    assert_eq!(headers["x-model"], json!("model"));
    assert_eq!(headers["x-explicit"], json!("explicit"));
    assert_eq!(headers["x-hook"], json!("provider:model:explicit"));
}

/// TS: "Handlers are awaited in stream order, so slow handlers delay stream consumption."
#[tokio::test]
async fn awaits_slow_provider_stream_event_handlers_before_the_stream_continues() {
    let finished = Arc::new(Mutex::new(false));
    let f = finished.clone();
    capture_stream_options(
        "openai-completions",
        json!({}),
        Default::default(),
        Some(Arc::new(move |pi| {
            let f = f.clone();
            on_event::<ProviderStreamEvent, _, _>(&*pi, move |_event, _ctx| {
                let f = f.clone();
                async move {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    *f.lock() = true;
                    Ok(None)
                }
            })?;
            Ok(())
        })),
        Some(json!({"chunk": 1})),
    )
    .await
    .unwrap();

    // The provider only ends its stream after the observer resolved, so the prompt saw the handler finish.
    assert!(*finished.lock());
}
