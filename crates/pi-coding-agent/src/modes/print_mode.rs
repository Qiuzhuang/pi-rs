//! Port of `packages/coding-agent/src/modes/print-mode.ts`.

use crate::core::agent_session_runtime::AgentSessionRuntime;
use pi_ai::types::ImageContent;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrintModeOptionsMode {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json")]
    Json,
}

impl PrintModeOptionsMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

/// Options for print mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintModeOptions {
    /// Output mode: "text" for final response only, "json" for all events
    pub mode: PrintModeOptionsMode,
    /// Array of additional prompts to send after initialMessage
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<String>>,
    /// First message to send (may contain @file content)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_message: Option<String>,
    /// Images to attach to the initial message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_images: Option<Vec<ImageContent>>,
}

/// Run in print (single-shot) mode.
/// Sends prompts to the agent and outputs the result.
pub async fn run_print_mode(
    runtime_host: Arc<AgentSessionRuntime>,
    options: &PrintModeOptions,
) -> anyhow::Result<f64> {
    todo!("port modes/print-mode.ts:runPrintMode")
}
