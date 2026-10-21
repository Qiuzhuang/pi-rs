//! A small event loop that serializes UI work, timers and render scheduling.
//!
//! The TypeScript TUI relies on the Node.js event loop (`process.nextTick`, `setTimeout`,
//! `setInterval`). This module provides the equivalent for the Rust port:
//!
//! - [`EventLoop::post`] queues a task (like `process.nextTick`/`setImmediate`).
//! - [`EventLoop::set_timeout`] / [`EventLoop::set_interval`] schedule timers.
//! - [`EventLoop::run`] processes tasks until [`EventLoop::stop`]; [`EventLoop::spawn`]
//!   runs it on a background thread.
//!
//! A *manual* loop ([`EventLoop::new_manual`]) uses a virtual clock and never runs on its
//! own; tests drive it with [`EventLoop::run_pending`] and [`EventLoop::advance`].
//!
//! Components that need timers (loaders, scroll views) use [`current`], which returns the
//! loop installed for the current thread with [`EventLoop::make_current`] or the global
//! real-time loop.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Task = Box<dyn FnOnce() + Send>;
type IntervalFn = Arc<Mutex<dyn FnMut() + Send>>;

/// Identifies a timer created by [`EventLoop::set_timeout`] or [`EventLoop::set_interval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

enum TimerKind {
    Once(Task),
    Interval { f: IntervalFn, period: Duration },
}

struct State {
    tasks: VecDeque<Task>,
    timers: BTreeMap<(Duration, u64), TimerKind>,
    due_by_id: std::collections::HashMap<u64, Duration>,
    next_id: u64,
    virtual_now: Duration,
    stopped: bool,
    running: bool,
}

struct Inner {
    state: Mutex<State>,
    cv: Condvar,
    manual: bool,
    start: Instant,
}

/// A task/timer queue. Cheap to clone (shared handle).
#[derive(Clone)]
pub struct EventLoop {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for EventLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventLoop").field("manual", &self.inner.manual).finish()
    }
}

static GLOBAL: LazyLock<EventLoop> = LazyLock::new(|| {
    let lp = EventLoop::new();
    lp.spawn();
    lp
});

thread_local! {
    static CURRENT: RefCell<Option<EventLoop>> = const { RefCell::new(None) };
}

/// The event loop for the current thread: the one installed with
/// [`EventLoop::make_current`], or the global real-time loop (running on its own thread).
pub fn current() -> EventLoop {
    CURRENT.with(|c| c.borrow().clone()).unwrap_or_else(|| GLOBAL.clone())
}

/// Guard returned by [`EventLoop::make_current`]; restores the previous loop on drop.
pub struct CurrentGuard {
    previous: Option<EventLoop>,
}

impl Drop for CurrentGuard {
    fn drop(&mut self) {
        let prev = self.previous.take();
        CURRENT.with(|c| *c.borrow_mut() = prev);
    }
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLoop {
    fn with_mode(manual: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    tasks: VecDeque::new(),
                    timers: BTreeMap::new(),
                    due_by_id: Default::default(),
                    next_id: 1,
                    virtual_now: Duration::ZERO,
                    stopped: false,
                    running: false,
                }),
                cv: Condvar::new(),
                manual,
                start: Instant::now(),
            }),
        }
    }

    /// A real-time loop.
    pub fn new() -> Self {
        Self::with_mode(false)
    }

    /// A loop with a virtual clock, driven by [`run_pending`](Self::run_pending) and
    /// [`advance`](Self::advance).
    pub fn new_manual() -> Self {
        Self::with_mode(true)
    }

    /// The process-wide real-time loop.
    pub fn global() -> EventLoop {
        GLOBAL.clone()
    }

    pub fn is_manual(&self) -> bool {
        self.inner.manual
    }

    /// Install this loop as [`current`] for this thread until the guard is dropped.
    pub fn make_current(&self) -> CurrentGuard {
        let previous = CURRENT.with(|c| c.borrow_mut().replace(self.clone()));
        CurrentGuard { previous }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now_locked(&self, state: &State) -> Duration {
        if self.inner.manual { state.virtual_now } else { self.inner.start.elapsed() }
    }

    /// Time elapsed since the loop was created (virtual for manual loops).
    pub fn now(&self) -> Duration {
        let state = self.lock();
        self.now_locked(&state)
    }

    /// Milliseconds since the loop was created, like `performance.now()`.
    pub fn now_ms(&self) -> f64 {
        self.now().as_secs_f64() * 1000.0
    }

    /// Queue a task to run on the loop.
    pub fn post(&self, f: impl FnOnce() + Send + 'static) {
        let mut state = self.lock();
        state.tasks.push_back(Box::new(f));
        drop(state);
        self.inner.cv.notify_all();
    }

    fn insert_timer(&self, delay: Duration, kind: TimerKind) -> TimerId {
        let mut state = self.lock();
        let id = state.next_id;
        state.next_id += 1;
        let due = self.now_locked(&state) + delay;
        state.timers.insert((due, id), kind);
        state.due_by_id.insert(id, due);
        drop(state);
        self.inner.cv.notify_all();
        TimerId(id)
    }

    /// Run `f` once after `delay`.
    pub fn set_timeout(&self, delay: Duration, f: impl FnOnce() + Send + 'static) -> TimerId {
        self.insert_timer(delay, TimerKind::Once(Box::new(f)))
    }

    /// Run `f` every `period`.
    pub fn set_interval(&self, period: Duration, f: impl FnMut() + Send + 'static) -> TimerId {
        let f: IntervalFn = Arc::new(Mutex::new(f));
        self.insert_timer(period, TimerKind::Interval { f, period })
    }

    /// Cancel a timer. Unknown or already-fired timers are ignored.
    pub fn clear_timer(&self, id: TimerId) {
        let mut state = self.lock();
        if let Some(due) = state.due_by_id.remove(&id.0) {
            state.timers.remove(&(due, id.0));
        }
    }

    /// Whether a timer is still scheduled.
    pub fn has_timer(&self, id: TimerId) -> bool {
        self.lock().due_by_id.contains_key(&id.0)
    }

    /// Number of scheduled timers.
    pub fn timer_count(&self) -> usize {
        self.lock().timers.len()
    }

    /// Take the next ready unit of work: a posted task or a due timer.
    fn take_ready(&self) -> Option<Task> {
        let mut state = self.lock();
        if let Some(task) = state.tasks.pop_front() {
            return Some(task);
        }
        let now = self.now_locked(&state);
        let key = *state.timers.keys().next()?;
        if key.0 > now {
            return None;
        }
        let kind = state.timers.remove(&key).unwrap();
        state.due_by_id.remove(&key.1);
        match kind {
            TimerKind::Once(task) => Some(task),
            TimerKind::Interval { f, period } => {
                // Re-arm before running (keeps the same id so clear_timer works).
                let due = key.0 + period.max(Duration::from_millis(1));
                state.timers.insert((due, key.1), TimerKind::Interval { f: f.clone(), period });
                state.due_by_id.insert(key.1, due);
                Some(Box::new(move || {
                    let mut g = f.lock().unwrap_or_else(|e| e.into_inner());
                    (g)();
                }))
            }
        }
    }

    /// Run all ready tasks and due timers (including ones they schedule that are ready).
    pub fn run_pending(&self) {
        while let Some(task) = self.take_ready() {
            task();
        }
    }

    /// Manual loops: advance the virtual clock by `d`, firing timers in order.
    pub fn advance(&self, d: Duration) {
        assert!(self.inner.manual, "advance() requires a manual event loop");
        let target = self.now() + d;
        loop {
            self.run_pending();
            let mut state = self.lock();
            let next = state.timers.keys().next().map(|k| k.0);
            match next {
                Some(due) if due <= target => {
                    if due > state.virtual_now {
                        state.virtual_now = due;
                    }
                }
                _ => {
                    state.virtual_now = target;
                    drop(state);
                    self.run_pending();
                    return;
                }
            }
        }
    }

    /// Manual loops: advance by `ms` milliseconds.
    pub fn advance_ms(&self, ms: u64) {
        self.advance(Duration::from_millis(ms));
    }

    /// Run tasks until [`stop`](Self::stop) is called. Real-time loops only.
    pub fn run(&self) {
        {
            let mut state = self.lock();
            state.stopped = false;
            state.running = true;
        }
        loop {
            self.run_pending();
            let state = self.lock();
            if state.stopped {
                drop(state);
                break;
            }
            if !state.tasks.is_empty() {
                continue;
            }
            let wait = state.timers.keys().next().map(|k| k.0.saturating_sub(self.now_locked(&state)));
            match wait {
                Some(w) if w.is_zero() => continue,
                Some(w) => drop(self.inner.cv.wait_timeout(state, w)),
                None => drop(self.inner.cv.wait(state)),
            }
        }
        self.lock().running = false;
    }

    /// Run the loop on a new background thread.
    pub fn spawn(&self) -> JoinHandle<()> {
        let lp = self.clone();
        std::thread::Builder::new()
            .name("pi-tui-event-loop".into())
            .spawn(move || {
                let _guard = lp.make_current();
                lp.run();
            })
            .expect("spawn event loop thread")
    }

    /// Whether [`run`](Self::run) is active.
    pub fn is_running(&self) -> bool {
        self.lock().running
    }

    /// Stop a running loop after the current task.
    pub fn stop(&self) {
        self.lock().stopped = true;
        self.inner.cv.notify_all();
    }
}

// ---------------------------------------------------------------------------
// Deferred values (minimal promise/oneshot)
// ---------------------------------------------------------------------------

struct DeferredState<T> {
    value: Option<T>,
    wakers: Vec<Waker>,
}

/// A value that becomes available later, like a resolved `Promise`.
///
/// Await it, block on it with [`Deferred::wait`], or poll with [`Deferred::try_take`].
pub struct Deferred<T> {
    shared: Arc<(Mutex<DeferredState<T>>, Condvar)>,
}

/// Resolves the paired [`Deferred`].
pub struct Resolver<T> {
    shared: Arc<(Mutex<DeferredState<T>>, Condvar)>,
}

impl<T> Clone for Resolver<T> {
    fn clone(&self) -> Self {
        Self { shared: self.shared.clone() }
    }
}

/// Create a deferred value and its resolver.
pub fn deferred<T>() -> (Resolver<T>, Deferred<T>) {
    let shared = Arc::new((Mutex::new(DeferredState { value: None, wakers: Vec::new() }), Condvar::new()));
    (Resolver { shared: shared.clone() }, Deferred { shared })
}

impl<T> Resolver<T> {
    /// Resolve with `value`. Later calls are ignored if the value was not taken yet.
    pub fn resolve(&self, value: T) {
        let mut st = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        if st.value.is_some() {
            return;
        }
        st.value = Some(value);
        for w in st.wakers.drain(..) {
            w.wake();
        }
        drop(st);
        self.shared.1.notify_all();
    }
}

impl<T> Deferred<T> {
    /// Take the value if it is available.
    pub fn try_take(&self) -> Option<T> {
        self.shared.0.lock().unwrap_or_else(|e| e.into_inner()).value.take()
    }

    /// Block until the value is available.
    pub fn wait(self) -> T {
        let mut st = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(v) = st.value.take() {
                return v;
            }
            st = self.shared.1.wait(st).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Block until the value is available or the timeout passes.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        let mut st = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(v) = st.value.take() {
                return Some(v);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            st = self.shared.1.wait_timeout(st, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

impl<T> Future for Deferred<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut st = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = st.value.take() {
            return Poll::Ready(v);
        }
        st.wakers.push(cx.waker().clone());
        Poll::Pending
    }
}
