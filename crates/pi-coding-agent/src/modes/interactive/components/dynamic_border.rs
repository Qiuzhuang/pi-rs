//! Port of `packages/coding-agent/src/modes/interactive/components/dynamic-border.ts`.

use pi_tui::tui::Component;
use std::sync::Arc;

/// Dynamic border component that adjusts to viewport width.
///
/// Note: When used from extensions loaded via jiti, the global `theme` may be undefined
/// because jiti creates a separate module cache. Always pass an explicit color
/// function when using DynamicBorder in components exported for extension use.
/// TS: `implements Component`
pub struct DynamicBorder {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl DynamicBorder {
    pub fn new(color: Option<Arc<dyn Fn(String) -> String + Send + Sync>>) -> Self {
        todo!("port modes/interactive/components/dynamic-border.ts:DynamicBorder.constructor")
    }
}

impl Component for DynamicBorder {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/dynamic-border.ts:DynamicBorder.render")
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/dynamic-border.ts:DynamicBorder.invalidate")
    }
}
