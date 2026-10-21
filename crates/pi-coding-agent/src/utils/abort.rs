//! Port of `packages/coding-agent/src/utils/abort.ts`.

use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Normalize an optional public signal without imposing a deadline.
pub fn operation_signal(signal: Option<CancellationToken>) -> CancellationToken {
    todo!("port utils/abort.ts:operationSignal")
}

/// Stop waiting on abort while observing the abandoned operation through settlement.
pub async fn race_with_abort_signal(
    operation: futures::future::Shared<BoxFuture<'static, Result<Value, Arc<anyhow::Error>>>>,
    signal: Option<CancellationToken>,
) -> anyhow::Result<Value> {
    todo!("port utils/abort.ts:raceWithAbortSignal")
}
