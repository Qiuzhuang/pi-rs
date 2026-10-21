//! Abort signal helpers (port of `utils/abort.ts` and `utils/abort-signals.ts`).
//!
//! `tokio_util::sync::CancellationToken` is the Rust counterpart of `AbortSignal`.

use std::future::Future;

use tokio_util::sync::CancellationToken;

use super::provider_retry::AbortError;

/// An operation-local signal for APIs whose signal is optional.
pub fn operation_signal(signal: Option<&CancellationToken>) -> CancellationToken {
    signal.cloned().unwrap_or_default()
}

/// Stop waiting for `operation` when `signal` is cancelled (the operation is dropped).
pub async fn race_with_abort_signal<T>(
    operation: impl Future<Output = anyhow::Result<T>>,
    signal: &CancellationToken,
) -> anyhow::Result<T> {
    if signal.is_cancelled() {
        return Err(AbortError.into());
    }
    tokio::select! {
        result = operation => result,
        _ = signal.cancelled() => Err(AbortError.into()),
    }
}

/// Combine signals: `None` when there are none, the signal itself when there is one, and
/// otherwise a new token cancelled when any input is cancelled (requires a tokio runtime).
pub fn combine_abort_signals(signals: &[Option<CancellationToken>]) -> Option<CancellationToken> {
    let active: Vec<CancellationToken> = signals.iter().flatten().cloned().collect();
    match active.len() {
        0 => None,
        1 => Some(active[0].clone()),
        _ => {
            let combined = CancellationToken::new();
            if active.iter().any(CancellationToken::is_cancelled) {
                combined.cancel();
                return Some(combined);
            }
            let target = combined.clone();
            tokio::spawn(async move {
                let waits = active.iter().map(|s| Box::pin(s.cancelled())).collect::<Vec<_>>();
                tokio::select! {
                    _ = futures::future::select_all(waits) => target.cancel(),
                    _ = target.cancelled() => {}
                }
            });
            Some(combined)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn combines_signals() {
        assert!(combine_abort_signals(&[None, None]).is_none());
        let a = CancellationToken::new();
        let b = CancellationToken::new();
        let combined = combine_abort_signals(&[Some(a.clone()), None, Some(b.clone())]).unwrap();
        assert!(!combined.is_cancelled());
        b.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(1), combined.cancelled()).await.unwrap();
        let err = race_with_abort_signal(async { Ok::<_, anyhow::Error>(1) }, &a).await.unwrap();
        assert_eq!(err, 1);
    }
}
