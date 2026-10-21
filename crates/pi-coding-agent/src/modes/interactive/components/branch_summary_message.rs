//! Port of `packages/coding-agent/src/modes/interactive/components/branch-summary-message.ts`.

use crate::compat::MarkdownTheme;
use crate::core::messages::BranchSummaryMessage;
use pi_tui::tui::Component;

/// Component that renders a branch summary message with collapsed/expanded state.
/// Uses same background color as custom messages for visual consistency.
/// TS: `extends Box`
pub struct BranchSummaryMessageComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl BranchSummaryMessageComponent {
    pub fn new(
        message: &BranchSummaryMessage,
        markdown_theme: Option<&MarkdownTheme>,
        output_pad: Option<i64>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/branch-summary-message.ts:BranchSummaryMessageComponent.constructor"
        )
    }

    pub fn set_expanded(&self, expanded: bool) {
        todo!(
            "port modes/interactive/components/branch-summary-message.ts:BranchSummaryMessageComponent.setExpanded"
        )
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        todo!(
            "port modes/interactive/components/branch-summary-message.ts:BranchSummaryMessageComponent.setOutputPad"
        )
    }
}

// TODO(port): impl pi-tui::Box for BranchSummaryMessageComponent

impl Component for BranchSummaryMessageComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/branch-summary-message.ts:BranchSummaryMessageComponent.render"
        )
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/branch-summary-message.ts:BranchSummaryMessageComponent.invalidate"
        )
    }
}
