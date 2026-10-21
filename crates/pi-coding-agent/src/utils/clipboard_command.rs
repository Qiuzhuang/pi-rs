//! Port of `packages/coding-agent/src/utils/clipboard-command.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunClipboardCommandOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_buffer_bytes: Option<i64>,
}

/// Undefined means the command failed; an empty buffer is a successful result.
pub async fn run_clipboard_command(
    command: &str,
    args: &[String],
    options: Option<&RunClipboardCommandOptions>,
) -> anyhow::Result<Option<Vec<u8>>> {
    todo!("port utils/clipboard-command.ts:runClipboardCommand")
}
