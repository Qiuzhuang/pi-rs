//! Port of `packages/coding-agent/src/modes/interactive/components/dynamic-border.ts`.

use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use pi_tui::tui::Component;
use std::sync::Arc;

/// Border color function.
pub type BorderColorFn = Arc<dyn Fn(String) -> String + Send + Sync>;

/// Dynamic border component that adjusts to viewport width.
///
/// Note: When used from extensions loaded via jiti, the global `theme` may be undefined
/// because jiti creates a separate module cache. Always pass an explicit color
/// function when using DynamicBorder in components exported for extension use.
/// TS: `implements Component`
pub struct DynamicBorder {
    color: BorderColorFn,
}

impl DynamicBorder {
    pub fn new(color: Option<Arc<dyn Fn(String) -> String + Send + Sync>>) -> Self {
        let color = color.unwrap_or_else(|| Arc::new(|s: String| theme().fg(&ThemeColor::Border, &s)));
        Self { color }
    }
}

impl Component for DynamicBorder {
    fn render(&self, width: usize) -> Vec<String> {
        vec![(self.color)("─".repeat(width.max(1)))]
    }

    fn invalidate(&self) {
        // No cached state to invalidate currently
    }
}
