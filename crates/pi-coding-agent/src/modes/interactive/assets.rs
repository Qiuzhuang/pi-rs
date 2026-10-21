//! Built-in interactive assets (TS: `modes/interactive/assets/*`), embedded in the binary.

/// `clankolas.png`, shown by the Earendil announcement.
pub const CLANKOLAS_PNG: &[u8] = include_bytes!("assets/clankolas.png");

/// A bundled asset by file name.
pub fn get_bundled_interactive_asset(name: &str) -> Option<&'static [u8]> {
    match name {
        "clankolas.png" => Some(CLANKOLAS_PNG),
        _ => None,
    }
}
