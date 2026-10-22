//! Model and thinking-level persistence in interactive mode, driven through the real `InteractiveMode`
//! on pi-tui's `VirtualTerminal` with reasoning faux models.
//!
//! Matches pi 1.3.0: `Enter` in `/model` and `/thinking` applies the selection to the session only (a
//! `model_change` / `thinking_level_change` entry), `Ctrl+S` (`app.models.save` / `app.thinking.save`)
//! also saves it as the default for new sessions (`defaultProvider`/`defaultModel` /
//! `defaultThinkingLevel`), and the cycling keys (`Ctrl+P`, `Shift+Tab`) never touch the defaults.
//! The on-disk side (settings.json, `pi -c`, `--resume`) is covered by `tests/model_persistence.rs`.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pi_ai::providers::faux::FauxModelDefinition;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
use pi_tui::terminal::Terminal;
use pi_tui::testing::VirtualTerminal;
use serde_json::{Value, json};
use support::*;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const ENTER: &str = "\r";
const DOWN: &str = "\x1b[B";
const CTRL_S: &str = "\x13";
const CTRL_P: &str = "\x10";
const SHIFT_TAB: &str = "\x1b[Z";
/// Ctrl+S as the kitty keyboard protocol encodes it (CSI 115;5u).
const CTRL_S_KITTY: &str = "\x1b[115;5u";

struct Fixture {
    harness: Harness,
    mode: InteractiveMode,
    terminal: VirtualTerminal,
    agent_dir: std::path::PathBuf,
    run: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl Fixture {
    async fn new(settings: Value) -> Fixture {
        let agent_dir = std::env::temp_dir().join(format!("pi-model-persistence-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent_dir).unwrap();
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
            std::env::set_var("PI_OFFLINE", "1");
            std::env::set_var("PI_SKIP_VERSION_CHECK", "1");
        }
        let mut merged = json!({ "quietStartup": true, "retry": { "enabled": false } });
        if let Value::Object(extra) = settings {
            for (k, v) in extra {
                merged[k] = v;
            }
        }
        let models = ["alpha", "beta", "gamma"]
            .iter()
            .map(|id| FauxModelDefinition {
                id: id.to_string(),
                name: Some(id.to_string()),
                reasoning: Some(true),
                ..Default::default()
            })
            .collect();
        let harness = create_harness(HarnessOptions { models, settings: Some(merged), ..Default::default() })
            .await
            .unwrap();
        harness.session.set_thinking_level(pi_agent::types::ThinkingLevel::Medium, None);
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
        let terminal = VirtualTerminal::new(110, 40);
        let mode = InteractiveMode::new(
            runtime,
            Some(&InteractiveModeOptions {
                terminal: Some(Arc::new(terminal.clone()) as Arc<dyn Terminal>),
                ..Default::default()
            }),
        );
        mode.init().await.unwrap();
        let mut fx = Fixture { harness, mode, terminal, agent_dir, run: None };
        let mode = fx.mode.clone();
        fx.run = Some(tokio::spawn(async move { mode.run().await }));
        fx.wait_for("the editor", |s| s.contains('─')).await;
        fx
    }

    fn viewport(&self) -> String {
        self.terminal.get_viewport().join("\n")
    }

    async fn wait_for(&self, what: &str, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.mode.tui().request_render(false);
            tokio::time::sleep(Duration::from_millis(30)).await;
            let screen = self.viewport();
            if predicate(&screen) {
                return screen;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}; screen:\n{screen}");
            }
        }
    }

    /// The footer's model/thinking indicator (last line of the viewport).
    fn footer(&self) -> String {
        self.terminal.get_viewport().iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default()
    }

    async fn wait_footer(&self, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.mode.tui().request_render(false);
            tokio::time::sleep(Duration::from_millis(30)).await;
            if self.footer().trim_end().ends_with(expected) {
                return;
            }
            assert!(Instant::now() < deadline, "footer never showed {expected:?}; screen:\n{}", self.viewport());
        }
    }

    async fn open_model_selector(&self) {
        self.mode.submit("/model");
        self.wait_for("the model selector", |s| s.contains("to set as default")).await;
    }

    async fn open_thinking_selector(&self) {
        self.mode.submit("/thinking");
        self.wait_for("the thinking selector", |s| s.contains("Thinking Level")).await;
    }

    fn defaults(&self) -> (Option<String>, Option<String>, Option<String>) {
        let settings = &self.harness.settings_manager;
        (
            settings.get_default_provider(),
            settings.get_default_model(),
            settings.get_default_thinking_level().map(|l| l.as_str().to_string()),
        )
    }

    /// `model_change` / `thinking_level_change` entries of the session, as `model:<p>/<id>` and
    /// `thinking:<level>`.
    fn selection_entries(&self) -> Vec<String> {
        self.harness
            .session_manager
            .get_entries()
            .into_iter()
            .filter_map(|entry| {
                let value = serde_json::to_value(&entry).unwrap();
                match value["type"].as_str()? {
                    "model_change" => Some(format!(
                        "model:{}/{}",
                        value["provider"].as_str()?,
                        value["modelId"].as_str()?
                    )),
                    "thinking_level_change" => Some(format!("thinking:{}", value["thinkingLevel"].as_str()?)),
                    _ => None,
                }
            })
            .collect()
    }

    fn cleanup(self) {
        if let Some(run) = self.run {
            run.abort();
        }
        self.mode.stop(None);
        self.harness.cleanup();
        let _ = std::fs::remove_dir_all(&self.agent_dir);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_selector_enter_applies_to_the_session_only() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(json!({})).await;
    fx.wait_footer("alpha • medium").await;
    fx.open_model_selector().await;
    let screen = fx.viewport();
    assert!(
        screen.contains("Enter to select · Ctrl+S to set as default · Escape/Ctrl+C to cancel"),
        "the selector hint names the save key:\n{screen}"
    );
    // Filter rather than navigate: provider credentials in the environment can add other models.
    fx.terminal.send_input("beta");
    fx.terminal.send_input(ENTER);
    fx.wait_for("the status", |s| s.contains("Model: beta")).await;
    fx.wait_footer("beta • medium").await;
    assert_eq!(fx.defaults(), (None, None, None));
    assert_eq!(fx.selection_entries().last().map(String::as_str), Some("model:faux/beta"));
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_selector_ctrl_s_saves_the_default_model() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(json!({})).await;
    fx.open_model_selector().await;
    fx.terminal.send_input("gamma");
    fx.terminal.send_input(CTRL_S);
    fx.wait_for("the status", |s| s.contains("Default model: faux/gamma")).await;
    fx.wait_footer("gamma • medium").await;
    // Saving the model does not rewrite the thinking default.
    assert_eq!(fx.defaults(), (Some("faux".into()), Some("gamma".into()), None));
    assert_eq!(fx.selection_entries().last().map(String::as_str), Some("model:faux/gamma"));

    // The kitty keyboard protocol encoding of Ctrl+S saves too.
    fx.open_model_selector().await;
    fx.terminal.send_input("beta");
    fx.terminal.send_input(CTRL_S_KITTY);
    fx.wait_for("the status", |s| s.contains("Default model: faux/beta")).await;
    assert_eq!(fx.defaults(), (Some("faux".into()), Some("beta".into()), None));
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn thinking_selector_enter_is_session_only_and_ctrl_s_saves_the_default() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(json!({})).await;
    fx.open_thinking_selector().await;
    let screen = fx.viewport();
    assert!(
        screen.contains("Enter to select · Ctrl+S to set as default · Escape/Ctrl+C to cancel"),
        "the selector hint names the save key:\n{screen}"
    );
    assert!(screen.contains("Shift+Tab cycles thinking levels in-session"), "{screen}");
    // medium -> high, Enter: session only.
    fx.terminal.send_input(DOWN);
    fx.terminal.send_input(ENTER);
    fx.wait_for("the status", |s| s.contains("Thinking level: high")).await;
    fx.wait_footer("alpha • high").await;
    assert_eq!(fx.defaults(), (None, None, None));
    assert_eq!(fx.selection_entries().last().map(String::as_str), Some("thinking:high"));

    // high -> off (wraps), Ctrl+S: saved as the default and applied.
    fx.open_thinking_selector().await;
    fx.terminal.send_input("off");
    fx.terminal.send_input(CTRL_S);
    fx.wait_for("the status", |s| s.contains("Default thinking level: off")).await;
    fx.wait_footer("alpha • thinking off").await;
    assert_eq!(fx.defaults(), (None, None, Some("off".into())));
    assert_eq!(fx.selection_entries().last().map(String::as_str), Some("thinking:off"));
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cycling_keys_never_save_defaults() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(json!({ "modelThinkingLevels": { "faux/beta": "low" } })).await;
    fx.terminal.send_input(CTRL_P);
    // The per-model level applies when switching to beta.
    fx.wait_footer("beta • low").await;
    fx.terminal.send_input(SHIFT_TAB);
    fx.wait_footer("beta • medium").await;
    fx.terminal.send_input(CTRL_P);
    // No per-model level and no saved default: the current level carries over.
    fx.wait_footer("gamma • medium").await;
    assert_eq!(fx.defaults(), (None, None, None));
    assert_eq!(
        fx.harness.settings_manager.get_model_thinking_level("faux", "beta").map(|l| l.as_str().to_string()),
        Some("low".into())
    );
    let entries = fx.selection_entries();
    assert!(
        entries.ends_with(&["model:faux/beta", "thinking:low", "thinking:medium", "model:faux/gamma"].map(String::from)),
        "{entries:?}"
    );
    fx.cleanup();
}
