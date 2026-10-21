//! Default stream function (port of `stream-fn.ts`).

use parking_lot::RwLock;

use crate::error::AgentError;
use crate::types::StreamFn;

static DEFAULT_STREAM_FN: RwLock<Option<StreamFn>> = RwLock::new(None);

/// Configure the fallback used by `Agent` and low-level loops when callers omit `stream_fn`.
///
/// Hosts that provide a default model runtime can install its stream function here without
/// making the agent core depend on a provider catalog. `None` clears it.
pub fn set_default_stream_fn(stream_fn: Option<StreamFn>) {
    *DEFAULT_STREAM_FN.write() = stream_fn;
}

/// The configured default stream function.
pub fn get_default_stream_fn() -> Result<StreamFn, AgentError> {
    DEFAULT_STREAM_FN.read().clone().ok_or_else(|| {
        AgentError::new(
            "No default stream function configured. Pass stream_fn explicitly or call set_default_stream_fn().",
        )
    })
}
