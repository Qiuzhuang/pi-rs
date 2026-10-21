//! Port of `packages/coding-agent/src/modes/interactive/components/first-time-setup.ts`.

use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstTimeSetupResult {
    pub theme: String,
    pub share_analytics: bool,
}

#[derive(Clone)]
pub struct FirstTimeSetupOptions {
    pub on_theme_preview: Arc<dyn Fn(String) + Send + Sync>,
    pub on_submit: Arc<dyn Fn(FirstTimeSetupResult) + Send + Sync>,
    pub on_cancel: Arc<dyn Fn() + Send + Sync>,
}

/// First-time setup dialog: theme choice and analytics opt-in.
/// TS: `extends Container`
pub struct FirstTimeSetupComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl FirstTimeSetupComponent {
    pub fn new(options: &FirstTimeSetupOptions) -> Self {
        todo!(
            "port modes/interactive/components/first-time-setup.ts:FirstTimeSetupComponent.constructor"
        )
    }
}

// TODO(port): impl pi-tui::Container for FirstTimeSetupComponent

impl Component for FirstTimeSetupComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/first-time-setup.ts:FirstTimeSetupComponent.render"
        )
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/first-time-setup.ts:FirstTimeSetupComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/first-time-setup.ts:FirstTimeSetupComponent.invalidate"
        )
    }
}
