//! Port of `packages/coding-agent/src/modes/interactive/components/custom-entry.ts`.

use crate::core::extensions::types::EntryRenderer;
use crate::core::session_manager::CustomEntry;
use pi_tui::tui::Component;

/// Component that renders a custom session entry from extensions.
/// The host owns transcript spacing; renderer output should provide only its content.
/// TS: `extends Container`
pub struct CustomEntryComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl CustomEntryComponent {
    pub fn new(entry: &CustomEntry, renderer: &EntryRenderer, output_pad: Option<i64>) -> Self {
        todo!("port modes/interactive/components/custom-entry.ts:CustomEntryComponent.constructor")
    }

    pub fn has_content(&self) -> bool {
        todo!("port modes/interactive/components/custom-entry.ts:CustomEntryComponent.hasContent")
    }

    pub fn set_expanded(&self, expanded: bool) {
        todo!("port modes/interactive/components/custom-entry.ts:CustomEntryComponent.setExpanded")
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        todo!("port modes/interactive/components/custom-entry.ts:CustomEntryComponent.setOutputPad")
    }
}

// TODO(port): impl pi-tui::Container for CustomEntryComponent

impl Component for CustomEntryComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/custom-entry.ts:CustomEntryComponent.render")
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/custom-entry.ts:CustomEntryComponent.invalidate")
    }
}
