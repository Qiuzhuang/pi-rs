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

static COMPAT_STREAM_SIMPLE: std::sync::LazyLock<StreamFn> = std::sync::LazyLock::new(|| {
    std::sync::Arc::new(|model, context, options| {
        let context = pi_ai::Context {
            system_prompt: None,
            messages: context.messages,
            tools: None,
        };
        let stream = pi_ai::compat::stream_simple(&model, &context, options);
        Box::pin(async move { Ok(stream) })
    })
});

/// pi-ai's global `streamSimple` (`@earendil-works/pi-ai/compat`) as a [`StreamFn`]. Always the
/// same handle, so callers can recognize it with [`is_compat_stream_simple_fn`] (TypeScript
/// compares `agent.streamFunction === streamSimple`).
pub fn compat_stream_simple_fn() -> StreamFn {
    COMPAT_STREAM_SIMPLE.clone()
}

/// Whether `stream_fn` is [`compat_stream_simple_fn`].
pub fn is_compat_stream_simple_fn(stream_fn: &StreamFn) -> bool {
    std::ptr::addr_eq(
        std::sync::Arc::as_ptr(stream_fn),
        std::sync::Arc::as_ptr(&*COMPAT_STREAM_SIMPLE),
    )
}

/// A [`StreamFn`] backed by a pi-ai [`pi_ai::Models`] collection (`Models.streamSimple`).
pub fn models_stream_fn(models: pi_ai::Models) -> StreamFn {
    std::sync::Arc::new(move |model, context, options| {
        // The transcript already carries the system prompt and tools in its system messages,
        // so normalizing it again inside `stream_simple` is a no-op.
        let context = pi_ai::Context {
            system_prompt: None,
            messages: context.messages,
            tools: None,
        };
        let stream = models.stream_simple(&model, &context, options);
        Box::pin(async move { Ok(stream) })
    })
}
