//! Terminal restore on process exit (ported from exit-restore.test.ts, regression #11170).
//!
//! The TS test spawns a fixture script; here the test binary re-runs itself with
//! `PI_EXIT_FIXTURE` set, and the fixture branch starts an alt-screen TUI on the real
//! process terminal and exits the process (optionally after `stop()`).

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::sync::Arc;

use pi_tui::event_loop::EventLoop;
use pi_tui::terminal::ProcessTerminal;
use pi_tui::tui::{Tui, TuiOptions, TuiStopOptions};
use pi_tui::tui_alt_screen::TuiAltScreenOptions;

const FIXTURE_ENV: &str = "PI_EXIT_FIXTURE";

fn run_fixture_if_requested() {
    let Ok(mode) = std::env::var(FIXTURE_ENV) else { return };
    let lp = EventLoop::new_manual();
    let _guard = lp.make_current();
    let tui = Tui::alt_screen_with(
        Arc::new(ProcessTerminal::new()),
        TuiOptions { event_loop: Some(lp.clone()), ..Default::default() },
        TuiAltScreenOptions { mouse: Some(true), ..Default::default() },
    );
    tui.start();
    if mode == "stop" {
        tui.stop(TuiStopOptions { preserve_screen: true });
    }
    std::process::exit(0);
}

fn run_fixture(mode: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let output = Command::new(exe)
        .args(["--exact", "fixture_entry", "--nocapture", "--test-threads=1"])
        .env(FIXTURE_ENV, mode)
        .env("TERM", "xterm-256color")
        .env_remove("TMUX")
        .env_remove("STY")
        .env_remove("ZELLIJ")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// Entry point of the fixture process; a no-op in normal test runs.
#[test]
fn fixture_entry() {
    run_fixture_if_requested();
}

#[test]
fn restores_terminal_modes_when_exiting_without_stop() {
    let output = run_fixture("exit");
    for seq in ["\x1b[?1006l", "\x1b[?1004l", "\x1b[?1003l", "\x1b[?1000l", "\x1b[<u", "\x1b[?2004l"] {
        assert_eq!(count(&output, seq), 1, "{seq:?} in {output:?}");
    }
    assert!(
        output.find("\x1b[<u").unwrap() < output.find("\x1b[?1049l").unwrap(),
        "Kitty pop must precede leaving alt screen"
    );
    assert!(output.ends_with("\x1b[?1049l\x1b[?25h"), "{output:?}");
}

#[test]
fn does_not_restore_twice_after_stop() {
    let output = run_fixture("stop");
    for seq in ["\x1b[?1006l", "\x1b[?1004l", "\x1b[<u", "\x1b[?2004l", "\x1b[?1049l"] {
        assert_eq!(count(&output, seq), 1, "{seq:?} in {output:?}");
    }
}
