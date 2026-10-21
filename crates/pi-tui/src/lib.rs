//! Terminal UI framework with differential rendering (Rust port of `@earendil-works/pi-tui`).

pub mod colors;
pub mod keys;
pub mod oklab;
pub mod terminal_colors;
#[rustfmt::skip]
mod unicode_tables;
pub mod utils;

/// A styling function, e.g. a theme color: takes text and returns it wrapped in ANSI codes.
pub type StyleFn = std::sync::Arc<dyn Fn(&str) -> String + Send + Sync>;
