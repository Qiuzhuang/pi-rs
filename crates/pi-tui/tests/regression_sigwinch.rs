//! Port of regression-sigwinch-kill-eacces.test.ts, plus the native-platform.test.ts behavior
//! that applies without the TypeScript N-API prebuilds.
//!
//! The TS cases mock `process.kill` to throw EACCES/EPERM and fake `process.platform`. The Rust
//! `refresh_terminal_dimensions` discards the `kill(2)` result (so no errno can escape) and is
//! compiled out on non-Unix targets, so the cases reduce to "never panics" plus checking that the
//! signal really reaches this process on Unix. This file is its own test binary so the SIGWINCH
//! handler cannot race other tests.

use pi_tui::terminal::refresh_terminal_dimensions;

#[cfg(unix)]
mod unix {
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub static RECEIVED: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn on_sigwinch(_: libc::c_int) {
        RECEIVED.fetch_add(1, Ordering::SeqCst);
    }

    pub fn install() {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_sigwinch as extern "C" fn(libc::c_int) as usize;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(libc::SIGWINCH, &action, std::ptr::null_mut());
        }
    }
}

#[test]
fn refresh_is_best_effort_and_signals_this_process() {
    #[cfg(unix)]
    unix::install();
    // EACCES/EPERM from kill(2) are ignored: the call has no error path.
    refresh_terminal_dimensions();
    refresh_terminal_dimensions();
    #[cfg(unix)]
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while unix::RECEIVED.load(std::sync::atomic::Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(unix::RECEIVED.load(std::sync::atomic::Ordering::SeqCst) >= 1, "SIGWINCH was not delivered");
    }
}

/// native-platform.test.ts: the native clipboard helper (arboard in pi-rs) exists where TS loads its
/// addon: macOS and Windows, and Linux only with an X11 `DISPLAY` (the Wayland-only case of "Linux
/// loads X11 lazily" expects `undefined`). Modifier state is never available. The prebuild ELF
/// layout and module-cache cases test the TS binaries and are not portable.
#[test]
fn native_helper_only_where_ts_loads_its_addon() {
    use pi_tui::native_platform::{ModifierKey, get_native_clipboard, is_native_modifier_pressed};
    let display = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
    let expected = match std::env::consts::OS {
        "macos" | "windows" => true,
        "linux" => display,
        _ => false,
    };
    assert_eq!(get_native_clipboard().is_some(), expected);
    for key in [ModifierKey::Shift, ModifierKey::Command, ModifierKey::Control, ModifierKey::Option] {
        assert!(!is_native_modifier_pressed(key));
    }
}
