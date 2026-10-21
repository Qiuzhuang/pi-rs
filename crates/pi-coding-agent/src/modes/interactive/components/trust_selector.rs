//! Port of `packages/coding-agent/src/modes/interactive/components/trust-selector.ts`.

use crate::core::trust_manager::ProjectTrustStoreEntry;
use crate::core::trust_manager::ProjectTrustUpdate;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// TS: Pick<ProjectTrustOption, "trusted" | "updates">
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustSelection {
    pub trusted: bool,
    pub updates: Vec<ProjectTrustUpdate>,
}

#[derive(Clone)]
pub struct TrustSelectorOptions {
    pub cwd: String,
    pub saved_decision: Option<ProjectTrustStoreEntry>,
    pub project_trusted: bool,
    pub on_select: Arc<dyn Fn(TrustSelection) + Send + Sync>,
    pub on_cancel: Arc<dyn Fn() + Send + Sync>,
}

/// TS: `extends Container`
pub struct TrustSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl TrustSelectorComponent {
    pub fn new(options: &TrustSelectorOptions) -> Self {
        todo!(
            "port modes/interactive/components/trust-selector.ts:TrustSelectorComponent.constructor"
        )
    }
}

// TODO(port): impl pi-tui::Container for TrustSelectorComponent

impl Component for TrustSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/trust-selector.ts:TrustSelectorComponent.render")
    }

    fn handle_input(&self, data: &str) {
        todo!(
            "port modes/interactive/components/trust-selector.ts:TrustSelectorComponent.handleInput"
        )
    }

    fn handles_input(&self) -> bool {
        true
    }
}
