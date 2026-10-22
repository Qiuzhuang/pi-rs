//! A process-wide, reentrant lock that makes a frame see a consistent component tree.
//!
//! The TypeScript TUI runs on one thread: an event handler that rebuilds a component (clear a
//! container, add the new children) always finishes before the next frame renders. In the Rust
//! port, rendering happens on the TUI [`EventLoop`](crate::event_loop::EventLoop) thread while
//! application code (agent events, tool updates) may run on other threads. Without coordination a
//! frame can render a container between `clear()` and `add_child()`, so content vanishes for one
//! frame and reappears in the next: visible flicker, even inside synchronized output.
//!
//! [`Tui`](crate::tui::Tui) holds this lock while it renders a frame. Code that mutates several
//! components as one logical update from a thread other than the render loop holds it for the
//! duration of that update with [`lock_ui`]. The lock is reentrant, so a holder may render
//! synchronously (`render_now`) or nest updates.
//!
//! Never wait for another thread (a channel, a `Deferred`, a blocking `block_on`) while holding
//! the guard: the render loop may be waiting for it. A thread that other threads wait on while
//! they hold the lock (such as a JS runtime thread that renders extension components) must call
//! [`exempt_current_thread`]; its [`lock_ui`] calls then never block. As a last line of defense a
//! waiter gives up after [`MAX_WAIT`] and proceeds without the lock, so an unforeseen wait cycle
//! costs a stall instead of a hang.

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// How long [`lock_ui`] waits for another holder before it proceeds without the lock.
pub const MAX_WAIT: Duration = Duration::from_millis(500);

static OWNER: Mutex<Option<std::thread::ThreadId>> = Mutex::new(None);
static RELEASED: Condvar = Condvar::new();

/// Waits that gave up after [`MAX_WAIT`] (each one is a stall and a missed exclusion).
static WAIT_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

/// Diagnostics: with `PI_TUI_UI_LOCK_LOG=<file>`, every give-up appends the waiter's backtrace and
/// the backtrace where the current owner took the lock to `<file>` (stress tests fail on any).
static LOG_PATH: LazyLock<Option<String>> =
    LazyLock::new(|| std::env::var("PI_TUI_UI_LOCK_LOG").ok().filter(|p| !p.is_empty()));
static OWNER_SITE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// Number of [`lock_ui`] waits that gave up after [`MAX_WAIT`] since the process started.
pub fn wait_timeouts() -> u64 {
    WAIT_TIMEOUTS.load(AtomicOrdering::SeqCst)
}

fn thread_label() -> String {
    let t = std::thread::current();
    format!("{} ({:?})", t.name().unwrap_or("unnamed"), t.id())
}

fn log_timeout() {
    WAIT_TIMEOUTS.fetch_add(1, AtomicOrdering::SeqCst);
    let Some(path) = LOG_PATH.as_ref() else { return };
    let owner_site = match OWNER_SITE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        Some((since, site)) => format!("(held for {:?}) {site}", since.elapsed()),
        None => String::new(),
    };
    let entry = format!(
        "=== ui_lock wait gave up after {MAX_WAIT:?} on {}\n--- waiter:\n{}\n--- owner took the lock at:\n{owner_site}\n",
        thread_label(),
        std::backtrace::Backtrace::force_capture()
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(entry.as_bytes());
    }
}

thread_local! {
    /// Guards alive on this thread.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
    /// Whether the outermost guard on this thread owns the lock.
    static OWNED: Cell<bool> = const { Cell::new(false) };
    static EXEMPT: Cell<bool> = const { Cell::new(false) };
}

/// Make [`lock_ui`] a no-op on the current thread (see the module docs).
pub fn exempt_current_thread() {
    EXEMPT.with(|e| e.set(true));
}

/// Holds the UI lock until dropped. Not `Send`: it must be dropped on the thread that took it.
#[must_use = "the UI lock is released when the guard is dropped"]
pub struct UiLockGuard {
    _not_send: PhantomData<*const ()>,
}

/// Take the UI lock, blocking while another thread renders or updates components.
/// Reentrant on the same thread.
pub fn lock_ui() -> UiLockGuard {
    let depth = DEPTH.with(Cell::get);
    if depth == 0 && !EXEMPT.with(Cell::get) {
        let me = std::thread::current().id();
        let deadline = Instant::now() + MAX_WAIT;
        let mut owner = OWNER.lock().unwrap_or_else(|e| e.into_inner());
        let mut acquired = true;
        while owner.is_some() {
            let now = Instant::now();
            if now >= deadline {
                acquired = false;
                break;
            }
            owner = RELEASED.wait_timeout(owner, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
        if acquired {
            *owner = Some(me);
            drop(owner);
            if LOG_PATH.is_some() {
                *OWNER_SITE.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((Instant::now(), format!("{}\n{}", thread_label(), std::backtrace::Backtrace::force_capture())));
            }
        } else {
            drop(owner);
            log_timeout();
        }
        OWNED.with(|o| o.set(acquired));
    }
    DEPTH.with(|d| d.set(depth + 1));
    UiLockGuard { _not_send: PhantomData }
}

/// Whether the current thread holds the UI lock.
pub fn is_ui_locked_by_current_thread() -> bool {
    DEPTH.with(Cell::get) > 0 && OWNED.with(Cell::get)
}

impl Drop for UiLockGuard {
    fn drop(&mut self) {
        let depth = DEPTH.with(Cell::get);
        DEPTH.with(|d| d.set(depth.saturating_sub(1)));
        if depth == 1 && OWNED.with(|o| o.replace(false)) {
            *OWNER.lock().unwrap_or_else(|e| e.into_inner()) = None;
            RELEASED.notify_one();
        }
    }
}

/// A future whose every `poll` holds the UI lock (see [`atomic`]).
pub struct UiAtomic<F> {
    inner: std::pin::Pin<Box<F>>,
}

impl<F: std::future::Future> std::future::Future for UiAtomic<F> {
    type Output = F::Output;
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<F::Output> {
        let _ui = lock_ui();
        self.inner.as_mut().poll(cx)
    }
}

/// Run each synchronous segment of `future` (the code between two `.await`s) under the UI lock,
/// like a JS async function whose segments run to completion before the next frame: a frame never
/// shows a rebuild that segment left half-applied. The lock is not held across `.await`s.
pub fn atomic<F: std::future::Future>(future: F) -> UiAtomic<F> {
    UiAtomic { inner: Box::pin(future) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn reentrant_on_one_thread() {
        let a = lock_ui();
        let b = lock_ui();
        assert!(is_ui_locked_by_current_thread());
        drop(b);
        assert!(is_ui_locked_by_current_thread());
        drop(a);
        assert!(!is_ui_locked_by_current_thread());
    }

    #[test]
    fn excludes_other_threads() {
        let inside = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let inside = inside.clone();
                let max = max.clone();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        let _g = lock_ui();
                        let _nested = lock_ui();
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        max.fetch_max(now, Ordering::SeqCst);
                        std::hint::spin_loop();
                        inside.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(max.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn exempt_thread_never_blocks() {
        let held = lock_ui();
        std::thread::spawn(|| {
            exempt_current_thread();
            let start = Instant::now();
            let _g = lock_ui();
            assert!(start.elapsed() < MAX_WAIT);
            assert!(!is_ui_locked_by_current_thread());
        })
        .join()
        .unwrap();
        drop(held);
    }

    #[test]
    fn atomic_futures_hold_the_lock_per_poll_only() {
        use std::future::Future;
        use std::task::{Context, Poll, Waker};
        let mut yielded = false;
        let mut fut = Box::pin(atomic(std::future::poll_fn(move |_cx| {
            assert!(is_ui_locked_by_current_thread(), "each poll runs under the UI lock");
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                Poll::Pending
            }
        })));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(fut.as_mut().poll(&mut cx).is_pending());
        // Not held between polls (across the await).
        assert!(!is_ui_locked_by_current_thread());
        assert!(fut.as_mut().poll(&mut cx).is_ready());
        assert!(!is_ui_locked_by_current_thread());
    }

    #[test]
    fn waiter_gives_up_instead_of_hanging() {
        let held = lock_ui();
        std::thread::spawn(|| {
            let start = Instant::now();
            let g = lock_ui();
            assert!(start.elapsed() >= MAX_WAIT);
            assert!(!is_ui_locked_by_current_thread());
            drop(g);
        })
        .join()
        .unwrap();
        // The timed-out waiter must not have released the lock held here.
        assert!(is_ui_locked_by_current_thread());
        drop(held);
        let _again = lock_ui();
    }
}
