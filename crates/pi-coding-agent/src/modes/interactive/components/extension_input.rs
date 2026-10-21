//! Port of `packages/coding-agent/src/modes/interactive/components/extension-input.ts`.

use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use std::sync::Arc;

#[derive(Clone)]
pub struct ExtensionInputOptions {
    pub tui: Option<Arc<Tui>>,
    pub timeout: Option<i64>,
    pub initial_value: Option<String>,
    pub description: Option<String>,
}

/// TS: `extends Container implements Focusable`
pub struct ExtensionInputComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ExtensionInputComponent {
    pub fn new(
        title: &str,
        placeholder: Option<&str>,
        on_submit: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        opts: Option<&ExtensionInputOptions>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/extension-input.ts:ExtensionInputComponent.constructor"
        )
    }

    pub fn dispose(&self) {
        todo!(
            "port modes/interactive/components/extension-input.ts:ExtensionInputComponent.dispose"
        )
    }
}

// TODO(port): impl pi-tui::Container for ExtensionInputComponent

impl Component for ExtensionInputComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/extension-input.ts:ExtensionInputComponent.render")
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/extension-input.ts:ExtensionInputComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
