//! Port of `packages/coding-agent/src/utils/image-convert.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub type Photon = Value;

pub async fn convert_image_bytes_to_png(bytes: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    todo!("port utils/image-convert.ts:convertImageBytesToPng")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertToPngConvertToPngResult {
    pub data: String,
    pub mime_type: String,
}

/// Convert image to PNG format for terminal display.
/// Kitty graphics protocol requires PNG format (f=100).
pub async fn convert_to_png(
    base64_data: &str,
    mime_type: &str,
) -> anyhow::Result<Option<ConvertToPngConvertToPngResult>> {
    todo!("port utils/image-convert.ts:convertToPng")
}

/// Load photon and return a synchronous PNG transcoder for pi-tui's Kitty image rendering.
/// Returns undefined if photon cannot be loaded.
pub async fn load_png_transcoder() -> anyhow::Result<Option<Value /* TS: pi-tui::ImageTranscoder */>>
{
    todo!("port utils/image-convert.ts:loadPngTranscoder")
}

/// On Kitty-protocol terminals, register photon as pi-tui's image transcoder so non-PNG images render.
/// Loads photon once. `onRegistered` runs after registration so callers can re-render images that
/// showed text fallbacks; it is not called if the transcoder was already registered or cannot load.
pub fn ensure_png_transcoder(on_registered: Arc<dyn Fn() + Send + Sync>) {
    todo!("port utils/image-convert.ts:ensurePngTranscoder")
}
