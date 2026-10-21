//! Cloudflare Workers AI and AI Gateway providers (port of `providers/cloudflare-auth.ts`,
//! `cloudflare-stream.ts`, `cloudflare-workers-ai.ts`, `cloudflare-ai-gateway.ts`).

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use indexmap::IndexMap;

use super::builtin::{catalog_chat_models, catalog_classifier_models};
use crate::api::registry::{anthropic_messages_api, builtin_classifier_api, openai_completions_api, openai_responses_api};
use crate::auth::types::{
    ApiKeyAuth, ApiKeyCredential, ApiKeyResolveInput, AuthPrompt, AuthResult, ModelAuth, ProviderAuth,
    ProviderAuthInteraction,
};
use crate::models::{CreateProviderOptions, Provider, ProviderApi, ProviderClassifier, ProviderStreams, create_provider};
use crate::types::{
    AssistantMessageEventStream, ClassifierContext, ClassifierModel, ClassifierResult, DeferredHandle, Model, ProviderEnv,
    ProviderHeaders, StreamOptions, TranscriptContext,
};

const CLOUDFLARE_API_KEY: &str = "CLOUDFLARE_API_KEY";
const CLOUDFLARE_ACCOUNT_ID: &str = "CLOUDFLARE_ACCOUNT_ID";
const CLOUDFLARE_GATEWAY_ID: &str = "CLOUDFLARE_GATEWAY_ID";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloudflareAuthKind {
    WorkersAi,
    AiGateway,
}

async fn resolve_value(name: &str, input: &ApiKeyResolveInput) -> anyhow::Result<Option<String>> {
    if let Some(credential) = &input.credential {
        let from_credential =
            if name == CLOUDFLARE_API_KEY { credential.key.clone() } else { credential.env.as_ref().and_then(|e| e.get(name)).cloned() };
        if from_credential.is_some() {
            return Ok(from_credential);
        }
    }
    input.throw_if_aborted()?;
    let value = input.ctx.env(name).await;
    input.throw_if_aborted()?;
    Ok(value)
}

async fn resolve_cloudflare_env(
    kind: CloudflareAuthKind,
    input: &ApiKeyResolveInput,
) -> anyhow::Result<Option<(String, ProviderEnv, String)>> {
    let api_key = resolve_value(CLOUDFLARE_API_KEY, input).await?;
    let account_id = resolve_value(CLOUDFLARE_ACCOUNT_ID, input).await?;
    let gateway_id =
        if kind == CloudflareAuthKind::AiGateway { resolve_value(CLOUDFLARE_GATEWAY_ID, input).await? } else { None };
    let (Some(api_key), Some(account_id)) = (api_key.filter(|v| !v.is_empty()), account_id.filter(|v| !v.is_empty())) else {
        return Ok(None);
    };
    if kind == CloudflareAuthKind::AiGateway && gateway_id.as_deref().is_none_or(str::is_empty) {
        return Ok(None);
    }
    let mut env = ProviderEnv::new();
    env.insert(CLOUDFLARE_ACCOUNT_ID.into(), account_id);
    if let Some(gateway_id) = gateway_id.filter(|g| !g.is_empty()) {
        env.insert(CLOUDFLARE_GATEWAY_ID.into(), gateway_id);
    }
    let source = if input.credential.is_some() { "stored credential" } else { CLOUDFLARE_API_KEY };
    Ok(Some((api_key, env, source.to_string())))
}

struct CloudflareAuth {
    kind: CloudflareAuthKind,
}

#[async_trait]
impl ApiKeyAuth for CloudflareAuth {
    fn name(&self) -> &str {
        "Cloudflare API key"
    }

    fn supports_login(&self) -> bool {
        true
    }

    async fn login(&self, interaction: ProviderAuthInteraction) -> anyhow::Result<ApiKeyCredential> {
        let key = interaction.interaction.prompt(AuthPrompt::secret("Enter Cloudflare API key")).await?;
        let account_id = interaction.interaction.prompt(AuthPrompt::text("Enter Cloudflare account ID")).await?;
        let mut env = ProviderEnv::new();
        env.insert(CLOUDFLARE_ACCOUNT_ID.into(), account_id);
        if self.kind == CloudflareAuthKind::AiGateway {
            let gateway_id = interaction.interaction.prompt(AuthPrompt::text("Enter Cloudflare AI Gateway ID")).await?;
            env.insert(CLOUDFLARE_GATEWAY_ID.into(), gateway_id);
        }
        Ok(ApiKeyCredential { key: Some(key), env: Some(env) })
    }

    async fn resolve(&self, input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        let Some((api_key, env, source)) = resolve_cloudflare_env(self.kind, &input).await? else { return Ok(None) };
        let auth = match self.kind {
            CloudflareAuthKind::WorkersAi => ModelAuth { api_key: Some(api_key), ..Default::default() },
            CloudflareAuthKind::AiGateway => {
                let mut headers = ProviderHeaders::new();
                headers.insert("cf-aig-authorization".into(), Some(format!("Bearer {api_key}")));
                headers.insert("Authorization".into(), None);
                headers.insert("x-api-key".into(), None);
                ModelAuth { headers: Some(headers), ..Default::default() }
            }
        };
        Ok(Some(AuthResult { auth, env: Some(env), source: Some(source) }))
    }
}

pub fn cloudflare_workers_ai_auth() -> Arc<dyn ApiKeyAuth> {
    Arc::new(CloudflareAuth { kind: CloudflareAuthKind::WorkersAi })
}

pub fn cloudflare_ai_gateway_auth() -> Arc<dyn ApiKeyAuth> {
    Arc::new(CloudflareAuth { kind: CloudflareAuthKind::AiGateway })
}

/// Materialize `{CLOUDFLARE_ACCOUNT_ID}`/`{CLOUDFLARE_GATEWAY_ID}` placeholders in a base URL.
pub fn resolve_cloudflare_base_url(base_url: &str, env: Option<&ProviderEnv>) -> String {
    let Some(env) = env else { return base_url.to_string() };
    let account = env.get(CLOUDFLARE_ACCOUNT_ID).cloned().unwrap_or_else(|| format!("{{{CLOUDFLARE_ACCOUNT_ID}}}"));
    let gateway = env.get(CLOUDFLARE_GATEWAY_ID).cloned().unwrap_or_else(|| format!("{{{CLOUDFLARE_GATEWAY_ID}}}"));
    base_url
        .replace(&format!("{{{CLOUDFLARE_ACCOUNT_ID}}}"), &account)
        .replace(&format!("{{{CLOUDFLARE_GATEWAY_ID}}}"), &gateway)
}

fn resolve_model(model: &Model, env: Option<&ProviderEnv>) -> Model {
    let base_url = resolve_cloudflare_base_url(&model.base_url, env);
    if base_url == model.base_url {
        return model.clone();
    }
    Model { base_url, ..model.clone() }
}

/// Wrap an API so Cloudflare endpoint placeholders resolve from the provider env.
pub struct CloudflareStreams(pub Arc<dyn ProviderStreams>);

impl ProviderStreams for CloudflareStreams {
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        let model = resolve_model(model, options.env.as_ref());
        self.0.stream(&model, context, options)
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        let model = resolve_model(model, options.env.as_ref());
        self.0.stream_simple(&model, context, options)
    }
    fn supports_fetch_deferred(&self) -> bool {
        false
    }
    fn supports_cancel_deferred(&self) -> bool {
        false
    }
    fn cancel_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> BoxFuture<'static, anyhow::Result<()>> {
        self.0.cancel_deferred(model, handle, options)
    }
}

/// Classifier counterpart of [`CloudflareStreams`].
pub struct CloudflareClassifier(pub Arc<dyn ProviderClassifier>);

#[async_trait]
impl ProviderClassifier for CloudflareClassifier {
    async fn classify(&self, model: &ClassifierModel, context: &ClassifierContext, options: StreamOptions) -> ClassifierResult {
        let base_url = resolve_cloudflare_base_url(&model.base_url, options.env.as_ref());
        let model = ClassifierModel { base_url, ..model.clone() };
        self.0.classify(&model, context, options).await
    }
}

pub fn cloudflare_workers_ai_provider() -> Arc<dyn Provider> {
    let mut classifiers: IndexMap<String, Arc<dyn ProviderClassifier>> = IndexMap::new();
    classifiers.insert(
        "cloudflare-workers-ai-system-one".into(),
        Arc::new(CloudflareClassifier(builtin_classifier_api("cloudflare-workers-ai-system-one"))),
    );
    let mut models = catalog_chat_models("cloudflare-workers-ai");
    models.extend(catalog_classifier_models("cloudflare-workers-ai"));
    create_provider(CreateProviderOptions {
        id: "cloudflare-workers-ai".into(),
        name: Some("Cloudflare Workers AI".into()),
        auth: ProviderAuth { api_key: Some(cloudflare_workers_ai_auth()), oauth: None },
        models,
        api: Some(ProviderApi::Single(Arc::new(CloudflareStreams(openai_completions_api())))),
        classifiers,
        ..Default::default()
    })
    .expect("cloudflare workers ai provider is valid")
}

pub fn cloudflare_ai_gateway_provider() -> Arc<dyn Provider> {
    let mut apis: IndexMap<String, Arc<dyn ProviderStreams>> = IndexMap::new();
    apis.insert("anthropic-messages".into(), Arc::new(CloudflareStreams(anthropic_messages_api())));
    apis.insert("openai-completions".into(), Arc::new(CloudflareStreams(openai_completions_api())));
    apis.insert("openai-responses".into(), Arc::new(CloudflareStreams(openai_responses_api())));
    create_provider(CreateProviderOptions {
        id: "cloudflare-ai-gateway".into(),
        name: Some("Cloudflare AI Gateway".into()),
        auth: ProviderAuth { api_key: Some(cloudflare_ai_gateway_auth()), oauth: None },
        models: catalog_chat_models("cloudflare-ai-gateway"),
        api: Some(ProviderApi::ByApi(apis)),
        ..Default::default()
    })
    .expect("cloudflare ai gateway provider is valid")
}
