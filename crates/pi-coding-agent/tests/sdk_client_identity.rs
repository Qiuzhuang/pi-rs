//! Port of `test/sdk-client-identity.test.ts`: the client identity and OpenCode session headers
//! `createAgentSession` adds to provider requests.
//!
//! The tests read and set `PI_TELEMETRY`, so they run one at a time.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_ai::auth::types::{ApiKeyCredential, Credential};
use pi_ai::*;
use pi_coding_agent::config::VERSION;
use pi_coding_agent::core::auth_storage::AuthStorage;
use pi_coding_agent::core::extensions::types::ProviderConfig;
use pi_coding_agent::core::model_registry::{ModelRegistry, ModelRegistryRegisterProviderProviderOrName};
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, create_agent_session};
use pi_coding_agent::core::session_manager::{NewSessionOptions, SessionManager};
use pi_coding_agent::core::settings_manager::SettingsManager;
use serde_json::json;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn create_model(provider: &str, base_url: &str) -> Model {
    serde_json::from_value(json!({
        "id": format!("{provider}-test-model"), "name": format!("{provider} Test Model"),
        "api": "openai-completions", "provider": provider, "baseUrl": base_url, "reasoning": false,
        "input": ["text"], "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
        "contextWindow": 128000, "maxTokens": 4096
    }))
    .unwrap()
}

fn done_stream() -> AssistantMessageEventStream {
    let (sender, stream) = AssistantMessageEventStream::new();
    let message: AssistantMessage = serde_json::from_value(json!({
        "role": "assistant", "content": [{ "type": "text", "text": "ok" }], "api": "openai-completions",
        "provider": "capture-provider", "model": "capture-model",
        "usage": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 } },
        "stopReason": "stop", "timestamp": 0
    }))
    .unwrap();
    sender.end(Some(message));
    stream
}

#[derive(Default)]
struct CaptureOptions {
    telemetry_enabled: Option<bool>,
    provider_headers: Option<IndexMap<String, String>>,
    session_id: Option<String>,
}

async fn capture_options(model: &Model, options: CaptureOptions) -> Option<SimpleStreamOptions> {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("project");
    let agent_dir = temp.path().join("agent");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&agent_dir).unwrap();
    let cwd = cwd.to_string_lossy().into_owned();
    let agent_dir = agent_dir.to_string_lossy().into_owned();

    let settings_manager = SettingsManager::create(&cwd, Some(&agent_dir), None);
    if options.telemetry_enabled == Some(false) {
        settings_manager.set_enable_install_telemetry(false);
    }
    let credentials: IndexMap<String, Credential> = [(
        model.provider.clone(),
        Credential::ApiKey(ApiKeyCredential { key: Some("test-api-key".into()), env: None }),
    )]
    .into_iter()
    .collect();
    let auth_storage = AuthStorage::in_memory(Some(&credentials));
    let model_runtime = ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(auth_storage),
        models_path: Some(String::new()),
        allow_model_network: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap();
    let registry = ModelRegistry::new(model_runtime.clone());
    let captured: Arc<Mutex<Option<SimpleStreamOptions>>> = Arc::new(Mutex::new(None));
    let sink = captured.clone();
    registry
        .register_provider(
            &ModelRegistryRegisterProviderProviderOrName::String(model.provider.clone()),
            Some(&ProviderConfig {
                api: Some(model.api.clone()),
                headers: options.provider_headers.clone(),
                stream_simple: Some(Arc::new(move |_model, _context, provider_options| {
                    *sink.lock() = provider_options;
                    done_stream()
                })),
                ..Default::default()
            }),
        )
        .unwrap();

    let session_manager = SessionManager::in_memory(Some(&cwd), None, None).unwrap();
    if let Some(id) = &options.session_id {
        session_manager.new_session(Some(&NewSessionOptions { id: Some(id.clone()), parent_session: None })).unwrap();
    }
    let result = create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(cwd.clone()),
        agent_dir: Some(agent_dir.clone()),
        model: Some(model.clone()),
        model_runtime: Some(model_runtime),
        settings_manager: Some(settings_manager),
        session_manager: Some(session_manager),
        ..Default::default()
    }))
    .await
    .unwrap();
    let session = result.session;
    let stream_fn = session.agent().stream_fn().expect("session stream function");
    let stream = stream_fn(
        model.clone(),
        normalize_context(&Context::default()),
        SimpleStreamOptions { session_id: Some(session.session_id()), ..Default::default() },
    )
    .await
    .unwrap();
    stream.result().await;
    session.dispose();
    registry.unregister_provider(&model.provider);
    captured.lock().clone()
}

fn target(model: &Model) -> ClientIdentityTarget<'_> {
    ClientIdentityTarget { provider: &model.provider, base_url: &model.base_url }
}

#[tokio::test]
async fn identifies_pi_with_a_detailed_user_agent_and_attribution_as_a_cli_agent() {
    let _guard = ENV_LOCK.lock().await;
    let saved = std::env::var("PI_TELEMETRY").ok();
    unsafe { std::env::remove_var("PI_TELEMETRY") };
    let model = create_model("openrouter", "https://openrouter.ai/api/v1");
    let options = capture_options(&model, CaptureOptions::default()).await.unwrap();
    if let Some(saved) = saved {
        unsafe { std::env::set_var("PI_TELEMETRY", saved) };
    }

    assert_eq!(
        options.identity,
        Some(ClientIdentity {
            user_agent: Some(UserAgentMode::Named(UserAgentNamed::Detailed)),
            version: Some(VERSION.to_string()),
            attribution: Some(Some(pi_attribution())),
        })
    );
    let headers = get_attribution_headers(target(&model), options.identity.as_ref());
    let expected: IndexMap<String, String> = [
        ("HTTP-Referer", "https://pi.dev"),
        ("X-OpenRouter-Title", "pi"),
        ("X-OpenRouter-Categories", "cli-agent"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert_eq!(headers, expected);
}

#[tokio::test]
async fn turns_attribution_off_when_telemetry_is_disabled() {
    let _guard = ENV_LOCK.lock().await;
    let saved = std::env::var("PI_TELEMETRY").ok();
    unsafe { std::env::remove_var("PI_TELEMETRY") };
    let model = create_model("nvidia", "https://integrate.api.nvidia.com/v1");
    let options = capture_options(&model, CaptureOptions { telemetry_enabled: Some(false), ..Default::default() })
        .await
        .unwrap();
    if let Some(saved) = saved {
        unsafe { std::env::set_var("PI_TELEMETRY", saved) };
    }

    assert_eq!(
        options.identity,
        Some(ClientIdentity {
            user_agent: Some(UserAgentMode::Named(UserAgentNamed::Detailed)),
            version: Some(VERSION.to_string()),
            attribution: Some(None),
        })
    );
    assert!(get_attribution_headers(target(&model), options.identity.as_ref()).is_empty());
}

#[tokio::test]
async fn turns_attribution_off_with_pi_telemetry_0() {
    let _guard = ENV_LOCK.lock().await;
    let saved = std::env::var("PI_TELEMETRY").ok();
    unsafe { std::env::set_var("PI_TELEMETRY", "0") };
    let options = capture_options(&create_model("openrouter", "https://openrouter.ai/api/v1"), CaptureOptions::default())
        .await
        .unwrap();
    match saved {
        Some(saved) => unsafe { std::env::set_var("PI_TELEMETRY", saved) },
        None => unsafe { std::env::remove_var("PI_TELEMETRY") },
    }
    assert_eq!(options.identity.and_then(|i| i.attribution), Some(None));
}

#[tokio::test]
async fn adds_opencode_session_headers_for_custom_opencode_providers() {
    let _guard = ENV_LOCK.lock().await;
    let options = capture_options(
        &create_model("custom-opencode", "https://opencode.ai/zen/v1"),
        CaptureOptions { session_id: Some("opencode-session".into()), ..Default::default() },
    )
    .await
    .unwrap();
    assert_eq!(
        options.headers.and_then(|h| h.get("x-opencode-session").cloned()),
        Some(Some("opencode-session".to_string()))
    );
}

#[tokio::test]
async fn lets_configured_opencode_headers_override_the_session_header() {
    let _guard = ENV_LOCK.lock().await;
    let options = capture_options(
        &create_model("opencode", "https://opencode.ai/zen/v1"),
        CaptureOptions {
            session_id: Some("opencode-session".into()),
            provider_headers: Some([("x-opencode-session".to_string(), "configured-session".to_string())].into_iter().collect()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        options.headers.and_then(|h| h.get("x-opencode-session").cloned()),
        Some(Some("configured-session".to_string()))
    );
}
