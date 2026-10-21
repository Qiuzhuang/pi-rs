//! Compat reads of the generated catalog restricted to image models (port of `image-models.ts`).

use crate::models_generated::CATALOG;
use crate::types::ImageModel;

/// One built-in image model.
pub fn get_image_model(provider: &str, model_id: &str) -> Option<ImageModel> {
    CATALOG.image.get(provider)?.get(model_id).cloned()
}

/// Built-in providers with at least one image model.
pub fn get_image_providers() -> Vec<String> {
    CATALOG.image.iter().filter(|(_, models)| !models.is_empty()).map(|(p, _)| p.clone()).collect()
}

/// Built-in image models of a provider.
pub fn get_image_models(provider: &str) -> Vec<ImageModel> {
    CATALOG.image.get(provider).map(|m| m.values().cloned().collect()).unwrap_or_default()
}
