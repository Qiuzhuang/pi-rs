//! Process exit helpers: the Rust counterpart of Node's `process.exitCode` / `process.exit()`.
//!
//! TS commands set `process.exitCode` and return; the binary (`src/main.rs`) reads
//! [`exit_code`] after `main()` returns. [`exit`] mirrors `process.exit(code)`: it flushes
//! stdout/stderr and terminates immediately.

use std::io::Write;
use std::sync::atomic::{AtomicI32, Ordering};

const UNSET: i32 = i32::MIN;
static EXIT_CODE: AtomicI32 = AtomicI32::new(UNSET);

/// `process.exitCode = code`.
pub fn set_exit_code(code: i32) {
    EXIT_CODE.store(code, Ordering::SeqCst);
}

/// `process.exitCode` (`None` when unset).
pub fn exit_code() -> Option<i32> {
    match EXIT_CODE.load(Ordering::SeqCst) {
        UNSET => None,
        code => Some(code),
    }
}

/// `process.exit(code)`: flush the standard streams and exit.
pub fn exit(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}
