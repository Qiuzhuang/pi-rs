//! Terminal UI framework with differential rendering (Rust port of `@earendil-works/pi-tui`).

pub mod autocomplete;
pub mod colors;
pub mod components;
pub mod event_loop;
pub mod fuzzy;
pub mod keybindings;
pub mod keys;
pub mod kill_ring;
pub mod layout;
pub mod layout_node;
pub mod oklab;
pub mod program_status;
pub mod stdin_buffer;
pub mod terminal;
pub mod terminal_colors;
pub mod terminal_image;
pub mod testing;
pub mod tui;
pub mod tui_alt_screen;
pub mod tui_main_screen;
pub mod undo_stack;
#[rustfmt::skip]
mod unicode_tables;
pub mod utils;
pub mod word_navigation;

/// A styling function, e.g. a theme color: takes text and returns it wrapped in ANSI codes.
pub type StyleFn = std::sync::Arc<dyn Fn(&str) -> String + Send + Sync>;
