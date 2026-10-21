//! Port of `packages/coding-agent/src/modes/interactive/components/thinking-selector.ts`.

use pi_agent::types::ThinkingLevel;
use pi_tui::components::select_list::SelectList;
use pi_tui::tui::Component;
use std::sync::Arc;

/// Component that renders a thinking level selector with borders
/// TS: `extends Container implements Focusable`
pub struct ThinkingSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ThinkingSelectorComponent {
    pub fn new(
        current_level: ThinkingLevel,
        available_levels: &[ThinkingLevel],
        on_select: Arc<dyn Fn(ThinkingLevel) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_select_as_default: Option<Arc<dyn Fn(ThinkingLevel) + Send + Sync>>,
        default_thinking_level: Option<ThinkingLevel>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/thinking-selector.ts:ThinkingSelectorComponent.constructor"
        )
    }

    pub fn get_select_list(&self) -> Arc<SelectList> {
        todo!(
            "port modes/interactive/components/thinking-selector.ts:ThinkingSelectorComponent.getSelectList"
        )
    }
}

// TODO(port): impl pi-tui::Container for ThinkingSelectorComponent

impl Component for ThinkingSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/thinking-selector.ts:ThinkingSelectorComponent.render"
        )
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/thinking-selector.ts:ThinkingSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
