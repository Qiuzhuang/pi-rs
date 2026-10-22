//! Ports of `test/remote-catalog-provider.test.ts` and `test/radius.test.ts`.
//!
//! TS spies on `fetch`; here the catalog and gateway URLs point at a local server (pi-ai's
//! `mock_http`). The built-in Radius provider's gateway is fixed, so the network cases of
//! `radius.test.ts` run against a models.json Radius gateway, which uses the same provider code.

#[path = "../../pi-ai/tests/mock_http/mod.rs"]
mod mock_http;

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use indexmap::IndexMap;
use mock_http::{MockServer, Response};
use pi_ai::auth::types::{
    ApiKeyAuth, ApiKeyCredential, ApiKeyResolveInput, AuthResult, Credential, ModelAuth, OAuthCredentials, ProviderAuth,
};
use pi_ai::*;
use pi_coding_agent::config::VERSION;
use pi_coding_agent::core::auth_storage::AuthStorage;
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use pi_coding_agent::core::radius::RADIUS_PROVIDER_ID;
use pi_coding_agent::core::remote_catalog_provider::{
    PersonalizeCatalog, REMOTE_CATALOG_MODEL_TYPES, with_personalized_catalog, with_remote_catalog,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn model_value(id: &str) -> Value {
    json!({
        "id": id, "name": id, "api": "openai-completions", "provider": "test-provider",
        "baseUrl": "https://example.test/v1", "reasoning": false, "input": ["text"],
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
        "contextWindow": 1000, "maxTokens": 100
    })
}

fn model(id: &str) -> Model {
    serde_json::from_value(model_value(id)).unwrap()
}

struct NoopAuth;

#[async_trait::async_trait]
impl ApiKeyAuth for NoopAuth {
    fn name(&self) -> &str {
        "Test"
    }
    async fn resolve(&self, _input: ApiKeyResolveInput) -> anyhow::Result<Option<AuthResult>> {
        Ok(Some(AuthResult { auth: ModelAuth::default(), env: None, source: None }))
    }
}

struct UnusedStreams;

impl ProviderStreams for UnusedStreams {
    fn stream(&self, _model: &Model, _context: &TranscriptContext, _options: StreamOptions) -> AssistantMessageEventStream {
        panic!("not used")
    }
    fn stream_simple(&self, _model: &Model, _context: &TranscriptContext, _options: StreamOptions) -> AssistantMessageEventStream {
        panic!("not used")
    }
}

fn builtin_provider() -> Arc<dyn Provider> {
    create_provider(CreateProviderOptions {
        id: "test-provider".into(),
        auth: ProviderAuth { api_key: Some(Arc::new(NoopAuth)), oauth: None },
        models: vec![AnyModel::Chat(model("static"))],
        api: Some(ProviderApi::Single(Arc::new(UnusedStreams))),
        ..Default::default()
    })
    .unwrap()
}

fn test_provider(server: &MockServer, local_generated_at: Option<i64>) -> Arc<dyn Provider> {
    with_remote_catalog(builtin_provider(), Some(&server.url), local_generated_at)
}

#[derive(Default)]
struct RefreshOverrides {
    allow_network: Option<bool>,
    force: Option<bool>,
}

/// TS `refreshProvider`: run `refreshModels` with a publish that writes through to `store`.
async fn refresh_provider(provider: &Arc<dyn Provider>, store: &Arc<InMemoryModelsStore>, overrides: RefreshOverrides) -> anyhow::Result<()> {
    let id = provider.id().to_string();
    let publish_store = store.clone();
    let publish: PublishFn = Arc::new(move |publication: ModelsPublication| {
        let store = publish_store.clone();
        let id = id.clone();
        async move {
            match publication.persist {
                Some(None) => store.delete(&id, None).await?,
                Some(Some(entry)) => store.write(&id, entry, None).await?,
                None => {}
            }
            if let Some(update) = publication.update {
                update();
            }
            Ok(true)
        }
        .boxed()
    });
    provider
        .refresh_models(RefreshModelsContext {
            credential: Some(Credential::ApiKey(ApiKeyCredential::default())),
            stored: store.read(provider.id(), None).await?,
            publish,
            allow_network: overrides.allow_network.unwrap_or(true),
            force: overrides.force,
            signal: CancellationToken::new(),
            base_url: None,
        })
        .await
}

fn ids(models: &[Model]) -> Vec<String> {
    models.iter().map(|m| m.id.clone()).collect()
}

fn any_ids(models: &[AnyModel]) -> Vec<String> {
    models.iter().map(|m| m.id().to_string()).collect()
}

fn catalog_response(models: &[&str]) -> Response {
    let body: serde_json::Map<String, Value> = models.iter().map(|id| (id.to_string(), model_value(id))).collect();
    Response::json(200, Value::Object(body))
}

// ---------------------------------------------------------------------------
// remote catalog provider
// ---------------------------------------------------------------------------

#[tokio::test]
async fn parses_keyed_catalogs_sends_version_headers_observes_the_ttl_and_supports_forced_refreshes() {
    let server = MockServer::start(|_, _| catalog_response(&["dynamic"])).await;
    let provider = test_provider(&server, None);
    let store = Arc::new(InMemoryModelsStore::new());
    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();
    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();
    refresh_provider(&provider, &store, RefreshOverrides { force: Some(true), ..Default::default() }).await.unwrap();

    assert_eq!(ids(&provider.get_models()), vec!["static", "dynamic"]);
    assert_eq!(any_ids(&store.read("test-provider", None).await.unwrap().unwrap().models), vec!["dynamic"]);
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].header("user-agent").unwrap_or_default().contains(&format!("pi/{VERSION}")));
    assert_eq!(requests[0].path_only(), "/api/models/providers/test-provider");
    let query = requests[0].path.split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
    let types = url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "types")
        .map(|(_, v)| v.into_owned());
    assert_eq!(types, Some(REMOTE_CATALOG_MODEL_TYPES.join(",")));
}

#[tokio::test]
async fn overlays_image_and_classifier_models_and_drops_unknown_model_types() {
    let mut chat = model_value("chat");
    chat["type"] = json!("chat");
    let mut clip = model_value("clip");
    clip["type"] = json!("video");
    let body = json!({
        "chat": chat,
        "flux": {
            "type": "image", "id": "flux", "name": "FLUX", "api": "openrouter-images", "provider": "test-provider",
            "baseUrl": "https://example.test/v1", "input": ["text"], "output": ["image"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        },
        "jev": {
            "type": "classifier", "id": "jev", "name": "Jev", "api": "typesafe-system-one", "provider": "test-provider",
            "baseUrl": "https://example.test/v1", "input": ["text"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }, "contextWindow": 64000
        },
        "clip": clip
    });
    let server = MockServer::start(move |_, _| Response::json(200, body.clone())).await;
    let provider = test_provider(&server, None);
    let store = Arc::new(InMemoryModelsStore::new());
    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();

    let models = create_models(CreateModelsOptions { models_store: Some(store.clone()), ..Default::default() });
    models.set_provider(provider.clone());
    assert_eq!(any_ids(&models.get_all_models(Some("test-provider"))), vec!["static", "chat", "flux", "jev"]);
    assert!(matches!(models.get_model_of_type(ModelType::Image, "test-provider", "flux"), Some(AnyModel::Image(_))));
    assert!(matches!(
        models.get_model_of_type(ModelType::Classifier, "test-provider", "jev"),
        Some(AnyModel::Classifier(_))
    ));
    assert!(models.get_model("test-provider", "flux").is_none());
    assert_eq!(any_ids(&store.read("test-provider", None).await.unwrap().unwrap().models), vec!["chat", "flux", "jev"]);
}

fn http_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap().format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

#[tokio::test]
async fn prefers_the_newer_of_the_generated_and_remote_catalogs() {
    let local_generated_at = chrono::DateTime::parse_from_rfc3339("2026-07-23T10:00:00.000Z").unwrap().timestamp_millis();
    let older = http_date(local_generated_at - 60_000);
    let newer = http_date(local_generated_at + 60_000);
    let newer_ms = local_generated_at + 60_000;
    let server = MockServer::sequence(vec![
        catalog_response(&["old"]).with_header("last-modified", &older),
        catalog_response(&["newer"]).with_header("last-modified", &newer),
    ])
    .await;
    let provider = test_provider(&server, Some(local_generated_at));
    let store = Arc::new(InMemoryModelsStore::new());

    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();
    assert_eq!(ids(&provider.get_models()), vec!["static"]);

    refresh_provider(&provider, &store, RefreshOverrides { force: Some(true), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&provider.get_models()), vec!["static", "newer"]);
    assert_eq!(store.read("test-provider", None).await.unwrap().unwrap().last_modified, Some(newer_ms));
}

#[tokio::test]
async fn revalidates_a_stored_catalog_with_its_etag_and_keeps_the_overlay_on_304() {
    let server = MockServer::sequence(vec![
        catalog_response(&["dynamic"]).with_header("etag", "\"catalog-1\""),
        Response::new(304, "text/plain", "").with_header("etag", "\"catalog-1\""),
    ])
    .await;
    let provider = test_provider(&server, None);
    let store = Arc::new(InMemoryModelsStore::new());

    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();
    assert_eq!(server.request(0).header("if-none-match"), None);
    let stored = store.read("test-provider", None).await.unwrap().unwrap();
    assert_eq!(stored.etag.as_deref(), Some("\"catalog-1\""));

    let checked_at = stored.checked_at.unwrap_or(0);
    refresh_provider(&provider, &store, RefreshOverrides { force: Some(true), ..Default::default() }).await.unwrap();
    assert_eq!(server.request(1).header("if-none-match"), Some("\"catalog-1\""));
    assert_eq!(ids(&provider.get_models()), vec!["static", "dynamic"]);
    let stored = store.read("test-provider", None).await.unwrap().unwrap();
    assert_eq!(any_ids(&stored.models), vec!["dynamic"]);
    assert_eq!(stored.etag.as_deref(), Some("\"catalog-1\""));
    assert!(stored.checked_at.unwrap() >= checked_at);
}

#[tokio::test]
async fn drops_a_stale_etag_when_the_overlay_becomes_unavailable() {
    let server = MockServer::sequence(vec![
        catalog_response(&["dynamic"]).with_header("etag", "\"catalog-1\""),
        Response::text(501, "not implemented"),
    ])
    .await;
    let provider = test_provider(&server, None);
    let store = Arc::new(InMemoryModelsStore::new());
    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();
    refresh_provider(&provider, &store, RefreshOverrides { force: Some(true), ..Default::default() }).await.unwrap();
    assert_eq!(store.read("test-provider", None).await.unwrap().unwrap().etag, None);
}

#[tokio::test]
async fn keeps_the_etag_and_overlay_after_a_transient_failure() {
    let server = MockServer::sequence(vec![
        catalog_response(&["dynamic"]).with_header("etag", "\"catalog-1\""),
        Response::text(429, "rate limited"),
        Response::text(429, "rate limited"),
        Response::text(429, "rate limited"),
        Response::new(304, "text/plain", "").with_header("etag", "\"catalog-1\""),
    ])
    .await;
    let provider = test_provider(&server, None);
    let store = Arc::new(InMemoryModelsStore::new());

    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();
    let error = refresh_provider(&provider, &store, RefreshOverrides { force: Some(true), ..Default::default() })
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("429"), "{error:#}");

    let stored = store.read("test-provider", None).await.unwrap().unwrap();
    assert_eq!(stored.etag.as_deref(), Some("\"catalog-1\""));
    assert_eq!(any_ids(&stored.models), vec!["dynamic"]);

    refresh_provider(&provider, &store, RefreshOverrides { force: Some(true), ..Default::default() }).await.unwrap();
    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[4].header("if-none-match"), Some("\"catalog-1\""));
    assert_eq!(ids(&provider.get_models()), vec!["static", "dynamic"]);
}

#[tokio::test]
async fn lets_a_newer_catalog_request_bypass_a_stalled_older_request_without_stale_publication() {
    // TS resolves the first fetch on demand; here the first response is delayed past the second.
    let server = MockServer::start(|_, index| {
        if index == 0 { catalog_response(&["older"]).with_delay(1_000) } else { catalog_response(&["newer"]) }
    })
    .await;
    let provider = test_provider(&server, None);
    let store = Arc::new(InMemoryModelsStore::new());
    let models = create_models(CreateModelsOptions { models_store: Some(store.clone()), ..Default::default() });
    models.set_provider(provider.clone());

    let refresh = |models: Models| async move {
        models
            .refresh(ModelsRefreshOptions {
                providers: Some(vec!["test-provider".into()]),
                force: Some(true),
                ..Default::default()
            })
            .await
    };
    let first = tokio::spawn(refresh(models.clone()));
    for _ in 0..200 {
        if !server.requests().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(server.requests().len(), 1);
    refresh(models.clone()).await;
    assert_eq!(ids(&provider.get_models()), vec!["static", "newer"]);
    first.await.unwrap();
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    assert_eq!(ids(&provider.get_models()), vec!["static", "newer"]);
    assert_eq!(any_ids(&store.read("test-provider", None).await.unwrap().unwrap().models), vec!["newer"]);
}

#[tokio::test]
async fn treats_unimplemented_pi_dev_catalog_routes_as_an_unavailable_overlay() {
    let server = MockServer::start(|_, _| Response::text(501, "not implemented")).await;
    let provider = test_provider(&server, None);
    let store = Arc::new(InMemoryModelsStore::new());
    refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap();
    assert_eq!(ids(&provider.get_models()), vec!["static"]);
    let stored = store.read("test-provider", None).await.unwrap().unwrap();
    assert!(stored.models.is_empty());
    assert!(stored.checked_at.is_some());
}

// ---------------------------------------------------------------------------
// personalized catalog provider
// ---------------------------------------------------------------------------

fn image_model() -> AnyModel {
    serde_json::from_value(json!({
        "type": "image", "id": "image", "name": "image", "api": "openrouter-images", "provider": "test-provider",
        "baseUrl": "https://example.test/v1", "input": ["text"], "output": ["image"],
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
    }))
    .unwrap()
}

#[tokio::test]
async fn keeps_the_saved_list_and_saves_nothing_when_personalization_fails() {
    // Saving the catalog without personalization would replace the key's list with every model.
    let server = MockServer::start(|_, _| Response::text(404, "not found")).await;
    let store = Arc::new(InMemoryModelsStore::new());
    let saved = ModelsStoreEntry { models: vec![AnyModel::Chat(model("allowed"))], checked_at: Some(1), ..Default::default() };
    store.write("test-provider", saved.clone(), None).await.unwrap();
    let personalize: PersonalizeCatalog = Arc::new(|_, _| async { anyhow::bail!("user models request failed") }.boxed());
    let provider = with_personalized_catalog(builtin_provider(), &personalize, Some(&server.url), None);

    let error = refresh_provider(&provider, &store, RefreshOverrides::default()).await.unwrap_err();
    assert!(error.to_string().contains("user models request failed"));
    assert_eq!(ids(&provider.get_models()), vec!["allowed"]);
    assert_eq!(store.read("test-provider", None).await.unwrap(), Some(saved));
}

#[tokio::test]
async fn restores_a_saved_list_as_is_and_falls_back_to_the_built_in_models_only_when_it_is_empty() {
    let personalize: PersonalizeCatalog = Arc::new(|_, _| async { anyhow::bail!("not used offline") }.boxed());
    let store = Arc::new(InMemoryModelsStore::new());
    let provider = with_personalized_catalog(builtin_provider(), &personalize, None, None);
    let offline = || RefreshOverrides { allow_network: Some(false), ..Default::default() };

    // pi versions before per-key lists stored the plain pi.dev catalog here, and saved an empty list
    // after a missing pi.dev catalog or a failed first request. Only such an entry can be empty.
    store.write("test-provider", ModelsStoreEntry { models: vec![], checked_at: Some(1), ..Default::default() }, None).await.unwrap();
    refresh_provider(&provider, &store, offline()).await.unwrap();
    assert_eq!(ids(&provider.get_models()), vec!["static"]);

    // A key that may use no chat models keeps its image models and shows no chat models.
    store
        .write("test-provider", ModelsStoreEntry { models: vec![image_model()], checked_at: Some(1), ..Default::default() }, None)
        .await
        .unwrap();
    refresh_provider(&provider, &store, offline()).await.unwrap();
    assert!(provider.get_models().is_empty());
    assert_eq!(any_ids(&provider.get_all_models()), vec!["image"]);
}

// ---------------------------------------------------------------------------
// radius.test.ts
// ---------------------------------------------------------------------------

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn radius_config(base_url: &str) -> Value {
    json!({
        "baseUrl": base_url,
        "models": [{
            "id": "auto", "name": "Radius Auto", "reasoning": false, "input": ["text"],
            "cost": { "input": 1, "output": 2, "cacheRead": 0.1, "cacheWrite": 0.2 },
            "contextWindow": 128000, "maxTokens": 16384
        }]
    })
}

fn radius_oauth(gateway_config: Option<Value>) -> Credential {
    let mut extra = serde_json::Map::new();
    if let Some(config) = gateway_config {
        extra.insert("gatewayConfig".into(), config);
    }
    Credential::OAuth(OAuthCredentials {
        access: "access-token".into(),
        refresh: "refresh-token".into(),
        expires: now_ms() + 3_600_000,
        extra,
    })
}

async fn radius_runtime(
    credentials: &[(&str, Credential)],
    models_json: Option<Value>,
    allow_model_network: Option<bool>,
    models_store: Arc<InMemoryModelsStore>,
) -> (Arc<ModelRuntime>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let models_path = match models_json {
        Some(json) => {
            let path = dir.path().join("models.json");
            std::fs::write(&path, json.to_string()).unwrap();
            path.to_string_lossy().into_owned()
        }
        None => String::new(),
    };
    let data: IndexMap<String, Credential> = credentials.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    let runtime = ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(AuthStorage::in_memory(Some(&data))),
        models_store: Some(models_store),
        models_path: Some(models_path),
        allow_model_network,
        ..Default::default()
    }))
    .await
    .unwrap();
    (runtime, dir)
}

#[tokio::test]
async fn restores_the_legacy_credential_catalog_without_network_access() {
    let (runtime, _dir) = radius_runtime(
        &[(RADIUS_PROVIDER_ID, radius_oauth(Some(radius_config("https://radius.example.com/v1"))))],
        None,
        Some(false),
        Arc::new(InMemoryModelsStore::new()),
    )
    .await;
    let model = runtime.get_model(RADIUS_PROVIDER_ID, "auto").unwrap();
    assert_eq!(model.api, "pi-messages");
    assert_eq!(model.base_url, "https://radius.example.com/v1");
    assert_eq!(runtime.get_provider(RADIUS_PROVIDER_ID).unwrap().name(), "Radius");
    assert!(runtime.has_configured_auth(RADIUS_PROVIDER_ID));
}

fn radius_gateway_json(server: &MockServer) -> Value {
    json!({ "providers": { "radius-dev": { "name": "Radius (dev)", "baseUrl": server.url, "oauth": "radius" } } })
}

#[tokio::test]
async fn fetches_and_stores_the_catalog_for_configured_radius_auth() {
    let server = MockServer::start(|_, _| Response::json(200, radius_config("https://radius.example.com/v1"))).await;
    let store = Arc::new(InMemoryModelsStore::new());
    let (runtime, _dir) =
        radius_runtime(&[("radius-dev", radius_oauth(None))], Some(radius_gateway_json(&server)), Some(true), store.clone()).await;

    assert!(runtime.get_model("radius-dev", "auto").is_some());
    assert_eq!(store.read("radius-dev", None).await.unwrap().unwrap().models.len(), 1);
    let request = server.requests().into_iter().find(|r| r.path == "/v1/config").expect("config request");
    assert_eq!(request.header("authorization"), Some("Bearer access-token"));
}

#[tokio::test]
async fn does_not_refresh_catalogs_over_the_network_by_default() {
    let server = MockServer::start(|_, _| Response::text(500, "unexpected catalog fetch")).await;
    let (runtime, _dir) = radius_runtime(
        &[
            (RADIUS_PROVIDER_ID, radius_oauth(Some(radius_config("https://radius.example.com/v1")))),
            ("radius-dev", radius_oauth(None)),
        ],
        Some(radius_gateway_json(&server)),
        None,
        Arc::new(InMemoryModelsStore::new()),
    )
    .await;
    assert!(runtime.get_model(RADIUS_PROVIDER_ID, "auto").is_some());
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn does_not_fetch_or_make_radius_models_available_without_configured_auth() {
    let server = MockServer::start(|_, _| Response::json(200, radius_config("https://radius.example.com/v1"))).await;
    let (runtime, _dir) =
        radius_runtime(&[], Some(radius_gateway_json(&server)), Some(true), Arc::new(InMemoryModelsStore::new())).await;
    assert!(
        !runtime
            .get_available_snapshot()
            .iter()
            .any(|m| m.provider == RADIUS_PROVIDER_ID || m.provider == "radius-dev")
    );
    assert!(!server.requests().iter().any(|r| r.path.contains("/v1/config")));
}

#[tokio::test]
async fn supports_custom_radius_gateways_from_models_json() {
    let server = MockServer::start(|_, _| Response::json(200, radius_config("http://localhost:8788/v1"))).await;
    let (runtime, _dir) =
        radius_runtime(&[("radius-dev", radius_oauth(None))], Some(radius_gateway_json(&server)), Some(true), Arc::new(InMemoryModelsStore::new()))
            .await;
    let model = runtime.get_model("radius-dev", "auto").unwrap();
    assert_eq!(model.api, "pi-messages");
    assert_eq!(model.base_url, "http://localhost:8788/v1");
    assert_eq!(runtime.get_provider("radius-dev").unwrap().name(), "Radius (dev)");
}

#[tokio::test]
async fn requires_base_url_for_custom_radius_gateways() {
    let (runtime, _dir) = radius_runtime(
        &[],
        Some(json!({ "providers": { "radius-dev": { "oauth": "radius" } } })),
        Some(false),
        Arc::new(InMemoryModelsStore::new()),
    )
    .await;
    assert!(
        runtime.get_error().unwrap_or_default().contains("\"baseUrl\" is required when \"oauth\" is set"),
        "{:?}",
        runtime.get_error()
    );
}
