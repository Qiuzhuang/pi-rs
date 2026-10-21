use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use futures::future::BoxFuture;
use parking_lot::Mutex;

use crate::abort::AbortSignal;
use crate::error::{Error, Result};
use crate::protocol::jsonrpc::JsonRpcMessage;

pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

pub type McpTransportMessageListener = dyn Fn(JsonRpcMessage) + Send + Sync;
pub type McpTransportErrorListener = dyn Fn(Error) + Send + Sync;
pub type McpTransportCloseListener = dyn Fn() + Send + Sync;

/// Removes a listener or handler when called. Dropping it keeps the registration.
#[must_use = "dropping a Disposer keeps the registration; call dispose() to remove it"]
pub struct Disposer(Option<Box<dyn FnOnce() + Send + Sync>>);

impl Disposer {
    pub fn new(dispose: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self(Some(Box::new(dispose)))
    }

    pub fn noop() -> Self {
        Self(None)
    }

    pub fn dispose(mut self) {
        if let Some(dispose) = self.0.take() {
            dispose();
        }
    }
}

impl std::fmt::Debug for Disposer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Disposer")
    }
}

static NEXT_LISTENER_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_listener_id() -> u64 {
    NEXT_LISTENER_ID.fetch_add(1, Ordering::Relaxed)
}

type ListenerEntries<F> = Vec<(u64, Arc<F>)>;

/// An ordered listener set.
pub(crate) struct Listeners<F: ?Sized> {
    entries: Arc<Mutex<ListenerEntries<F>>>,
}

impl<F: ?Sized> Default for Listeners<F> {
    fn default() -> Self {
        Self {
            entries: Arc::default(),
        }
    }
}

impl<F: ?Sized + Send + Sync + 'static> Listeners<F> {
    pub(crate) fn add(&self, listener: Arc<F>) -> Disposer {
        let id = next_listener_id();
        self.entries.lock().push((id, listener));
        let entries = Arc::downgrade(&self.entries);
        Disposer::new(move || {
            if let Some(entries) = entries.upgrade() {
                entries.lock().retain(|(entry, _)| *entry != id);
            }
        })
    }

    pub(crate) fn snapshot(&self) -> Vec<Arc<F>> {
        self.entries
            .lock()
            .iter()
            .map(|(_, listener)| listener.clone())
            .collect()
    }
}

/// Listener bookkeeping shared by transports. `emit_close` fires at most once per transport.
#[derive(Default)]
pub struct TransportEvents {
    message: Listeners<McpTransportMessageListener>,
    error: Listeners<McpTransportErrorListener>,
    close: Listeners<McpTransportCloseListener>,
    close_emitted: AtomicBool,
}

impl TransportEvents {
    pub fn on_message(
        &self,
        listener: impl Fn(JsonRpcMessage) + Send + Sync + 'static,
    ) -> Disposer {
        self.message.add(Arc::new(listener))
    }

    pub fn on_error(&self, listener: impl Fn(Error) + Send + Sync + 'static) -> Disposer {
        self.error.add(Arc::new(listener))
    }

    pub fn on_close(&self, listener: impl Fn() + Send + Sync + 'static) -> Disposer {
        self.close.add(Arc::new(listener))
    }

    pub fn emit_message(&self, message: JsonRpcMessage) {
        for listener in self.message.snapshot() {
            listener(message.clone());
        }
    }

    pub fn emit_error(&self, error: Error) {
        for listener in self.error.snapshot() {
            listener(error.clone());
        }
    }

    pub fn emit_close(&self) {
        if self.close_emitted.swap(true, Ordering::SeqCst) {
            return;
        }
        for listener in self.close.snapshot() {
            listener();
        }
    }

    pub fn close_emitted(&self) -> bool {
        self.close_emitted.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, Default)]
pub struct McpTransportSendOptions {
    /// Aborted once the sender no longer waits for a reply (the request was cancelled, timed out,
    /// answered, or the client closed). A transport must not dispatch the message after that, for
    /// example when retrying it after an authorization refresh. It does not undo a message already
    /// dispatched.
    pub signal: Option<AbortSignal>,
}

/// A bidirectional JSON-RPC message channel to an MCP server.
///
/// Methods return `'static` futures, and the synchronous part of a call (before the returned
/// future is first polled) is where implementations order their work, like the synchronous part
/// of a JavaScript async function: `send` must enqueue messages in call order.
pub trait McpTransport: Send + Sync + 'static {
    fn start(&self) -> BoxFuture<'static, Result<()>>;
    fn send(
        &self,
        message: JsonRpcMessage,
        options: McpTransportSendOptions,
    ) -> BoxFuture<'static, Result<()>>;
    fn close(&self) -> BoxFuture<'static, Result<()>>;
    /// Message, error, and close listeners.
    fn events(&self) -> &TransportEvents;
    fn set_protocol_version(&self, _version: &str) {}
}
