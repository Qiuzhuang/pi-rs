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
    todo!("port utils/tool-result-images.ts:normalizeToolResultImages")
}
