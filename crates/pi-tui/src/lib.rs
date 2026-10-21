//! Terminal UI framework with differential rendering (Rust port of `@earendil-works/pi-tui`).

pub mod colors;
pub mod fuzzy;
pub mod keybindings;
pub mod keys;
pub mod kill_ring;
pub mod oklab;
pub mod stdin_buffer;
pub mod terminal_colors;
pub mod undo_stack;
#[rustfmt::skip]
mod unicode_tables;
pub mod utils;
pub mod word_navigation;

/// A styling function, e.g. a theme color: takes text and returns it wrapped in ANSI codes.
pub type StyleFn = std::sync::Arc<dyn Fn(&str) -> String + Send + Sync>;
