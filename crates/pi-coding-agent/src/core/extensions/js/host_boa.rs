//! In-process backend: the existing `pi-jsrt` runtime (Boa engine).

use std::sync::Arc;

use pi_jsrt::{JsHandle as BoaJsHandle, JsRuntime};
use serde_json::Value;

use super::host::{BoxFuture, HandleOps, JsHandle, JsHost};

/// A [`JsRuntime`] behind the [`JsHost`] interface.
pub struct BoaHost {
    rt: JsRuntime,
}

struct BoaHandle {
    h: BoaJsHandle,
}

impl HandleOps for BoaHandle {
    fn id(&self) -> u64 {
        self.h.id()
    }

    fn is_js_thread(&self) -> bool {
        self.h.runtime().is_js_thread()
    }

    fn to_value(&self) -> Value {
        self.h.to_value()
    }

    fn call(&self, args: Vec<Value>) -> BoxFuture<'static, anyhow::Result<Value>> {
        let h = self.h.clone();
        Box::pin(async move { h.call(args).await.map_err(super::js_error) })
    }

    fn call_method(&self, method: String, args: Vec<Value>) -> BoxFuture<'static, anyhow::Result<Value>> {
        let h = self.h.clone();
        Box::pin(async move { h.call_method(&method, args).await.map_err(super::js_error) })
    }

    fn call_blocking(&self, args: Vec<Value>) -> anyhow::Result<Value> {
        self.h.call_blocking(args).map_err(super::js_error)
    }

    fn call_method_blocking(&self, method: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        self.h.call_method_blocking(method, args).map_err(super::js_error)
    }

    fn get_blocking(&self, name: &str) -> anyhow::Result<Value> {
        self.h.get_blocking(name).map_err(super::js_error)
    }
}

impl JsHost for BoaHost {
    fn import(&self, specifier: &str) -> BoxFuture<'static, anyhow::Result<JsHandle>> {
        let rt = self.rt.clone();
        let spec = specifier.to_string();
        Box::pin(async move {
            rt.import(&spec)
                .await
                .map(|h| JsHandle::new(Arc::new(BoaHandle { h })))
                .map_err(super::js_error)
        })
    }

    fn clear_module_cache(&self) -> BoxFuture<'static, anyhow::Result<()>> {
        let rt = self.rt.clone();
        Box::pin(async move { rt.clear_module_cache().await.map_err(super::js_error) })
    }

    fn adopt(&self, marker: &Value) -> Option<JsHandle> {
        self.rt.handle(marker).map(|h| JsHandle::new(Arc::new(BoaHandle { h })))
    }

    fn wait_servicing<R: Send + 'static>(&self, start: impl FnOnce(Box<dyn FnOnce(R) + Send>) + Send) -> Option<R> {
        self.rt.wait_servicing(start)
    }

    fn reentrant_js(&self) -> bool {
        true
    }

}

impl BoaHost {
    pub fn new(rt: JsRuntime) -> Arc<Self> {
        Arc::new(Self { rt })
    }

    /// Wrap an already-imported Boa handle in the bridge's [`JsHandle`].
    pub fn wrap(h: BoaJsHandle) -> JsHandle {
        JsHandle::new(Arc::new(BoaHandle { h }))
    }
}
