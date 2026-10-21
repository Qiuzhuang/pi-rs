//! Global image generation (port of `images.ts`).

use crate::images_api_registry::{check_images_api, get_images_api_provider};
use crate::types::{AssistantImages, ImageModel, ImagesContext, StreamOptions};

/// Image generation dispatched on `model.api` through the images API registry. Auth must be
/// passed explicitly via `options.api_key`; prefer `Models::generate_images`, which resolves
/// provider auth. Fails when no API is registered for `model.api`.
pub async fn generate_images(
    model: &ImageModel,
    context: &ImagesContext,
    options: StreamOptions,
) -> anyhow::Result<AssistantImages> {
    let Some(provider) = get_images_api_provider(&model.api) else {
        anyhow::bail!("No API provider registered for api: {}", model.api);
    };
    check_images_api(model, &provider.api)?;
    Ok(provider.implementation.generate_images(model, context, options).await)
}
