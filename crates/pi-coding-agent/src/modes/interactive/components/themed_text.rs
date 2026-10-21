//! Port of `packages/coding-agent/src/modes/interactive/components/themed-text.ts`.

use pi_tui::tui::Component;
use std::sync::Arc;

/// Text whose content applies theme colors. Plain `Text` keeps the colors its string was built with, so
/// a theme change, or the system theme receiving the terminal's colors, would leave it stale. This
/// rebuilds the string from `build` after every invalidation, which the UI performs on theme changes.
///
/// `build` must return the same content each time, apart from colors. Snapshot changing data before
/// creating the component, or call `invalidate()` after changing state that `build` reads.
/// TS: `extends Text`
pub struct ThemedText {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ThemedText {
    pub fn new(
        build: Arc<dyn Fn() -> String + Send + Sync>,
        padding_x: Option<i64>,
        padding_y: Option<i64>,
    ) -> Self {
        todo!("port modes/interactive/components/themed-text.ts:ThemedText.constructor")
    }
}

// TODO(port): impl pi-tui::Text for ThemedText

impl Component for ThemedText {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/themed-text.ts:ThemedText.render")
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/themed-text.ts:ThemedText.invalidate")
    }
}
