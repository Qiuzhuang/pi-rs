//! Port of `packages/coding-agent/src/modes/interactive/components/theme-selector.ts`.

use pi_tui::components::select_list::SelectList;
use pi_tui::tui::Component;
use std::sync::Arc;

/// Component that renders a theme selector
/// TS: `extends Container`
pub struct ThemeSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ThemeSelectorComponent {
    pub fn new(
        current_theme: &str,
        on_select: Arc<dyn Fn(String) + Send + Sync>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        on_preview: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/theme-selector.ts:ThemeSelectorComponent.constructor"
        )
    }

    pub fn get_select_list(&self) -> Arc<SelectList> {
        todo!(
            "port modes/interactive/components/theme-selector.ts:ThemeSelectorComponent.getSelectList"
        )
    }
}

// TODO(port): impl pi-tui::Container for ThemeSelectorComponent

impl Component for ThemeSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/theme-selector.ts:ThemeSelectorComponent.render")
    }
}
