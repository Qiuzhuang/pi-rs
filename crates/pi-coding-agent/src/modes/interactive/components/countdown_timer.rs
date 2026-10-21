//! Port of `packages/coding-agent/src/modes/interactive/components/countdown-timer.ts`.
//!
//! Reusable countdown timer for dialog components.

use parking_lot::Mutex;
use pi_tui::event_loop::{EventLoop, TimerId};
use pi_tui::tui::Tui;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

struct Inner {
    event_loop: EventLoop,
    interval_id: Mutex<Option<TimerId>>,
    remaining_seconds: AtomicI64,
}

impl Inner {
    fn dispose(&self) {
        if let Some(id) = self.interval_id.lock().take() {
            self.event_loop.clear_timer(id);
        }
    }
}

/// Ticks once per second on the TUI's event loop (or the current thread's loop without a TUI).
/// Dropping the timer disposes it.
pub struct CountdownTimer {
    inner: Arc<Inner>,
}

impl CountdownTimer {
    pub fn new(
        timeout_ms: i64,
        tui: Option<Arc<Tui>>,
        on_tick: Arc<dyn Fn(i64) + Send + Sync>,
        on_expire: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let event_loop = match &tui {
            Some(tui) => tui.event_loop().clone(),
            None => pi_tui::event_loop::current(),
        };
        let remaining = (timeout_ms as f64 / 1000.0).ceil() as i64;
        let inner = Arc::new(Inner {
            event_loop,
            interval_id: Mutex::new(None),
            remaining_seconds: AtomicI64::new(remaining),
        });
        on_tick(remaining);

        let weak: Weak<Inner> = Arc::downgrade(&inner);
        let tui: Option<Weak<Tui>> = tui.as_ref().map(Arc::downgrade);
        let id = inner.event_loop.set_interval(Duration::from_millis(1000), move || {
            let Some(inner) = weak.upgrade() else { return };
            let remaining = inner.remaining_seconds.fetch_sub(1, Ordering::SeqCst) - 1;
            on_tick(remaining);
            if let Some(tui) = tui.as_ref().and_then(Weak::upgrade) {
                tui.request_render(false);
            }
            if remaining <= 0 {
                inner.dispose();
                on_expire();
            }
        });
        *inner.interval_id.lock() = Some(id);
        Self { inner }
    }

    pub fn dispose(&self) {
        self.inner.dispose();
    }
}

impl Drop for CountdownTimer {
    fn drop(&mut self) {
        self.inner.dispose();
    }
}
