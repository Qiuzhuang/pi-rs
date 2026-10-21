//! Port of `packages/coding-agent/src/modes/interactive/components/bash-execution.ts`.

use crate::core::tools::truncate::TruncationResult;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use std::sync::Arc;

/// TS: `extends Container`
pub struct BashExecutionComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl BashExecutionComponent {
    pub fn new(
        command: &str,
        ui: Arc<Tui>,
        exclude_from_context: Option<bool>,
        output_pad: Option<i64>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.constructor"
        )
    }

    /// Set whether the output is expanded (shows full output) or collapsed (preview only).
    pub fn set_expanded(&self, expanded: bool) {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.setExpanded"
        )
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.setOutputPad"
        )
    }

    pub fn append_output(&self, chunk: &str) {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.appendOutput"
        )
    }

    pub fn set_complete(
        &self,
        exit_code: Option<i64>,
        cancelled: bool,
        truncation_result: Option<&TruncationResult>,
        full_output_path: Option<&str>,
    ) {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.setComplete"
        )
    }

    /// Get the raw output for creating BashExecutionMessage.
    pub fn get_output(&self) -> String {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.getOutput"
        )
    }

    /// Get the command that was executed.
    pub fn get_command(&self) -> String {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.getCommand"
        )
    }
}

// TODO(port): impl pi-tui::Container for BashExecutionComponent

impl Component for BashExecutionComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/bash-execution.ts:BashExecutionComponent.render")
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/bash-execution.ts:BashExecutionComponent.invalidate"
        )
    }
}
