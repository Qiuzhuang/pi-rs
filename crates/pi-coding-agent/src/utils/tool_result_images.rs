//! Port of `packages/coding-agent/src/utils/tool-result-images.ts`.

use pi_ai::providers::model_schema::ModelImageResizeOptions;
use pi_ai::types::UserContentBlock;
use serde::{Deserialize, Serialize};

pub type ToolResultContent = UserContentBlock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizeToolResultImagesOptions {
    /// Whether oversized images are resized to inline provider limits. Default: true
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resize_images: Option<bool>,
    /// Model-specific resize profile. Uses the conservative built-in defaults when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize_options: Option<ModelImageResizeOptions>,
}

/// Normalize image blocks returned by tool results.
///
/// The `read` tool and `@file` CLI attachments run their images through `processImage`, but tools
/// that produce images themselves (extensions, MCP bridges, screenshot tools) hand back arbitrary
/// base64 payloads that go straight into session history and every subsequent provider request.
/// Oversized images make the provider reject the whole conversation, not just the offending turn,
/// so normalize them once as they enter history.
///
/// Returns the original array when nothing changed so callers can skip rewriting the result.
pub async fn normalize_tool_result_images(
    content: &[ToolResultContent],
    options: Option<&NormalizeToolResultImagesOptions>,
) -> anyhow::Result<Vec<ToolResultContent>> {
    use crate::utils::image_process::{ProcessImageOptions, ProcessImageResult, process_image};
    use base64::Engine;
    if !content.iter().any(|block| matches!(block, UserContentBlock::Image(_))) {
        return Ok(content.to_vec());
    }

    let auto_resize_images = options.and_then(|o| o.auto_resize_images).unwrap_or(true);
    let resize_options = options
        .and_then(|o| o.resize_options.as_ref())
        .map(crate::core::tools::read::to_image_resize_options);
    let mut normalized: Vec<ToolResultContent> = Vec::new();
    let mut changed = false;

    for block in content {
        let UserContentBlock::Image(image) = block else {
            normalized.push(block.clone());
            continue;
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image.data.trim())
            .unwrap_or_default();
        let processed = process_image(
            &bytes,
            &image.mime_type,
            Some(&ProcessImageOptions {
                auto_resize_images: Some(auto_resize_images),
                resize_options: resize_options.clone(),
            }),
        )
        .await?;
        let ProcessImageResult::ProcessImageResultVariant1(processed) = processed else {
            // Unlike `read`, keep the original block. The tool already produced this image and the
            // failure may just be an unavailable image backend, so passing it through preserves the
            // behavior tools have today instead of silently deleting their output.
            normalized.push(block.clone());
            continue;
        };
        if processed.data == image.data && processed.mime_type == image.mime_type && processed.hints.is_empty() {
            normalized.push(block.clone());
            continue;
        }
        normalized.push(UserContentBlock::Image(pi_ai::types::ImageContent {
            data: processed.data,
            mime_type: processed.mime_type,
        }));
        if !processed.hints.is_empty() {
            normalized.push(UserContentBlock::text(processed.hints.join("\n")));
        }
        changed = true;
    }

    Ok(if changed { normalized } else { content.to_vec() })
}
