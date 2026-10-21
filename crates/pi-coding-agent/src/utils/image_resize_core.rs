//! Port of `packages/coding-agent/src/utils/image-resize-core.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

// 4.5MB of base64 payload. Provides headroom below Anthropic's 5MB limit.
const DEFAULT_MAX_BYTES: i64 = 4_718_592;
const DEFAULT_MAX_WIDTH: i64 = 2000;
const DEFAULT_MAX_HEIGHT: i64 = 2000;
const DEFAULT_JPEG_QUALITY: f64 = 80.0;

fn encode_candidate(buffer: &[u8], mime_type: &str) -> EncodedCandidate {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(buffer);
    EncodedCandidate {
        encoded_size: data.len() as i64,
        data,
        mime_type: mime_type.to_string(),
    }
}

/// Synchronous [`resize_image_in_process`] (the work the TS worker thread does).
pub fn resize_image_sync(
    input_bytes: &[u8],
    mime_type: &str,
    options: Option<&ImageResizeOptions>,
) -> Option<ResizedImage> {
    use crate::utils::exif_orientation::apply_exif_orientation;
    use crate::utils::photon::{PhotonImage, SamplingFilter, photon};
    use base64::Engine;

    let max_width = options.and_then(|o| o.max_width).unwrap_or(DEFAULT_MAX_WIDTH);
    let max_height = options.and_then(|o| o.max_height).unwrap_or(DEFAULT_MAX_HEIGHT);
    let max_bytes = options.and_then(|o| o.max_bytes).unwrap_or(DEFAULT_MAX_BYTES);
    let jpeg_quality = options.and_then(|o| o.jpeg_quality).unwrap_or(DEFAULT_JPEG_QUALITY);
    let input_base64_size = (input_bytes.len() as i64 + 2) / 3 * 4;

    let photon = photon();
    let raw_image = PhotonImage::new_from_byteslice(input_bytes).ok()?;
    let image = apply_exif_orientation(photon, raw_image, input_bytes);

    let original_width = image.get_width() as i64;
    let original_height = image.get_height() as i64;
    let format = mime_type.split('/').nth(1).unwrap_or("png");

    // Check if already within all limits (dimensions AND encoded size).
    if original_width <= max_width && original_height <= max_height && input_base64_size < max_bytes {
        return Some(ResizedImage {
            data: base64::engine::general_purpose::STANDARD.encode(input_bytes),
            mime_type: if mime_type.is_empty() {
                format!("image/{format}")
            } else {
                mime_type.to_string()
            },
            original_width,
            original_height,
            width: original_width,
            height: original_height,
            was_resized: false,
        });
    }

    // Calculate initial dimensions respecting max limits (Math.round).
    let js_round = |value: f64| (value + 0.5).floor();
    let mut target_width = original_width as f64;
    let mut target_height = original_height as f64;
    if target_width > max_width as f64 {
        target_height = js_round(target_height * max_width as f64 / target_width);
        target_width = max_width as f64;
    }
    if target_height > max_height as f64 {
        target_width = js_round(target_width * max_height as f64 / target_height);
        target_height = max_height as f64;
    }

    let try_encodings = |width: u32, height: u32, qualities: &[u8]| -> Vec<EncodedCandidate> {
        let resized = photon.resize(&image, width, height, SamplingFilter::Lanczos3);
        let mut candidates = Vec::new();
        if let Ok(png) = resized.get_bytes() {
            candidates.push(encode_candidate(&png, "image/png"));
        }
        for quality in qualities {
            if let Ok(jpeg) = resized.get_bytes_jpeg(*quality) {
                candidates.push(encode_candidate(&jpeg, "image/jpeg"));
            }
        }
        candidates
    };

    // Array.from(new Set([opts.jpegQuality, 85, 70, 55, 40]))
    let mut quality_steps: Vec<u8> = Vec::new();
    for quality in [jpeg_quality, 85.0, 70.0, 55.0, 40.0] {
        let quality = quality.round().clamp(1.0, 100.0) as u8;
        if !quality_steps.contains(&quality) {
            quality_steps.push(quality);
        }
    }

    let mut current_width = (target_width.max(1.0)) as u32;
    let mut current_height = (target_height.max(1.0)) as u32;
    loop {
        let candidates = try_encodings(current_width, current_height, &quality_steps);
        if let Some(candidate) = candidates.into_iter().find(|c| c.encoded_size < max_bytes) {
            return Some(ResizedImage {
                data: candidate.data,
                mime_type: candidate.mime_type,
                original_width,
                original_height,
                width: current_width as i64,
                height: current_height as i64,
                was_resized: true,
            });
        }
        if current_width == 1 && current_height == 1 {
            break;
        }
        let next_width = if current_width == 1 {
            1
        } else {
            ((current_width as f64 * 0.75).floor() as u32).max(1)
        };
        let next_height = if current_height == 1 {
            1
        } else {
            ((current_height as f64 * 0.75).floor() as u32).max(1)
        };
        if next_width == current_width && next_height == current_height {
            break;
        }
        current_width = next_width;
        current_height = next_height;
    }
    None
}

/// Resize an image to fit within the specified max dimensions and encoded file size.
/// Returns null if the image cannot be resized below maxBytes.
///
/// Uses the `image` crate (the TS version uses Photon). Returns `None` when the image cannot be
/// decoded.
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
    Ok(resize_image_sync(input_bytes, mime_type, options))
}
