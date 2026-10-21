//! Adds mouse handling to an existing component. Port of `components/mouse-region.ts`.

use std::sync::Arc;

use crate::tui::{Component, ComponentRef, MouseEvent, MouseResult, dispatch_mouse_event};

/// Mouse handler for [`MouseRegion`].
pub type MouseRegionHandler = Arc<dyn Fn(&MouseEvent) -> Option<MouseResult> + Send + Sync>;

/// Adds mouse handling to a component without changing its rendering.
pub struct MouseRegion {
    child: ComponentRef,
    on_mouse: MouseRegionHandler,
}

impl MouseRegion {
    pub fn new(child: ComponentRef, on_mouse: MouseRegionHandler) -> Self {
        Self { child, on_mouse }
    }
}

impl Component for MouseRegion {
    fn render(&self, width: usize) -> Vec<String> {
        self.child.render(width)
    }
    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        dispatch_mouse_event(&self.child, event).or_else(|| (self.on_mouse)(event))
    }
    fn invalidate(&self) {
        self.child.invalidate();
    }
}
