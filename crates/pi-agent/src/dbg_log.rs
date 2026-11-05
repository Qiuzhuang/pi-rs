//! Stall tracer. Enabled by `PI_DBG_LOG=<file path>`.
//!
//! Writes one line per call: milliseconds since start + message. Append mode,
//! unbuffered, so a hung process still leaves every line it wrote. Cost is one
//! small write syscall per line, far below the millisecond scale of the races
//! this traces.
//!
//! Test-only abort hook: `PI_DBG_ABORT_AFTER=<pattern>` fires the registered
//! hook the first time a traced line contains the pattern. Used to place an
//! `abort()` call at an exact phase of the settle chain.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static ABORT_PAT: OnceLock<Option<String>> = OnceLock::new();
static ABORT_HOOK: OnceLock<Mutex<Option<Box<dyn Fn() + Send + Sync>>>> = OnceLock::new();

/// Register the abort hook used with `PI_DBG_ABORT_AFTER`. Test only.
pub fn set_abort_hook(h: Option<Box<dyn Fn() + Send + Sync + 'static>>) {
    let slot = ABORT_HOOK.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = h;
}

/// Register the anchor pattern for `PI_DBG_ABORT_AFTER`. Test only.
pub fn set_abort_pattern(p: Option<String>) {
    let _ = ABORT_PAT.set(p);
}

/// Write a trace line, then fire the abort hook if `msg` holds the anchor pattern.
/// The hook fires even when `PI_DBG_LOG` is not set, so a sweep run needs only
/// `PI_DBG_ABORT_AFTER`.
pub fn dbg_log(msg: &str) {
    if let Some(pat) = ABORT_PAT.get().and_then(|p| p.as_ref())
        && msg.contains(pat.as_str())
    {
        if let Some(slot) = ABORT_HOOK.get()
            && let Ok(g) = slot.lock()
            && let Some(h) = g.as_ref()
        {
            h();
        }
    }
    let sink = SINK.get_or_init(|| {
        std::env::var("PI_DBG_LOG").ok().and_then(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()
                .map(|f| Mutex::new(f))
        })
    });
    START.get_or_init(Instant::now);
    if let Some(sink) = sink {
        let mut sink = sink.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(sink, "{} {}", wall_ms(), msg);
    }
}

fn wall_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
