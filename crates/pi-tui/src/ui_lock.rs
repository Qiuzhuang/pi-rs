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
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long [`lock_ui`] waits for another holder before it proceeds without the lock.
pub const MAX_WAIT: Duration = Duration::from_millis(500);

static OWNER: Mutex<Option<std::thread::ThreadId>> = Mutex::new(None);
static RELEASED: Condvar = Condvar::new();

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
