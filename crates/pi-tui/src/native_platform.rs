//! Native platform helpers. Port of `native-platform.ts` and `native-modifiers.ts`.
//!
//! The TypeScript package loads prebuilt N-API modules (`*.node`) for clipboard access and
//! modifier-key state on macOS/Windows and X11 clipboard access on Linux. Those binaries
//! cannot be loaded from Rust, so no native helper is available: [`get_native_clipboard`]
//! returns `None` and [`is_native_modifier_pressed`] returns `false`. Hosts can supply their
//! own [`NativeClipboard`] implementation (e.g. via command-line clipboard tools).

use crate::event_loop::BoxFuture;

/// Modifier keys that can be queried natively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierKey {
    Shift,
    Command,
    Control,
    Option,
}

/// Result of a clipboard read: `Unavailable` (no access), `Empty` (no data of that kind) or the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardRead<T> {
    Unavailable,
    Empty,
    Data(T),
}

/// System clipboard access.
pub trait NativeClipboard: Send + Sync {
    fn get_text(&self) -> BoxFuture<Result<ClipboardRead<String>, String>>;
    fn get_image(&self) -> BoxFuture<Result<ClipboardRead<Vec<u8>>, String>>;
    /// POSIX paths of file URLs on the clipboard.
    fn get_file_paths(&self) -> Option<BoxFuture<Result<ClipboardRead<Vec<String>>, String>>> {
        None
    }
    fn set_text(&self, _text: &str) -> Option<BoxFuture<Result<(), String>>> {
        None
    }
}

/// The native clipboard helper for this platform (none in the Rust port).
pub fn get_native_clipboard() -> Option<std::sync::Arc<dyn NativeClipboard>> {
    None
}

/// Whether `key` is currently held, as reported by the native helper (always `false` here).
pub fn is_native_modifier_pressed(_key: ModifierKey) -> bool {
    false
}
