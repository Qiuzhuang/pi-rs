//! Port of `packages/coding-agent/src/core/remote-catalog-provider.ts`.

use futures::future::BoxFuture;
use pi_ai::models::Provider;
use pi_ai::models::RefreshModelsContext;
use pi_ai::types::AnyModel;
use std::sync::Arc;

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

/// Add a persisted pi.dev catalog overlay to a static built-in provider.
pub fn with_remote_catalog(
    provider: Arc<dyn Provider>,
    catalog_base_url: Option<&str>,
    local_generated_at: Option<f64>,
) -> anyhow::Result<Arc<dyn Provider>> {
    todo!("port core/remote-catalog-provider.ts:withRemoteCatalog")
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
    local_generated_at: Option<f64>,
) -> anyhow::Result<Arc<dyn Provider>> {
    todo!("port core/remote-catalog-provider.ts:withPersonalizedCatalog")
}
