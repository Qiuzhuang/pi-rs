//! Empty lines. Port of `components/spacer.ts`.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::tui::Component;

/// Renders a number of empty lines.
pub struct Spacer {
    lines: AtomicUsize,
}

impl Default for Spacer {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Spacer {
    pub fn new(lines: usize) -> Self {
        Self { lines: AtomicUsize::new(lines) }
    }

    pub fn set_lines(&self, lines: usize) {
        self.lines.store(lines, Ordering::SeqCst);
    }
}

impl Component for Spacer {
    fn render(&self, _width: usize) -> Vec<String> {
        vec![String::new(); self.lines.load(Ordering::SeqCst)]
    }
}
