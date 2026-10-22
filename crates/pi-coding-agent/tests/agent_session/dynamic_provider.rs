//! Port of `test/agent-session-dynamic-provider.test.ts`.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_ai::AnyModel;
use pi_ai::auth::types::{
    ApiKeyAuth, ApiKeyCredential, ApiKeyResolveInput, AuthResult, Credential, ModelAuth, ProviderAuth,
};
use pi_ai::models::{CreateProviderOptions, ProviderApi, ProviderStreams, create_provider};
use pi_ai::types::{Model, StreamOptions, TranscriptContext};
use pi_coding_agent::core::agent_session::{AgentSession, ExtensionBindings};
use pi_coding_agent::core::auth_storage::AuthStorage;
use pi_coding_agent::core::extensions::types::{
    CommandOptions, ExtensionAPIRef, ExtensionFactory, InlineExtension, ProviderConfig,
    ProviderRegistration, SessionStartEvent, on_event,
};
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use pi_coding_agent::core::resource_loader::{
    DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader,
};
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, create_agent_session};
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::core::settings_manager::SettingsManager;

struct StaticKey;

#[async_trait::async_trait]
impl ApiKeyAuth for StaticKey {
    fn name(&self) -> &str {
        "Test API key"
    }
    async fn resolve(&self, _input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        Ok(Some(AuthResult {
            auth: ModelAuth {
                api_key: Some("test-key".into()),
                headers: None,
                base_url: None,
            },
            env: None,
            source: Some("test".into()),
        }))
    }
}

struct Unused;

impl ProviderStreams for Unused {
    fn stream(&self, _m: &Model, _c: &TranscriptContext, _o: StreamOptions) -> pi_ai::AssistantMessageEventStream {
        panic!("unused")
    }
    fn stream_simple(&self, _m: &Model, _c: &TranscriptContext, _o: StreamOptions) -> pi_ai::AssistantMessageEventStream {
        panic!("unused")
    }
}

fn sonnet() -> Model {
    pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5").expect("catalog model")
}

fn native_anthropic_provider(base_url: &str) -> ProviderRegistration {
    let mut model = sonnet();
    model.base_url = base_url.to_string();
    ProviderRegistration::Native(
        create_provider(CreateProviderOptions {
            id: "anthropic".into(),
            name: Some("Native Anthropic".into()),
            base_url: Some(base_url.into()),
            auth: ProviderAuth {
                api_key: Some(Arc::new(StaticKey)),
                oauth: None,
            },
            models: vec![AnyModel::Chat(model)],
            api: Some(ProviderApi::Single(Arc::new(Unused))),
            ..Default::default()
        })
        .unwrap(),
    )
}

fn base_url_override(base_url: &str) -> ProviderRegistration {
    ProviderRegistration::Config {
        name: "anthropic".into(),
        config: ProviderConfig {
            base_url: Some(base_url.into()),
            ..Default::default()
        },
    }
}

struct Setup {
    session: Arc<AgentSession>,
    temp_dir: String,
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.session.dispose();
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

async fn create_session(setup: impl Fn(ExtensionAPIRef) -> anyhow::Result<()> + Send + Sync + 'static) -> Setup {
    let temp_dir = std::env::temp_dir()
        .join(format!("pi-dynamic-provider-test-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();
    let agent_dir = format!("{temp_dir}/agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    let settings_manager = SettingsManager::create(&temp_dir, Some(&agent_dir), None);
    let session_manager = SessionManager::in_memory(Some(&temp_dir), None, None).unwrap();
    let mut credentials: IndexMap<String, Credential> = IndexMap::new();
    credentials.insert(
        "anthropic".into(),
        Credential::ApiKey(ApiKeyCredential {
            key: Some("test-key".into()),
            env: None,
        }),
    );
    let model_runtime = ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(AuthStorage::in_memory(Some(&credentials))),
        models_path: Some(format!("{agent_dir}/models.json")),
        allow_model_network: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap();
    let setup = Arc::new(setup);
    let factory: ExtensionFactory = Arc::new(move |pi| {
        let setup = setup.clone();
        Box::pin(async move { setup(pi) })
    });
    let resource_loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: temp_dir.clone(),
        agent_dir: agent_dir.clone(),
        settings_manager: Some(settings_manager.clone()),
        extension_factories: Some(vec![InlineExtension::Factory(factory)]),
        ..Default::default()
    }));
    resource_loader.reload(None).await.unwrap();

    let result = create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(temp_dir.clone()),
        agent_dir: Some(agent_dir),
        model: Some(sonnet()),
        settings_manager: Some(settings_manager),
        session_manager: Some(session_manager),
        model_runtime: Some(model_runtime),
        resource_loader: Some(resource_loader as Arc<dyn ResourceLoader>),
        ..Default::default()
    }))
    .await
    .unwrap();
    Setup {
        session: result.session,
        temp_dir,
    }
}

async fn capture_prompt_base_url(session: &AgentSession) -> Option<String> {
    let base_url = Arc::new(Mutex::new(None::<String>));
    let b = base_url.clone();
    session.agent().set_stream_fn(Some(Arc::new(move |model, _context, _options| {
        *b.lock() = Some(model.base_url.clone());
        Box::pin(async { Err(anyhow::anyhow!("stop")) })
    })));
    session.prompt("hello", None).await.unwrap();
    base_url.lock().clone()
}

fn model_base_url(session: &AgentSession) -> Option<String> {
    session.model().map(|m| m.base_url)
}

fn register_command(
    pi: &ExtensionAPIRef,
    name: &str,
    registration: impl Fn() -> ProviderRegistration + Send + Sync + 'static,
) -> anyhow::Result<()> {
    let api = pi.clone();
    let registration = Arc::new(registration);
    pi.register_command(
        name,
        CommandOptions {
            description: Some(name.into()),
            get_argument_completions: None,
            handler: Arc::new(move |_args, _ctx| {
                let api = api.clone();
                let registration = registration.clone();
                Box::pin(async move { api.register_provider(registration()) })
            }),
        },
    )
}

#[tokio::test]
async fn applies_top_level_register_provider_overrides_to_the_active_model() {
    let s = create_session(|pi| pi.register_provider(base_url_override("http://localhost:8080/top-level"))).await;

    assert_eq!(model_base_url(&s.session).as_deref(), Some("http://localhost:8080/top-level"));
    assert_eq!(capture_prompt_base_url(&s.session).await.as_deref(), Some("http://localhost:8080/top-level"));
}

#[tokio::test]
async fn applies_session_start_register_provider_overrides_to_the_active_model() {
    let s = create_session(|pi| {
        let api = pi.clone();
        on_event::<SessionStartEvent, _, _>(&*pi, move |_event, _ctx| {
            let api = api.clone();
            async move {
                api.register_provider(base_url_override("http://localhost:8080/session-start"))?;
                Ok(None)
            }
        })?;
        Ok(())
    })
    .await;

    s.session.bind_extensions(&ExtensionBindings::default()).await.unwrap();

    assert_eq!(model_base_url(&s.session).as_deref(), Some("http://localhost:8080/session-start"));
    assert_eq!(capture_prompt_base_url(&s.session).await.as_deref(), Some("http://localhost:8080/session-start"));
}

#[tokio::test]
async fn registers_native_pi_ai_providers_during_extension_loading() {
    let s = create_session(|pi| pi.register_provider(native_anthropic_provider("http://localhost:8080/native-top-level"))).await;

    assert_eq!(model_base_url(&s.session).as_deref(), Some("http://localhost:8080/native-top-level"));
    assert_eq!(
        capture_prompt_base_url(&s.session).await.as_deref(),
        Some("http://localhost:8080/native-top-level")
    );
}

#[tokio::test]
async fn applies_command_time_register_provider_overrides_without_reload() {
    let s = create_session(|pi| {
        register_command(&pi, "use-proxy", || base_url_override("http://localhost:8080/command"))
    })
    .await;

    s.session.bind_extensions(&ExtensionBindings::default()).await.unwrap();
    s.session.prompt("/use-proxy", None).await.unwrap();

    assert_eq!(model_base_url(&s.session).as_deref(), Some("http://localhost:8080/command"));
    assert_eq!(capture_prompt_base_url(&s.session).await.as_deref(), Some("http://localhost:8080/command"));
}

#[tokio::test]
async fn registers_native_pi_ai_providers_at_command_time() {
    let s = create_session(|pi| {
        register_command(&pi, "use-native", || native_anthropic_provider("http://localhost:8080/native-command"))
    })
    .await;

    s.session.bind_extensions(&ExtensionBindings::default()).await.unwrap();
    s.session.prompt("/use-native", None).await.unwrap();

    assert_eq!(model_base_url(&s.session).as_deref(), Some("http://localhost:8080/native-command"));
    assert_eq!(
        capture_prompt_base_url(&s.session).await.as_deref(),
        Some("http://localhost:8080/native-command")
    );
}
