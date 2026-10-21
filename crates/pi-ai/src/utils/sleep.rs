//! Abortable sleep (port of `utils/sleep.ts`).

use tokio_util::sync::CancellationToken;

use super::provider_retry::AbortError;

/// Sleep for `ms` milliseconds, failing with [`AbortError`] when `signal` is cancelled.
pub async fn sleep(ms: u64, signal: &CancellationToken) -> Result<(), AbortError> {
    if signal.is_cancelled() {
        return Err(AbortError);
    }
    tokio::select! {
        _ = signal.cancelled() => Err(AbortError),
        _ = tokio::time::sleep(std::time::Duration::from_millis(ms)) => Ok(()),
    }
}
