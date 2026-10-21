//! Port of `packages/coding-agent/src/modes/interactive/components/extension-selector.ts`.

use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use std::sync::Arc;

#[derive(Clone)]
pub struct ExtensionSelectorOptions {
    pub tui: Option<Arc<Tui>>,
    pub timeout: Option<i64>,
    pub on_toggle_tools_expanded: Option<Arc<dyn Fn() + Send + Sync>>,
    pub description: Option<String>,
}

/// TS: `extends Container`
pub struct ExtensionSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ExtensionSelectorComponent {
    pub fn new(
        title: &str,
        options: &[String],
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        opts: Option<&ExtensionSelectorOptions>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/extension-selector.ts:ExtensionSelectorComponent.constructor"
        )
    }

    pub fn dispose(&self) {
        todo!(
            "port modes/interactive/components/extension-selector.ts:ExtensionSelectorComponent.dispose"
        )
    }
}

// TODO(port): impl pi-tui::Container for ExtensionSelectorComponent

impl Component for ExtensionSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/extension-selector.ts:ExtensionSelectorComponent.render"
        )
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/extension-selector.ts:ExtensionSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
