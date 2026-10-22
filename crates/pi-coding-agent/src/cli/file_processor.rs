//! Port of `packages/coding-agent/src/cli/file-processor.ts`.
//!
//! Process @file CLI arguments into text content and image attachments.

use crate::core::tools::path_utils::resolve_read_path;
use crate::utils::chalk;
use crate::utils::image_process::{ProcessImageOptions, ProcessImageResult, process_image};
use crate::utils::mime::detect_supported_image_mime_type_from_file;
use crate::utils::process_exit::exit;
use crate::utils::text::strip_bom;
use pi_ai::types::ImageContent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessedFiles {
    pub text: String,
    pub images: Vec<ImageContent>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessFileOptions {
    /// Whether to auto-resize images. Default: true
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resize_images: Option<bool>,
}

/// `path.resolve(p)` against the process cwd (lexical normalization, no symlink resolution).
fn resolve_absolute(path: &str) -> String {
    let joined = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Process @file arguments into text content and image attachments.
///
/// Like TS, a missing or unreadable file prints an error and exits the process with code 1.
pub async fn process_file_arguments(
    file_args: &[String],
    options: Option<&ProcessFileOptions>,
) -> anyhow::Result<ProcessedFiles> {
    let auto_resize_images = options.and_then(|o| o.auto_resize_images).unwrap_or(true);
    let mut text = String::new();
    let mut images: Vec<ImageContent> = Vec::new();
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();

    for file_arg in file_args {
        // Expand and resolve path (handles ~ expansion and macOS screenshot Unicode spaces)
        let absolute_path = resolve_absolute(&resolve_read_path(file_arg, &cwd));

        // Check if file exists
        let metadata = match tokio::fs::metadata(&absolute_path).await {
            Ok(metadata) => metadata,
            Err(_) => {
                eprintln!("{}", chalk::red(&format!("Error: File not found: {absolute_path}")));
                exit(1);
            }
        };

        // Skip empty files
        if metadata.len() == 0 {
            continue;
        }

        let mime_type = detect_supported_image_mime_type_from_file(&absolute_path).await?;

        if let Some(mime_type) = mime_type {
            // Handle image file
            let content = tokio::fs::read(&absolute_path).await?;
            let processed = process_image(
                &content,
                &mime_type,
                Some(&ProcessImageOptions { auto_resize_images: Some(auto_resize_images), resize_options: None }),
            )
            .await?;
            match processed {
                ProcessImageResult::ProcessImageResultVariant2(failure) => {
                    text.push_str(&format!("<file name=\"{absolute_path}\">{}</file>\n", failure.message));
                    continue;
                }
                ProcessImageResult::ProcessImageResultVariant1(success) => {
                    images.push(ImageContent { data: success.data, mime_type: success.mime_type });
                    // Add text reference to image with optional processing hints
                    if success.hints.is_empty() {
                        text.push_str(&format!("<file name=\"{absolute_path}\"></file>\n"));
                    } else {
                        text.push_str(&format!(
                            "<file name=\"{absolute_path}\">{}</file>\n",
                            success.hints.join("\n")
                        ));
                    }
                }
            }
        } else {
            // Handle text file
            match tokio::fs::read(&absolute_path).await {
                Ok(bytes) => {
                    let content = strip_bom(&String::from_utf8_lossy(&bytes));
                    text.push_str(&format!("<file name=\"{absolute_path}\">\n{content}\n</file>\n"));
                }
                Err(error) => {
                    eprintln!(
                        "{}",
                        chalk::red(&format!("Error: Could not read file {absolute_path}: {error}"))
                    );
                    exit(1);
                }
            }
        }
    }

    Ok(ProcessedFiles { text, images })
}
