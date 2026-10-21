//! Port of `packages/coding-agent/src/utils/sleep.ts`.

use tokio_util::sync::CancellationToken;

/// Sleep helper that respects abort signal.
pub async fn sleep(ms: f64, signal: Option<CancellationToken>) -> anyhow::Result<()> {
    todo!("port utils/sleep.ts:sleep")
}
