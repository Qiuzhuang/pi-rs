//! Azure provider (port of `providers/azure.ts`).

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::Value;

use super::builtin::catalog_chat_models;
use crate::api::azure_openai_config::{resolve_azure_base_url, resolve_deployment_name};
use crate::api::lazy::lazy_stream;
use crate::api::registry::{azure_openai_responses_api, openai_completions_api};
use crate::auth::helpers::env_api_key_auth;
use crate::auth::types::ProviderAuth;
use crate::models::{CreateProviderOptions, Provider, ProviderApi, ProviderStreams, create_provider};
use crate::types::{AssistantMessageEventStream, Model, OnPayload, StreamOptions, TranscriptContext};

fn with_deployment_name(model: &Model, mut options: StreamOptions) -> StreamOptions {
    let deployment = resolve_deployment_name(model, Some(&options));
    if deployment == model.id {
        return options;
    }
    let previous = options.on_payload.clone();
    let on_payload: OnPayload = Arc::new(move |payload: Value, payload_model: Model| {
        let deployment = deployment.clone();
        let previous = previous.clone();
        Box::pin(async move {
            let mut params = payload;
            if let Value::Object(map) = &mut params {
                map.insert("model".into(), Value::String(deployment));
            }
            match previous {
                Some(previous) => Some(previous(params.clone(), payload_model).await.unwrap_or(params)),
                None => Some(params),
            }
        })
    });
    options.on_payload = Some(on_payload);
    options
}

/// Resolve the Azure endpoint and deployment before dispatch.
pub struct AzureStreams(pub Arc<dyn ProviderStreams>);

impl AzureStreams {
    fn dispatch(&self, model: &Model, context: &TranscriptContext, options: StreamOptions, simple: bool) -> AssistantMessageEventStream {
        let inner = self.0.clone();
        let owned_model = model.clone();
        let context = context.clone();
        lazy_stream(
            model,
            Box::pin(async move {
                let base_url = resolve_azure_base_url(&owned_model, Some(&options))?;
                let request_model = Model { base_url, ..owned_model.clone() };
                let options = with_deployment_name(&owned_model, options);
                Ok(if simple {
                    inner.stream_simple(&request_model, &context, options)
                } else {
                    inner.stream(&request_model, &context, options)
                })
            }),
            None,
        )
    }
}

impl ProviderStreams for AzureStreams {
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.dispatch(model, context, options, false)
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.dispatch(model, context, options, true)
    }
}

pub fn azure_provider() -> Arc<dyn Provider> {
    let mut apis: IndexMap<String, Arc<dyn ProviderStreams>> = IndexMap::new();
    apis.insert("azure-openai-responses".into(), azure_openai_responses_api());
    apis.insert("openai-completions".into(), Arc::new(AzureStreams(openai_completions_api())));
    create_provider(CreateProviderOptions {
        id: "azure".into(),
        name: Some("Azure".into()),
        auth: ProviderAuth { api_key: Some(env_api_key_auth("Azure OpenAI API key", &["AZURE_OPENAI_API_KEY"])), oauth: None },
        models: catalog_chat_models("azure"),
        api: Some(ProviderApi::ByApi(apis)),
        ..Default::default()
    })
    .expect("azure provider is valid")
}
