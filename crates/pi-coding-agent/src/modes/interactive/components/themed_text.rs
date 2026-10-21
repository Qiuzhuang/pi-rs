//! Port of `packages/coding-agent/src/modes/interactive/components/themed-text.ts`.

use pi_tui::components::text::Text;
use pi_tui::tui::Component;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Text whose content applies theme colors. Plain `Text` keeps the colors its string was built with, so
/// a theme change, or the system theme receiving the terminal's colors, would leave it stale. This
/// rebuilds the string from `build` after every invalidation, which the UI performs on theme changes.
///
/// `build` must return the same content each time, apart from colors. Snapshot changing data before
/// creating the component, or call `invalidate()` after changing state that `build` reads.
/// TS: `extends Text`
pub struct ThemedText {
    text: Text,
    build: Arc<dyn Fn() -> String + Send + Sync>,
    stale: AtomicBool,
}

impl ThemedText {
    /// `padding_x`/`padding_y` default to 1.
    pub fn new(
        build: Arc<dyn Fn() -> String + Send + Sync>,
        padding_x: Option<i64>,
        padding_y: Option<i64>,
    ) -> Self {
        let padding_x = padding_x.unwrap_or(1).max(0) as usize;
        let padding_y = padding_y.unwrap_or(1).max(0) as usize;
        Self { text: Text::new("", padding_x, padding_y, None), build, stale: AtomicBool::new(true) }
    }

    /// The underlying `Text` (TS `this` as a `Text`).
    pub fn text(&self) -> &Text {
        &self.text
    }
}

impl Component for ThemedText {
    fn render(&self, width: usize) -> Vec<String> {
        if self.stale.swap(false, Ordering::SeqCst) {
            self.text.set_text((self.build)());
        }
        self.text.render(width)
    }

    fn invalidate(&self) {
        self.text.invalidate();
        self.stale.store(true, Ordering::SeqCst);
    }
}
