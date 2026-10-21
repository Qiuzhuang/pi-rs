//! Port of `packages/coding-agent/src/utils/fs-watch.ts`.

use serde_json::Value;
use std::sync::Arc;

pub const FS_WATCH_RETRY_DELAY_MS: i64 = 5000;

pub fn close_watcher(watcher: Option<Value>) {
    todo!("port utils/fs-watch.ts:closeWatcher")
}

pub fn watch_with_error_handler(
    path: &str,
    listener: Value,
    on_error: Arc<dyn Fn() + Send + Sync>,
) -> Option<Value> {
    todo!("port utils/fs-watch.ts:watchWithErrorHandler")
}
