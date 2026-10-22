//! Freeze/crash regressions of the interactive mode, driven through the real input path
//! (`VirtualTerminal::send_input` → TUI event loop → focused component), with a deadlock detector:
//! after each step the TUI event loop must run a posted task and the mode's state lock must be
//! free within a timeout. A deadlocked step fails the test instead of hanging it.
//!
//! Covered:
//! - `/settings` (idle and while a response streams): building the settings config used to hold a
//!   temporary `state` guard across `tui_mode()`, which locks `state` again (self-deadlock).
//! - typing slash commands, arrow keys, Tab, Escape and `@` completions while the agent streams and
//!   runs tools.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pi_ai::providers::faux::{FauxMessageOptions, FauxResponseStep, faux_assistant_message, faux_text, faux_tool_call};
use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
use pi_tui::terminal::Terminal;
use pi_tui::testing::VirtualTerminal;
use serde_json::json;
use support::*;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const DEADLOCK_TIMEOUT: Duration = Duration::from_secs(10);

struct Fixture {
    harness: Harness,
    mode: InteractiveMode,
    terminal: VirtualTerminal,
    agent_dir: std::path::PathBuf,
    run: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl Fixture {
    async fn new(settings: serde_json::Value) -> Fixture {
        let agent_dir = std::env::temp_dir().join(format!("pi-interactive-freeze-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent_dir).unwrap();
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
            std::env::set_var("PI_OFFLINE", "1");
            std::env::set_var("PI_SKIP_VERSION_CHECK", "1");
        }
        let harness = create_harness(HarnessOptions { settings: Some(settings), ..Default::default() }).await.unwrap();
        std::fs::write(std::path::Path::new(&harness.session_manager.get_cwd()).join("hello.txt"), "hello\n").unwrap();
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
        mode.init().await.unwrap();
        // Prompts reach the agent through the main loop, like in the real binary.
        let m = mode.clone();
        let run = Some(tokio::spawn(async move { m.run().await }));
        let fx = Fixture { harness, mode, terminal, agent_dir, run };
        fx.wait_for("the editor", |s| s.contains('─')).await;
        fx
    }

    fn screen(&self) -> String {
        self.terminal.get_viewport().join("\n")
    }

    async fn wait_for(&self, what: &str, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.assert_alive(what).await;
            self.mode.tui().request_render(false);
            tokio::time::sleep(Duration::from_millis(30)).await;
            let screen = self.screen();
            if predicate(&screen) {
                return screen;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}; screen:\n{screen}");
            }
        }
    }

    /// Deadlock detector: the TUI event loop runs a posted task and the mode's state lock can be
    /// taken, both within [`DEADLOCK_TIMEOUT`].
    async fn assert_alive(&self, after: &str) {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        self.mode.tui().event_loop().post(move || {
            let _ = tx.send(());
        });
        let hooks = self.mode.test_hooks();
        let (tx2, rx2) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let _ = hooks.is_initialized();
            let _ = tx2.send(());
        });
        let deadline = Instant::now() + DEADLOCK_TIMEOUT;
        let (mut loop_ok, mut state_ok) = (false, false);
        while !(loop_ok && state_ok) {
            loop_ok |= rx.try_recv().is_ok();
            state_ok |= rx2.try_recv().is_ok();
            if Instant::now() > deadline {
                // A panic would not end the test: dropping the runtime waits for the deadlocked
                // worker threads forever. Report and exit instead.
                // Written to the real stderr: the harness's output capture dies with the process.
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr(),
                    "deadlock after {after}: TUI event loop {}, interactive state lock {}; screen:\n{}",
                    if loop_ok { "alive" } else { "STUCK" },
                    if state_ok { "free" } else { "STUCK" },
                    self.screen()
                );
                std::process::exit(101);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Type `data` through the terminal (each call is one input chunk) and check for deadlocks.
    async fn input(&self, data: &str) {
        self.terminal.send_input(data);
        self.assert_alive(&format!("input {data:?}")).await;
    }

    async fn type_text(&self, text: &str) {
        for ch in text.chars() {
            self.terminal.send_input(&ch.to_string());
        }
        self.assert_alive(&format!("typing {text:?}")).await;
    }

    fn cleanup(mut self) {
        if let Some(run) = self.run.take() {
            run.abort();
        }
        self.mode.stop(None);
        self.harness.cleanup();
        let _ = std::fs::remove_dir_all(&self.agent_dir);
    }
}

const ESC: &str = "\x1b";
const ENTER: &str = "\r";
const TAB: &str = "\t";
const UP: &str = "\x1b[A";
const DOWN: &str = "\x1b[B";
const BACKSPACE: &str = "\x7f";
const CTRL_U: &str = "\x15";

/// A response that waits `delay` before arriving (the agent is streaming meanwhile).
fn slow(delay: Duration, message: pi_ai::types::AssistantMessage) -> FauxResponseStep {
    FauxResponseStep::Factory(Arc::new(move |_, _, _, _| {
        let message = message.clone();
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(message)
        })
    }))
}

fn tool_turn(command: &str) -> pi_ai::types::AssistantMessage {
    let mut args = serde_json::Map::new();
    args.insert("command".into(), json!(command));
    let mut read_args = serde_json::Map::new();
    read_args.insert("path".into(), json!("hello.txt"));
    faux_assistant_message(
        vec![faux_text("Running tools."), faux_tool_call("bash", args, None), faux_tool_call("read", read_args, None)],
        FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_command_opens_navigates_and_closes_without_deadlock() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(json!({ "quietStartup": true })).await;

    fx.type_text("/settings").await;
    fx.input(ENTER).await;
    fx.wait_for("the settings selector", |s| s.contains("Esc to cancel")).await;
    for _ in 0..5 {
        fx.input(DOWN).await;
    }
    fx.input(UP).await;
    fx.input(ESC).await;
    fx.wait_for("the editor after closing settings", |s| !s.contains("Esc to cancel")).await;
    // The editor is focused again and accepts input.
    fx.type_text("still alive").await;
    fx.wait_for("typed text", |s| s.contains("still alive")).await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slash_commands_and_completions_while_streaming_and_running_tools() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(json!({ "quietStartup": true })).await;
    fx.harness.set_responses(vec![
        slow(Duration::from_millis(300), tool_turn("for i in 1 2 3; do echo line $i; sleep 0.2; done")),
        slow(Duration::from_millis(300), tool_turn("sleep 0.5; echo done")),
        slow(Duration::from_millis(500), faux_assistant_message(vec![faux_text("All done.")], FauxMessageOptions::default())),
    ]);

    fx.type_text("please run tools").await;
    fx.input(ENTER).await;
    let started = Instant::now();
    let mut round = 0;
    while started.elapsed() < Duration::from_secs(3) || fx.harness.get_pending_response_count() > 0 {
        round += 1;
        fx.type_text("/").await;
        fx.type_text("mo").await;
        fx.input(DOWN).await;
        fx.input(UP).await;
        fx.input(TAB).await;
        fx.input(BACKSPACE).await;
        fx.input(CTRL_U).await;
        fx.type_text("/model ").await;
        fx.input(DOWN).await;
        fx.input(CTRL_U).await;
        fx.type_text("look at @hel").await;
        fx.input(TAB).await;
        fx.input(CTRL_U).await;
        fx.type_text("/").await;
        fx.input(ESC).await;
        fx.input(CTRL_U).await;
        assert!(round < 500, "the scripted responses never finished");
    }
    fx.wait_for("the final answer", |s| s.contains("All done.")).await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_selector_while_streaming() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(json!({ "quietStartup": true })).await;
    fx.harness.set_responses(vec![
        slow(Duration::from_millis(200), tool_turn("sleep 0.5; echo done")),
        slow(Duration::from_millis(800), faux_assistant_message(vec![faux_text("All done.")], FauxMessageOptions::default())),
    ]);
    fx.type_text("please run tools").await;
    fx.input(ENTER).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    fx.type_text("/settings").await;
    fx.input(ENTER).await;
    fx.wait_for("the settings selector", |s| s.contains("Esc to cancel")).await;
    fx.input(DOWN).await;
    fx.input(ENTER).await;
    fx.input(ESC).await;
    fx.wait_for("the editor after closing settings", |s| !s.contains("Esc to cancel")).await;
    fx.wait_for("the final answer", |s| s.contains("All done.")).await;
    fx.cleanup();
}
