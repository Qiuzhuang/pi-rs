//! OpenCode Zen and OpenCode Go providers (port of `providers/opencode*.ts`).

use std::sync::Arc;

use futures::future::BoxFuture;
use indexmap::IndexMap;

use super::builtin::{catalog_chat_models, catalog_classifier_models};
use crate::api::registry::{
    anthropic_messages_api, builtin_classifier_api, google_generative_ai_api, openai_completions_api, openai_responses_api,
};
use crate::auth::helpers::env_api_key_auth;
use crate::auth::types::ProviderAuth;
use crate::models::{CreateProviderOptions, Provider, ProviderApi, ProviderStreams, create_provider};
use crate::types::{AssistantMessageEventStream, DeferredHandle, Model, StreamOptions, TranscriptContext};

const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";

fn with_session_header(mut options: StreamOptions) -> StreamOptions {
    let Some(session_id) = options.session_id.clone().filter(|s| !s.is_empty()) else { return options };
    let has = options.headers.as_ref().is_some_and(|h| h.keys().any(|k| k.eq_ignore_ascii_case(OPENCODE_SESSION_HEADER)));
    if has {
        return options;
    }
    options.headers.get_or_insert_with(Default::default).insert(OPENCODE_SESSION_HEADER.into(), Some(session_id));
    options
}

/// Adds OpenCode's required per-conversation routing header before API dispatch.
pub struct WithOpenCodeSessionHeader(pub Arc<dyn ProviderStreams>);

impl ProviderStreams for WithOpenCodeSessionHeader {
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.0.stream(model, context, with_session_header(options))
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.0.stream_simple(model, context, with_session_header(options))
    }
    fn supports_fetch_deferred(&self) -> bool {
        self.0.supports_fetch_deferred()
    }
    fn fetch_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> AssistantMessageEventStream {
        self.0.fetch_deferred(model, handle, options)
    }
    fn supports_cancel_deferred(&self) -> bool {
        self.0.supports_cancel_deferred()
    }
    fn cancel_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> BoxFuture<'static, anyhow::Result<()>> {
        self.0.cancel_deferred(model, handle, options)
    }
}

fn opencode_apis() -> IndexMap<String, Arc<dyn ProviderStreams>> {
    let mut apis: IndexMap<String, Arc<dyn ProviderStreams>> = IndexMap::new();
    apis.insert("anthropic-messages".into(), Arc::new(WithOpenCodeSessionHeader(anthropic_messages_api())));
    apis.insert("google-generative-ai".into(), Arc::new(WithOpenCodeSessionHeader(google_generative_ai_api())));
    apis.insert("openai-completions".into(), Arc::new(WithOpenCodeSessionHeader(openai_completions_api())));
    apis.insert("openai-responses".into(), Arc::new(WithOpenCodeSessionHeader(openai_responses_api())));
    apis
}

pub fn opencode_provider() -> Arc<dyn Provider> {
    let mut classifiers = IndexMap::new();
    classifiers.insert("typesafe-system-one".to_string(), builtin_classifier_api("typesafe-system-one"));
    let mut models = catalog_chat_models("opencode");
    models.extend(catalog_classifier_models("opencode"));
    create_provider(CreateProviderOptions {
        id: "opencode".into(),
        name: Some("OpenCode Zen".into()),
        auth: ProviderAuth { api_key: Some(env_api_key_auth("OpenCode API key", &["OPENCODE_API_KEY"])), oauth: None },
        models,
        api: Some(ProviderApi::ByApi(opencode_apis())),
        classifiers,
        ..Default::default()
    })
    .expect("opencode provider is valid")
}

pub fn opencode_go_provider() -> Arc<dyn Provider> {
    create_provider(CreateProviderOptions {
        id: "opencode-go".into(),
        name: Some("OpenCode Go".into()),
        auth: ProviderAuth { api_key: Some(env_api_key_auth("OpenCode API key", &["OPENCODE_API_KEY"])), oauth: None },
        models: catalog_chat_models("opencode-go"),
        api: Some(ProviderApi::ByApi(opencode_apis())),
        ..Default::default()
    })
    .expect("opencode go provider is valid")
}
