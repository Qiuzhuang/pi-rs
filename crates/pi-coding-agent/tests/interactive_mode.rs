//! End-to-end tests of `InteractiveMode` against pi-tui's `VirtualTerminal` and the faux provider.
//!
//! The TypeScript interactive-mode tests call private methods on fake `this` objects; Rust cannot do
//! that, so these drive a real `InteractiveMode` (real `AgentSession`, real TUI) through its public
//! hooks and assert on the rendered viewport. Covered: startup + prompt + streaming response
//! (interactive-tui.test.ts), status coalescing (interactive-mode-status.test.ts), `/clone` with an
//! empty session (interactive-mode-clone-command.test.ts) and `/import` of a missing file
//! (interactive-mode-import-command.test.ts).

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
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
}

impl Fixture {
    async fn new() -> Fixture {
        let agent_dir = std::env::temp_dir().join(format!("pi-interactive-agent-{}", uuid::Uuid::new_v4()));
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
        mode.init().await.unwrap();
        Fixture { harness, mode, terminal, agent_dir }
    }

    fn screen(&self) -> String {
        self.terminal.get_viewport().join("\n")
    }

    /// Wait until the viewport satisfies `predicate`, rendering as needed.
    async fn wait_for(&self, what: &str, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.mode.tui().request_render(false);
            tokio::time::sleep(Duration::from_millis(50)).await;
            let screen = self.screen();
            if predicate(&screen) {
                return screen;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}; screen:\n{screen}");
            }
        }
    }

    fn cleanup(self) {
        self.mode.stop(None);
        self.harness.cleanup();
        let _ = std::fs::remove_dir_all(&self.agent_dir);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_submits_a_prompt_and_streams_the_response() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new().await;
    fx.wait_for("the editor", |s| s.contains('─')).await;

    fx.harness.set_responses(vec![faux_text_message("Hello from the faux provider").into()]);
    // Prompts reach the agent through the main loop (`run`), like in the real binary.
    let mode = fx.mode.clone();
    let run = tokio::spawn(async move { mode.run().await });
    fx.mode.submit("say hello");
    let screen = fx
        .wait_for("the streamed response", |s| s.contains("Hello from the faux provider"))
        .await;
    assert!(screen.contains("say hello"), "user message missing:\n{screen}");
    assert_eq!(fx.harness.user_texts(), vec!["say hello".to_string()]);
    run.abort();
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn coalesces_immediately_sequential_status_messages() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new().await;
    fx.mode.show_status("STATUS_ONE");
    fx.wait_for("first status", |s| s.contains("STATUS_ONE")).await;
    fx.mode.show_status("STATUS_TWO");
    let screen = fx.wait_for("second status", |s| s.contains("STATUS_TWO")).await;
    assert!(!screen.contains("STATUS_ONE"), "status lines were not coalesced:\n{screen}");

    // Something else in between: the next status gets its own line.
    fx.mode.show_warning("WARNING_BETWEEN");
    fx.mode.show_status("STATUS_THREE");
    let screen = fx.wait_for("third status", |s| s.contains("STATUS_THREE")).await;
    assert!(screen.contains("STATUS_TWO") && screen.contains("WARNING_BETWEEN"), "{screen}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clone_shows_a_status_message_when_there_is_nothing_to_clone() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new().await;
    fx.mode.submit("/clone");
    fx.wait_for("clone status", |s| s.contains("Nothing to clone yet")).await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn import_shows_a_non_fatal_error_when_the_path_does_not_exist() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new().await;
    fx.mode.submit("/import '/definitely/missing/session.jsonl'");
    // The path is unquoted before the confirmation dialog shows it.
    fx.wait_for("import confirmation", |s| s.contains("Replace current session with /definitely/missing/session.jsonl?"))
        .await;
    fx.terminal.send_input("\r");
    fx.wait_for("import error", |s| s.contains("Failed to import session")).await;
    fx.cleanup();
}
