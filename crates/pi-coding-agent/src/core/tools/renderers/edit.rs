//! Port of `packages/coding-agent/src/core/tools/renderers/edit.ts`.

use crate::core::extensions::types::ToolRenderContext;
use crate::core::extensions::types::ToolRenderResultOptions;
use crate::core::tools::edit::EditToolDetails;
use crate::core::tools::edit_diff::Edit;
use crate::core::tools::edit_diff::EditDiffError;
use crate::core::tools::edit_diff::EditDiffResult;
use crate::modes::interactive::theme::theme::Theme;
use pi_agent::types::AgentToolResult;
use pi_tui::tui::ComponentRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EditPreview {
    EditDiffResult(EditDiffResult),
    EditDiffError(EditDiffError),
}

#[derive(Clone)]
pub struct EditRenderState {
    pub call_component: Option<EditCallRenderComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderableEditArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "file_path", default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edits: Option<Vec<Edit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditToolResultLikeContent {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditToolResultLike {
    pub content: Vec<EditToolResultLikeContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<EditToolDetails>,
}

#[derive(Clone)]
pub struct EditCallRenderComponentMouseLayoutChildren {
    pub component: ComponentRef,
    pub height: i64,
}

#[derive(Clone)]
pub struct EditCallRenderComponentMouseLayout {
    pub width: i64,
    pub children: Vec<EditCallRenderComponentMouseLayoutChildren>,
}

// TS: Box & { preview?: EditPreview; previewArgsKey?: string; previewPending?: boolean; settledError?: boolean; }
#[derive(Clone)]
pub struct EditCallRenderComponent {
    pub children: Vec<ComponentRef>,
    pub padding_x: i64,
    pub padding_y: i64,
    pub bg_fn: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    pub cache: Option<Value /* TS: pi-tui::RenderCache */>,
    pub mouse_layout: Option<EditCallRenderComponentMouseLayout>,
    pub add_child: Arc<dyn Fn(ComponentRef) + Send + Sync>,
    pub remove_child: Arc<dyn Fn(ComponentRef) + Send + Sync>,
    pub clear: Arc<dyn Fn() + Send + Sync>,
    pub set_bg_fn: Arc<dyn Fn(Option<Arc<dyn Fn(String) -> String + Send + Sync>>) + Send + Sync>,
    pub set_padding_x: Arc<dyn Fn(i64) + Send + Sync>,
    pub invalidate_cache: Arc<dyn Fn() + Send + Sync>,
    pub match_cache: Arc<dyn Fn(i64, Vec<String>, Option<String>) -> bool + Send + Sync>,
    pub invalidate: Arc<dyn Fn() + Send + Sync>,
    pub handle_mouse: Arc<
        dyn Fn(
                Value, /* TS: pi-tui::TuiMouseEvent */
            ) -> Option<Value /* TS: pi-tui::TuiMouseDispatchResult */>
            + Send
            + Sync,
    >,
    pub render: Arc<dyn Fn(i64) -> Vec<String> + Send + Sync>,
    pub apply_bg: Arc<dyn Fn(String, i64) -> String + Send + Sync>,
    pub preview: Option<EditPreview>,
    pub preview_args_key: Option<String>,
    pub preview_pending: Option<bool>,
    pub settled_error: Option<bool>,
}

#[derive(Clone)]
pub struct EditRenderersValue {
    /// Custom rendering for tool call display
    pub render_call:
        Option<Arc<dyn Fn(Value, Arc<Theme>, ToolRenderContext) -> ComponentRef + Send + Sync>>,
    /// Custom rendering for tool result display
    pub render_result: Option<
        Arc<
            dyn Fn(
                    AgentToolResult,
                    ToolRenderResultOptions,
                    Arc<Theme>,
                    ToolRenderContext,
                ) -> ComponentRef
                + Send
                + Sync,
        >,
    >,
}

pub static EDIT_RENDERERS: LazyLock<EditRenderersValue> =
    LazyLock::new(|| todo!("port core/tools/renderers/edit.ts:editRenderers"));
