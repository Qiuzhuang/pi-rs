use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use futures::future::BoxFuture;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::transport::{McpTransport, McpTransportSendOptions, TransportEvents};
use crate::error::{Error, Result};
use crate::protocol::jsonrpc::JsonRpcMessage;

struct Inner {
    events: TransportEvents,
    peer: Mutex<Option<Weak<Inner>>>,
    started: AtomicBool,
    closed: AtomicBool,
    /// Messages for this transport, delivered in order by a task started in `start`.
    inbox: Mutex<Option<mpsc::UnboundedSender<JsonRpcMessage>>>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<JsonRpcMessage>>>,
}

/// One end of an in-process transport pair, for tests. Messages are delivered asynchronously and
/// in order, like `queueMicrotask` in TypeScript.
#[derive(Clone)]
pub struct InMemoryTransport {
    inner: Arc<Inner>,
}

impl Default for InMemoryTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryTransport {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            inner: Arc::new(Inner {
                events: TransportEvents::default(),
                peer: Mutex::new(None),
                started: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                inbox: Mutex::new(Some(sender)),
                receiver: Mutex::new(Some(receiver)),
            }),
        }
    }

    pub fn connect_peer(&self, peer: &InMemoryTransport) -> Result<()> {
        let mut current = self.inner.peer.lock();
        if current.is_some() {
            return Err(Error::other("In-memory MCP transport already has a peer"));
        }
        *current = Some(Arc::downgrade(&peer.inner));
        Ok(())
    }

    /// Exposed so tests can simulate transport-level failures.
    pub fn emit_error(&self, error: Error) {
        self.inner.events.emit_error(error);
    }

    pub fn on_message(
        &self,
        listener: impl Fn(JsonRpcMessage) + Send + Sync + 'static,
    ) -> super::Disposer {
        self.inner.events.on_message(listener)
    }
}

impl Inner {
    fn close(self: &Arc<Self>) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.inbox.lock().take();
        self.events.emit_close();
        let peer = self.peer.lock().as_ref().and_then(Weak::upgrade);
        if let Some(peer) = peer {
            peer.close();
        }
    }
}

impl McpTransport for InMemoryTransport {
    fn start(&self) -> BoxFuture<'static, Result<()>> {
        let result = if self.inner.closed.load(Ordering::SeqCst) {
            Err(Error::connection_closed())
        } else {
            if let Some(mut receiver) = self.inner.receiver.lock().take() {
                let inner = Arc::downgrade(&self.inner);
                tokio::spawn(async move {
                    while let Some(message) = receiver.recv().await {
                        let Some(inner) = inner.upgrade() else { break };
                        if !inner.closed.load(Ordering::SeqCst) {
                            inner.events.emit_message(message);
                        }
                    }
                });
            }
            self.inner.started.store(true, Ordering::SeqCst);
            Ok(())
        };
        Box::pin(async move { result })
    }

    fn send(
        &self,
        message: JsonRpcMessage,
        _options: McpTransportSendOptions,
    ) -> BoxFuture<'static, Result<()>> {
        let result = (|| {
            if !self.inner.started.load(Ordering::SeqCst)
                || self.inner.closed.load(Ordering::SeqCst)
            {
                return Err(Error::connection_closed());
            }
            let peer = self.inner.peer.lock().as_ref().and_then(Weak::upgrade);
            let not_connected =
                || Error::ConnectionClosed("In-memory MCP peer is not connected".to_string());
            let Some(peer) = peer else {
                return Err(not_connected());
            };
            if !peer.started.load(Ordering::SeqCst) || peer.closed.load(Ordering::SeqCst) {
                return Err(not_connected());
            }
            let inbox = peer.inbox.lock();
            match inbox.as_ref() {
                Some(inbox) if inbox.send(message).is_ok() => Ok(()),
                _ => Err(not_connected()),
            }
        })();
        Box::pin(async move { result })
    }

    fn close(&self) -> BoxFuture<'static, Result<()>> {
        self.inner.close();
        Box::pin(async { Ok(()) })
    }

    fn events(&self) -> &TransportEvents {
        &self.inner.events
    }
}

/// A connected client/server pair of in-memory transports.
pub fn create_in_memory_transport_pair() -> (InMemoryTransport, InMemoryTransport) {
    let client = InMemoryTransport::new();
    let server = InMemoryTransport::new();
    client
        .connect_peer(&server)
        .expect("fresh transport has no peer");
    server
        .connect_peer(&client)
        .expect("fresh transport has no peer");
    (client, server)
}
