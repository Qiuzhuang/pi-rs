//! The Rust counterpart of Node's `uncaughtException` / `unhandledRejection` handling.
//!
//! TS interactive mode registers a `process.on("uncaughtException")` listener that restores the
//! terminal, records the crash and exits with 1 (`uncaughtCrash`). An exception is "uncaught"
//! when nothing up the stack (or no `.catch()` on the promise) handles it; exceptions caught with
//! `try`/`catch` never reach the listener.
//!
//! A Rust panic is the analog of a thrown exception. Whether it is caught is only known once it
//! stops unwinding, so the decision is made where the unwind ends, not in the panic hook:
//!
//! - the panic hook ([`set_uncaught_handler`] installs it) only records where the panic happened
//!   ([`PanicError`]) and, while a quiet handler is registered, keeps the default "thread
//!   panicked" message from garbling the TUI. Panics the code deliberately catches (`catch_unwind`, a
//!   `JoinHandle` whose `JoinError` is inspected) therefore never fire the handler;
//! - the places where a panic would otherwise end up unhandled hand it to [`report_uncaught`]:
//!   the interactive main loop (main thread), detached tasks wrapped in [`supervise`]
//!   (the analog of a promise nobody awaits, i.e. `unhandledRejection`), and pi-tui's event-loop
//!   and stdin threads (through `pi_tui::event_loop::set_thread_panic_handler`).
//!
//! Without a registered handler everything behaves as before (the panic resumes unwinding).

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Once};

use futures::FutureExt;
use parking_lot::RwLock;

/// Handler for an uncaught panic. Interactive mode's handler restores the terminal, records the
/// crash and exits the process.
pub type UncaughtHandler = Arc<dyn Fn(anyhow::Error) + Send + Sync>;

static HANDLER: RwLock<Option<Registration>> = RwLock::new(None);

struct Registration {
    owner: u64,
    handler: UncaughtHandler,
    quiet: bool,
}
static HOOK: Once = Once::new();

thread_local! {
    static LAST_PANIC: RefCell<Option<PanicError>> = const { RefCell::new(None) };
}

/// A panic described like a thrown JS error: `message` is the panic message, `stack` the
/// location and (when `RUST_BACKTRACE` enables it) the backtrace.
#[derive(Debug, Clone)]
pub struct PanicError {
    pub message: String,
    pub stack: String,
}

impl fmt::Display for PanicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PanicError {}

/// Payload of the unwind [`crate::utils::process_exit::exit`] starts when a test exit hook
/// panics to keep the process alive. It is not a crash and keeps unwinding.
pub(crate) struct ExitHookUnwind(#[allow(dead_code)] pub Box<dyn Any + Send>);

/// The message of a panic payload (`panic!("...")` gives a `&str` or a `String`).
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(error) = payload.downcast_ref::<anyhow::Error>() {
        error.to_string()
    } else {
        "Box<dyn Any>".to_string()
    }
}

fn install_hook() {
    HOOK.call_once(|| {
        pi_tui::event_loop::set_thread_panic_handler(Some(Arc::new(report_tui_thread_panic)));
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = panic_message(info.payload());
            let thread = std::thread::current();
            let name = thread.name().unwrap_or("<unnamed>");
            let location = info.location().map(|l| l.to_string()).unwrap_or_default();
            let mut stack = format!("thread '{name}' panicked at {location}:\n{message}");
            let backtrace = std::backtrace::Backtrace::capture();
            if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
                stack.push_str(&format!("\nstack backtrace:\n{backtrace}"));
            }
            LAST_PANIC.with(|last| *last.borrow_mut() = Some(PanicError { message, stack }));
            // While interactive mode owns the real terminal, caught panics stay silent (like a
            // caught JS exception) instead of garbling the TUI, and uncaught ones are reported by
            // the handler after restoring it.
            let quiet = HANDLER.read().as_ref().is_some_and(|r| r.quiet);
            if !quiet {
                previous(info);
            }
        }));
    });
}

/// Register (or with `None`, unregister) the uncaught-panic handler. `owner` identifies the
/// registration: unregistering only clears the handler when it still belongs to `owner`. `quiet`
/// keeps the default panic message off stderr while the handler is registered (set it when a
/// TUI draws on the real terminal).
pub fn set_uncaught_handler(owner: u64, handler: Option<UncaughtHandler>, quiet: bool) {
    install_hook();
    let mut slot = HANDLER.write();
    match handler {
        Some(handler) => *slot = Some(Registration { owner, handler, quiet }),
        None => {
            if slot.as_ref().is_some_and(|r| r.owner == owner) {
                *slot = None;
            }
        }
    }
}

/// The error for a caught panic payload: the details the hook recorded on this thread, or just
/// the payload's message.
pub fn panic_error(payload: &(dyn Any + Send)) -> PanicError {
    let message = panic_message(payload);
    LAST_PANIC
        .with(|last| last.borrow_mut().take())
        .filter(|recorded| recorded.message == message)
        .unwrap_or_else(|| PanicError { stack: message.clone(), message })
}

fn current_handler() -> Option<UncaughtHandler> {
    HANDLER.read().as_ref().map(|r| r.handler.clone())
}

/// A panic reached a point where nothing handles it: hand it to the registered handler (which
/// normally exits the process). Without a handler, or when the handler returns, the panic keeps
/// unwinding as it would have without this module.
pub fn report_uncaught(payload: Box<dyn Any + Send>) -> ! {
    if !payload.is::<ExitHookUnwind>()
        && let Some(handler) = current_handler()
    {
        handler(anyhow::Error::new(panic_error(payload.as_ref())));
    }
    std::panic::resume_unwind(payload)
}

/// pi-tui's handler for panics escaping its event-loop and stdin threads. It runs on the dying
/// thread; the crash is reported from a fresh thread because restoring the terminal joins the
/// stdin reader thread, and the dying thread then ends normally.
fn report_tui_thread_panic(payload: Box<dyn Any + Send>) {
    if payload.is::<ExitHookUnwind>() {
        std::panic::resume_unwind(payload);
    }
    let Some(handler) = current_handler() else {
        std::panic::resume_unwind(payload);
    };
    // Read the details the hook recorded on this thread before leaving it.
    let error = panic_error(payload.as_ref());
    // Not joined: the reporter may itself join this thread (the stdin reader) while restoring.
    let _ = std::thread::Builder::new()
        .name("pi-uncaught".into())
        .spawn(move || handler(anyhow::Error::new(error)));
}

/// Run a detached future so that a panic in it is reported as uncaught (TS: a rejected promise
/// nobody handles ends in the `uncaughtException` handler) instead of being dropped with its
/// `JoinHandle`.
pub async fn supervise<F: Future>(future: F) -> F::Output {
    match std::panic::AssertUnwindSafe(future).catch_unwind().await {
        Ok(output) => output,
        Err(payload) => report_uncaught(payload),
    }
}

/// `tokio::spawn` for a detached task: a panic in it is reported as uncaught (see [`supervise`]).
/// Use it whenever the `JoinHandle` is dropped or only used to abort the task; keep plain
/// `tokio::spawn` where the handle is awaited and its `JoinError` handled.
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(supervise(future))
}

/// [`spawn`] on a specific runtime handle.
pub fn spawn_on<F>(handle: &tokio::runtime::Handle, future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    handle.spawn(supervise(future))
}

/// Wrap a thread body so that a panic escaping it is reported as uncaught (TS: an exception
/// thrown from a timer or I/O callback reaches `uncaughtException`).
pub fn supervise_fn<F, T>(f: F) -> impl FnOnce() -> T + Send + 'static
where
    F: FnOnce() -> T + Send + 'static,
    T: 'static,
{
    move || match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => report_uncaught(payload),
    }
}

/// `std::thread::spawn` for a detached thread whose panics are reported as uncaught.
pub fn spawn_thread<F, T>(f: F) -> std::thread::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(supervise_fn(f))
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    static REPORTS: parking_lot::Mutex<Vec<String>> = parking_lot::Mutex::new(Vec::new());

    fn reported(message: &str) -> bool {
        REPORTS.lock().iter().any(|m| m == message)
    }

    #[test]
    fn detached_task_and_thread_panics_reach_the_uncaught_handler() {
        const OWNER: u64 = 0x5eed_0001;
        set_uncaught_handler(OWNER, Some(Arc::new(|error| REPORTS.lock().push(error.to_string()))), true);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let result = rt.block_on(async { spawn(async { panic!("task boom") }).await });
        assert!(result.is_err());
        assert!(reported("task boom"));
        let joined = spawn_thread(|| panic!("thread boom")).join();
        assert!(joined.is_err());
        assert!(reported("thread boom"));
        // Ordinary results pass through.
        assert_eq!(spawn_thread(|| 7).join().unwrap(), 7);
        set_uncaught_handler(OWNER, None, false);
    }
}
