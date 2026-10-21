//! `AbortController` / `AbortSignal` on top of [`CancellationToken`], keeping the abort reason.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// The reason of a signal aborted without one, as `String(new DOMException(...))` prints it.
pub const DEFAULT_ABORT_REASON: &str = "AbortError: This operation was aborted";

/// A cancellation signal with an optional reason. Cheap to clone; clones share state.
#[derive(Clone, Debug, Default)]
pub struct AbortSignal {
    token: CancellationToken,
    reason: Arc<Mutex<Option<String>>>,
}

impl AbortSignal {
    /// A signal that is never aborted unless its controller aborts it.
    pub fn new() -> Self {
        Self::default()
    }

    /// A signal that is already aborted.
    pub fn aborted_with(reason: impl Into<String>) -> Self {
        let signal = Self::new();
        signal.abort(Some(reason.into()));
        signal
    }

    /// A signal that aborts after `duration` (`AbortSignal.timeout`). Requires a Tokio runtime.
    pub fn timeout(duration: Duration) -> Self {
        let signal = Self::new();
        let timer = signal.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    timer.abort(Some("TimeoutError: The operation was aborted due to timeout".to_string()));
                }
                _ = timer.token.cancelled() => {}
            }
        });
        signal
    }

    /// A signal that aborts when any of `signals` aborts (`AbortSignal.any`). Requires a Tokio runtime.
    pub fn any(signals: &[AbortSignal]) -> Self {
        let combined = Self::new();
        if let Some(aborted) = signals.iter().find(|signal| signal.is_aborted()) {
            combined.abort(aborted.reason());
            return combined;
        }
        for signal in signals {
            let (signal, combined) = (signal.clone(), combined.clone());
            tokio::spawn(async move {
                tokio::select! {
                    _ = signal.token.cancelled() => combined.abort(signal.reason()),
                    _ = combined.token.cancelled() => {}
                }
            });
        }
        combined
    }

    pub fn is_aborted(&self) -> bool {
        self.token.is_cancelled()
    }

    /// The abort reason, once aborted.
    pub fn reason(&self) -> Option<String> {
        if !self.is_aborted() {
            return None;
        }
        Some(
            self.reason
                .lock()
                .clone()
                .unwrap_or_else(|| DEFAULT_ABORT_REASON.to_string()),
        )
    }

    /// Resolves once the signal is aborted.
    pub async fn aborted(&self) {
        self.token.cancelled().await
    }

    /// The underlying cancellation token.
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    fn abort(&self, reason: Option<String>) {
        if self.is_aborted() {
            return;
        }
        if let Some(reason) = reason {
            *self.reason.lock() = Some(reason);
        }
        self.token.cancel();
    }
}

impl From<CancellationToken> for AbortSignal {
    fn from(token: CancellationToken) -> Self {
        Self {
            token,
            reason: Arc::default(),
        }
    }
}

impl From<&CancellationToken> for AbortSignal {
    fn from(token: &CancellationToken) -> Self {
        Self::from(token.clone())
    }
}

/// Aborts its [`AbortSignal`].
#[derive(Clone, Debug, Default)]
pub struct AbortController {
    signal: AbortSignal,
}

impl AbortController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn signal(&self) -> AbortSignal {
        self.signal.clone()
    }

    /// Abort with the default reason.
    pub fn abort(&self) {
        self.signal.abort(None);
    }

    /// Abort with `reason`.
    pub fn abort_with(&self, reason: impl Into<String>) {
        self.signal.abort(Some(reason.into()));
    }
}
