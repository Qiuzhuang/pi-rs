//! Built-in API implementation lookup (the Rust counterpart of the `*.lazy.ts` wrappers).
//!
//! Provider definitions reference APIs by id through [`builtin_api`]; the implementation is
//! resolved per request. To add an API implementation, add a module under `api/` and either
//! list it in [`resolve_builtin_api`] or call [`register_builtin_api_implementation`] at startup.
//! APIs without an implementation produce an error stream.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

use super::lazy::error_stream;
use crate::models::{ProviderClassifier, ProviderImages, ProviderStreams};
use crate::types::{
    AssistantImages, AssistantMessageEventStream, ClassifierContext, ClassifierModel, ClassifierResult,
    DeferredHandle, ImageModel, ImagesContext, Model, StreamOptions, TranscriptContext,
};
use crate::utils::model_operations::{classifier_error_result, image_error_result};

static STREAM_IMPLS: Lazy<RwLock<HashMap<String, Arc<dyn ProviderStreams>>>> = Lazy::new(|| RwLock::new(HashMap::new()));
static IMAGE_IMPLS: Lazy<RwLock<HashMap<String, Arc<dyn ProviderImages>>>> = Lazy::new(|| RwLock::new(HashMap::new()));
static CLASSIFIER_IMPLS: Lazy<RwLock<HashMap<String, Arc<dyn ProviderClassifier>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Register the implementation of a built-in chat API (overrides the built-in one).
pub fn register_builtin_api_implementation(api: &str, implementation: Arc<dyn ProviderStreams>) {
    STREAM_IMPLS.write().insert(api.to_string(), implementation);
}

/// Register the implementation of a built-in image API.
pub fn register_builtin_images_implementation(api: &str, implementation: Arc<dyn ProviderImages>) {
    IMAGE_IMPLS.write().insert(api.to_string(), implementation);
}

/// Register the implementation of a built-in classifier API.
pub fn register_builtin_classifier_implementation(api: &str, implementation: Arc<dyn ProviderClassifier>) {
    CLASSIFIER_IMPLS.write().insert(api.to_string(), implementation);
}

/// The built-in implementation of `api`, if one exists.
pub fn resolve_builtin_api(api: &str) -> Option<Arc<dyn ProviderStreams>> {
    if let Some(registered) = STREAM_IMPLS.read().get(api) {
        return Some(registered.clone());
    }
    match api {
        "anthropic-messages" => Some(Arc::new(super::anthropic_messages::AnthropicMessagesApi)),
        "openai-completions" => Some(Arc::new(super::openai_completions::OpenAICompletionsApi)),
        _ => None,
    }
}

fn not_implemented(model: &Model, api: &str) -> AssistantMessageEventStream {
    error_stream(model, anyhow::anyhow!("API \"{api}\" is not implemented in this build"))
}

/// A lazily resolved built-in chat API.
pub struct BuiltinApi {
    api: String,
}

/// A [`ProviderStreams`] for the built-in API `api`, resolved per request.
pub fn builtin_api(api: &str) -> Arc<dyn ProviderStreams> {
    Arc::new(BuiltinApi { api: api.to_string() })
}

impl ProviderStreams for BuiltinApi {
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        match resolve_builtin_api(&self.api) {
            Some(implementation) => implementation.stream(model, context, options),
            None => not_implemented(model, &self.api),
        }
    }

    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        match resolve_builtin_api(&self.api) {
            Some(implementation) => implementation.stream_simple(model, context, options),
            None => not_implemented(model, &self.api),
        }
    }

    fn supports_fetch_deferred(&self) -> bool {
        resolve_builtin_api(&self.api).is_some_and(|i| i.supports_fetch_deferred())
    }

    fn fetch_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> AssistantMessageEventStream {
        match resolve_builtin_api(&self.api) {
            Some(implementation) => implementation.fetch_deferred(model, handle, options),
            None => not_implemented(model, &self.api),
        }
    }

    fn supports_cancel_deferred(&self) -> bool {
        resolve_builtin_api(&self.api).is_some_and(|i| i.supports_cancel_deferred())
    }

    fn cancel_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> BoxFuture<'static, anyhow::Result<()>> {
        match resolve_builtin_api(&self.api) {
            Some(implementation) => implementation.cancel_deferred(model, handle, options),
            None => {
                let api = self.api.clone();
                Box::pin(async move { Err(anyhow::anyhow!("API \"{api}\" is not implemented in this build")) })
            }
        }
    }
}

pub fn anthropic_messages_api() -> Arc<dyn ProviderStreams> {
    builtin_api("anthropic-messages")
}
pub fn openai_completions_api() -> Arc<dyn ProviderStreams> {
    builtin_api("openai-completions")
}
pub fn openai_responses_api() -> Arc<dyn ProviderStreams> {
    builtin_api("openai-responses")
}
pub fn azure_openai_responses_api() -> Arc<dyn ProviderStreams> {
    builtin_api("azure-openai-responses")
}
pub fn openai_codex_responses_api() -> Arc<dyn ProviderStreams> {
    builtin_api("openai-codex-responses")
}
pub fn google_generative_ai_api() -> Arc<dyn ProviderStreams> {
    builtin_api("google-generative-ai")
}
pub fn google_vertex_api() -> Arc<dyn ProviderStreams> {
    builtin_api("google-vertex")
}
pub fn mistral_conversations_api() -> Arc<dyn ProviderStreams> {
    builtin_api("mistral-conversations")
}
pub fn bedrock_converse_stream_api() -> Arc<dyn ProviderStreams> {
    builtin_api("bedrock-converse-stream")
}
pub fn pi_messages_api() -> Arc<dyn ProviderStreams> {
    builtin_api("pi-messages")
}

/// A lazily resolved built-in image API.
pub struct BuiltinImagesApi {
    api: String,
}

pub fn builtin_images_api(api: &str) -> Arc<dyn ProviderImages> {
    Arc::new(BuiltinImagesApi { api: api.to_string() })
}

#[async_trait]
impl ProviderImages for BuiltinImagesApi {
    async fn generate_images(&self, model: &ImageModel, context: &ImagesContext, options: StreamOptions) -> AssistantImages {
        let implementation = IMAGE_IMPLS.read().get(&self.api).cloned();
        match implementation {
            Some(implementation) => implementation.generate_images(model, context, options).await,
            None => image_error_result(
                model,
                &format!("Image API \"{}\" is not implemented in this build", self.api),
                false,
            ),
        }
    }
}

/// A lazily resolved built-in classifier API.
pub struct BuiltinClassifierApi {
    api: String,
}

pub fn builtin_classifier_api(api: &str) -> Arc<dyn ProviderClassifier> {
    Arc::new(BuiltinClassifierApi { api: api.to_string() })
}

#[async_trait]
impl ProviderClassifier for BuiltinClassifierApi {
    async fn classify(&self, model: &ClassifierModel, context: &ClassifierContext, options: StreamOptions) -> ClassifierResult {
        let implementation = CLASSIFIER_IMPLS.read().get(&self.api).cloned();
        match implementation {
            Some(implementation) => implementation.classify(model, context, options).await,
            None => classifier_error_result(
                model,
                &format!("Classifier API \"{}\" is not implemented in this build", self.api),
                false,
            ),
        }
    }
}
