//! Port of `packages/coding-agent/src/modes/interactive/components/custom-message.ts`.

use crate::compat::MarkdownTheme;
use crate::core::extensions::types::MessageRenderer;
use crate::core::messages::CustomMessage;
use pi_tui::tui::Component;

/// Component that renders a custom message entry from extensions.
/// Uses distinct styling to differentiate from user messages.
/// TS: `extends Container`
pub struct CustomMessageComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl CustomMessageComponent {
    pub fn new(
        message: &CustomMessage,
        custom_renderer: Option<&MessageRenderer>,
        markdown_theme: Option<&MarkdownTheme>,
        output_pad: Option<i64>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/custom-message.ts:CustomMessageComponent.constructor"
        )
    }

    pub fn set_expanded(&self, expanded: bool) {
        todo!(
            "port modes/interactive/components/custom-message.ts:CustomMessageComponent.setExpanded"
        )
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        todo!(
            "port modes/interactive/components/custom-message.ts:CustomMessageComponent.setOutputPad"
        )
    }
}

// TODO(port): impl pi-tui::Container for CustomMessageComponent

impl Component for CustomMessageComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/custom-message.ts:CustomMessageComponent.render")
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/custom-message.ts:CustomMessageComponent.invalidate"
        )
    }
}
