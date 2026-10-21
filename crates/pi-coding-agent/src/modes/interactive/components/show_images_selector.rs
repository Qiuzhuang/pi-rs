//! Port of `packages/coding-agent/src/modes/interactive/components/show-images-selector.ts`.

use pi_tui::components::select_list::SelectList;
use pi_tui::tui::Component;
use std::sync::Arc;

/// Component that renders a show images selector with borders
/// TS: `extends Container`
pub struct ShowImagesSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ShowImagesSelectorComponent {
    pub fn new(
        current_value: bool,
        on_select: Arc<dyn Fn(bool) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/show-images-selector.ts:ShowImagesSelectorComponent.constructor"
        )
    }

    pub fn get_select_list(&self) -> Arc<SelectList> {
        todo!(
            "port modes/interactive/components/show-images-selector.ts:ShowImagesSelectorComponent.getSelectList"
        )
    }
}

// TODO(port): impl pi-tui::Container for ShowImagesSelectorComponent

impl Component for ShowImagesSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!(
            "port modes/interactive/components/show-images-selector.ts:ShowImagesSelectorComponent.render"
        )
    }
}
