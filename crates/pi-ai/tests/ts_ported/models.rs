//! `Models` runtime tests: `images-models.test.ts`, `model-types.test.ts`,
//! `radius-provider.test.ts`, `openrouter-user-models.test.ts`, `opencode-provider-headers.test.ts`,
//! `compat-env.test.ts`, `message-types.test.ts` and `max-thinking.test.ts`.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use pi_ai::auth::{ApiKeyAuth, ApiKeyResolveInput, AuthResolutionOverrides, AuthResult, Credential, CredentialStore, InMemoryCredentialStore, ModelAuth, ProviderAuth};
use pi_ai::providers::all::{
    builtin_models, get_all_builtin_models, get_builtin_classifier_models, get_builtin_image_model, get_builtin_image_models, get_builtin_model,
    get_builtin_models,
};
use pi_ai::providers::faux::{RegisterFauxProviderOptions, faux_provider, faux_text_message};
use pi_ai::*;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::mock_http::{MockServer, Response};
use crate::support::*;

fn chat_model(provider: &str, id: &str) -> Model {
    model(json!({"id": id, "name": id, "api": "test-chat", "provider": provider, "baseUrl": "https://example.test/v1", "reasoning": false,
        "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 1000, "maxTokens": 100}))
}

fn image_value(provider: &str, id: &str) -> Value {
    json!({"type": "image", "id": id, "name": id, "api": "test-images", "provider": provider, "baseUrl": "https://example.test/v1",
        "input": ["text"], "output": ["image"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}})
}

fn image_model(provider: &str, id: &str) -> ImageModel {
    serde_json::from_value(image_value(provider, id)).unwrap()
}

fn images_context() -> ImagesContext {
    ImagesContext { input: vec![UserContentBlock::text("a red circle")] }
}

fn ok_images(model: &ImageModel) -> AssistantImages {
    AssistantImages {
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        output: vec![UserContentBlock::Image(ImageContent { data: "aGk=".into(), mime_type: "image/png".into() })],
        stop_reason: ImagesStopReason::Stop,
        timestamp: 1,
        ..Default::default()
    }
}

type Calls = Arc<Mutex<Vec<(ImageModel, StreamOptions)>>>;

struct RecordingImages(Calls);

#[async_trait]
impl ProviderImages for RecordingImages {
    async fn generate_images(&self, model: &ImageModel, _context: &ImagesContext, options: StreamOptions) -> AssistantImages {
        self.0.lock().push((model.clone(), options));
        ok_images(model)
    }
}

struct NoopStreams;

impl ProviderStreams for NoopStreams {
    fn stream(&self, model: &Model, _context: &TranscriptContext, _options: StreamOptions) -> AssistantMessageEventStream {
        let (sender, stream) = AssistantMessageEventStream::new();
        let message = AssistantMessage::empty_for(model, 1);
        sender.push(AssistantMessageEvent::Start { partial: message.clone() });
        sender.push(AssistantMessageEvent::Done { reason: StopReason::Stop, message });
        stream
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        self.stream(model, context, options)
    }
}

/// Api-key auth reading `env_var` from the auth context (or always configured when `None`).
struct EnvKeyAuth(Option<&'static str>);

#[async_trait]
impl ApiKeyAuth for EnvKeyAuth {
    fn name(&self) -> &str {
        "Test key"
    }
    async fn resolve(&self, input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        let Some(var) = self.0 else { return Ok(Some(AuthResult::default())) };
        let stored = input.credential.as_ref().and_then(|c| c.key.clone());
        let key = match stored.clone() {
            Some(key) => Some(key),
            None => input.ctx.env(var).await,
        };
        Ok(key.map(|key| AuthResult {
            auth: ModelAuth { api_key: Some(key), ..Default::default() },
            env: None,
            source: Some(if stored.is_some() { "stored".into() } else { var.into() }),
        }))
    }
}

fn test_provider(id: &str, models: Vec<AnyModel>, env_var: Option<&'static str>, calls: Calls, image_apis: &[&str]) -> Arc<dyn Provider> {
    let mut apis: indexmap::IndexMap<String, Arc<dyn ProviderStreams>> = indexmap::IndexMap::new();
    apis.insert("test-chat".into(), Arc::new(NoopStreams));
    let images = image_apis.iter().map(|api| (api.to_string(), Arc::new(RecordingImages(calls.clone())) as Arc<dyn ProviderImages>)).collect();
    create_provider(CreateProviderOptions {
        id: id.into(),
        auth: ProviderAuth { api_key: Some(Arc::new(EnvKeyAuth(env_var))), oauth: None },
        models,
        api: Some(ProviderApi::ByApi(apis)),
        images,
        ..Default::default()
    })
    .unwrap()
}

fn ids(models: &[AnyModel]) -> Vec<String> {
    models.iter().map(|m| m.id().to_string()).collect()
}

// ---------------------------------------------------------------------------
// Port of images-models.test.ts
// ---------------------------------------------------------------------------

#[test]
fn model_discriminants() {
    let chat = AnyModel::Chat(chat_model("p", "c"));
    let image = AnyModel::Image(image_model("p", "i"));
    assert_eq!(chat.model_type(), ModelType::Chat);
    let typed: AnyModel = serde_json::from_value(json!({"type": "chat", "id": "c", "name": "c", "api": "test-chat", "provider": "p", "baseUrl": "", "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}})).unwrap();
    assert_eq!(typed.model_type(), ModelType::Chat);
    assert_eq!(image.model_type(), ModelType::Image);
    // hasApi never matches an image model, even on an equal api string.
    let mut same_api = image_model("p", "i");
    same_api.api = "test-chat".into();
    assert!(!has_api(&AnyModel::Image(same_api), "test-chat"));
    assert!(has_api(&chat, "test-chat"));
}

#[tokio::test]
async fn typed_accessors_and_availability() {
    let calls: Calls = Arc::default();
    let models = create_models(CreateModelsOptions::default());
    models.set_provider(test_provider(
        "p1",
        vec![AnyModel::Chat(chat_model("p1", "c1")), AnyModel::Image(image_model("p1", "i1")), AnyModel::Image(image_model("p1", "i2"))],
        None,
        calls.clone(),
        &["test-images"],
    ));
    models.set_provider(test_provider("p2", vec![AnyModel::Image(image_model("p2", "i3"))], None, calls.clone(), &["test-images"]));
    assert_eq!(models.get_models(None).iter().map(|m| m.id.clone()).collect::<Vec<_>>(), ["c1"]);
    assert_eq!(ids(&models.get_models_of_type(ModelType::Chat, None)), ["c1"]);
    assert_eq!(ids(&models.get_models_of_type(ModelType::Image, None)), ["i1", "i2", "i3"]);
    assert_eq!(ids(&models.get_models_of_type(ModelType::Image, Some("p1"))), ["i1", "i2"]);
    assert!(models.get_models_of_type(ModelType::Classifier, None).is_empty());
    assert_eq!(ids(&models.get_all_models(None)), ["c1", "i1", "i2", "i3"]);
    assert_eq!(models.get_model("p1", "c1").map(|m| m.id), Some("c1".into()));
    assert!(models.get_model("p1", "i1").is_none());
    assert_eq!(models.get_model_of_type(ModelType::Chat, "p1", "c1").map(|m| m.id().to_string()), Some("c1".into()));
    assert_eq!(models.get_model_of_type(ModelType::Image, "p1", "i1").map(|m| m.id().to_string()), Some("i1".into()));
    assert!(models.get_model_of_type(ModelType::Image, "p1", "c1").is_none());

    let models = create_models(CreateModelsOptions { auth_context: Some(MapAuthContext::new(&[("KEY", "k")])), ..Default::default() });
    models.set_provider(test_provider("p1", vec![AnyModel::Chat(chat_model("p1", "c1")), AnyModel::Image(image_model("p1", "i1"))], Some("KEY"), calls.clone(), &["test-images"]));
    models.set_provider(test_provider("p2", vec![AnyModel::Image(image_model("p2", "i2"))], Some("MISSING"), calls, &["test-images"]));
    assert_eq!(models.get_available(None, None).await.unwrap().iter().map(|m| m.id.clone()).collect::<Vec<_>>(), ["c1"]);
    assert_eq!(ids(&models.get_available_of_type(ModelType::Chat, None, None).await.unwrap()), ["c1"]);
    assert_eq!(ids(&models.get_available_of_type(ModelType::Image, None, None).await.unwrap()), ["i1"]);
    assert_eq!(ids(&models.get_all_available(None, None).await.unwrap()), ["c1", "i1"]);
}

#[tokio::test]
async fn image_requests_merge_resolved_auth_and_explicit_options_win() {
    let calls: Calls = Arc::default();
    let models = create_models(CreateModelsOptions { auth_context: Some(MapAuthContext::new(&[("TEST_KEY", "env-key")])), ..Default::default() });
    models.set_provider(test_provider("p1", vec![AnyModel::Image(image_model("p1", "model-a"))], Some("TEST_KEY"), calls.clone(), &["test-images"]));
    let any = models.get_model_of_type(ModelType::Image, "p1", "model-a").unwrap();
    let AnyModel::Image(model) = any.clone() else { panic!() };
    assert_eq!(models.get_auth_for_model(&any, Default::default()).await.unwrap().unwrap().auth.api_key.as_deref(), Some("env-key"));
    assert_eq!(models.get_auth("p1", Default::default()).await.unwrap().unwrap().auth.api_key.as_deref(), Some("env-key"));
    let explicit = AuthResolutionOverrides { api_key: Some("explicit-key".into()), ..Default::default() };
    assert_eq!(models.get_auth_for_model(&any, explicit).await.unwrap().unwrap().auth.api_key.as_deref(), Some("explicit-key"));
    let result = models.generate_images(&model, &images_context(), StreamOptions::default()).await;
    assert_eq!(result.stop_reason, ImagesStopReason::Stop);
    assert_eq!(calls.lock()[0].1.api_key.as_deref(), Some("env-key"));
    models.generate_images(&model, &images_context(), StreamOptions { api_key: Some("explicit".into()), ..Default::default() }).await;
    assert_eq!(calls.lock()[1].1.api_key.as_deref(), Some("explicit"));
}

struct ProviderEnvAuth;

#[async_trait]
impl ApiKeyAuth for ProviderEnvAuth {
    fn name(&self) -> &str {
        "Test key"
    }
    async fn resolve(&self, _input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        let mut headers = ProviderHeaders::new();
        headers.insert("x-base".into(), Some("1".into()));
        Ok(Some(AuthResult {
            auth: ModelAuth { api_key: Some("provider-key".into()), headers: Some(headers), ..Default::default() },
            env: Some([("PROVIDER_ONLY".to_string(), "provider".to_string()), ("SHARED".to_string(), "provider".to_string())].into_iter().collect()),
            source: None,
        }))
    }
}

#[tokio::test]
async fn image_requests_merge_provider_env_and_transform_headers() {
    let calls: Calls = Arc::default();
    let mut images: indexmap::IndexMap<String, Arc<dyn ProviderImages>> = indexmap::IndexMap::new();
    images.insert("test-images".into(), Arc::new(RecordingImages(calls.clone())));
    let models = create_models(CreateModelsOptions::default());
    models.set_provider(
        create_provider(CreateProviderOptions {
            id: "p1".into(),
            auth: ProviderAuth { api_key: Some(Arc::new(ProviderEnvAuth)), oauth: None },
            models: vec![AnyModel::Image(image_model("p1", "model-a"))],
            images,
            ..Default::default()
        })
        .unwrap(),
    );
    let model = image_model("p1", "model-a");
    let env: ProviderEnv = [("REQUEST_ONLY".to_string(), "request".to_string()), ("SHARED".to_string(), "request".to_string())].into_iter().collect();
    let request = ModelsRequestOptions {
        options: StreamOptions { api_key: Some("request-key".into()), env: Some(env), ..Default::default() },
        transform_headers: Some(Arc::new(|mut headers: ProviderHeaders| {
            headers.insert("x-extra".into(), Some("2".into()));
            Box::pin(async move { headers })
        })),
    };
    models.generate_images(&model, &images_context(), request).await;
    let options = calls.lock()[0].1.clone();
    assert_eq!(options.api_key.as_deref(), Some("request-key"));
    let env = options.env.unwrap();
    assert_eq!(env.len(), 3);
    assert_eq!(env["PROVIDER_ONLY"], "provider");
    assert_eq!(env["REQUEST_ONLY"], "request");
    assert_eq!(env["SHARED"], "request");
    let headers = options.headers.unwrap();
    assert_eq!(headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>(), vec![
        ("x-base".to_string(), Some("1".to_string())),
        ("x-extra".to_string(), Some("2".to_string())),
    ]);
}

#[tokio::test]
async fn image_generation_errors_become_results() {
    let models = create_models(CreateModelsOptions { auth_context: Some(MapAuthContext::new(&[])), ..Default::default() });
    let ghost = models.generate_images(&image_model("ghost", "m"), &images_context(), StreamOptions::default()).await;
    assert_eq!(ghost.stop_reason, ImagesStopReason::Error);
    assert!(ghost.error_message.as_deref().unwrap().contains("Unknown provider: ghost"));

    let calls: Calls = Arc::default();
    models.set_provider(test_provider("p1", vec![AnyModel::Image(image_model("p1", "model-a"))], Some("MISSING"), calls.clone(), &["test-images"]));
    let any = models.get_model_of_type(ModelType::Image, "p1", "model-a").unwrap();
    assert!(models.get_auth_for_model(&any, Default::default()).await.unwrap().is_none());
    let model = image_model("p1", "model-a");
    let unconfigured = models.generate_images(&model, &images_context(), StreamOptions::default()).await;
    assert_eq!(unconfigured.stop_reason, ImagesStopReason::Error);
    assert!(unconfigured.error_message.as_deref().unwrap().contains("not configured"), "{:?}", unconfigured.error_message);
    let signal = CancellationToken::new();
    signal.cancel();
    let cancelled = models.generate_images(&model, &images_context(), StreamOptions { signal: Some(signal), ..Default::default() }).await;
    assert_eq!(cancelled.stop_reason, ImagesStopReason::Aborted);
    assert!(calls.lock().is_empty());

    // A provider without any images implementation rejects image models it lists.
    let mut apis: indexmap::IndexMap<String, Arc<dyn ProviderStreams>> = indexmap::IndexMap::new();
    apis.insert("test-chat".into(), Arc::new(NoopStreams));
    models.set_provider(
        create_provider(CreateProviderOptions {
            id: "chat-only".into(),
            auth: ProviderAuth { api_key: Some(Arc::new(EnvKeyAuth(None))), oauth: None },
            models: vec![AnyModel::Image(image_model("chat-only", "i"))],
            api: Some(ProviderApi::ByApi(apis)),
            ..Default::default()
        })
        .unwrap(),
    );
    let unsupported = models.generate_images(&image_model("chat-only", "i"), &images_context(), StreamOptions::default()).await;
    assert_eq!(unsupported.stop_reason, ImagesStopReason::Error);
    assert!(unsupported.error_message.as_deref().unwrap().contains("does not support image generation"));

    models.set_provider(test_provider("wrong-api", vec![AnyModel::Image(image_model("wrong-api", "i"))], None, calls, &["other-images"]));
    let missing = models.generate_images(&image_model("wrong-api", "i"), &images_context(), StreamOptions::default()).await;
    assert_eq!(missing.stop_reason, ImagesStopReason::Error);
    assert!(missing.error_message.as_deref().unwrap().contains("no image generation implementation for \"test-images\""), "{:?}", missing.error_message);
}

#[tokio::test]
async fn entry_points_reject_models_of_the_wrong_type() {
    let calls: Calls = Arc::default();
    let models = create_models(CreateModelsOptions::default());
    models.set_provider(test_provider("p1", vec![AnyModel::Chat(chat_model("p1", "chat"))], None, calls.clone(), &["test-chat"]));
    let as_image: ImageModel = serde_json::from_value(serde_json::to_value(chat_model("p1", "chat")).unwrap().as_object().cloned().map(|mut m| {
        m.insert("output".into(), json!([]));
        m.insert("type".into(), json!("chat"));
        Value::Object(m)
    }).unwrap()).unwrap();
    assert_eq!(as_image.model_type, "chat");
    let result = models.generate_images(&as_image, &images_context(), StreamOptions::default()).await;
    assert_eq!(result.stop_reason, ImagesStopReason::Error);
    assert!(result.error_message.as_deref().unwrap().contains("is not an image model"), "{:?}", result.error_message);

    let models = create_models(CreateModelsOptions::default());
    models.set_provider(test_provider("p1", vec![AnyModel::Image(image_model("p1", "model-a"))], None, calls, &["test-images"]));
    let mut as_chat: Model = serde_json::from_value(image_value("p1", "model-a")).unwrap();
    as_chat.model_type = Some("image".into());
    let result = models.stream_simple(&as_chat, &Context::default(), StreamOptions::default()).result().await;
    assert_eq!(result.stop_reason, StopReason::Error);
    assert!(result.error_message.as_deref().unwrap().contains("is not a chat model"), "{:?}", result.error_message);
}

#[test]
fn providers_require_an_operation_implementation() {
    let message = "at least one of \"api\", \"images\", or \"classifiers\"";
    let empty = |api: Option<ProviderApi>| {
        create_provider(CreateProviderOptions {
            id: "empty".into(),
            auth: ProviderAuth { api_key: Some(Arc::new(EnvKeyAuth(None))), oauth: None },
            api,
            ..Default::default()
        })
    };
    assert!(empty(None).err().unwrap().to_string().contains(message));
    assert!(empty(Some(ProviderApi::ByApi(Default::default()))).err().unwrap().to_string().contains(message));
}

#[tokio::test]
async fn dynamic_providers_list_image_models_through_refresh() {
    let fetches = Arc::new(Mutex::new(0usize));
    let counter = fetches.clone();
    let store = Arc::new(InMemoryModelsStore::new());
    let models = create_models(CreateModelsOptions { models_store: Some(store.clone()), ..Default::default() });
    let mut images: indexmap::IndexMap<String, Arc<dyn ProviderImages>> = indexmap::IndexMap::new();
    images.insert("test-images".into(), Arc::new(RecordingImages(Arc::default())));
    models.set_provider(
        create_provider(CreateProviderOptions {
            id: "dyn".into(),
            auth: ProviderAuth { api_key: Some(Arc::new(EnvKeyAuth(None))), oauth: None },
            fetch_models: Some(Arc::new(move |_ctx| {
                *counter.lock() += 1;
                Box::pin(async { Ok(vec![AnyModel::Image(image_model("dyn", "listed")), AnyModel::Chat(chat_model("dyn", "chat"))]) })
            })),
            images,
            ..Default::default()
        })
        .unwrap(),
    );
    assert!(models.get_all_models(Some("dyn")).is_empty());
    let result = models.refresh(ModelsRefreshOptions { providers: Some(vec!["dyn".into()]), ..Default::default() }).await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(*fetches.lock(), 1);
    assert!(models.get_model_of_type(ModelType::Image, "dyn", "listed").is_some());
    assert!(models.get_model("dyn", "chat").is_some());
    let stored = store.read("dyn", None).await.unwrap().unwrap();
    assert_eq!(ids(&stored.models), ["listed", "chat"]);
}

#[tokio::test]
async fn builtin_reads_stay_chat_only_and_openrouter_exposes_image_models() {
    let chat = get_builtin_models("openrouter");
    let images = get_builtin_image_models("openrouter");
    let all = get_all_builtin_models("openrouter");
    assert!(chat.iter().all(|m| m.model_type.as_deref().unwrap_or("chat") == "chat"));
    assert!(images.iter().all(|m| m.model_type == "image"));
    assert!(all.iter().any(|m| m.model_type() == ModelType::Image));
    assert_eq!(compat::get_models("openrouter"), chat);
    assert!(chat.iter().all(|m| m.context_window > 0));
    assert_eq!(chat.len() + images.len() + get_builtin_classifier_models("openrouter").len(), all.len());
    assert_eq!(get_builtin_image_model("openrouter", "black-forest-labs/flux.2-pro").unwrap().model_type, "image");

    let models = builtin_models(CreateModelsOptions { auth_context: Some(MapAuthContext::new(&[("OPENROUTER_API_KEY", "or-key")])), ..Default::default() });
    let provider = models.get_provider("openrouter").unwrap();
    let image_models = models.get_models_of_type(ModelType::Image, Some("openrouter"));
    assert!(!image_models.is_empty());
    assert!(provider.get_all_models().iter().any(|m| m.model_type() == ModelType::Image));
    assert!(image_models.iter().all(|m| m.api() == "openrouter-images"));
    assert!(models.get_models_of_type(ModelType::Image, None).iter().all(|m| m.provider() == "openrouter"));
    let chat_model = models.get_model("openrouter", "google/gemini-3-pro-image").unwrap();
    let image_model = models.get_model_of_type(ModelType::Image, "openrouter", "google/gemini-3-pro-image").unwrap();
    assert_eq!(chat_model.api, "openai-completions");
    assert_eq!(image_model.api(), "openrouter-images");
    assert_eq!(models.get_auth_for_model(&image_models[0], Default::default()).await.unwrap().unwrap().auth.api_key.as_deref(), Some("or-key"));
    assert_eq!(models.get_auth_for_model(&AnyModel::Chat(chat_model), Default::default()).await.unwrap().unwrap().auth.api_key.as_deref(), Some("or-key"));
    assert!(provider.supports_images());
}

// ---------------------------------------------------------------------------
// Port of model-types.test.ts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn untyped_chat_models_work_through_a_faux_provider() {
    let faux = faux_provider(RegisterFauxProviderOptions { provider: Some("handwritten".into()), ..Default::default() });
    let models = create_models(CreateModelsOptions::default());
    models.set_provider(faux.provider.clone());
    let model = models.get_model("handwritten", &faux.models()[0].id).unwrap();
    assert_eq!(model.model_type, None);
    let any = AnyModel::Chat(model.clone());
    assert_eq!(any.model_type(), ModelType::Chat);
    assert!(has_api(&any, &model.api));
    let mut typed = model.clone();
    typed.model_type = Some("chat".into());
    assert!(models_are_equal(Some(&any), Some(&AnyModel::Chat(typed))));
    assert!(!models_are_equal(Some(&any), Some(&AnyModel::Image(image_model(&model.provider, &model.id)))));
    assert_eq!(ids(&models.get_models_of_type(ModelType::Chat, Some("handwritten"))), faux.models().iter().map(|m| m.id.clone()).collect::<Vec<_>>());
    assert_eq!(models.get_all_models(Some("handwritten")).len(), faux.models().len());
    assert!(models.get_models_of_type(ModelType::Image, Some("handwritten")).is_empty());
    faux.set_responses(vec![faux_text_message("hi").into()]);
    let result = models.complete(&model, &user_ctx("hi"), StreamOptions::default()).await;
    assert_eq!(result.stop_reason, StopReason::Stop, "{:?}", result.error_message);
}

#[test]
fn builtin_getters_return_plain_models() {
    let _ = get_builtin_model("openai", "gpt-4o-mini").unwrap();
    assert_eq!(get_builtin_model("openai", "gpt-4o").unwrap().id, "gpt-4o");
    assert_eq!(compat::get_model("openai", "gpt-4o").unwrap().id, "gpt-4o");
}

#[tokio::test]
async fn unknown_model_types_are_dropped_from_stores_and_fetches() {
    let entry = ModelsStoreEntry::from_value_lenient(json!({"models": [
        serde_json::to_value(chat_model("dyn", "stored-chat")).unwrap(),
        image_value("dyn", "stored-image"),
        {"type": "embedding", "id": "future-embedding", "name": "e", "api": "test-chat", "provider": "dyn", "baseUrl": "", "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}},
        {"type": "video", "id": "future-video", "name": "v", "api": "test-images", "provider": "dyn", "baseUrl": "", "input": ["text"], "output": ["video"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}}
    ]}))
    .unwrap();
    assert_eq!(ids(&entry.models), ["stored-chat", "stored-image"]);
    let store = Arc::new(InMemoryModelsStore::new());
    store.write("dyn", entry, None).await.unwrap();
    let fetched = Arc::new(Mutex::new(Vec::<AnyModel>::new()));
    let source = fetched.clone();
    let models = create_models(CreateModelsOptions { models_store: Some(store.clone()), ..Default::default() });
    let mut apis: indexmap::IndexMap<String, Arc<dyn ProviderStreams>> = indexmap::IndexMap::new();
    apis.insert("test-chat".into(), Arc::new(NoopStreams));
    models.set_provider(
        create_provider(CreateProviderOptions {
            id: "dyn".into(),
            auth: ProviderAuth { api_key: Some(Arc::new(EnvKeyAuth(None))), oauth: None },
            fetch_models: Some(Arc::new(move |_ctx| {
                let models = source.lock().clone();
                Box::pin(async move { Ok(models) })
            })),
            api: Some(ProviderApi::ByApi(apis)),
            ..Default::default()
        })
        .unwrap(),
    );
    let restored = models.refresh(ModelsRefreshOptions { providers: Some(vec!["dyn".into()]), allow_network: Some(false), ..Default::default() }).await;
    assert!(restored.errors.is_empty(), "{:?}", restored.errors);
    assert_eq!(ids(&models.get_all_models(Some("dyn"))), ["stored-chat", "stored-image"]);

    // Fetched models of unknown types never deserialize; the fetch returns only known ones.
    let fetched_values = vec![serde_json::to_value(chat_model("dyn", "fetched-chat")).unwrap(), json!({"type": "video", "id": "fetched-video"})];
    *fetched.lock() = fetched_values.into_iter().filter_map(|v| AnyModel::from_value(v).ok()).collect();
    let refreshed = models.refresh(ModelsRefreshOptions { providers: Some(vec!["dyn".into()]), ..Default::default() }).await;
    assert!(refreshed.errors.is_empty(), "{:?}", refreshed.errors);
    assert_eq!(ids(&models.get_all_models(Some("dyn"))), ["fetched-chat"]);
    assert_eq!(ids(&store.read("dyn", None).await.unwrap().unwrap().models), ["fetched-chat"]);
}

// ---------------------------------------------------------------------------
// Port of radius-provider.test.ts
// ---------------------------------------------------------------------------

fn radius_config(base_url: &str) -> Value {
    json!({"baseUrl": base_url, "models": [
        {"id": "balanced", "name": "Fresh Balanced", "reasoning": true, "input": ["text"], "cost": {"input": 1, "output": 2, "cacheRead": 0.1, "cacheWrite": 0}, "contextWindow": 424242, "maxTokens": 32000},
        {"id": "organization-only", "name": "Organization Only", "reasoning": false, "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 128000, "maxTokens": 16000}
    ]})
}

async fn with_key(id: &str) -> Arc<InMemoryCredentialStore> {
    let store = Arc::new(InMemoryCredentialStore::new());
    let credential = Credential::ApiKey(pi_ai::auth::ApiKeyCredential { key: Some("radius-key".into()), env: None });
    store.modify(id, Box::new(move |_| Box::pin(async move { Ok(Some(credential)) })), None).await.unwrap();
    store
}

#[tokio::test]
async fn radius_catalogs() {
    use pi_ai::providers::radius::{RadiusProviderOptions, radius_provider};
    let default = radius_provider(RadiusProviderOptions::default());
    let baseline = default.get_models();
    assert!(baseline.iter().any(|m| m.id == "balanced" && m.provider == "radius" && m.api == "pi-messages"));
    let custom = radius_provider(RadiusProviderOptions { id: Some("radius-dev".into()), gateway: Some("http://localhost:8788".into()), ..Default::default() });
    assert!(custom.get_models().is_empty());

    // A refreshed catalog replaces the static one, including models it no longer lists.
    let server = MockServer::start(|request, _| {
        assert_eq!(request.path, "/v1/config");
        assert_eq!(request.header("authorization"), Some("Bearer radius-key"));
        Response::json(200, radius_config("https://radius.example/v1"))
    })
    .await;
    let models = create_models(CreateModelsOptions { credentials: Some(with_key("radius").await), ..Default::default() });
    models.set_provider(radius_provider(RadiusProviderOptions { id: Some("radius".into()), gateway: Some(server.url.clone()), ..Default::default() }));
    let result = models.refresh(ModelsRefreshOptions { providers: Some(vec!["radius".into()]), ..Default::default() }).await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let balanced = models.get_model("radius", "balanced").unwrap();
    assert_eq!((balanced.name.as_str(), balanced.base_url.as_str(), balanced.context_window), ("Fresh Balanced", "https://radius.example/v1", 424242));
    assert_eq!(models.get_models(Some("radius")).iter().map(|m| m.id.clone()).collect::<Vec<_>>(), ["balanced", "organization-only"]);

    // A cached catalog replaces the static one without network access.
    let config: pi_ai::providers::radius_config::RadiusGatewayConfig = serde_json::from_value(radius_config("https://radius.example/v1")).unwrap();
    let cached = pi_ai::providers::radius_config::get_radius_models_from_config("radius", &config);
    let store = Arc::new(InMemoryModelsStore::new());
    store
        .write("radius", ModelsStoreEntry { models: cached.into_iter().map(AnyModel::Chat).collect(), checked_at: Some(pi_ai::utils::now_ms()), ..Default::default() }, None)
        .await
        .unwrap();
    let models = create_models(CreateModelsOptions { models_store: Some(store), ..Default::default() });
    models.set_provider(radius_provider(RadiusProviderOptions::default()));
    models.refresh(ModelsRefreshOptions { providers: Some(vec!["radius".into()]), allow_network: Some(false), ..Default::default() }).await;
    assert_eq!(models.get_model("radius", "balanced").unwrap().name, "Fresh Balanced");
    assert_eq!(models.get_models(Some("radius")).iter().map(|m| m.id.clone()).collect::<Vec<_>>(), ["balanced", "organization-only"]);

    // An organization that disabled every model exposes none.
    let server = MockServer::start(|_, _| Response::json(200, json!({"baseUrl": "https://radius.example/v1", "models": []}))).await;
    let models = create_models(CreateModelsOptions { credentials: Some(with_key("radius").await), ..Default::default() });
    models.set_provider(radius_provider(RadiusProviderOptions { id: Some("radius".into()), gateway: Some(server.url.clone()), ..Default::default() }));
    let result = models.refresh(ModelsRefreshOptions { providers: Some(vec!["radius".into()]), ..Default::default() }).await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(models.get_models(Some("radius")).is_empty());
}

// ---------------------------------------------------------------------------
// Port of openrouter-user-models.test.ts
// ---------------------------------------------------------------------------

fn openrouter_chat(id: &str) -> AnyModel {
    AnyModel::Chat(model(json!({"id": id, "name": id, "api": "openai-completions", "provider": "openrouter", "baseUrl": "https://openrouter.ai/api/v1",
        "reasoning": false, "input": ["text"], "cost": {"input": 1, "output": 2, "cacheRead": 0.1, "cacheWrite": 0}, "contextWindow": 200000, "maxTokens": 50000})))
}

fn refresh_context(base_url: &str) -> RefreshModelsContext {
    RefreshModelsContext {
        credential: Some(Credential::ApiKey(pi_ai::auth::ApiKeyCredential { key: Some("sk-or-test".into()), env: None })),
        stored: None,
        publish: Arc::new(|_| Box::pin(async { Ok(true) })),
        allow_network: true,
        force: None,
        signal: CancellationToken::new(),
        base_url: Some(base_url.to_string()),
    }
}

#[tokio::test]
async fn openrouter_catalog_is_personalized_to_the_key() {
    use pi_ai::providers::openrouter::personalize_open_router_catalog;
    let data = json!({"data": [
        {"id": "listed", "context_length": 1000000, "top_provider": {"context_length": 262144, "max_completion_tokens": 32768}, "pricing": {"prompt": "0.000003", "completion": "0.000015"}},
        {"id": "no-limits", "context_length": 0, "top_provider": {"context_length": null, "max_completion_tokens": null}},
        {"id": "router", "pricing": {"prompt": "-1", "completion": "-1"}}
    ]});
    let server = MockServer::start(move |request, _| {
        assert_eq!(request.path, "/models/user");
        assert_eq!(request.header("authorization"), Some("Bearer sk-or-test"));
        Response::json(200, data.clone())
    })
    .await;
    let image = AnyModel::Image(serde_json::from_value(json!({"type": "image", "id": "image", "name": "image", "api": "openrouter-images", "provider": "openrouter",
        "baseUrl": "https://openrouter.ai/api/v1", "input": ["text"], "output": ["image"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}})).unwrap());
    let classifier: AnyModel = serde_json::from_value(json!({"type": "classifier", "id": "classifier", "name": "classifier", "api": "typesafe-system-one", "provider": "openrouter",
        "baseUrl": "https://openrouter.ai/api/v1", "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 64000})).unwrap();
    let catalog = vec![openrouter_chat("listed"), openrouter_chat("no-limits"), openrouter_chat("router"), openrouter_chat("blocked"), image.clone(), classifier.clone()];
    let result = personalize_open_router_catalog(catalog, &refresh_context(&server.url)).await.unwrap();
    assert_eq!(ids(&result), ["listed", "no-limits", "router", "image", "classifier"]);
    let listed = result[0].as_chat().unwrap();
    assert_eq!((listed.context_window, listed.max_tokens), (262144, 32768));
    assert_eq!(serde_json::to_value(&listed.cost).unwrap(), json!({"input": 3, "output": 15, "cacheRead": 0, "cacheWrite": 0}));
    let no_limits = result[1].as_chat().unwrap();
    assert_eq!((no_limits.context_window, no_limits.max_tokens), (200000, 50000));
    assert_eq!(serde_json::to_value(&no_limits.cost).unwrap(), json!({"input": 1, "output": 2, "cacheRead": 0.1, "cacheWrite": 0}));
    assert_eq!(serde_json::to_value(&result[2].as_chat().unwrap().cost).unwrap(), json!({"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}));
    assert_eq!(result[3], image);
    assert_eq!(result[4], classifier);

    for status in [401u16, 403, 404] {
        let server = MockServer::start(move |_, _| Response::text(status, "")).await;
        let catalog = vec![openrouter_chat("a")];
        let result = personalize_open_router_catalog(catalog.clone(), &refresh_context(&server.url)).await.unwrap();
        assert_eq!(result, catalog, "{status}");
    }
}

// ---------------------------------------------------------------------------
// Port of opencode-provider-headers.test.ts
// ---------------------------------------------------------------------------

struct RecordingStreams(Arc<Mutex<Option<StreamOptions>>>);

impl ProviderStreams for RecordingStreams {
    fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        *self.0.lock() = Some(options.clone());
        NoopStreams.stream(model, context, options)
    }
    fn stream_simple(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
        *self.0.lock() = Some(options.clone());
        NoopStreams.stream(model, context, options)
    }
}

#[test]
fn opencode_session_header_wrapper() {
    use pi_ai::providers::opencode::WithOpenCodeSessionHeader;
    let model = model(json!({"id": "test-model", "name": "Test model", "api": "test-api", "provider": "opencode", "baseUrl": "https://opencode.ai/zen/v1",
        "reasoning": false, "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 1000, "maxTokens": 100}));
    let context = normalize_context(&user_ctx("hi"));
    let headers_of = |options: StreamOptions, simple: bool| {
        let captured = Arc::new(Mutex::new(None));
        let streams = WithOpenCodeSessionHeader(Arc::new(RecordingStreams(captured.clone())));
        if simple {
            streams.stream_simple(&model, &context, options);
        } else {
            streams.stream(&model, &context, options);
        }
        let options = captured.lock().clone().unwrap();
        options.headers.map(|h| h.into_iter().collect::<Vec<_>>()).unwrap_or_default()
    };
    // Regression test for https://github.com/earendil-works/pi/issues/9326
    for simple in [false, true] {
        let options = StreamOptions { session_id: Some("conversation-1".into()), cache_retention: Some(CacheRetention::None), ..Default::default() };
        assert_eq!(headers_of(options, simple), vec![("x-opencode-session".to_string(), Some("conversation-1".to_string()))]);
    }
    for value in [Some("caller-value".to_string()), None] {
        let mut headers = ProviderHeaders::new();
        headers.insert("X-OpenCode-Session".into(), value.clone());
        let options = StreamOptions { session_id: Some("generated-value".into()), headers: Some(headers), ..Default::default() };
        assert_eq!(headers_of(options, true), vec![("X-OpenCode-Session".to_string(), value)]);
    }
    let mut headers = ProviderHeaders::new();
    headers.insert("x-custom".into(), Some("value".into()));
    assert_eq!(headers_of(StreamOptions { headers: Some(headers), ..Default::default() }, true), vec![("x-custom".to_string(), Some("value".to_string()))]);
}

// ---------------------------------------------------------------------------
// Port of compat-env.test.ts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn compat_dispatches_unknown_providers_through_the_api_registry() {
    // The TS test overrides the global "openai-responses" registration; a dedicated api name keeps
    // this binary's other compat dispatches unaffected.
    let captured = Arc::new(Mutex::new(None::<StreamOptions>));
    compat::register_api_provider("ts-ported-legacy-api", Arc::new(RecordingStreams(captured.clone())), Some("ts-ported-compat-env"));
    let mut custom = chat_model("custom-openai", "test-model");
    custom.api = "ts-ported-legacy-api".into();
    let result = compat::complete(&custom, &user_ctx("hi"), StreamOptions { api_key: Some("request-key".into()), ..Default::default() }).await;
    compat::unregister_api_providers("ts-ported-compat-env");
    assert_eq!(result.stop_reason, StopReason::Stop, "{:?}", result.error_message);
    assert_eq!(captured.lock().clone().unwrap().api_key.as_deref(), Some("request-key"));
}

// ---------------------------------------------------------------------------
// Port of message-types.test.ts (runtime part; the rest are compile-time type checks)
// ---------------------------------------------------------------------------

#[test]
fn tool_result_details_round_trip_json_values() {
    for details in [json!({"path": "a.ts", "items": ["first"], "summary": {"lines": 3}}), json!(["a", "b"]), json!(["a", {"count": 1}]), json!("diagnostic"), Value::Null] {
        let value = json!({"role": "toolResult", "toolCallId": "call-1", "toolName": "read", "content": [], "isError": false, "timestamp": 1, "details": details});
        let message: Message = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(message.as_tool_result().unwrap().details.as_ref(), Some(&details));
        assert_eq!(serde_json::to_value(&message).unwrap(), value);
    }
}

// ---------------------------------------------------------------------------
// Port of max-thinking.test.ts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn max_thinking_level() {
    let level = |l: ModelThinkingLevel| serde_json::to_value(l).unwrap().as_str().unwrap().to_string();
    let levels = |m: &Model| get_supported_thinking_levels(m).into_iter().map(level).collect::<Vec<_>>();
    let ordinary = model(json!({"id": "ordinary-reasoning", "name": "Ordinary Reasoning", "api": "openai-completions", "provider": "test", "baseUrl": "https://example.com/v1",
        "reasoning": true, "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 128000, "maxTokens": 4096}));
    assert_eq!(levels(&ordinary), ["off", "minimal", "low", "medium", "high"]);
    assert_eq!(level(clamp_thinking_level(&ordinary, ModelThinkingLevel::Max)), "high");
    for id in ["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-6-luna", "gpt-6-sol"] {
        let model = get_builtin_model("openai-codex", id).unwrap();
        let map = serde_json::to_value(&model.thinking_level_map).unwrap();
        assert_eq!((map["xhigh"].clone(), map["max"].clone()), (json!("xhigh"), json!("max")), "{id}");
        assert_eq!(levels(&model), ["off", "minimal", "low", "medium", "high", "xhigh", "max"], "{id}");
    }
    let mut hole = ordinary.clone();
    hole.thinking_level_map = Some(serde_json::from_value(json!({"xhigh": null, "max": "max"})).unwrap());
    assert_eq!(levels(&hole), ["off", "minimal", "low", "medium", "high", "max"]);
    assert_eq!(level(clamp_thinking_level(&hole, ModelThinkingLevel::Xhigh)), "max");

    let token = {
        use base64::Engine;
        let payload = base64::engine::general_purpose::STANDARD.encode(json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acc_test"}}).to_string());
        format!("aaa.{payload}.bbb")
    };
    for id in ["gpt-5.6-sol", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-6.1-sol"] {
        let model = get_builtin_model("openai-codex", id).unwrap();
        let context = normalize_context(&ctx(json!({"systemPrompt": "You are a helpful assistant.", "messages": [{"role": "user", "content": "Hello"}]})));
        let signal = CancellationToken::new();
        let cancel = signal.clone();
        let options = StreamOptions {
            api_key: Some(token.clone()),
            reasoning: Some(ThinkingLevel::Max),
            signal: Some(signal),
            transport: Some(Transport::Sse),
            ..Default::default()
        };
        let (payload, _) = capture_payload_with(
            move |o| {
                let o = StreamOptions {
                    on_payload: o.on_payload.clone().map(|inner| {
                        let cancel = cancel.clone();
                        Arc::new(move |p: Value, m: Model| {
                            cancel.cancel();
                            inner(p, m)
                        }) as OnPayload
                    }),
                    ..o
                };
                pi_ai::api::openai_codex_responses::stream_simple(&model, &context, o)
            },
            options,
        )
        .await;
        assert_eq!(payload["reasoning"]["effort"], "max", "{id}");
        assert_eq!(payload["reasoning"]["summary"], "auto", "{id}");
    }
}
