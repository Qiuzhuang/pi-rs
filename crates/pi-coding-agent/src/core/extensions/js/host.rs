//! Backend-agnostic interface to the JavaScript runtime that hosts pi extensions.
//!
//! Two backends implement it:
//!
//! - [`super::host_boa::BoaHost`] runs JS in-process on `pi-jsrt` (the Boa engine).
//! - [`super::host_bun::BunHost`] runs JS in a `bun` subprocess; Rust talks to it over a
//!   line-delimited JSON protocol on stdio. Synchronous host calls block the subprocess's
//!   event loop, so on that backend synchronous host-call handlers must never re-enter JS
//!   ([`JsHost::reentrant_js`] is false).
//!
//! Values cross the boundary as JSON. JS functions (and the objects that wrap them) become
//! `{"$jsHandle": id}` markers; [`JsHost::adopt`] turns a marker back into a [`JsHandle`]
//! that Rust can invoke. Calls that return a promise are awaited before the result is
//! returned, on both backends.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Opaque reference to a JS value that lives inside the backend runtime.
#[derive(Clone)]
pub struct JsHandle {
    ops: Arc<dyn HandleOps>,
}

/// Backend-specific operations on one JS value.
pub trait HandleOps: Send + Sync {
    /// Handle id (stable for the value's lifetime).
    fn id(&self) -> u64;
    /// The `{"$jsHandle": id}` marker for this value.
    fn to_value(&self) -> Value;
    /// True when the current thread runs on top of the JS runtime (Boa's JS thread).
    fn is_js_thread(&self) -> bool;
    /// Call the function with JSON arguments; awaits a returned promise; returns JSON.
    fn call(&self, args: Vec<Value>) -> BoxFuture<'static, anyhow::Result<Value>>;
    /// Call `this[name](...args)`.
    fn call_method(&self, method: String, args: Vec<Value>) -> BoxFuture<'static, anyhow::Result<Value>>;
    /// Blocking [`Self::call`] for non-async threads (e.g. a render thread).
    fn call_blocking(&self, args: Vec<Value>) -> anyhow::Result<Value>;
    /// Blocking [`Self::call_method`].
    fn call_method_blocking(&self, method: &str, args: Vec<Value>) -> anyhow::Result<Value>;
    /// Read a property of the value.
    fn get_blocking(&self, name: &str) -> anyhow::Result<Value>;
}

impl JsHandle {
    pub fn new(ops: Arc<dyn HandleOps>) -> Self {
        Self { ops }
    }

    pub fn id(&self) -> u64 {
        self.ops.id()
    }

    /// The `{"$jsHandle": id}` marker that becomes this JS value again when passed back.
    pub fn to_value(&self) -> Value {
        self.ops.to_value()
    }

    /// True when the current thread runs on top of the JS runtime.
    pub fn is_js_thread(&self) -> bool {
        self.ops.is_js_thread()
    }

    pub async fn call(&self, args: Vec<Value>) -> anyhow::Result<Value> {
        self.ops.call(args).await
    }

    pub async fn call_method(&self, method: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        self.ops.call_method(method.to_string(), args).await
    }

    pub fn call_blocking(&self, args: Vec<Value>) -> anyhow::Result<Value> {
        self.ops.call_blocking(args)
    }

    pub fn call_method_blocking(&self, method: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        self.ops.call_method_blocking(method, args)
    }

    /// Read a property of the value.
    pub fn get_blocking(&self, name: &str) -> anyhow::Result<Value> {
        self.ops.get_blocking(name)
    }
}

/// A JavaScript runtime hosting pi extensions.
pub trait JsHost: Send + Sync {
    /// Import a module (bare specifier or absolute file path) and return its namespace.
    fn import(&self, specifier: &str) -> BoxFuture<'static, anyhow::Result<JsHandle>>;
    /// Forget loaded modules so the next [`Self::import`] re-evaluates them (jiti `moduleCache: false`).
    fn clear_module_cache(&self) -> BoxFuture<'static, anyhow::Result<()>>;
    /// Adopt a `{"$jsHandle": id}` marker from a value that came from JS.
    fn adopt(&self, marker: &Value) -> Option<JsHandle>;
    /// Post `start` (which queues a UI job and hands back the completion callback) and wait for
    /// its result. While waiting, the backend keeps the JS runtime free to service blocking
    /// calls into JS made by the UI job. The default implementation just blocks the current
    /// thread, which is enough for backends whose JS event loop services calls on its own.
    fn wait_servicing<R: Send + 'static>(&self, start: impl FnOnce(Box<dyn FnOnce(R) + Send>) + Send) -> Option<R> {
        let (tx, rx) = std::sync::mpsc::sync_channel::<R>(1);
        start(Box::new(move |r| {
            let _ = tx.send(r);
        }));
        rx.recv().ok()
    }
    /// Whether a synchronous host-call handler may re-enter JS from the thread that runs it
    /// (in-process Boa: yes, the handler runs on the JS thread; subprocess backends: no, their
    /// event loop is blocked for the duration of the synchronous host call).
    fn reentrant_js(&self) -> bool;
}

/// The concrete backend pair. The bridge holds this rather than `dyn JsHost` because the
/// generic `wait_servicing` makes the trait non-dyn-compatible.
#[derive(Clone)]
pub enum AnyHost {
    Boa(Arc<super::host_boa::BoaHost>),
    Bun(Arc<super::host_bun::BunHost>),
}

impl AnyHost {
    pub async fn import(&self, specifier: &str) -> anyhow::Result<JsHandle> {
        match self {
            Self::Boa(h) => h.import(specifier).await,
            Self::Bun(h) => h.import(specifier).await,
        }
    }

    pub async fn clear_module_cache(&self) -> anyhow::Result<()> {
        match self {
            Self::Boa(h) => h.clear_module_cache().await,
            Self::Bun(h) => h.clear_module_cache().await,
        }
    }

    pub fn adopt(&self, marker: &Value) -> Option<JsHandle> {
        match self {
            Self::Boa(h) => h.adopt(marker),
            Self::Bun(h) => h.adopt(marker),
        }
    }

    pub fn wait_servicing<R: Send + 'static>(&self, start: impl FnOnce(Box<dyn FnOnce(R) + Send>) + Send) -> Option<R> {
        match self {
            Self::Boa(h) => h.wait_servicing(start),
            Self::Bun(h) => h.wait_servicing(start),
        }
    }

    pub fn reentrant_js(&self) -> bool {
        match self {
            Self::Boa(h) => h.reentrant_js(),
            Self::Bun(h) => h.reentrant_js(),
        }
    }

}
