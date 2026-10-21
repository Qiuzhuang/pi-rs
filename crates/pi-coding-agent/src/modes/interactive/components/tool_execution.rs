//! Port of `packages/coding-agent/src/modes/interactive/components/tool-execution.ts`.

use crate::core::extensions::types::ToolDefinition;
use crate::core::extensions::types::ToolRenderers;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_images: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_width_cells: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_pad: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionComponentResultContent {
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
pub struct ToolExecutionComponentResult {
    pub content: Vec<ToolExecutionComponentResultContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    pub is_error: bool,
    /// Execution time of a final result.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub duration_ms: Option<f64>,
}

#[derive(Clone)]
pub enum ToolExecutionComponentToolDefinition {
    ToolRenderers(ToolRenderers),
    Arc(Arc<dyn ToolDefinition>),
}

/// TS: `extends Container`
pub struct ToolExecutionComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ToolExecutionComponent {
    pub fn new(
        tool_name: &str,
        tool_call_id: &str,
        args: Value,
        options: Option<&ToolExecutionOptions>,
        tool_definition: Option<&ToolExecutionComponentToolDefinition>,
        ui: Arc<Tui>,
        cwd: &str,
    ) -> Self {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.constructor"
        )
    }

    pub fn update_args(&self, args: Value) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.updateArgs"
        )
    }

    pub fn mark_execution_started(&self) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.markExecutionStarted"
        )
    }

    pub fn set_args_complete(&self) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.setArgsComplete"
        )
    }

    pub fn update_result(&self, result: &ToolExecutionComponentResult, is_partial: Option<bool>) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.updateResult"
        )
    }

    pub fn set_expanded(&self, expanded: bool) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.setExpanded"
        )
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.setOutputPad"
        )
    }

    pub fn set_show_images(&self, show: bool) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.setShowImages"
        )
    }

    pub fn set_image_width_cells(&self, width: i64) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.setImageWidthCells"
        )
    }
}

// TODO(port): impl pi-tui::Container for ToolExecutionComponent

impl Component for ToolExecutionComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.render")
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/tool-execution.ts:ToolExecutionComponent.invalidate"
        )
    }
}
