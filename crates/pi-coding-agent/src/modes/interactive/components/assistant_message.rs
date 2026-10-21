//! Port of `packages/coding-agent/src/modes/interactive/components/assistant-message.ts`.

use crate::compat::MarkdownTheme;
use crate::core::extensions::types::MarkdownTransformer;
use pi_ai::types::AssistantMessage;
use pi_tui::tui::Component;

/// Component that renders a complete assistant message
/// TS: `extends Container`
pub struct AssistantMessageComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl AssistantMessageComponent {
    pub fn new(
        message: Option<AssistantMessage>,
        hide_thinking_block: Option<bool>,
        markdown_theme: Option<&MarkdownTheme>,
        hidden_thinking_label: Option<&str>,
        output_pad: Option<i64>,
        markdown_transformers: Option<&[MarkdownTransformer]>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/assistant-message.ts:AssistantMessageComponent.constructor"
        )
    }

    pub fn set_hide_thinking_block(&self, hide: bool) {
        todo!(
            "port modes/interactive/components/assistant-message.ts:AssistantMessageComponent.setHideThinkingBlock"
        )
    }

    pub fn set_hidden_thinking_label(&self, label: &str) {
        todo!(
            "port modes/interactive/components/assistant-message.ts:AssistantMessageComponent.setHiddenThinkingLabel"
        )
    }

    pub fn set_output_pad(&self, padding: i64) {
        todo!(
            "port modes/interactive/components/assistant-message.ts:AssistantMessageComponent.setOutputPad"
        )
    }

    pub fn update_content(&self, message: AssistantMessage, is_streaming: Option<bool>) {
        todo!(
            "port modes/interactive/components/assistant-message.ts:AssistantMessageComponent.updateContent"
        )
    }
}

// TODO(port): impl pi-tui::Container for AssistantMessageComponent

impl Component for AssistantMessageComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/assistant-message.ts:AssistantMessageComponent.render"
        )
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/assistant-message.ts:AssistantMessageComponent.invalidate"
        )
    }
}
