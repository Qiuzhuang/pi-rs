//! Port of `packages/coding-agent/src/core/remote-catalog-provider.ts`.

use async_trait::async_trait;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use parking_lot::RwLock;
use pi_ai::auth::types::{Credential, ProviderAuth};
use pi_ai::models::{ModelsPublication, Provider, RefreshModelsContext};
use pi_ai::models_store::ModelsStoreEntry;
use pi_ai::types::{
    AnyModel, AssistantImages, AssistantMessageEventStream, ClassifierContext, ClassifierModel,
    ClassifierResult, DeferredHandle, ImageModel, ImagesContext, Model, ModelType, ProviderHeaders,
    StreamOptions, TranscriptContext,
};
use pi_ai::utils::now_ms;
use serde_json::Value;
use std::sync::Arc;

use crate::config::VERSION;
use crate::utils::management_http::{FetchInit, FetchRetryOptions, fetch_with_retry};
use crate::utils::pi_user_agent::get_pi_user_agent;

const DEFAULT_CATALOG_BASE_URL: &str = "https://pi.dev";
const REMOTE_CATALOG_ATTEMPT_TIMEOUT_MS: i64 = 4_000;

pub const REMOTE_CATALOG_REFRESH_INTERVAL_MS: i64 = 14400000;

/// Model types this client can consume. Sent as `?types=` so the catalog server
/// returns the full-type shard instead of the chat-only one served to clients
/// that predate model types. A server that ignores the parameter still returns
/// the chat-only shard, which this client handles unchanged.
pub const REMOTE_CATALOG_MODEL_TYPES: &[&str] = &["chat", "image", "classifier"];

/// Replaces the catalog with a credential-specific model list.
pub type PersonalizeCatalog = Arc<
    dyn Fn(Vec<AnyModel>, RefreshModelsContext) -> BoxFuture<'static, anyhow::Result<Vec<AnyModel>>>
        + Send
        + Sync,
>;

fn is_supported_model_type(model: &serde_json::Map<String, Value>) -> bool {
    match model.get("type") {
        None => true,
        Some(Value::String(kind)) => REMOTE_CATALOG_MODEL_TYPES.contains(&kind.as_str()),
        Some(_) => false,
    }
}

fn merge_models(baseline: &[AnyModel], dynamic: &[AnyModel]) -> Vec<AnyModel> {
    let mut merged: IndexMap<String, AnyModel> = IndexMap::new();
    for model in baseline.iter().chain(dynamic) {
        merged.insert(
            format!("{}\0{}", model.model_type().as_str(), model.id()),
            model.clone(),
        );
    }
    merged.into_values().collect()
}

fn parse_catalog(provider_id: &str, value: Value) -> anyhow::Result<Vec<AnyModel>> {
    let entries: Vec<Value> = match value {
        Value::Array(entries) => entries,
        Value::Object(mut map) => match map.remove("models") {
            Some(Value::Array(models)) => models,
            Some(other) => {
                map.insert("models".to_string(), other);
                map.into_iter().map(|(_, v)| v).collect()
            }
            None => map.into_iter().map(|(_, v)| v).collect(),
        },
        _ => anyhow::bail!("Invalid model catalog for provider \"{provider_id}\""),
    };
    Ok(entries
        .into_iter()
        .filter_map(|entry| match entry {
            Value::Object(map) if map.contains_key("id") && is_supported_model_type(&map) => {
                Some(map)
            }
            _ => None,
        })
        .filter_map(|mut map| {
            map.insert("provider".to_string(), Value::String(provider_id.to_string()));
            AnyModel::from_value(Value::Object(map)).ok()
        })
        .collect())
}

struct Catalog {
    models: Vec<AnyModel>,
    last_modified: i64,
    etag: Option<String>,
}

/// Read a successful pi.dev catalog response. A missing or invalid Last-Modified header is 0.
async fn read_catalog(provider_id: &str, response: reqwest::Response) -> anyhow::Result<Catalog> {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let last_modified = header("last-modified")
        .and_then(|value| chrono::DateTime::parse_from_rfc2822(value.trim()).ok())
        .map(|date| date.timestamp_millis())
        .unwrap_or(0);
    let etag = header("etag");
    let body: Value = response.json().await?;
    Ok(Catalog {
        models: parse_catalog(provider_id, body)?,
        last_modified,
        etag,
    })
}

/// Whether a pi.dev catalog is newer than the model data shipped with pi.
fn is_newer(last_modified: Option<i64>, local_generated_at: Option<i64>) -> bool {
    match local_generated_at {
        None => true,
        Some(local) => last_modified.is_some_and(|last| last > local),
    }
}

fn remote_models(entry: Option<&ModelsStoreEntry>, local_generated_at: Option<i64>) -> Vec<AnyModel> {
    match entry {
        Some(entry) if is_newer(entry.last_modified, local_generated_at) => entry.models.clone(),
        _ => Vec::new(),
    }
}

async fn request_catalog(
    provider_id: &str,
    catalog_base_url: &str,
    context: &RefreshModelsContext,
    validator: Option<&str>,
) -> anyhow::Result<reqwest::Response> {
    let mut url = url::Url::parse(catalog_base_url)?.join(&format!(
        "/api/models/providers/{}",
        encode_uri_component(provider_id)
    ))?;
    url.query_pairs_mut()
        .clear()
        .append_pair("types", &REMOTE_CATALOG_MODEL_TYPES.join(","));
    let mut headers = vec![
        ("accept".to_string(), "application/json".to_string()),
        ("User-Agent".to_string(), get_pi_user_agent(VERSION)),
    ];
    if let Some(validator) = validator {
        headers.push(("if-none-match".to_string(), validator.to_string()));
    }
    let init = FetchInit {
        headers,
        signal: Some(context.signal.clone()),
        ..Default::default()
    };
    let options = FetchRetryOptions {
        attempt_timeout_ms: Some(REMOTE_CATALOG_ATTEMPT_TIMEOUT_MS),
        ..Default::default()
    };
    fetch_with_retry(url.as_str(), Some(&init), Some(&options)).await
}

/// JS `encodeURIComponent`.
fn encode_uri_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let unreserved = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')');
        if unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

async fn publish(context: &RefreshModelsContext, publication: ModelsPublication) -> anyhow::Result<bool> {
    (context.publish)(publication).await
}

/// Delegates the synchronous provider operations except the model catalog to the wrapped provider
/// (`async_trait` does not see `async fn`s produced by a macro, so those are written out).
macro_rules! delegate_provider_operations {
    () => {
        fn id(&self) -> &str {
            self.inner.id()
        }
        fn name(&self) -> &str {
            self.inner.name()
        }
        fn base_url(&self) -> Option<&str> {
            self.inner.base_url()
        }
        fn headers(&self) -> Option<&ProviderHeaders> {
            self.inner.headers()
        }
        fn auth(&self) -> &ProviderAuth {
            self.inner.auth()
        }
        fn supports_refresh(&self) -> bool {
            true
        }
        fn filter_models(&self, models: &[Model], credential: Option<&Credential>) -> Option<Vec<Model>> {
            self.inner.filter_models(models, credential)
        }
        fn filter_all_models(
            &self,
            models: &[AnyModel],
            credential: Option<&Credential>,
        ) -> Option<Vec<AnyModel>> {
            self.inner.filter_all_models(models, credential)
        }
        fn stream(&self, model: &Model, context: &TranscriptContext, options: StreamOptions) -> AssistantMessageEventStream {
            self.inner.stream(model, context, options)
        }
        fn stream_simple(
            &self,
            model: &Model,
            context: &TranscriptContext,
            options: StreamOptions,
        ) -> AssistantMessageEventStream {
            self.inner.stream_simple(model, context, options)
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
        fn supports_images(&self) -> bool {
            self.inner.supports_images()
        }
        fn supports_classify(&self) -> bool {
            self.inner.supports_classify()
        }
    };
}

struct RemoteCatalogProvider {
    inner: Arc<dyn Provider>,
    catalog_base_url: String,
    local_generated_at: Option<i64>,
    dynamic_models: Arc<RwLock<Vec<AnyModel>>>,
}

#[async_trait]
impl Provider for RemoteCatalogProvider {
    delegate_provider_operations!();

    async fn cancel_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> anyhow::Result<()> {
        self.inner.cancel_deferred(model, handle, options).await
    }

    async fn generate_images(&self, model: &ImageModel, context: &ImagesContext, options: StreamOptions) -> AssistantImages {
        self.inner.generate_images(model, context, options).await
    }

    async fn classify(&self, model: &ClassifierModel, context: &ClassifierContext, options: StreamOptions) -> ClassifierResult {
        self.inner.classify(model, context, options).await
    }

    fn get_models(&self) -> Vec<Model> {
        let dynamic: Vec<AnyModel> = self
            .dynamic_models
            .read()
            .iter()
            .filter(|model| model.model_type() == ModelType::Chat)
            .cloned()
            .collect();
        let baseline: Vec<AnyModel> = self.inner.get_models().into_iter().map(AnyModel::Chat).collect();
        merge_models(&baseline, &dynamic)
            .into_iter()
            .filter_map(AnyModel::into_chat)
            .collect()
    }

    fn get_all_models(&self) -> Vec<AnyModel> {
        merge_models(&self.inner.get_all_models(), &self.dynamic_models.read())
    }

    async fn refresh_models(&self, context: RefreshModelsContext) -> anyhow::Result<()> {
        let provider_id = self.inner.id().to_string();
        let stored = context.stored.clone();
        let restored: Vec<AnyModel> = remote_models(stored.as_ref(), self.local_generated_at)
            .into_iter()
            .filter(|model| model.provider() == provider_id)
            .collect();
        let dynamic = self.dynamic_models.clone();
        let published = publish(
            &context,
            ModelsPublication {
                persist: None,
                update: Some(Box::new(move || *dynamic.write() = restored)),
            },
        )
        .await?;
        if !published {
            return Ok(());
        }
        if !context.allow_network || context.signal.is_cancelled() {
            return Ok(());
        }
        // Skip the request if pi.dev was checked less than REMOTE_CATALOG_REFRESH_INTERVAL_MS ago.
        // An entry without lastModified (saved by an older pi, or after a failed first request) is
        // always refetched: remote_models() ignores it, so waiting would leave the overlay empty.
        if context.force != Some(true)
            && let Some(stored) = &stored
            && let Some(checked_at) = stored.checked_at
            && stored.last_modified.is_some()
            && now_ms() - checked_at < REMOTE_CATALOG_REFRESH_INTERVAL_MS
        {
            return Ok(());
        }

        // Only revalidate when a cached body backs the validator, so a 304 can never
        // leave the overlay empty.
        let validator = stored
            .as_ref()
            .filter(|stored| !stored.models.is_empty())
            .and_then(|stored| stored.etag.clone());
        let response =
            request_catalog(&provider_id, &self.catalog_base_url, &context, validator.as_deref())
                .await?;
        if context.signal.is_cancelled() {
            return Ok(());
        }
        let checked_at = now_ms();
        let status = response.status().as_u16();
        // Unchanged: dynamic_models already holds the stored overlay, so only the
        // freshness window moves.
        if status == 304
            && let Some(stored) = &stored
        {
            publish(
                &context,
                ModelsPublication {
                    persist: Some(Some(ModelsStoreEntry {
                        checked_at: Some(checked_at),
                        ..stored.clone()
                    })),
                    update: None,
                },
            )
            .await?;
            return Ok(());
        }
        let base_entry = stored.clone().unwrap_or_default();
        if status == 404 || status == 501 {
            publish(
                &context,
                ModelsPublication {
                    persist: Some(Some(ModelsStoreEntry {
                        checked_at: Some(checked_at),
                        last_modified: Some(0),
                        etag: None,
                        ..base_entry
                    })),
                    update: None,
                },
            )
            .await?;
            return Ok(());
        }
        if !response.status().is_success() {
            // Transient failure: the cached body and its validator stay valid, so keep the
            // etag and let the next refresh revalidate instead of downloading the catalog.
            publish(
                &context,
                ModelsPublication {
                    persist: Some(Some(ModelsStoreEntry {
                        checked_at: Some(checked_at),
                        ..base_entry
                    })),
                    update: None,
                },
            )
            .await?;
            anyhow::bail!("Model catalog request failed for {provider_id}: {status}");
        }
        let catalog = read_catalog(&provider_id, response).await?;
        if context.signal.is_cancelled() {
            return Ok(());
        }
        let entry = ModelsStoreEntry {
            models: catalog.models,
            last_modified: Some(catalog.last_modified),
            checked_at: Some(checked_at),
            etag: catalog.etag,
        };
        let published_models = remote_models(Some(&entry), self.local_generated_at);
        let dynamic = self.dynamic_models.clone();
        publish(
            &context,
            ModelsPublication {
                persist: Some(Some(entry)),
                update: Some(Box::new(move || *dynamic.write() = published_models)),
            },
        )
        .await?;
        Ok(())
    }
}

/// Add a persisted pi.dev catalog overlay to a static built-in provider.
pub fn with_remote_catalog(
    provider: Arc<dyn Provider>,
    catalog_base_url: Option<&str>,
    local_generated_at: Option<i64>,
) -> Arc<dyn Provider> {
    Arc::new(RemoteCatalogProvider {
        inner: provider,
        catalog_base_url: catalog_base_url.unwrap_or(DEFAULT_CATALOG_BASE_URL).to_string(),
        local_generated_at,
        dynamic_models: Arc::default(),
    })
}

struct PersonalizedCatalogProvider {
    inner: Arc<dyn Provider>,
    personalize: PersonalizeCatalog,
    catalog_base_url: String,
    local_generated_at: Option<i64>,
    personal_models: Arc<RwLock<Vec<AnyModel>>>,
}

impl PersonalizedCatalogProvider {
    fn builtin_models(&self) -> Vec<AnyModel> {
        self.inner.get_all_models()
    }
}

#[async_trait]
impl Provider for PersonalizedCatalogProvider {
    delegate_provider_operations!();

    async fn cancel_deferred(&self, model: &Model, handle: &DeferredHandle, options: StreamOptions) -> anyhow::Result<()> {
        self.inner.cancel_deferred(model, handle, options).await
    }

    async fn generate_images(&self, model: &ImageModel, context: &ImagesContext, options: StreamOptions) -> AssistantImages {
        self.inner.generate_images(model, context, options).await
    }

    async fn classify(&self, model: &ClassifierModel, context: &ClassifierContext, options: StreamOptions) -> ClassifierResult {
        self.inner.classify(model, context, options).await
    }

    fn get_models(&self) -> Vec<Model> {
        self.get_all_models()
            .into_iter()
            .filter_map(AnyModel::into_chat)
            .collect()
    }

    fn get_all_models(&self) -> Vec<AnyModel> {
        let personal = self.personal_models.read();
        if personal.is_empty() {
            self.builtin_models()
        } else {
            personal.clone()
        }
    }

    async fn refresh_models(&self, context: RefreshModelsContext) -> anyhow::Result<()> {
        let provider_id = self.inner.id().to_string();
        let restored: Vec<AnyModel> = context
            .stored
            .as_ref()
            .map(|stored| stored.models.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|model| model.provider() == provider_id)
            .collect();
        let personal = self.personal_models.clone();
        let published = publish(
            &context,
            ModelsPublication {
                persist: None,
                update: Some(Box::new(move || *personal.write() = restored)),
            },
        )
        .await?;
        if !published {
            return Ok(());
        }
        if !context.allow_network || context.signal.is_cancelled() {
            return Ok(());
        }

        let response = request_catalog(&provider_id, &self.catalog_base_url, &context, None).await?;
        if context.signal.is_cancelled() {
            return Ok(());
        }
        let mut remote: Vec<AnyModel> = Vec::new();
        let status = response.status().as_u16();
        if status != 404 && status != 501 {
            if !response.status().is_success() {
                anyhow::bail!("Model catalog request failed for {provider_id}: {status}");
            }
            let catalog = read_catalog(&provider_id, response).await?;
            if is_newer(Some(catalog.last_modified), self.local_generated_at) {
                remote = catalog.models;
            }
        }
        let personalized =
            (self.personalize)(merge_models(&self.builtin_models(), &remote), context.clone())
                .await?;
        if context.signal.is_cancelled() {
            return Ok(());
        }
        let personal = self.personal_models.clone();
        let persisted = personalized.clone();
        publish(
            &context,
            ModelsPublication {
                persist: Some(Some(ModelsStoreEntry {
                    models: persisted,
                    checked_at: Some(now_ms()),
                    ..Default::default()
                })),
                update: Some(Box::new(move || *personal.write() = personalized)),
            },
        )
        .await?;
        Ok(())
    }
}

/// Like `withRemoteCatalog`, but on each network refresh the catalog (built-in models plus a newer
/// pi.dev catalog) is passed through `personalize` and the result is saved. The saved entry holds
/// the personalized list, not the pi.dev catalog, and replaces the catalog, including offline.
/// The refresh interval and etag revalidation are skipped. An empty list (nothing saved yet, or an
/// empty catalog saved by an older pi after a failed or missing pi.dev catalog) falls back to the
/// built-in models.
pub fn with_personalized_catalog(
    provider: Arc<dyn Provider>,
    personalize: &PersonalizeCatalog,
    catalog_base_url: Option<&str>,
    local_generated_at: Option<i64>,
) -> Arc<dyn Provider> {
    Arc::new(PersonalizedCatalogProvider {
        inner: provider,
        personalize: personalize.clone(),
        catalog_base_url: catalog_base_url.unwrap_or(DEFAULT_CATALOG_BASE_URL).to_string(),
        local_generated_at,
        personal_models: Arc::default(),
    })
}
