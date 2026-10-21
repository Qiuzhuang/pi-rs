//! Port of `packages/coding-agent/src/utils/image-process.ts`.

use crate::utils::image_resize_core::ImageResizeOptions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessImageOptions {
    /// Whether to resize images to inline provider limits. Default: true
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resize_images: Option<bool>,
    /// Optional resize overrides. Uses resizeImage defaults when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize_options: Option<ImageResizeOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessImageResultVariant1 {
    pub ok: bool,
    pub data: String,
    pub mime_type: String,
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessImageResultVariant2 {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProcessImageResult {
    ProcessImageResultVariant1(ProcessImageResultVariant1),
    ProcessImageResultVariant2(ProcessImageResultVariant2),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub converted_from: Option<String>,
}

fn base_mime_type(mime_type: &str) -> String {
    mime_type.split(';').next().unwrap_or(mime_type).trim().to_lowercase()
}

fn normalize_supported_image_mime_type(mime_type: &str) -> Option<&'static str> {
    match base_mime_type(mime_type).as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

async fn normalize_image(bytes: &[u8], mime_type: &str) -> anyhow::Result<Option<NormalizedImage>> {
    if let Some(normalized) = normalize_supported_image_mime_type(mime_type) {
        return Ok(Some(NormalizedImage {
            bytes: bytes.to_vec(),
            mime_type: normalized.to_string(),
            converted_from: None,
        }));
    }
    let Some(png_bytes) = crate::utils::image_convert::convert_image_bytes_to_png(bytes).await? else {
        return Ok(None);
    };
    Ok(Some(NormalizedImage {
        bytes: png_bytes,
        mime_type: "image/png".to_string(),
        converted_from: Some(base_mime_type(mime_type)),
    }))
}

fn conversion_hint(from: Option<&str>, to: &str) -> Option<String> {
    let from = from.filter(|f| !f.is_empty() && *f != to)?;
    Some(format!("[Image converted from {from} to {to}.]"))
}

pub async fn process_image(
    bytes: &[u8],
    mime_type: &str,
    options: Option<&ProcessImageOptions>,
) -> anyhow::Result<ProcessImageResult> {
    use base64::Engine;
    let failure = |message: &str| {
        ProcessImageResult::ProcessImageResultVariant2(ProcessImageResultVariant2 {
            ok: false,
            message: message.to_string(),
        })
    };
    let auto_resize_images = options.and_then(|o| o.auto_resize_images).unwrap_or(true);
    let Some(normalized) = normalize_image(bytes, mime_type).await? else {
        return Ok(failure(
            "[Image omitted: could not be converted to a supported inline image format.]",
        ));
    };

    if auto_resize_images {
        let resize_options = options.and_then(|o| o.resize_options.as_ref());
        let Some(resized) =
            crate::utils::image_resize::resize_image(&normalized.bytes, &normalized.mime_type, resize_options).await?
        else {
            return Ok(failure(
                "[Image omitted: could not be resized below the inline image size limit.]",
            ));
        };
        let mut hints = Vec::new();
        if let Some(hint) = conversion_hint(normalized.converted_from.as_deref(), &resized.mime_type) {
            hints.push(hint);
        }
        if let Some(note) = crate::utils::image_resize::format_dimension_note(&resized) {
            hints.push(note);
        }
        return Ok(ProcessImageResult::ProcessImageResultVariant1(ProcessImageResultVariant1 {
            ok: true,
            data: resized.data,
            mime_type: resized.mime_type,
            hints,
        }));
    }

    let mut hints = Vec::new();
    if let Some(hint) = conversion_hint(normalized.converted_from.as_deref(), &normalized.mime_type) {
        hints.push(hint);
    }
    Ok(ProcessImageResult::ProcessImageResultVariant1(ProcessImageResultVariant1 {
        ok: true,
        data: base64::engine::general_purpose::STANDARD.encode(&normalized.bytes),
        mime_type: normalized.mime_type,
        hints,
    }))
}
