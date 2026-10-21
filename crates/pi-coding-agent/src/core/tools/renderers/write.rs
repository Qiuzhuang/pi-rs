//! Port of `packages/coding-agent/src/core/tools/renderers/write.ts`.

use crate::core::extensions::types::ToolRenderContext;
use crate::core::extensions::types::ToolRenderResultOptions;
use crate::modes::interactive::theme::theme::Theme;
use pi_agent::types::AgentToolResult;
use pi_tui::tui::Component;
use pi_tui::tui::ComponentRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteHighlightCache {
    #[serde(default)]
    pub raw_path: Option<String>,
    pub lang: String,
    pub raw_content: String,
    pub normalized_lines: Vec<String>,
    pub highlighted_lines: Vec<String>,
}

/// TS: `extends Text`
pub struct WriteCallRenderComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl WriteCallRenderComponent {
    pub fn new() -> Self {
        todo!("port core/tools/renderers/write.ts:WriteCallRenderComponent.constructor")
    }

    pub fn cache(&self) -> Option<WriteHighlightCache> {
        todo!("port core/tools/renderers/write.ts:WriteCallRenderComponent.cache")
    }

    pub fn set_cache(&self, value: Option<WriteHighlightCache>) {
        todo!("port core/tools/renderers/write.ts:WriteCallRenderComponent.cache")
    }
}

// TODO(port): impl pi-tui::Text for WriteCallRenderComponent

impl Component for WriteCallRenderComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port core/tools/renderers/write.ts:WriteCallRenderComponent.render")
    }
}

#[derive(Clone)]
pub struct WriteRenderersValue {
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

pub static WRITE_RENDERERS: LazyLock<WriteRenderersValue> =
    LazyLock::new(|| todo!("port core/tools/renderers/write.ts:writeRenderers"));
