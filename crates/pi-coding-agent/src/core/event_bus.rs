//! Port of `packages/coding-agent/src/core/event-bus.ts`.

use serde_json::Value;
use std::sync::Arc;

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

pub fn create_event_bus() -> Arc<dyn EventBusController> {
    todo!("port core/event-bus.ts:createEventBus")
}
