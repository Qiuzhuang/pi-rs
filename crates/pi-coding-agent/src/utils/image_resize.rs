//! Port of `packages/coding-agent/src/utils/image-resize.ts`.

pub use crate::utils::image_resize_core::ImageResizeOptions;
pub use crate::utils::image_resize_core::ResizedImage;

/// Resize an image to fit within the specified max dimensions and encoded file size.
/// Runs Photon in a worker thread so WASM decoding, resizing, and encoding do not
/// block the TUI event loop. If the worker cannot be loaded (for example in some
/// Bun compiled executable layouts), fall back to in-process resizing so image
/// reads still work.
///
/// The Rust port runs the CPU-bound work on tokio's blocking pool instead of a worker thread.
pub async fn resize_image(
    input_bytes: &[u8],
    mime_type: &str,
    options: Option<&ImageResizeOptions>,
) -> anyhow::Result<Option<ResizedImage>> {
    let bytes = input_bytes.to_vec();
    let mime = mime_type.to_string();
    let opts = options.cloned();
    match tokio::task::spawn_blocking(move || {
        crate::utils::image_resize_core::resize_image_sync(&bytes, &mime, opts.as_ref())
    })
    .await
    {
        Ok(result) => Ok(result),
        // Fall back to in-process resizing if the blocking task could not run.
        Err(_) => crate::utils::image_resize_core::resize_image_in_process(input_bytes, mime_type, options).await,
    }
}

/// Format a dimension note for resized images.
/// This helps the model understand the coordinate mapping.
pub fn format_dimension_note(result: &ResizedImage) -> Option<String> {
    if !result.was_resized {
        return None;
    }
    let scale = result.original_width as f64 / result.width as f64;
    Some(format!(
        "[Image: original {}x{}, displayed at {}x{}. Multiply coordinates by {} to map to original image.]",
        result.original_width,
        result.original_height,
        result.width,
        result.height,
        crate::core::tools::js_util::js_to_fixed(scale, 2)
    ))
}
