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
