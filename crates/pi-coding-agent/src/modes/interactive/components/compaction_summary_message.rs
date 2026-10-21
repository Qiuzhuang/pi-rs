//! Port of `packages/coding-agent/src/modes/interactive/components/compaction-summary-message.ts`.

use crate::core::messages::CompactionSummaryMessage;
use pi_tui::tui::Component;
use serde_json::Value;

/// Component that renders a compaction message with collapsed/expanded state.
/// Uses same background color as custom messages for visual consistency.
/// TS: `extends Box`
pub struct CompactionSummaryMessageComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl CompactionSummaryMessageComponent {
    pub fn new(
        message: &CompactionSummaryMessage,
        markdown_theme: Option<Value /* TS: pi-tui::MarkdownTheme */>,
        output_pad: Option<i64>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/compaction-summary-message.ts:CompactionSummaryMessageComponent.constructor"
        )
    }

    pub fn set_expanded(&self, expanded: bool) {
        todo!(
            "port modes/interactive/components/compaction-summary-message.ts:CompactionSummaryMessageComponent.setExpanded"
        )
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        todo!(
            "port modes/interactive/components/compaction-summary-message.ts:CompactionSummaryMessageComponent.setOutputPad"
        )
    }
}

// TODO(port): impl pi-tui::Box for CompactionSummaryMessageComponent

impl Component for CompactionSummaryMessageComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/compaction-summary-message.ts:CompactionSummaryMessageComponent.render"
        )
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/compaction-summary-message.ts:CompactionSummaryMessageComponent.invalidate"
        )
    }
}
