//! Port of `packages/coding-agent/src/cli/initial-message.ts`.

use crate::cli::args::Args;
use pi_ai::types::ImageContent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialMessageInput {
    pub parsed: Args,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_images: Option<Vec<ImageContent>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialMessageResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_images: Option<Vec<ImageContent>>,
}

/// Combine stdin content, @file text, and the first CLI message into a single
/// initial prompt for non-interactive mode.
pub fn build_initial_message(options: &InitialMessageInput) -> InitialMessageResult {
    todo!("port cli/initial-message.ts:buildInitialMessage")
}
