//! Port of `packages/coding-agent/src/cli/file-processor.ts`.

use pi_ai::types::ImageContent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessedFiles {
    pub text: String,
    pub images: Vec<ImageContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessFileOptions {
    /// Whether to auto-resize images. Default: true
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resize_images: Option<bool>,
}

/// Process @file arguments into text content and image attachments
pub async fn process_file_arguments(
    file_args: &[String],
    options: Option<&ProcessFileOptions>,
) -> anyhow::Result<ProcessedFiles> {
    todo!("port cli/file-processor.ts:processFileArguments")
}
