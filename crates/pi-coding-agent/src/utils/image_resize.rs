//! Port of `packages/coding-agent/src/utils/image-resize.ts`.

pub use crate::utils::image_resize_core::ImageResizeOptions;
pub use crate::utils::image_resize_core::ResizedImage;

/// Resize an image to fit within the specified max dimensions and encoded file size.
/// Runs Photon in a worker thread so WASM decoding, resizing, and encoding do not
/// block the TUI event loop. If the worker cannot be loaded (for example in some
/// Bun compiled executable layouts), fall back to in-process resizing so image
/// reads still work.
pub async fn resize_image(
    input_bytes: &[u8],
    mime_type: &str,
    options: Option<&ImageResizeOptions>,
) -> anyhow::Result<Option<ResizedImage>> {
    todo!("port utils/image-resize.ts:resizeImage")
}

/// Format a dimension note for resized images.
/// This helps the model understand the coordinate mapping.
pub fn format_dimension_note(result: &ResizedImage) -> Option<String> {
    todo!("port utils/image-resize.ts:formatDimensionNote")
}
