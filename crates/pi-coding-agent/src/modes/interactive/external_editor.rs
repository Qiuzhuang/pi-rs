//! Port of `packages/coding-agent/src/modes/interactive/external-editor.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEditorOptions {
    pub command: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all_fields = "camelCase")]
pub enum ExternalEditorResult {
    #[serde(rename = "complete")]
    Complete { content: String },
    #[serde(rename = "failed")]
    Failed {},
}

pub async fn edit_in_external_editor(
    options: &ExternalEditorOptions,
) -> anyhow::Result<ExternalEditorResult> {
    todo!("port modes/interactive/external-editor.ts:editInExternalEditor")
}
