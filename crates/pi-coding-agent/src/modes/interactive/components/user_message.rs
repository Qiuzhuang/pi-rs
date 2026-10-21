//! Port of `packages/coding-agent/src/modes/interactive/components/user-message.ts`.

use crate::core::extensions::types::MarkdownTransformer;
use pi_tui::tui::Component;
use serde_json::Value;

/// Component that renders a user message
/// TS: `extends Container`
pub struct UserMessageComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl UserMessageComponent {
    pub fn new(
        text: &str,
        markdown_theme: Option<Value /* TS: pi-tui::MarkdownTheme */>,
        output_pad: Option<i64>,
        markdown_transformers: Option<&[MarkdownTransformer]>,
    ) -> Self {
        todo!("port modes/interactive/components/user-message.ts:UserMessageComponent.constructor")
    }

    pub fn set_output_pad(&self, padding: i64) {
        todo!("port modes/interactive/components/user-message.ts:UserMessageComponent.setOutputPad")
    }
}

// TODO(port): impl pi-tui::Container for UserMessageComponent

impl Component for UserMessageComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/user-message.ts:UserMessageComponent.render")
    }
}
