//! Port of `packages/coding-agent/src/utils/image-convert.ts`.

use pi_tui::components::image::ImageTranscoder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Photon = crate::utils::photon::Photon;

fn encode_png(photon: &Photon, bytes: &[u8]) -> Option<Vec<u8>> {
    let raw_image = crate::utils::photon::PhotonImage::new_from_byteslice(bytes).ok()?;
    let image = crate::utils::exif_orientation::apply_exif_orientation(photon, raw_image, bytes);
    image.get_bytes().ok()
}

pub async fn convert_image_bytes_to_png(bytes: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(photon) = crate::utils::photon::load_photon().await? else {
        // Photon not available, can't convert.
        return Ok(None);
    };
    let bytes = bytes.to_vec();
    Ok(tokio::task::spawn_blocking(move || encode_png(photon, &bytes)).await.ok().flatten())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertToPngResult {
    pub data: String,
    pub mime_type: String,
}

/// Convert image to PNG format for terminal display.
/// Kitty graphics protocol requires PNG format (f=100).
pub async fn convert_to_png(
    base64_data: &str,
    mime_type: &str,
) -> anyhow::Result<Option<ConvertToPngResult>> {
    use base64::Engine;
    // Already PNG, no conversion needed.
    if mime_type == "image/png" {
        return Ok(Some(ConvertToPngResult {
            data: base64_data.to_string(),
            mime_type: mime_type.to_string(),
        }));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data.trim())
        .unwrap_or_default();
    let Some(png_bytes) = convert_image_bytes_to_png(&bytes).await? else {
        return Ok(None);
    };
    Ok(Some(ConvertToPngResult {
        data: base64::engine::general_purpose::STANDARD.encode(png_bytes),
        mime_type: "image/png".to_string(),
    }))
}

/// Load photon and return a synchronous PNG transcoder for pi-tui's Kitty image rendering.
/// Returns undefined if photon cannot be loaded.
pub async fn load_png_transcoder() -> anyhow::Result<Option<ImageTranscoder>> {
    let Some(photon) = crate::utils::photon::load_photon().await? else {
        return Ok(None);
    };
    Ok(Some(Arc::new(move |base64_data: &str, _mime_type: &str| {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(base64_data.trim()).ok()?;
        let png = encode_png(photon, &bytes)?;
        Some(base64::engine::general_purpose::STANDARD.encode(png))
    })))
}

static PNG_TRANSCODER_REGISTERED: AtomicBool = AtomicBool::new(false);

/// On Kitty-protocol terminals, register photon as pi-tui's image transcoder so non-PNG images render.
/// Loads photon once. `onRegistered` runs after registration so callers can re-render images that
/// showed text fallbacks; it is not called if the transcoder was already registered or cannot load.
pub fn ensure_png_transcoder(on_registered: Arc<dyn Fn() + Send + Sync>) {
    if PNG_TRANSCODER_REGISTERED.load(Ordering::SeqCst)
        || pi_tui::terminal_image::get_capabilities().images != Some(pi_tui::terminal_image::ImageProtocol::Kitty)
    {
        return;
    }
    // Loading is synchronous in the Rust port.
    if PNG_TRANSCODER_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    let photon = crate::utils::photon::photon();
    pi_tui::components::image::set_image_transcoder(Some(Arc::new(move |base64_data: &str, _mime_type: &str| {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(base64_data.trim()).ok()?;
        let png = encode_png(photon, &bytes)?;
        Some(base64::engine::general_purpose::STANDARD.encode(png))
    })));
    on_registered();
}
