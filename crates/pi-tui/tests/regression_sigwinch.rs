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

/// native-platform.test.ts: without the N-API prebuilds there is no native helper on any platform
/// (the Linux Wayland case of "Linux loads X11 lazily" expects `undefined` too); hosts supply their
/// own clipboard. The prebuild ELF layout, Windows clipboard and X11 module-cache cases test the
/// TS binaries and are not portable.
#[test]
fn no_native_helper_without_prebuilds() {
    use pi_tui::native_platform::{ModifierKey, get_native_clipboard, is_native_modifier_pressed};
    assert!(get_native_clipboard().is_none());
    for key in [ModifierKey::Shift, ModifierKey::Command, ModifierKey::Control, ModifierKey::Option] {
        assert!(!is_native_modifier_pressed(key));
    }
}
