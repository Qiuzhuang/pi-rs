//! Unicode sanitization (port of `utils/sanitize-unicode.ts`).
//!
//! Rust strings are always valid UTF-8 and cannot hold unpaired UTF-16 surrogates, so
//! sanitizing is the identity on `&str`. [`sanitize_surrogates_utf16`] handles raw UTF-16.

/// Removes unpaired surrogates. Identity for Rust strings (they cannot contain any).
pub fn sanitize_surrogates(text: &str) -> String {
    text.to_string()
}

/// Remove unpaired surrogate code units from UTF-16 data and decode it.
pub fn sanitize_surrogates_utf16(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied()).filter_map(Result::ok).collect()
}
