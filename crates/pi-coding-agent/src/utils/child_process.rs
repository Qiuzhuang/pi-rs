//! Port of `packages/coding-agent/src/utils/child-process.ts`.

use serde_json::Value;
use std::sync::Arc;

pub fn spawn_process(
    command: &str,
    args: &[String],
    options: Value,
) -> Arc<tokio::sync::Mutex<tokio::process::Child>> {
    todo!("port utils/child-process.ts:spawnProcess")
}

pub fn spawn_process_sync(command: &str, args: &[String], options: Value) -> Value {
    todo!("port utils/child-process.ts:spawnProcessSync")
}

/// Wait for a child process to terminate without hanging on inherited stdio handles.
///
/// A short-lived child can `exit` while a detached descendant keeps its stdout/stderr
/// pipe open. We must not resolve and destroy the streams on a fixed deadline measured
/// from `exit`, or output still being written past that deadline is silently lost
/// (earendil-works/pi#5303). Instead, after `exit` we wait for the pipes to fall idle:
/// the grace timer is re-armed on every chunk, so an actively writing descendant keeps
/// us reading, while a quiet inherited handle (e.g. a Windows daemonized descendant
/// that never lets `close` fire) still releases us after the grace elapses.
pub async fn wait_for_child_process(
    child: Arc<tokio::sync::Mutex<tokio::process::Child>>,
) -> anyhow::Result<Option<f64>> {
    todo!("port utils/child-process.ts:waitForChildProcess")
}
