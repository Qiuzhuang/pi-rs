//! Port of `packages/coding-agent/src/core/event-bus.ts`.

use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub trait EventBus: Send + Sync {
    fn emit(&self, channel: &str, data: Value);
    fn on(
        &self,
        channel: &str,
        handler: Arc<dyn Fn(Value) + Send + Sync>,
    ) -> Arc<dyn Fn() + Send + Sync>;
}

pub trait EventBusController: EventBus + Send + Sync {
    fn clear(&self);
}

type Handler = Arc<dyn Fn(Value) + Send + Sync>;

#[derive(Default)]
struct EventBusInner {
    /// `(id, channel, handler)` in registration order (Node `EventEmitter` order).
    listeners: Mutex<Vec<(u64, String, Handler)>>,
    next_id: AtomicU64,
}

/// In-process event bus (Node `EventEmitter` semantics: handlers run synchronously, in order).
struct LocalEventBus {
    inner: Arc<EventBusInner>,
}

impl EventBus for LocalEventBus {
    fn emit(&self, channel: &str, data: Value) {
        // Snapshot so handlers can subscribe/unsubscribe while being called.
        let handlers: Vec<Handler> = self
            .inner
            .listeners
            .lock()
            .iter()
            .filter(|(_, c, _)| c == channel)
            .map(|(_, _, h)| h.clone())
            .collect();
        for handler in handlers {
            let data = data.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(data)));
            if let Err(panic) = result {
                let message = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown error".to_string());
                eprintln!("Event handler error ({channel}): {message}");
            }
        }
    }

    fn on(&self, channel: &str, handler: Handler) -> Arc<dyn Fn() + Send + Sync> {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        self.inner
            .listeners
            .lock()
            .push((id, channel.to_string(), handler));
        let weak = Arc::downgrade(&self.inner);
        Arc::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.listeners.lock().retain(|(lid, _, _)| *lid != id);
            }
        })
    }
}

impl EventBusController for LocalEventBus {
    fn clear(&self) {
        self.inner.listeners.lock().clear();
    }
}

pub fn create_event_bus() -> Arc<dyn EventBusController> {
    Arc::new(LocalEventBus {
        inner: Arc::new(EventBusInner::default()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn emits_to_channel_handlers_and_unsubscribes() {
        let bus = create_event_bus();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let off = bus.on("a", Arc::new(move |v| s.lock().push(v)));
        bus.emit("a", json!(1));
        bus.emit("b", json!(2));
        off();
        bus.emit("a", json!(3));
        assert_eq!(*seen.lock(), vec![json!(1)]);
    }

    #[test]
    fn handler_panics_are_contained() {
        let bus = create_event_bus();
        let seen = Arc::new(Mutex::new(0));
        bus.on("a", Arc::new(|_| panic!("boom")));
        let s = seen.clone();
        bus.on("a", Arc::new(move |_| *s.lock() += 1));
        bus.emit("a", Value::Null);
        assert_eq!(*seen.lock(), 1);
        bus.clear();
        bus.emit("a", Value::Null);
        assert_eq!(*seen.lock(), 1);
    }
}
