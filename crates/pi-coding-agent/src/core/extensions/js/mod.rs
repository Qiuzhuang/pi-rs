//! JavaScript extension bridge: loads `.ts` / `.js` extensions in the embedded `pi-jsrt` runtime.
//!
//! One [`JsExtensionHost`] (one JS runtime) serves all JavaScript extensions loaded for the same
//! working directory and extension cache generation; a reload (`clearExtensionCache()`) starts a
//! new one. Extensions import pi's packages as virtual modules implemented in `js/*.js` (in the
//! crate root) on top of host functions:
//!
//! - `__piHost.call(op, payload)`: synchronous, runs on the JS thread;
//! - `__piHost.callAsync(op, payload)`: returns a promise, runs on tokio.
//!
//! JS objects reach Rust as JSON (functions become handle markers, see `pi_jsrt::convert`); the
//! bridge turns them into the Rust extension API types: handlers become [`ExtensionHandler`]s
//! returning [`ExtensionEventResult::Json`], tools become [`ToolDefinition`]s whose closures call
//! into JS, components become [`JsComponent`]s rendering through blocking calls.

mod api;
mod builtin;
mod component;
mod context;
mod provider;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Weak};

use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_jsrt::{JsHandle, JsRuntime, RuntimeOptions, VirtualModule};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::core::extensions::runner::ExtensionErrorWithStack;
use crate::core::extensions::types::*;

pub use component::{JsComponent, JsEditorComponent};

const BRIDGE_JS: &str = include_str!("../../../../js/bridge.js");
const TYPEBOX_JS: &str = include_str!("../../../../js/typebox.js");
const PI_TUI_JS: &str = include_str!("../../../../js/pi-tui.js");
const PI_AI_JS: &str = include_str!("../../../../js/pi-ai.js");
const PI_AGENT_CORE_JS: &str = include_str!("../../../../js/pi-agent-core.js");
const PI_CODING_AGENT_JS: &str = include_str!("../../../../js/pi-coding-agent.js");

/// Specifier of the internal bridge module.
const BRIDGE_SPECIFIER: &str = "pi-rs:bridge";

/// Convert a JS error into an error that keeps the JS stack (reported by the runner).
pub(crate) fn js_error(err: pi_jsrt::Error) -> anyhow::Error {
    match err {
        pi_jsrt::Error::Js(e) => {
            anyhow::Error::new(ExtensionErrorWithStack { message: e.message.clone(), stack: e.stack.clone() })
        }
        other => anyhow::Error::new(ExtensionErrorWithStack { message: other.to_string(), stack: None }),
    }
}

/// A Rust callback callable from JS (`rustCallback(id)` in bridge.js).
pub(crate) type RustCallback = Arc<dyn Fn(Vec<Value>) -> anyhow::Result<Value> + Send + Sync>;

/// An async Rust callback callable from JS (`rustAsyncCallback(id)` in bridge.js).
pub(crate) type AsyncRustCallback = Arc<dyn Fn(Vec<Value>) -> BoxFuture<'static, anyhow::Result<Value>> + Send + Sync>;

/// A context registered for JS (`makeContext(id, kind)` in bridge.js).
#[derive(Clone)]
pub(crate) enum CtxEntry {
    Base(ExtensionContext),
    Tool(ExtensionToolContext),
    Command(ExtensionCommandContext),
    Replaced(ReplacedSessionContext),
    Trust(ProjectTrustContext),
}

impl CtxEntry {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Base(_) => "base",
            Self::Tool(_) => "tool",
            Self::Command(_) => "command",
            Self::Replaced(_) => "replaced",
            Self::Trust(_) => "trust",
        }
    }

    pub(crate) fn base(&self) -> Option<&ExtensionContext> {
        match self {
            Self::Base(c) => Some(c),
            Self::Tool(c) => Some(&c.ctx),
            Self::Command(c) => Some(&c.ctx),
            Self::Replaced(c) => Some(&c.cmd.ctx),
            Self::Trust(_) => None,
        }
    }
}

/// Id allocator plus map, the shape of every bridge registry.
pub(crate) struct Registry<T> {
    items: Mutex<HashMap<u64, T>>,
}

impl<T: Clone> Registry<T> {
    fn new() -> Self {
        Self { items: Mutex::new(HashMap::new()) }
    }

    pub(crate) fn insert(&self, value: T) -> u64 {
        let id = next_id();
        self.items.lock().insert(id, value);
        id
    }

    pub(crate) fn insert_with_id(&self, id: u64, value: T) {
        self.items.lock().insert(id, value);
    }

    pub(crate) fn get(&self, id: u64) -> Option<T> {
        self.items.lock().get(&id).cloned()
    }

    pub(crate) fn remove(&self, id: u64) -> Option<T> {
        self.items.lock().remove(&id)
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Shared state of one JS extension host. Host functions hold it weakly so the JS runtime can
/// stop once every extension using it is gone.
pub(crate) struct Bridge {
    pub(crate) rt: JsRuntime,
    /// The `pi-rs:bridge` module namespace.
    pub(crate) glue: JsHandle,
    pub(crate) cwd: String,
    pub(crate) apis: Registry<ExtensionAPIRef>,
    pub(crate) unsubscribes: Registry<Unsubscribe>,
    pub(crate) contexts: Registry<CtxEntry>,
    /// Context ids by host pointer, so repeated events share one JS ctx object.
    pub(crate) context_ids: Mutex<HashMap<usize, u64>>,
    pub(crate) tokens: Registry<CancellationToken>,
    pub(crate) callbacks: Registry<RustCallback>,
    pub(crate) async_callbacks: Registry<AsyncRustCallback>,
    pub(crate) callback_ids: Mutex<HashMap<usize, u64>>,
    /// Session managers handed to `newSession({ setup })` callbacks.
    pub(crate) session_managers: Registry<Arc<crate::core::session_manager::SessionManager>>,
    pub(crate) streams: Registry<Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Value>>>>,
    pub(crate) components: component::ComponentRegistries,
    pub(crate) builtins: Registry<builtin::BuiltinTool>,
    /// Ordered executor for UI calls made from JS (see [`Bridge::ui_dispatch`]).
    ui_tx: Mutex<std::sync::mpsc::Sender<Box<dyn FnOnce() + Send>>>,
}

impl Bridge {
    /// Register a context for JS and return its id. Contexts sharing a host share an id.
    pub(crate) fn register_ctx(&self, entry: CtxEntry) -> u64 {
        if let CtxEntry::Base(ctx) = &entry {
            let key = Arc::as_ptr(ctx.host()) as *const () as usize;
            if let Some(id) = self.context_ids.lock().get(&key) {
                return *id;
            }
            let id = self.contexts.insert(entry);
            self.context_ids.lock().insert(key, id);
            return id;
        }
        self.contexts.insert(entry)
    }

    pub(crate) fn release_ctx(&self, id: u64) {
        if let Some(CtxEntry::Base(ctx)) = self.contexts.remove(id) {
            let key = Arc::as_ptr(ctx.host()) as *const () as usize;
            self.context_ids.lock().remove(&key);
        }
        let glue = self.glue.clone();
        tokio_spawn(async move {
            let _ = glue.call_method("releaseContext", vec![json!(id)]).await;
        });
    }

    /// Register a Rust callback for JS and return its id.
    pub(crate) fn register_callback(&self, callback: RustCallback) -> u64 {
        self.callbacks.insert(callback)
    }

    /// Register a callback keyed by the identity of `key` (stable ids for recurring closures).
    pub(crate) fn register_callback_keyed(&self, key: usize, make: impl FnOnce() -> RustCallback) -> u64 {
        let mut ids = self.callback_ids.lock();
        if let Some(id) = ids.get(&key) {
            return *id;
        }
        let id = self.callbacks.insert(make());
        ids.insert(key, id);
        id
    }

    /// Mirror a Rust cancellation token as a JS AbortSignal for the lifetime of the guard.
    pub(crate) fn signal_for(self: &Arc<Self>, token: &CancellationToken) -> (Value, SignalGuard) {
        let id = next_id();
        let done = CancellationToken::new();
        let value = json!({ "id": id, "aborted": token.is_cancelled() });
        if !token.is_cancelled() {
            let token = token.clone();
            let done2 = done.clone();
            let glue = self.glue.clone();
            tokio_spawn(async move {
                tokio::select! {
                    _ = token.cancelled() => {
                        let _ = glue.call_method("abortSignal", vec![json!(id)]).await;
                    }
                    _ = done2.cancelled() => {
                        let _ = glue.call_method("releaseSignal", vec![json!(id)]).await;
                    }
                }
            });
        }
        (value, SignalGuard(done))
    }

    /// A signal that follows `token` until the token is cancelled or the bridge is dropped.
    pub(crate) fn detached_signal_for(self: &Arc<Self>, token: &CancellationToken) -> Value {
        let (value, guard) = self.signal_for(token);
        std::mem::forget(guard);
        value
    }

    /// Run a UI call made from JS on the ordered UI executor. JS never blocks on the UI (or on
    /// locks held by a thread that waits for JS to render).
    pub(crate) fn ui_dispatch(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.ui_tx.lock().send(Box::new(f));
    }

    /// Run a UI call on the ordered executor and wait for its result (getters).
    pub(crate) fn ui_dispatch_wait<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> anyhow::Result<R> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.ui_dispatch(move || {
            let _ = tx.send(f());
        });
        rx.recv().map_err(|_| anyhow::anyhow!("UI executor stopped"))
    }

    /// Resolve a token id sent by JS (`tokenFor(signal)`).
    pub(crate) fn token(&self, value: &Value) -> Option<CancellationToken> {
        value.as_u64().and_then(|id| self.tokens.get(id))
    }

    /// Adopt a JS function or object handle marker.
    pub(crate) fn handle(&self, value: &Value) -> Option<JsHandle> {
        self.rt.handle(value)
    }

    /// Call an exported function of the bridge module.
    pub(crate) async fn glue_call(&self, name: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        self.glue.call_method(name, args).await.map_err(js_error)
    }

    /// Blocking variant of [`Self::glue_call`] for synchronous Rust callers (renderers).
    pub(crate) fn glue_call_blocking(&self, name: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        self.glue.call_method_blocking(name, args).map_err(js_error)
    }
}

/// Stops mirroring a token into JS when dropped.
pub(crate) struct SignalGuard(CancellationToken);

impl Drop for SignalGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(crate) fn tokio_spawn(fut: impl std::future::Future<Output = ()> + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(fut);
        }
        Err(_) => {
            std::thread::spawn(move || futures::executor::block_on(fut));
        }
    }
}

/// Tokio runtime for the async host functions of extension runtimes. A shared host outlives the
/// tokio runtime that happened to create it (a CLI `block_on`, a test), so it gets its own.
fn extension_io_runtime() -> anyhow::Result<tokio::runtime::Handle> {
    static RUNTIME: std::sync::OnceLock<std::io::Result<tokio::runtime::Runtime>> = std::sync::OnceLock::new();
    let rt = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("pi-ext-io").enable_all().build()
    });
    match rt {
        Ok(rt) => Ok(rt.handle().clone()),
        Err(e) => Err(anyhow::anyhow!("failed to start the extension I/O runtime: {e}")),
    }
}

/// Loads JavaScript / TypeScript extensions in the embedded runtime.
pub struct JsExtensionHost {
    bridge: Arc<Bridge>,
}

struct SharedHost {
    cwd: String,
    generation: u64,
    host: Arc<JsExtensionHost>,
}

static SHARED: LazyLock<Mutex<Option<SharedHost>>> = LazyLock::new(|| Mutex::new(None));
static GENERATION: AtomicU64 = AtomicU64::new(0);

impl JsExtensionHost {
    /// The host shared by extension loads for `cwd` in the current cache generation.
    pub async fn shared(cwd: &str) -> anyhow::Result<Arc<JsExtensionHost>> {
        let generation = GENERATION.load(Ordering::SeqCst);
        {
            let shared = SHARED.lock();
            if let Some(s) = &*shared
                && s.cwd == cwd
                && s.generation == generation
            {
                return Ok(s.host.clone());
            }
        }
        let host = Arc::new(JsExtensionHost::new(cwd).await?);
        let mut shared = SHARED.lock();
        if let Some(s) = &*shared
            && s.cwd == cwd
            && s.generation == generation
        {
            return Ok(s.host.clone());
        }
        if GENERATION.load(Ordering::SeqCst) == generation {
            *shared = Some(SharedHost { cwd: cwd.to_string(), generation, host: host.clone() });
        }
        Ok(host)
    }

    /// Forget the shared host: the next load starts a new JS runtime. Extensions loaded from the
    /// previous runtime keep it alive until they are dropped.
    pub fn reset_shared() {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        *SHARED.lock() = None;
    }

    /// Start a new JS runtime with pi's virtual modules.
    pub async fn new(cwd: &str) -> anyhow::Result<JsExtensionHost> {
        let mut virtual_modules = HashMap::new();
        virtual_modules.insert(BRIDGE_SPECIFIER.to_string(), VirtualModule::Esm(BRIDGE_JS.to_string()));
        virtual_modules.insert("typebox".to_string(), VirtualModule::Esm(TYPEBOX_JS.to_string()));
        virtual_modules.insert("@earendil-works/pi-tui".to_string(), VirtualModule::Esm(PI_TUI_JS.to_string()));
        virtual_modules.insert("@earendil-works/pi-ai".to_string(), VirtualModule::Esm(PI_AI_JS.to_string()));
        virtual_modules
            .insert("@earendil-works/pi-agent-core".to_string(), VirtualModule::Esm(PI_AGENT_CORE_JS.to_string()));
        virtual_modules
            .insert("@earendil-works/pi-coding-agent".to_string(), VirtualModule::Esm(PI_CODING_AGENT_JS.to_string()));
        let mut aliases = HashMap::new();
        for (from, to) in [
            ("@sinclair/typebox", "typebox"),
            ("@sinclair/typebox/value", "typebox"),
            ("@sinclair/typebox/compiler", "typebox"),
            ("typebox/value", "typebox"),
            ("typebox/compile", "typebox"),
            ("typebox/type", "typebox"),
            ("@earendil-works/pi-ai/compat", "@earendil-works/pi-ai"),
            ("@earendil-works/pi-ai/models", "@earendil-works/pi-ai"),
            ("@earendil-works/pi-ai/oauth", "@earendil-works/pi-ai"),
            ("@earendil-works/pi-ai/providers/all", "@earendil-works/pi-ai"),
            ("@mariozechner/pi-coding-agent", "@earendil-works/pi-coding-agent"),
            ("@mariozechner/pi-agent-core", "@earendil-works/pi-agent-core"),
            ("@mariozechner/pi-tui", "@earendil-works/pi-tui"),
            ("@mariozechner/pi-ai", "@earendil-works/pi-ai"),
            ("@mariozechner/pi-ai/compat", "@earendil-works/pi-ai"),
            ("@mariozechner/pi-ai/models", "@earendil-works/pi-ai"),
            ("@mariozechner/pi-ai/oauth", "@earendil-works/pi-ai"),
            ("@mariozechner/pi-ai/providers/all", "@earendil-works/pi-ai"),
        ] {
            aliases.insert(from.to_string(), to.to_string());
        }
        let options = RuntimeOptions {
            cwd: Some(std::path::PathBuf::from(cwd)),
            virtual_modules,
            aliases,
            thread_name: "pi-extensions".to_string(),
            tokio: Some(extension_io_runtime()?),
            ..Default::default()
        };
        let rt = JsRuntime::new(options).map_err(js_error)?;

        let (ui_tx, ui_rx) = std::sync::mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let _ = std::thread::Builder::new().name("pi-ext-ui".to_string()).spawn(move || {
            while let Ok(f) = ui_rx.recv() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            }
        });

        let bridge_slot: Arc<Mutex<Weak<Bridge>>> = Arc::new(Mutex::new(Weak::new()));
        let rt2 = rt.clone();
        let slot2 = bridge_slot.clone();
        let init = async move {
            let slot = slot2.clone();
            rt2.register_function("__piHost.call", move |rt, args| {
                let bridge = slot.lock().upgrade().ok_or_else(|| pi_jsrt::Error::host("pi extension host is shut down"))?;
                let op = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
                let payload = args.get(1).cloned().unwrap_or(Value::Null);
                api::dispatch(&bridge, rt, &op, payload).map_err(|e| pi_jsrt::Error::host(format!("{e:#}")))
            })
            .await?;
            let slot = slot2.clone();
            rt2.register_async_function("__piHost.callAsync", move |rt, args| {
                let bridge = slot.lock().upgrade();
                async move {
                    let bridge = bridge.ok_or_else(|| pi_jsrt::Error::host("pi extension host is shut down"))?;
                    let op = args.first().and_then(Value::as_str).unwrap_or_default().to_string();
                    let payload = args.get(1).cloned().unwrap_or(Value::Null);
                    api::dispatch_async(bridge, rt, op, payload).await.map_err(|e| pi_jsrt::Error::host(format!("{e:#}")))
                }
            })
            .await?;
            let glue = rt2.import(BRIDGE_SPECIFIER).await?;
            Ok::<JsHandle, pi_jsrt::Error>(glue)
        };
        let glue = init.await.map_err(js_error)?;

        let bridge = Arc::new(Bridge {
            rt: rt.clone(),
            glue,
            cwd: cwd.to_string(),
            apis: Registry::new(),
            unsubscribes: Registry::new(),
            contexts: Registry::new(),
            context_ids: Mutex::new(HashMap::new()),
            tokens: Registry::new(),
            callbacks: Registry::new(),
            async_callbacks: Registry::new(),
            callback_ids: Mutex::new(HashMap::new()),
            session_managers: Registry::new(),
            streams: Registry::new(),
            components: component::ComponentRegistries::default(),
            builtins: Registry::new(),
            ui_tx: Mutex::new(ui_tx),
        });
        *bridge_slot.lock() = Arc::downgrade(&bridge);
        // pi-ai defines the stream class used by host-backed streams.
        rt.import("@earendil-works/pi-ai").await.map_err(js_error)?;
        Ok(JsExtensionHost { bridge })
    }

    /// The JS runtime of this host.
    pub fn runtime(&self) -> &JsRuntime {
        &self.bridge.rt
    }

    /// Import an extension module and return its factory, or `None` when the module does not
    /// export a function (default export or the module itself, like jiti `{ default: true }`).
    pub async fn import_factory(self: &Arc<Self>, path: &str) -> anyhow::Result<Option<ExtensionFactory>> {
        let rt = self.bridge.rt.clone();
        // jiti `moduleCache: false`: every import of an extension evaluates it again.
        rt.clear_module_cache().await.map_err(js_error)?;
        // Imported as a top-level module rather than through `import()` from the glue module:
        // Boa caches dynamic imports per referrer and would reject the re-evaluated module.
        let namespace = rt.import(path).await.map_err(js_error)?;
        let marker = self.bridge.glue_call("factoryFromModule", vec![namespace.to_value()]).await?;
        let Some(factory) = self.bridge.handle(&marker) else {
            return Ok(None);
        };
        let bridge = self.bridge.clone();
        let host = self.clone();
        Ok(Some(Arc::new(move |api: ExtensionAPIRef| -> BoxFuture<'static, anyhow::Result<()>> {
            let bridge = bridge.clone();
            let factory = factory.clone();
            let _host = host.clone();
            Box::pin(async move {
                let api_id = bridge.apis.insert(api);
                bridge.glue_call("runFactory", vec![factory.to_value(), json!(api_id)]).await?;
                Ok(())
            })
        })))
    }
}

