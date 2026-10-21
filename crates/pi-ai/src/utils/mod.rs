//! Utilities (port of `utils/*`).

pub mod diagnostics;
pub mod event_stream;
pub mod identity;
pub mod version;

/// Current Unix time in milliseconds (`Date.now()`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
