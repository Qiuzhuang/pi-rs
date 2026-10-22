//! Port of `packages/coding-agent/src/utils/deprecation.ts`.

use std::collections::HashSet;
use std::sync::LazyLock;

use parking_lot::Mutex;

static EMITTED_DEPRECATION_WARNINGS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

pub fn warn_deprecation(message: &str) {
    if !EMITTED_DEPRECATION_WARNINGS.lock().insert(message.to_string()) {
        return;
    }
    eprintln!("{}", super::chalk::yellow(&format!("Deprecation warning: {message}")));
}

/// Clear deprecation warning state. Exported for tests.
pub fn clear_deprecation_warnings_for_tests() {
    EMITTED_DEPRECATION_WARNINGS.lock().clear();
}
