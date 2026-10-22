//! Port of `packages/coding-agent/src/cli/setup.ts`.

use crate::core::http_dispatcher::configure_http_dispatcher;

/// Process-level setup run by the `pi` binary before `main()`.
///
/// TS also sets `process.title` (no portable Rust equivalent; the binary is already named
/// `pi`) and silences `process.emitWarning` (Rust has no runtime warnings to silence).
pub fn setup_cli() {
    // SAFETY: called at startup from the binary's main thread before other threads exist.
    unsafe {
        std::env::set_var("PI_CODING_AGENT", "true");
        std::env::set_var("AI_AGENT", "pi");
    }
    // Configure the HTTP client before provider requests. Settings are applied
    // once SettingsManager has loaded global/project configuration.
    let _ = configure_http_dispatcher(None);
}
