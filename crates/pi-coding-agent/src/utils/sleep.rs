//! Port of `packages/coding-agent/src/utils/sleep.ts`.

use tokio_util::sync::CancellationToken;

/// Sleep helper that respects abort signal.
pub async fn sleep(ms: f64, signal: Option<CancellationToken>) -> anyhow::Result<()> {
    if signal.as_ref().is_some_and(|s| s.is_cancelled()) {
        anyhow::bail!("Aborted");
    }
    let duration = std::time::Duration::from_millis(if ms.is_finite() && ms > 0.0 {
        ms as u64
    } else {
        0
    });
    match signal {
        Some(signal) => tokio::select! {
            _ = tokio::time::sleep(duration) => Ok(()),
            _ = signal.cancelled() => Err(anyhow::anyhow!("Aborted")),
        },
        None => {
            tokio::time::sleep(duration).await;
            Ok(())
        }
    }
}
