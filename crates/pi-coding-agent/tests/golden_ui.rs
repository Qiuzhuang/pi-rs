//! Golden (differential) rendering tests against the TypeScript reference.
//!
//! The fixtures in `tests/fixtures/golden/*.json` are rendered by the real TypeScript components
//! (`scripts/golden/gen-*.mjs`, regenerate with `PI_TS_REF=<pi checkout> scripts/golden/run.sh`) with a
//! fixed theme, terminal capabilities, widths and inputs. Each test rebuilds the same inputs with the
//! Rust components and compares the rendered lines byte for byte. `GOLDEN_FILTER=<substring>` limits
//! a run to matching case names.

#[path = "golden/harness.rs"]
mod harness;
#[path = "golden/messages.rs"]
mod messages;

#[test]
fn golden_messages() {
    harness::run("messages.json", messages::build);
}
#[path = "golden/tools.rs"]
mod tools;

#[test]
fn golden_tools() {
    harness::run("tools.json", tools::build);
}
#[path = "golden/chrome.rs"]
mod chrome;

#[test]
fn golden_chrome() {
    harness::run("chrome.json", chrome::build);
}
#[path = "golden/selectors.rs"]
mod selectors;

#[test]
fn golden_selectors() {
    harness::run("selectors.json", selectors::build);
}
#[path = "golden/sessions.rs"]
mod sessions;

#[test]
fn golden_sessions() {
    harness::run("sessions.json", sessions::build);
}
#[path = "support/mod.rs"]
mod support;
#[path = "golden/screens.rs"]
mod screens;

#[test]
fn golden_screens() {
    harness::run("screens.json", screens::build);
}
#[path = "golden/tui.rs"]
mod tui;

#[test]
fn golden_tui() {
    harness::run("tui.json", tui::build);
}
