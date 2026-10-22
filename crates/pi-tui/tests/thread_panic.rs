//! A panic escaping a pi-tui-owned thread reaches the thread panic handler (the hook the
//! coding agent uses for its `uncaughtException` handling), and stdin read errors reach the
//! stdin error handler.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use pi_tui::event_loop::{EventLoop, set_thread_panic_handler};

#[test]
fn event_loop_thread_panic_reaches_the_handler() {
    let (tx, rx) = mpsc::channel::<String>();
    let tx = std::sync::Mutex::new(tx);
    set_thread_panic_handler(Some(Arc::new(move |payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let _ = tx.lock().unwrap().send(message);
    })));

    let lp = EventLoop::new();
    let handle = lp.spawn();
    // A task the loop runs panics: nothing on that thread catches it.
    lp.post(|| panic!("boom from a key handler"));
    let message = rx.recv_timeout(Duration::from_secs(5)).expect("handler called");
    assert_eq!(message, "boom from a key handler");
    // The handler returned, so the thread ended normally instead of unwinding out.
    assert!(handle.join().is_ok());
    set_thread_panic_handler(None);

    // Without a handler the panic unwinds out of the thread as before.
    let lp = EventLoop::new();
    let handle = lp.spawn();
    lp.post(|| panic!("unhandled"));
    assert!(handle.join().is_err());
}

#[test]
fn stdin_errors_reach_the_stdin_error_handler() {
    let seen = Arc::new(std::sync::Mutex::new(None));
    let sink = seen.clone();
    assert!(!pi_tui::terminal::notify_stdin_error(&std::io::Error::from_raw_os_error(5)));
    pi_tui::terminal::set_stdin_error_handler(Some(Arc::new(move |error: &std::io::Error| {
        *sink.lock().unwrap() = error.raw_os_error();
    })));
    assert!(pi_tui::terminal::notify_stdin_error(&std::io::Error::from_raw_os_error(5)));
    assert_eq!(*seen.lock().unwrap(), Some(5));
    pi_tui::terminal::set_stdin_error_handler(None);
}
