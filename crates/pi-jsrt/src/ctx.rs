//! Helpers for code running on the JS thread via [`crate::JsRuntime::with_context`].
//!
//! These let embedders build host objects directly with Boa while still
//! interoperating with handles and JSON conversion.

use boa_engine::{Context, JsResult, JsValue};
use serde_json::Value;

use crate::engine::state;
use crate::error::Result;
use crate::runtime::{JsHandle, JsRuntime};

/// The runtime owning this context (if it is still alive).
pub fn runtime(ctx: &Context) -> Option<JsRuntime> {
    state(ctx).rt.upgrade().map(|inner| JsRuntime { inner })
}

/// Convert a JS value to JSON (functions become handle markers).
pub fn to_json(ctx: &mut Context, value: &JsValue) -> Result<Value> {
    crate::convert::to_json(value, ctx)
}

/// Convert JSON to a JS value (handle / host-function markers are revived).
pub fn from_json(ctx: &mut Context, value: &Value) -> JsResult<JsValue> {
    crate::convert::from_json(value, ctx)
}

/// Keep `value` alive in the handle table and return an owned handle to it.
pub fn make_handle(ctx: &mut Context, value: JsValue) -> Option<JsHandle> {
    let rt = runtime(ctx)?;
    let id = state(ctx).new_handle(value);
    rt.handle(&serde_json::json!({ crate::convert::HANDLE_KEY: id }))
}

/// Look up the JS value behind a handle.
pub fn handle_value(ctx: &Context, handle: &JsHandle) -> Option<JsValue> {
    state(ctx).get_handle(handle.id())
}

/// Number of live entries in the handle table.
pub fn handle_count(ctx: &Context) -> usize {
    state(ctx).handle_count()
}

/// Run all pending promise jobs now.
pub fn run_jobs(ctx: &mut Context) {
    crate::engine::run_microtasks(ctx);
}
