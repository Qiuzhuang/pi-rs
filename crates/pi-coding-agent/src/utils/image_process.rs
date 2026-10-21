//! Port of `packages/coding-agent/src/utils/image-process.ts`.

use crate::utils::image_resize_core::ImageResizeOptions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub async fn process_image(
    bytes: &[u8],
    mime_type: &str,
    options: Option<&ProcessImageOptions>,
) -> anyhow::Result<ProcessImageResult> {
    todo!("port utils/image-process.ts:processImage")
}
