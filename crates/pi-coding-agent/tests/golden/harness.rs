#![allow(dead_code)]
//! Shared machinery for the golden UI tests: fixture loading, the fixed environment the generators
//! use (`scripts/golden/common.mjs`), and byte-for-byte comparison with readable failure output.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Once};

use parking_lot::{Mutex, MutexGuard};
use pi_coding_agent::modes::interactive::theme::theme::init_theme;
use pi_tui::event_loop::EventLoop;
use pi_tui::terminal_image::{ImageProtocol, TerminalCapabilities, set_capabilities};
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, Tui, TuiOptions};
use serde::Deserialize;
use serde_json::Value;

pub const HOME: &str = "/home/golden";
pub const CWD: &str = "/home/golden/project";
/// 2026-01-15T12:00:00Z, the clock of the generators.
pub const NOW: i64 = 1_768_478_400_000;

#[derive(Debug, Deserialize)]
pub struct Fixture {
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
pub struct Case {
    pub name: String,
    pub kind: String,
    pub input: Value,
    pub widths: Vec<usize>,
    #[serde(default)]
    pub caps: Option<Value>,
    /// theme name -> one list of lines per width
    pub out: indexmap::IndexMap<String, Vec<Vec<String>>>,
}

static LOCK: Mutex<()> = Mutex::new(());
static ENV: Once = Once::new();

/// Serializes the golden tests (they share the global theme and capabilities) and pins the
/// environment the generators ran in.
pub fn lock() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock();
    ENV.call_once(|| {
        // SAFETY: set once, before any golden case runs; all golden tests hold LOCK.
        unsafe {
            std::env::set_var("HOME", HOME);
            std::env::set_var("PI_CODING_AGENT_DIR", format!("{HOME}/.pi/agent"));
            std::env::set_var("PI_OFFLINE", "1");
            std::env::set_var("TZ", "UTC");
            std::env::set_var("COLORTERM", "truecolor");
            // chalk (theme.bold/italic/...) picks its level from stdout; the generators force level 3.
            std::env::set_var("FORCE_COLOR", "3");
            std::env::set_var("TERM", "xterm-256color");
            std::env::remove_var("NO_COLOR");
            for var in ["TERM_PROGRAM", "KITTY_WINDOW_ID", "WEZTERM_PANE", "GHOSTTY_RESOURCES_DIR", "ITERM_SESSION_ID", "TMUX", "STY"] {
                std::env::remove_var(var);
            }
        }
    });
    guard
}

pub fn load(file: &str) -> Fixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden").join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

pub fn set_caps(overrides: Option<&Value>) {
    let mut caps = TerminalCapabilities { images: None, true_color: true, hyperlinks: false };
    if let Some(o) = overrides {
        if let Some(h) = o.get("hyperlinks").and_then(Value::as_bool) {
            caps.hyperlinks = h;
        }
        if let Some(t) = o.get("trueColor").and_then(Value::as_bool) {
            caps.true_color = t;
        }
        if let Some(i) = o.get("images").and_then(Value::as_str) {
            caps.images = match i {
                "kitty" => Some(ImageProtocol::Kitty),
                "iterm2" => Some(ImageProtocol::Iterm2),
                _ => None,
            };
        }
    }
    set_capabilities(caps);
}

pub fn use_theme(name: &str) {
    set_caps(None);
    init_theme(Some(name), Some(false));
}

/// A TUI on a virtual terminal with a manual event loop, for components that need one.
pub fn test_tui() -> Arc<Tui> {
    let event_loop = EventLoop::new_manual();
    Tui::main_screen_with(
        Arc::new(VirtualTerminal::new(80, 24)),
        TuiOptions { event_loop: Some(event_loop), ..Default::default() },
    )
}

/// What a case builder returns: a component, or a function from width to lines.
pub enum Built {
    Component(Arc<dyn Component>),
    Render(Box<dyn FnMut(usize) -> Vec<String>>),
}

impl<T: Component + 'static> From<Arc<T>> for Built {
    fn from(c: Arc<T>) -> Self {
        Built::Component(c)
    }
}

pub fn component<T: Component + 'static>(c: T) -> Built {
    Built::Component(Arc::new(c))
}

pub type Builder = fn(&Case, &Value) -> Built;

/// Run every case of `file` through `build` and compare with the recorded output. Panics with a
/// report of every mismatching case. Set `GOLDEN_FILTER` to run only cases whose name contains it.
pub fn run(file: &str, build: impl Fn(&str, &Value) -> Option<Built>) {
    let _guard = lock();
    let fixture = load(file);
    let filter = std::env::var("GOLDEN_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    for case in &fixture.cases {
        if filter.as_deref().is_some_and(|f| !case.name.contains(f)) {
            continue;
        }
        for (theme_name, expected_per_width) in &case.out {
            use_theme(theme_name);
            if case.caps.is_some() {
                set_caps(case.caps.as_ref());
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let Some(built) = build(&case.kind, &case.input) else {
                    return Err(format!("unknown kind {}", case.kind));
                };
                let mut render: Box<dyn FnMut(usize) -> Vec<String>> = match built {
                    Built::Component(c) => Box::new(move |w| c.render(w)),
                    Built::Render(f) => f,
                };
                Ok(case.widths.iter().map(|&w| render(w)).collect::<Vec<_>>())
            }));
            ran += 1;
            let actual = match result {
                Ok(Ok(actual)) => actual,
                Ok(Err(e)) => {
                    failures.push(format!("{} [{theme_name}]: {e}", case.name));
                    continue;
                }
                Err(panic) => {
                    let msg = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    failures.push(format!("{} [{theme_name}]: panicked: {msg}", case.name));
                    continue;
                }
            };
            for (i, width) in case.widths.iter().enumerate() {
                let expected = &expected_per_width[i];
                let got = &actual[i];
                if expected != got {
                    failures.push(describe_diff(&case.name, theme_name, *width, expected, got));
                }
            }
        }
    }
    eprintln!("{file}: {ran} case/theme runs, {} mismatches", failures.len());
    assert!(ran > 0 || filter.is_some(), "{file}: no cases");
    if !failures.is_empty() {
        let total = failures.len();
        let shown: Vec<_> = failures.into_iter().take(40).collect();
        panic!("{file}: {total} golden mismatches (of {ran} case/theme runs)\n\n{}", shown.join("\n"));
    }
}

fn describe_diff(name: &str, theme_name: &str, width: usize, expected: &[String], got: &[String]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "--- {name} [{theme_name} w={width}] expected {} lines, got {}", expected.len(), got.len());
    let mut shown = 0;
    for i in 0..expected.len().max(got.len()) {
        let e = expected.get(i);
        let g = got.get(i);
        if e != g {
            let _ = writeln!(out, "  line {i}:\n    want {:?}\n    got  {:?}", e.map(String::as_str).unwrap_or("<none>"), g.map(String::as_str).unwrap_or("<none>"));
            shown += 1;
            if shown >= 3 {
                break;
            }
        }
    }
    out
}

// --- input helpers -------------------------------------------------------------------------------

pub fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn opt_s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub fn b(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub fn opt_i(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64)
}

pub fn from<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> T {
    serde_json::from_value(v.get(key).cloned().unwrap_or(Value::Null))
        .unwrap_or_else(|e| panic!("deserialize {key}: {e}"))
}

pub fn opt_from<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Option<T> {
    v.get(key).filter(|x| !x.is_null()).map(|x| {
        serde_json::from_value(x.clone()).unwrap_or_else(|e| panic!("deserialize {key}: {e}"))
    })
}
