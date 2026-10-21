//! The global compatibility API (port of `compat.ts`): api-dispatch `stream()`/`complete()`
//! with env API key injection, the api-registry, and generated catalog reads.
//!
//! New code uses [`crate::models::create_models`] and the provider factories.

use std::sync::Arc;

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

use crate::api::lazy::error_stream;
use crate::api::registry::builtin_api;
use crate::models::{CreateModelsOptions, Models, ProviderStreams};
use crate::providers::all::builtin_models;
use crate::providers::faux::{FauxCore, FauxProviderState, FauxResponseStep, FauxStreams, RegisterFauxProviderOptions};
use crate::types::{
    AssistantMessage, AssistantMessageEventStream, Context, DeferredHandle, Model, StreamOptions, TranscriptContext,
};
use crate::utils::transcript::normalize_context;

pub use crate::env_api_keys::*;
pub use crate::image_models::*;
pub use crate::providers::all::{
    get_builtin_model as get_model, get_builtin_models as get_models, get_builtin_providers as get_providers,
};

/// A registered API implementation (`ApiProvider`).
#[derive(Clone)]
pub struct ApiProvider {
    pub api: String,
    pub streams: Arc<dyn ProviderStreams>,
}

struct RegisteredApiProvider {
    provider: ApiProvider,
    source_id: Option<String>,
}

static API_PROVIDER_REGISTRY: Lazy<RwLock<IndexMap<String, RegisteredApiProvider>>> = Lazy::new(|| {
    let registry = RwLock::new(IndexMap::new());
    register_builtins_into(&registry);
    registry
});

static BUILTIN_INSTANCES: Lazy<RwLock<IndexMap<String, Arc<dyn ProviderStreams>>>> = Lazy::new(|| RwLock::new(IndexMap::new()));

const BUILTIN_APIS: [&str; 10] = [
    "anthropic-messages",
    "openai-completions",
    "openai-responses",
    "openai-codex-responses",
    "azure-openai-responses",
    "google-generative-ai",
    "google-vertex",
    "mistral-conversations",
    "bedrock-converse-stream",
    "pi-messages",
];

fn register_builtins_into(registry: &RwLock<IndexMap<String, RegisteredApiProvider>>) {
    let mut registry = registry.write();
    let mut instances = BUILTIN_INSTANCES.write();
    for api in BUILTIN_APIS {
        if !registry.contains_key(api) {
            registry.insert(
                api.to_string(),
                RegisteredApiProvider { provider: ApiProvider { api: api.to_string(), streams: builtin_api(api) }, source_id: None },
            );
        }
        if let Some(entry) = registry.get(api) {
            instances.insert(api.to_string(), entry.provider.streams.clone());
        }
    }
}

/// Wraps an implementation so a model with a different api fails (`wrapStream`).
struct ApiChecked {
    api: String,
    inner: Arc<dyn ProviderStreams>,
}

impl ApiChecked {
    fn mismatch(&self, model: &Model) -> Option<AssistantMessageEventStream> {
        if model.api != self.api {
            return Some(error_stream(model, anyhow::anyhow!("Mismatched api: {} expected {}", model.api, self.api)));
        }
        None
    }
}

impl ProviderStreams for ApiChecked {
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.mismatch(model).unwrap_or_else(|| self.inner.stream(model, context, options))
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.mismatch(model).unwrap_or_else(|| self.inner.stream_simple(model, context, options))
    }
    fn supports_fetch_deferred(&self) -> bool {
        self.inner.supports_fetch_deferred()
    }
    fn fetch_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> AssistantMessageEventStream {
        self.inner.fetch_deferred(model, handle, options)
    }
    fn supports_cancel_deferred(&self) -> bool {
        self.inner.supports_cancel_deferred()
    }
    fn cancel_deferred(
        &self,
        model: &Model,
        handle: &DeferredHandle,
        options: StreamOptions,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<()>> {
        self.inner.cancel_deferred(model, handle, options)
    }
}

/// Register an API implementation for `api` (replaces any existing registration).
pub fn register_api_provider(api: &str, streams: Arc<dyn ProviderStreams>, source_id: Option<&str>) {
    let checked: Arc<dyn ProviderStreams> = Arc::new(ApiChecked { api: api.to_string(), inner: streams });
    API_PROVIDER_REGISTRY.write().insert(
        api.to_string(),
        RegisteredApiProvider {
            provider: ApiProvider { api: api.to_string(), streams: checked },
            source_id: source_id.map(str::to_string),
        },
    );
}

/// The implementation registered for `api`.
pub fn get_api_provider(api: &str) -> Option<ApiProvider> {
    API_PROVIDER_REGISTRY.read().get(api).map(|e| e.provider.clone())
}

pub fn get_api_providers() -> Vec<ApiProvider> {
    API_PROVIDER_REGISTRY.read().values().map(|e| e.provider.clone()).collect()
}

/// Remove every registration made with `source_id`.
pub fn unregister_api_providers(source_id: &str) {
    API_PROVIDER_REGISTRY.write().retain(|_, entry| entry.source_id.as_deref() != Some(source_id));
}

/// Register the built-in API implementations without clobbering existing entries.
pub fn register_builtin_api_providers() {
    register_builtins_into(&API_PROVIDER_REGISTRY);
}

/// Clear all registrations and re-register the built-ins.
pub fn reset_api_providers() {
    API_PROVIDER_REGISTRY.write().clear();
    BUILTIN_INSTANCES.write().clear();
    register_builtin_api_providers();
}

/// A faux provider registered in the global api-registry.
pub struct FauxProviderRegistration {
    pub core: Arc<FauxCore>,
    source_id: String,
}

impl FauxProviderRegistration {
    pub fn api(&self) -> &str {
        &self.core.api
    }
    pub fn models(&self) -> &[Model] {
        &self.core.models
    }
    pub fn get_model(&self) -> Model {
        self.core.get_model(None).expect("faux provider has a model")
    }
    pub fn get_model_by_id(&self, id: &str) -> Option<Model> {
        self.core.get_model(Some(id))
    }
    pub fn state(&self) -> FauxProviderState {
        self.core.state()
    }
    pub fn set_responses(&self, responses: Vec<FauxResponseStep>) {
        self.core.set_responses(responses)
    }
    pub fn append_responses(&self, responses: Vec<FauxResponseStep>) {
        self.core.append_responses(responses)
    }
    pub fn get_pending_response_count(&self) -> usize {
        self.core.get_pending_response_count()
    }
    pub fn unregister(&self) {
        unregister_api_providers(&self.source_id);
    }
}

/// Register a faux provider in the global api-registry.
pub fn register_faux_provider(options: RegisterFauxProviderOptions) -> FauxProviderRegistration {
    let core = FauxCore::new(&options);
    let source_id = format!("faux-provider-{}", crate::utils::uuid::uuidv7());
    register_api_provider(&core.api.clone(), Arc::new(FauxStreams(core.clone())), Some(&source_id));
    FauxProviderRegistration { core, source_id }
}

static COMPAT_MODELS: Lazy<Models> = Lazy::new(|| builtin_models(CreateModelsOptions::default()));

fn has_explicit_api_key(api_key: Option<&str>) -> bool {
    api_key.is_some_and(|k| !k.trim().is_empty())
}

fn with_env_api_key(model: &Model, mut options: StreamOptions) -> StreamOptions {
    if has_explicit_api_key(options.api_key.as_deref()) {
        return options;
    }
    if let Some(key) = get_env_api_key(&model.provider, options.env.as_ref()).filter(|k| k != AUTHENTICATED_MARKER) {
        options.api_key = Some(key);
    }
    options
}

fn has_resolved_cloudflare_auth(options: &StreamOptions) -> bool {
    has_explicit_api_key(options.api_key.as_deref())
        || options.headers.as_ref().is_some_and(|h| h.get("cf-aig-authorization").is_some_and(|v| v.is_some()))
}

fn builtin_provider_for(model: &Model) -> Option<Arc<dyn crate::models::Provider>> {
    let registered = get_api_provider(&model.api)?;
    let builtin = BUILTIN_INSTANCES.read().get(&model.api).cloned()?;
    if !Arc::ptr_eq(&registered.streams, &builtin) {
        return None;
    }
    let provider = COMPAT_MODELS.get_provider(&model.provider)?;
    provider.get_models().iter().any(|m| m.api == model.api).then_some(provider)
}

fn context_of(transcript: &TranscriptContext) -> Context {
    Context { system_prompt: None, messages: transcript.messages.clone(), tools: None }
}

fn dispatch(model: &Model, context: &Context, options: StreamOptions, simple: bool) -> AssistantMessageEventStream {
    let transcript = normalize_context(context);
    if let Some(provider) = builtin_provider_for(model) {
        if model.provider.starts_with("cloudflare-") && !has_resolved_cloudflare_auth(&options) {
            let ctx = context_of(&transcript);
            return if simple {
                COMPAT_MODELS.stream_simple(model, &ctx, options)
            } else {
                COMPAT_MODELS.stream(model, &ctx, options)
            };
        }
        let options = with_env_api_key(model, options);
        return if simple { provider.stream_simple(model, &transcript, options) } else { provider.stream(model, &transcript, options) };
    }
    let Some(provider) = get_api_provider(&model.api) else {
        return error_stream(model, anyhow::anyhow!("No API provider registered for api: {}", model.api));
    };
    let options = with_env_api_key(model, options);
    if simple {
        provider.streams.stream_simple(model, &transcript, options)
    } else {
        provider.streams.stream(model, &transcript, options)
    }
}

/// Stream through the registered API implementation for `model.api`.
pub fn stream(model: &Model, context: &Context, options: StreamOptions) -> AssistantMessageEventStream {
    dispatch(model, context, options, false)
}

pub async fn complete(model: &Model, context: &Context, options: StreamOptions) -> AssistantMessage {
    stream(model, context, options).result().await
}

/// Stream with provider-neutral reasoning options.
pub fn stream_simple(model: &Model, context: &Context, options: StreamOptions) -> AssistantMessageEventStream {
    dispatch(model, context, options, true)
}

pub async fn complete_simple(model: &Model, context: &Context, options: StreamOptions) -> AssistantMessage {
    stream_simple(model, context, options).result().await
}
