//! Port of `packages/coding-agent/src/core/output-guard.ts`.

use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct StdoutTakeoverState {
    pub raw_stdout_write: Arc<
        dyn Fn(String, Option<Arc<dyn Fn(Option<Arc<anyhow::Error>>) + Send + Sync>>) -> bool
            + Send
            + Sync,
    >,
    pub raw_stderr_write: Arc<
        dyn Fn(String, Option<Arc<dyn Fn(Option<Arc<anyhow::Error>>) + Send + Sync>>) -> bool
            + Send
            + Sync,
    >,
    pub original_stdout_write: Value,
}

pub fn take_over_stdout() {
    todo!("port core/output-guard.ts:takeOverStdout")
}

pub fn restore_stdout() {
    todo!("port core/output-guard.ts:restoreStdout")
}

pub fn is_stdout_taken_over() -> bool {
    todo!("port core/output-guard.ts:isStdoutTakenOver")
}

pub fn write_raw_stdout(text: &str) {
    todo!("port core/output-guard.ts:writeRawStdout")
}

pub async fn wait_for_raw_stdout_backpressure() -> anyhow::Result<()> {
    todo!("port core/output-guard.ts:waitForRawStdoutBackpressure")
}

pub async fn flush_raw_stdout() -> anyhow::Result<()> {
    todo!("port core/output-guard.ts:flushRawStdout")
}
