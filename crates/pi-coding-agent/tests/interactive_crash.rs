//! Interactive-mode process-level behavior ported from `interactive-mode.ts`:
//!
//! - uncaught panics (TS `uncaughtException`/`unhandledRejection` → `uncaughtCrash`): restore the
//!   terminal, record the crash and exit with 1, while deliberately caught panics do nothing;
//! - stdin errors on a dead terminal (EIO/ENOTTY) exit with 129 (`emergencyTerminalExit`);
//! - extension shortcut contexts, where `shutdown()` only sets the shutdown flag;
//! - the abort text written into the shared assistant message, visible after a chat rebuild;
//! - `handleEvent` initializing the mode lazily.
//!
//! `process.exit()` is intercepted with the exit hook, which records the code and unwinds.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::types::{AssistantMessage, StopReason};
use pi_coding_agent::core::agent_session::AgentSessionEvent;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::core::crash_log::{CrashRecordKind, read_crash_log};
use pi_coding_agent::core::extensions::types::ExtensionMode;
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
use pi_coding_agent::utils::process_exit::set_exit_hook;
use pi_tui::terminal::Terminal;
use pi_tui::testing::VirtualTerminal;
use serde_json::json;
use support::*;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fixture {
    harness: Harness,
    mode: InteractiveMode,
    terminal: VirtualTerminal,
    agent_dir: std::path::PathBuf,
    exit_code: Arc<Mutex<Option<i32>>>,
}

impl Fixture {
    async fn new(init: bool) -> Fixture {
        let agent_dir = std::env::temp_dir().join(format!("pi-interactive-crash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent_dir).unwrap();
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
            std::env::set_var("PI_OFFLINE", "1");
            std::env::set_var("PI_SKIP_VERSION_CHECK", "1");
        }
        let harness = create_harness(HarnessOptions {
            settings: Some(json!({ "quietStartup": true })),
            ..Default::default()
        })
        .await
        .unwrap();
        let session = harness.session.clone();
        let services = AgentSessionServices {
            cwd: harness.session_manager.get_cwd(),
            agent_dir: agent_dir.to_string_lossy().into_owned(),
            model_runtime: harness.model_runtime.clone(),
            settings_manager: harness.settings_manager.clone(),
            resource_loader: session.resource_loader(),
            diagnostics: Vec::new(),
        };
        let factory: CreateAgentSessionRuntimeFactory =
            Arc::new(|_| Box::pin(async { Err(anyhow::anyhow!("session replacement is not supported in this test")) }));
        let runtime = Arc::new(AgentSessionRuntime::new(session, &services, &factory, None, None));
        let terminal = VirtualTerminal::new(100, 30);
        let mode = InteractiveMode::new(
            runtime,
            Some(&InteractiveModeOptions {
                terminal: Some(Arc::new(terminal.clone()) as Arc<dyn Terminal>),
                ..Default::default()
            }),
        );
        if init {
            mode.init().await.unwrap();
        }
        let exit_code = Arc::new(Mutex::new(None));
        let code = exit_code.clone();
        set_exit_hook(Some(Arc::new(move |c| {
            *code.lock() = Some(c);
            panic!("process.exit({c})");
        })));
        Fixture { harness, mode, terminal, agent_dir, exit_code }
    }

    async fn wait_until(&self, what: &str, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !predicate() {
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_exit(&self) -> i32 {
        self.wait_until("process.exit", || self.exit_code.lock().is_some()).await;
        self.exit_code.lock().unwrap()
    }

    fn crashes(&self) -> Vec<pi_coding_agent::core::crash_log::CrashRecord> {
        read_crash_log(Some(&self.agent_dir.join("crashes.json").to_string_lossy()))
    }

    fn cleanup(self) {
        set_exit_hook(None);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.mode.stop(None)));
        self.harness.cleanup();
        let _ = std::fs::remove_dir_all(&self.agent_dir);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uncaught_panic_in_a_detached_task_restores_the_terminal_records_the_crash_and_exits_1() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(true).await;
    assert!(!fx.terminal.is_cursor_visible(), "the TUI hides the cursor while running");

    fx.mode.test_hooks().spawn(async {
        panic!("boom in a detached task");
    });

    assert_eq!(fx.wait_for_exit().await, 1);
    // `ui.stop()` restored the terminal.
    assert!(fx.terminal.is_cursor_visible());
    let crashes = fx.crashes();
    assert_eq!(crashes.len(), 1);
    assert_eq!(crashes[0].kind, CrashRecordKind::UncaughtException);
    assert_eq!(crashes[0].message, "boom in a detached task");
    let stack = crashes[0].stack.clone().unwrap_or_default();
    assert!(stack.contains("panicked at"), "{stack}");
    assert!(stack.contains("interactive_crash.rs"), "{stack}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uncaught_panic_on_a_tui_thread_is_reported_too() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(true).await;

    // A panic escaping a pi-tui event-loop thread (key handlers, renders, timers run there).
    let lp = pi_tui::event_loop::EventLoop::new();
    let _thread = lp.spawn();
    lp.post(|| panic!("boom in a key handler"));

    assert_eq!(fx.wait_for_exit().await, 1);
    assert!(fx.terminal.is_cursor_visible());
    let crashes = fx.crashes();
    assert_eq!(crashes.len(), 1);
    assert_eq!(crashes[0].message, "boom in a key handler");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deliberately_caught_panics_do_not_crash() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(true).await;

    // `catch_unwind` (a TS try/catch).
    let caught = std::panic::catch_unwind(|| panic!("caught on purpose"));
    assert!(caught.is_err());
    // A task whose `JoinError` is inspected (an awaited promise whose rejection is handled).
    let joined = tokio::spawn(async { panic!("handled join error") }).await;
    assert!(joined.unwrap_err().is_panic());
    let blocking = tokio::task::spawn_blocking(|| panic!("handled blocking panic")).await;
    assert!(blocking.is_err());

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(*fx.exit_code.lock(), None);
    assert!(fx.crashes().is_empty());
    assert!(!fx.terminal.is_cursor_visible(), "the TUI keeps running");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dead_terminal_stdin_errors_exit_with_129_without_recording_a_crash() {
    let _lock = LOCK.lock().await;
    for code in [libc::EIO, libc::ENOTTY] {
        let fx = Fixture::new(true).await;
        // What `ProcessTerminal`'s stdin reader reports when a read fails.
        let _ = std::panic::catch_unwind(|| {
            pi_tui::terminal::notify_stdin_error(&std::io::Error::from_raw_os_error(code));
        });
        assert_eq!(*fx.exit_code.lock(), Some(129), "errno {code}");
        assert!(fx.crashes().is_empty());
        fx.cleanup();
    }

    // Other stdin errors are rethrown and end in the uncaught-exception handler.
    let fx = Fixture::new(true).await;
    fx.mode.test_hooks().handle_terminal_error(&std::io::Error::from_raw_os_error(libc::EBADF));
    assert_eq!(fx.wait_for_exit().await, 1);
    let crashes = fx.crashes();
    assert_eq!(crashes.len(), 1);
    assert_eq!(crashes[0].kind, CrashRecordKind::UncaughtException);
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dead_terminal_output_errors_exit_with_129_without_recording_a_crash() {
    // TS registers the same `terminalErrorHandler` on stdout and stderr.
    let _lock = LOCK.lock().await;
    for code in [libc::EIO, libc::EPIPE, libc::ENOTCONN] {
        let fx = Fixture::new(true).await;
        // What `ProcessTerminal` reports when a write to stdout/stderr fails.
        let _ = std::panic::catch_unwind(|| {
            assert!(pi_tui::terminal::notify_output_error(&std::io::Error::from_raw_os_error(code)));
        });
        assert_eq!(*fx.exit_code.lock(), Some(129), "errno {code}");
        assert!(fx.crashes().is_empty());
        fx.cleanup();
    }
    // Other write errors end in the uncaught-exception handler.
    let fx = Fixture::new(true).await;
    assert!(pi_tui::terminal::notify_output_error(&std::io::Error::from_raw_os_error(libc::ENOSPC)));
    assert_eq!(fx.wait_for_exit().await, 1);
    assert_eq!(fx.crashes().len(), 1);
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shortcut_context_shutdown_only_sets_the_shutdown_flag() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(true).await;
    let hooks = fx.mode.test_hooks();
    let ctx = hooks.shortcut_context();
    assert_eq!(ctx.mode().unwrap(), ExtensionMode::Tui);
    assert!(ctx.has_ui().unwrap());
    assert_eq!(ctx.cwd().unwrap(), fx.harness.session_manager.get_cwd());

    assert!(!hooks.shutdown_requested());
    ctx.shutdown().unwrap();
    assert!(hooks.shutdown_requested());
    // The session is idle, yet nothing shuts down until the next shutdown check.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(*fx.exit_code.lock(), None);
    fx.cleanup();
}

fn aborted_message(model: &pi_ai::types::Model, text: &str, error: &str) -> AssistantMessage {
    let mut message = faux_text_message(text);
    message.api = model.api.clone();
    message.provider = model.provider.clone();
    message.model = model.id.clone();
    message.stop_reason = StopReason::Aborted;
    message.error_message = Some(error.to_string());
    message
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abort_text_is_written_into_the_message_and_survives_a_chat_rebuild() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(true).await;
    // A stream that hangs until the request is aborted.
    fx.harness.session.agent().set_stream_fn(Some(Arc::new(move |model, _context, options| {
        let (sender, stream) = pi_ai::AssistantMessageEventStream::new();
        let mut start = aborted_message(&model, "", "");
        start.stop_reason = StopReason::Stop;
        start.error_message = None;
        let aborted = aborted_message(&model, "partial answer", "Request aborted by user");
        let signal = options.signal.clone();
        tokio::spawn(async move {
            sender.push(pi_ai::AssistantMessageEvent::Start { partial: start });
            if let Some(signal) = signal {
                signal.cancelled().await;
            }
            sender.push(pi_ai::AssistantMessageEvent::Error { reason: StopReason::Aborted, error: aborted });
        });
        Box::pin(async move { Ok(stream) })
    })));

    // Prompts reach the agent through the main loop (`run`), like in the real binary.
    let mode = fx.mode.clone();
    let run = tokio::spawn(async move { mode.run().await });
    fx.mode.submit("hello");
    let session = fx.harness.session.clone();
    fx.wait_until("streaming", || session.is_streaming()).await;
    fx.harness.session.abort().await.unwrap();
    fx.harness.session.wait_for_idle().await.unwrap();

    // The persisted entry and agent state carry the text the UI showed.
    let assistant = fx
        .harness
        .session_manager
        .get_entries()
        .into_iter()
        .find_map(|entry| match entry {
            SessionEntry::Message(m) => match m.message {
                AgentMessage::Assistant(a) => Some(a),
                _ => None,
            },
            _ => None,
        })
        .expect("assistant entry");
    let state_message = fx.harness.session.agent().messages().into_iter().rev().find_map(|m| match m {
        AgentMessage::Assistant(a) => Some(a),
        _ => None,
    });
    let hooks = fx.mode.test_hooks();
    hooks.rebuild_chat_from_messages();
    let chat = hooks.render_chat(100).join("\n");
    // Assert after cleanup: while the mode is running, panic messages stay off the terminal.
    run.abort();
    fx.cleanup();

    assert_eq!(assistant.stop_reason, StopReason::Aborted);
    assert_eq!(assistant.error_message.as_deref(), Some("Operation aborted"));
    assert_eq!(state_message.and_then(|a| a.error_message).as_deref(), Some("Operation aborted"));
    assert!(chat.contains("Operation aborted"), "{chat}");
    assert!(!chat.contains("Request aborted by user"), "{chat}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handle_event_initializes_the_mode_first() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(false).await;
    let hooks = fx.mode.test_hooks();
    assert!(!hooks.is_initialized());
    hooks.handle_event(AgentSessionEvent::AgentStart);
    fx.wait_until("init", || hooks.is_initialized()).await;
    fx.cleanup();
}
