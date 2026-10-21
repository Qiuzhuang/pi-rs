//! Port of `packages/coding-agent/src/utils/image-resize-core.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResizeOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub jpeg_quality: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizedImage {
    pub data: String,
    pub mime_type: String,
    pub original_width: i64,
    pub original_height: i64,
    pub width: i64,
    pub height: i64,
    pub was_resized: bool,
}

/// Tag on image resize worker replies. Node can post its own messages on the worker channel
/// (for example `{ "watch:require": [...] }` under `node --watch`), so replies must be identifiable.
pub const IMAGE_RESIZE_WORKER_RESPONSE_TYPE: &str = "pi:image-resize-response";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeImageWorkerResponseVariant1 {
    pub r#type: Value,
    #[serde(default)]
    pub result: Option<ResizedImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeImageWorkerResponseVariant2 {
    pub r#type: Value,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResizeImageWorkerResponse {
    ResizeImageWorkerResponseVariant1(ResizeImageWorkerResponseVariant1),
    ResizeImageWorkerResponseVariant2(ResizeImageWorkerResponseVariant2),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedCandidate {
    pub data: String,
    pub encoded_size: i64,
    pub mime_type: String,
}

/// Resize an image to fit within the specified max dimensions and encoded file size.
/// Returns null if the image cannot be resized below maxBytes.
///
/// Uses Photon (Rust/WASM) for image processing. If Photon is not available,
/// returns null.
///
/// Strategy for staying under maxBytes:
/// 1. First resize to maxWidth/maxHeight
/// 2. Try both PNG and JPEG formats, pick the smaller one
/// 3. If still too large, try JPEG with decreasing quality
/// 4. If still too large, progressively reduce dimensions until 1x1
pub async fn resize_image_in_process(
    input_bytes: &[u8],
    mime_type: &str,
    options: Option<&ImageResizeOptions>,
) -> anyhow::Result<Option<ResizedImage>> {
    todo!("port utils/image-resize-core.ts:resizeImageInProcess")
}
