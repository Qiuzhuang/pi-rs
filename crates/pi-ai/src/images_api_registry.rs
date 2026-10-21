//! Image API registry (port of `images-api-registry.ts` and `providers/images/register-builtins.ts`).

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use crate::models::ProviderImages;
use crate::types::ImageModel;

/// A registered image API: `api` and its implementation.
#[derive(Clone)]
pub struct ImagesApiProvider {
    pub api: String,
    pub implementation: Arc<dyn ProviderImages>,
}

struct Registered {
    provider: ImagesApiProvider,
    #[allow(dead_code)]
    source_id: Option<String>,
}

static REGISTRY: Lazy<RwLock<HashMap<String, Registered>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    for provider in builtin_images_api_providers() {
        map.insert(provider.api.clone(), Registered { provider, source_id: None });
    }
    RwLock::new(map)
});

/// The built-in image APIs.
pub fn builtin_images_api_providers() -> Vec<ImagesApiProvider> {
    vec![ImagesApiProvider {
        api: "openrouter-images".into(),
        implementation: Arc::new(crate::api::openrouter_images::OpenRouterImagesApi),
    }]
}

/// Register (or replace) the implementation of an image API.
pub fn register_images_api_provider(provider: ImagesApiProvider, source_id: Option<&str>) {
    REGISTRY
        .write()
        .insert(provider.api.clone(), Registered { provider, source_id: source_id.map(str::to_string) });
}

/// Re-register the built-in image APIs (`registerBuiltInImagesApiProviders`).
pub fn register_builtin_images_api_providers() {
    for provider in builtin_images_api_providers() {
        register_images_api_provider(provider, None);
    }
}

/// Remove every image API registered with `source_id`.
pub fn unregister_images_api_providers(source_id: &str) {
    REGISTRY.write().retain(|_, r| r.source_id.as_deref() != Some(source_id));
}

/// The registered image API for `api`.
pub fn get_images_api_provider(api: &str) -> Option<ImagesApiProvider> {
    REGISTRY.read().get(api).map(|r| r.provider.clone())
}

/// Validate `model.api` against a registered API, as the TS wrapper does before dispatching.
pub fn check_images_api(model: &ImageModel, api: &str) -> anyhow::Result<()> {
    if model.api != api {
        anyhow::bail!("Mismatched api: {} expected {}", model.api, api);
    }
    Ok(())
}
