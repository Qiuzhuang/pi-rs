//! Ports of `interactive-mode-*.test.ts` cases that can run against the real `InteractiveMode` on
//! pi-tui's `VirtualTerminal` with the faux provider. The TypeScript tests call private methods on
//! fake `this` objects; these drive the same code paths end to end and assert on the screen:
//!
//! - `interactive-mode-extension-dialogs.test.ts`: stacked confirms, aborted hidden dialogs, stacked
//!   inline custom components, a confirm opened from a custom component.
//! - `interactive-mode-anthropic-warning.test.ts`: subscription auth warnings.
//! - `interactive-mode-assistant-diagnostics.test.ts`: Anthropic thinking-drop notices.
//! - `interactive-mode-bug-report-hint.test.ts`: `/bug` hints after errors.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use pi_ai::auth::types::{CredentialStore, Credential};
use pi_ai::providers::faux::{FauxModelDefinition, FauxResponseStep};
use pi_ai::types::AssistantMessage;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::core::extensions::types::{
    CommandOptions, CustomUiOptions, CustomUiResult, ExtensionAPIRef, ExtensionFactory, ExtensionUIContext,
    ExtensionUIDialogOptions, custom_typed,
};
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
use pi_tui::components::text::Text;
use pi_tui::terminal::Terminal;
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, ComponentRef};
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const ESC: &str = "\x1b";
const ENTER: &str = "\r";

type UiSlot = Arc<Mutex<Option<Arc<dyn ExtensionUIContext>>>>;

/// An extension with a `/capture-ui` command that hands the command context's UI to the test.
fn capture_ui_extension(slot: UiSlot) -> TestExtension {
    let factory: ExtensionFactory = Arc::new(move |pi: ExtensionAPIRef| {
        let slot = slot.clone();
        Box::pin(async move {
            pi.register_command(
                "capture-ui",
                CommandOptions {
                    description: Some("Capture the UI context".into()),
                    get_argument_completions: None,
                    handler: Arc::new(move |_args, ctx| {
                        let slot = slot.clone();
                        Box::pin(async move {
                            *slot.lock() = Some(ctx.ui()?);
                            Ok(())
                        })
                    }),
                },
            )?;
            Ok(())
        })
    });
    TestExtension::new(factory)
}

#[derive(Default)]
struct FixtureOptions {
    provider: Option<String>,
    models: Vec<FauxModelDefinition>,
    settings: Option<Value>,
    extensions: Vec<TestExtension>,
    credential: Option<Value>,
}

struct Fixture {
    harness: Harness,
    mode: InteractiveMode,
    terminal: VirtualTerminal,
    agent_dir: std::path::PathBuf,
    run: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl Fixture {
    async fn new(options: FixtureOptions) -> Fixture {
        let agent_dir = std::env::temp_dir().join(format!("pi-interactive-ported-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent_dir).unwrap();
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
            std::env::set_var("PI_OFFLINE", "1");
            std::env::set_var("PI_SKIP_VERSION_CHECK", "1");
        }
        let mut settings = json!({ "quietStartup": true, "retry": { "enabled": false } });
        if let Some(Value::Object(extra)) = options.settings {
            for (k, v) in extra {
                settings[k] = v;
            }
        }
        let harness = create_harness(HarnessOptions {
            provider: options.provider,
            models: options.models,
            settings: Some(settings),
            extension_factories: options.extensions,
            ..Default::default()
        })
        .await
        .unwrap();
        if let Some(credential) = options.credential {
            let provider = harness.get_model().provider.to_string();
            let credential: Credential = serde_json::from_value(credential).unwrap();
            harness
                .auth_storage
                .modify(&provider, Box::new(move |_| Box::pin(async move { Ok(Some(credential)) })), None)
                .await
                .unwrap();
        }
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
        let terminal = VirtualTerminal::new(100, 40);
        let mode = InteractiveMode::new(
            runtime,
            Some(&InteractiveModeOptions {
                terminal: Some(Arc::new(terminal.clone()) as Arc<dyn Terminal>),
                ..Default::default()
            }),
        );
        mode.init().await.unwrap();
        let fx = Fixture { harness, mode, terminal, agent_dir, run: None };
        fx.wait_for("the editor", |s| s.contains('─')).await;
        fx
    }

    /// Start the main loop (`InteractiveMode::run`), which handles startup warnings and prompts.
    async fn start(&mut self) {
        let mode = self.mode.clone();
        self.run = Some(tokio::spawn(async move { mode.run().await }));
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    fn screen(&self) -> String {
        self.terminal.get_scroll_buffer().join("\n")
    }

    fn viewport(&self) -> String {
        self.terminal.get_viewport().join("\n")
    }

    async fn wait_for(&self, what: &str, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
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

    /// Let pending renders and spawned tasks settle.
    async fn settle(&self) -> String {
        for _ in 0..5 {
            self.mode.tui().request_render(false);
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        self.screen()
    }

    async fn type_text(&self, text: &str) {
        for ch in text.chars() {
            self.terminal.send_input(&ch.to_string());
        }
        self.terminal.send_input(ENTER);
    }

    async fn capture_ui(&mut self, slot: &UiSlot) -> Arc<dyn ExtensionUIContext> {
        if self.run.is_none() {
            self.start().await;
        }
        self.mode.submit("/capture-ui");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(ui) = slot.lock().clone() {
                return ui;
            }
            assert!(Instant::now() < deadline, "the capture command did not run:\n{}", self.screen());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
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

fn spawn_confirm(
    ui: &Arc<dyn ExtensionUIContext>,
    title: &'static str,
    message: &'static str,
    signal: Option<CancellationToken>,
) -> tokio::task::JoinHandle<bool> {
    let ui = ui.clone();
    tokio::spawn(async move { ui.confirm(title, message, ExtensionUIDialogOptions { signal, timeout: None }).await })
}

async fn resolved<T>(handle: tokio::task::JoinHandle<T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), handle).await.expect("dialog did not resolve").unwrap()
}

// -------------------------------------------------------------------------------------------------
// interactive-mode-extension-dialogs.test.ts
// -------------------------------------------------------------------------------------------------

// Regression for #11166: a second dialog replaced the first one and the first promise never settled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shows_the_earlier_confirm_again_after_the_later_one_is_answered() {
    let _lock = LOCK.lock().await;
    let slot = UiSlot::default();
    let mut fx = Fixture::new(FixtureOptions { extensions: vec![capture_ui_extension(slot.clone())], ..Default::default() }).await;
    let ui = fx.capture_ui(&slot).await;

    let first = spawn_confirm(&ui, "First", "first dialog", None);
    fx.wait_for("first", |_| true).await;
    let second = spawn_confirm(&ui, "Second", "second dialog", None);
    fx.wait_for("the second dialog", |s| s.contains("second dialog")).await;
    assert!(!fx.viewport().contains("first dialog"), "{}", fx.viewport());

    fx.terminal.send_input(ESC);
    assert!(!resolved(second).await);
    fx.wait_for("the first dialog again", |_| fx.viewport().contains("first dialog")).await;

    fx.terminal.send_input(ENTER);
    assert!(resolved(first).await);
    let screen = fx.settle().await;
    assert!(!fx.viewport().contains("first dialog"), "{screen}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removes_a_hidden_dialog_when_its_signal_aborts() {
    let _lock = LOCK.lock().await;
    let slot = UiSlot::default();
    let mut fx = Fixture::new(FixtureOptions { extensions: vec![capture_ui_extension(slot.clone())], ..Default::default() }).await;
    let ui = fx.capture_ui(&slot).await;

    let controller = CancellationToken::new();
    let first = spawn_confirm(&ui, "First", "first dialog", Some(controller.clone()));
    fx.wait_for("first", |_| true).await;
    let second = spawn_confirm(&ui, "Second", "second dialog", None);
    fx.wait_for("the second dialog", |_| fx.viewport().contains("second dialog")).await;

    controller.cancel();
    assert!(!resolved(first).await);
    fx.settle().await;
    assert!(fx.viewport().contains("second dialog"), "{}", fx.viewport());

    fx.terminal.send_input(ENTER);
    assert!(resolved(second).await);
    fx.settle().await;
    assert!(!fx.viewport().contains("dialog"), "{}", fx.viewport());
    fx.cleanup();
}

/// A custom component whose `done` callback the test keeps.
fn open_custom(
    ui: &Arc<dyn ExtensionUIContext>,
    label: &'static str,
    dones: Arc<Mutex<Vec<pi_coding_agent::core::extensions::types::CustomUiDone>>>,
) -> tokio::task::JoinHandle<anyhow::Result<String>> {
    let ui = ui.clone();
    tokio::spawn(async move {
        let factory: pi_coding_agent::core::extensions::types::CustomComponentFactory =
            Arc::new(move |_tui, _theme, _keybindings, done| {
                dones.lock().push(done);
                Box::pin(async move { Ok(Arc::new(Text::new(label, 1, 0, None)) as ComponentRef) })
            });
        custom_typed::<String>(ui.as_ref(), factory, CustomUiOptions::default()).await
    })
}

fn finish(done: &pi_coding_agent::core::extensions::types::CustomUiDone, value: &str) {
    done(Box::new(value.to_string()) as CustomUiResult);
}

// Regression for #9909: a second inline custom component replaced the first one and the first promise never settled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stacks_concurrent_inline_custom_components() {
    let _lock = LOCK.lock().await;
    let slot = UiSlot::default();
    let mut fx = Fixture::new(FixtureOptions { extensions: vec![capture_ui_extension(slot.clone())], ..Default::default() }).await;
    let ui = fx.capture_ui(&slot).await;

    let dones: Arc<Mutex<Vec<_>>> = Arc::default();
    let first = open_custom(&ui, "custom one", dones.clone());
    fx.wait_for("custom one", |_| fx.viewport().contains("custom one")).await;
    let second = open_custom(&ui, "custom two", dones.clone());
    fx.wait_for("custom two", |_| fx.viewport().contains("custom two")).await;
    assert_eq!(dones.lock().len(), 2);
    assert!(!fx.viewport().contains("custom one"), "{}", fx.viewport());

    let done_two = dones.lock()[1].clone();
    finish(&done_two, "two");
    assert_eq!(resolved(second).await.unwrap(), "two");
    fx.wait_for("custom one again", |_| fx.viewport().contains("custom one")).await;

    let done_one = dones.lock()[0].clone();
    finish(&done_one, "one");
    assert_eq!(resolved(first).await.unwrap(), "one");
    fx.settle().await;
    assert!(!fx.viewport().contains("custom one"), "{}", fx.viewport());
    fx.cleanup();
}

/// A custom form that opens a confirm when it receives input.
struct DiscardForm {
    runtime: tokio::runtime::Handle,
    ui: Arc<dyn ExtensionUIContext>,
    confirmed: Arc<Mutex<Option<tokio::task::JoinHandle<bool>>>>,
    text: Text,
}

impl Component for DiscardForm {
    fn render(&self, width: usize) -> Vec<String> {
        self.text.render(width)
    }

    fn invalidate(&self) {
        self.text.invalidate();
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_input(&self, _data: &str) {
        // Input arrives on the terminal's input thread; open the dialog on the test runtime.
        let _guard = self.runtime.enter();
        *self.confirmed.lock() = Some(spawn_confirm(&self.ui, "Discard?", "discard changes", None));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shows_a_confirm_opened_from_an_inline_custom_component_and_then_restores_the_component() {
    let _lock = LOCK.lock().await;
    let slot = UiSlot::default();
    let mut fx = Fixture::new(FixtureOptions { extensions: vec![capture_ui_extension(slot.clone())], ..Default::default() }).await;
    let ui = fx.capture_ui(&slot).await;

    let confirmed: Arc<Mutex<Option<tokio::task::JoinHandle<bool>>>> = Arc::default();
    let done: Arc<Mutex<Option<pi_coding_agent::core::extensions::types::CustomUiDone>>> = Arc::default();
    let custom = {
        let ui = ui.clone();
        let confirmed = confirmed.clone();
        let done = done.clone();
        tokio::spawn(async move {
            let form_ui = ui.clone();
            let factory: pi_coding_agent::core::extensions::types::CustomComponentFactory =
                Arc::new(move |_tui, _theme, _keybindings, finish| {
                    *done.lock() = Some(finish);
                    let form = DiscardForm {
                        runtime: tokio::runtime::Handle::current(),
                        ui: form_ui.clone(),
                        confirmed: confirmed.clone(),
                        text: Text::new("custom form", 1, 0, None),
                    };
                    Box::pin(async move { Ok(Arc::new(form) as ComponentRef) })
                });
            custom_typed::<String>(ui.as_ref(), factory, CustomUiOptions::default()).await
        })
    };
    fx.wait_for("the custom form", |_| fx.viewport().contains("custom form")).await;

    fx.terminal.send_input("x");
    fx.wait_for("the confirm", |_| fx.viewport().contains("Discard?")).await;
    fx.terminal.send_input(ESC);
    let confirm = confirmed.lock().take().unwrap();
    assert!(!resolved(confirm).await);

    fx.wait_for("the custom form again", |_| fx.viewport().contains("custom form")).await;
    let finish_form = done.lock().clone().unwrap();
    finish(&finish_form, "kept");
    assert_eq!(resolved(custom).await.unwrap(), "kept");
    fx.settle().await;
    assert!(!fx.viewport().contains("custom form"), "{}", fx.viewport());
    fx.cleanup();
}

// -------------------------------------------------------------------------------------------------
// interactive-mode-anthropic-warning.test.ts
// -------------------------------------------------------------------------------------------------

const ANTHROPIC_WARNING: &str = "Anthropic subscription auth is active";

fn anthropic_models() -> Vec<FauxModelDefinition> {
    ["claude-a", "claude-b"]
        .iter()
        .map(|id| FauxModelDefinition { id: id.to_string(), name: Some(id.to_string()), ..Default::default() })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn warns_once_when_anthropic_subscription_auth_is_detected() {
    let _lock = LOCK.lock().await;
    let mut fx = Fixture::new(FixtureOptions {
        provider: Some("anthropic".into()),
        models: anthropic_models(),
        credential: Some(json!({ "type": "api_key", "key": "sk-ant-oat01-test" })),
        settings: Some(json!({ "enabledModels": ["anthropic/claude-a", "anthropic/claude-b"] })),
        ..Default::default()
    })
    .await;
    fx.start().await;
    fx.wait_for("the warning", |s| s.contains(ANTHROPIC_WARNING)).await;
    // Switching models checks again; the warning stays a one-off.
    fx.terminal.send_input("\x10");
    fx.wait_for("the model switch", |s| s.contains("Switched to")).await;
    let screen = fx.settle().await;
    assert_eq!(screen.matches(ANTHROPIC_WARNING).count(), 1, "{screen}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn warns_when_anthropic_oauth_is_stored() {
    let _lock = LOCK.lock().await;
    let mut fx = Fixture::new(FixtureOptions {
        provider: Some("anthropic".into()),
        models: anthropic_models(),
        credential: Some(json!({ "type": "oauth", "refresh": "r", "access": "a", "expires": 0 })),
        ..Default::default()
    })
    .await;
    fx.start().await;
    fx.wait_for("the warning", |s| s.contains(ANTHROPIC_WARNING)).await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn does_not_warn_for_non_anthropic_models() {
    let _lock = LOCK.lock().await;
    let mut fx = Fixture::new(FixtureOptions {
        credential: Some(json!({ "type": "api_key", "key": "sk-ant-oat01-test" })),
        ..Default::default()
    })
    .await;
    fx.start().await;
    let screen = fx.settle().await;
    assert!(!screen.contains(ANTHROPIC_WARNING), "{screen}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn does_not_warn_when_the_anthropic_extra_usage_warning_is_disabled() {
    let _lock = LOCK.lock().await;
    let mut fx = Fixture::new(FixtureOptions {
        provider: Some("anthropic".into()),
        models: anthropic_models(),
        credential: Some(json!({ "type": "api_key", "key": "sk-ant-oat01-test" })),
        settings: Some(json!({ "warnings": { "anthropicExtraUsage": false } })),
        ..Default::default()
    })
    .await;
    fx.start().await;
    let screen = fx.settle().await;
    assert!(!screen.contains(ANTHROPIC_WARNING), "{screen}");
    fx.cleanup();
}

// -------------------------------------------------------------------------------------------------
// interactive-mode-assistant-diagnostics.test.ts
// -------------------------------------------------------------------------------------------------

fn thinking_drop_reply(text: &str) -> FauxResponseStep {
    let message: AssistantMessage = serde_json::from_value(json!({
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "api": "anthropic-messages",
        "provider": "anthropic",
        "model": "claude-fable-5-1",
        "usage": { "input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 2,
                   "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0 } },
        "stopReason": "stop",
        "timestamp": 1,
        "diagnostics": [{
            "type": "anthropic_input_transformations",
            "timestamp": 1,
            "details": { "transformations": [
                { "type": "thinking_dropped", "path": "messages.2.content.0", "reason": "prefix_binding_mismatch" },
                { "type": "thinking_dropped", "path": "messages.5.content.0", "reason": "prefix_binding_mismatch" },
                { "type": "thinking_dropped", "path": "messages.8.content.0", "reason": "prefix_binding_mismatch" }
            ] }
        }]
    }))
    .unwrap();
    message.into()
}

const DROP_NOTICE: &str = "Anthropic dropped 3 thinking blocks (details in session)";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shows_anthropic_thinking_drops_only_when_cache_miss_notices_are_enabled() {
    let _lock = LOCK.lock().await;
    for enabled in [true, false] {
        let mut fx = Fixture::new(FixtureOptions {
            settings: Some(json!({ "showCacheMissNotices": enabled })),
            ..Default::default()
        })
        .await;
        fx.harness.set_responses(vec![thinking_drop_reply("survived")]);
        fx.start().await;
        fx.type_text("hello").await;
        let screen = fx.wait_for("the reply", |s| s.contains("survived")).await;
        let screen = if enabled { fx.wait_for("the notice", |s| s.contains(DROP_NOTICE)).await } else { screen };
        assert_eq!(screen.contains(DROP_NOTICE), enabled, "{screen}");
        fx.cleanup();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn does_not_repeat_unchanged_anthropic_thinking_drops() {
    let _lock = LOCK.lock().await;
    let mut fx = Fixture::new(FixtureOptions {
        settings: Some(json!({ "showCacheMissNotices": true })),
        ..Default::default()
    })
    .await;
    fx.harness.set_responses(vec![thinking_drop_reply("first reply"), thinking_drop_reply("second reply")]);
    fx.start().await;
    fx.type_text("one").await;
    fx.wait_for("the first notice", |s| s.contains("first reply") && s.contains(DROP_NOTICE)).await;
    fx.type_text("two").await;
    fx.wait_for("the second reply", |s| s.contains("second reply")).await;
    let screen = fx.settle().await;
    assert_eq!(screen.matches(DROP_NOTICE).count(), 1, "{screen}");
    fx.cleanup();
}

// -------------------------------------------------------------------------------------------------
// interactive-mode-bug-report-hint.test.ts
// -------------------------------------------------------------------------------------------------

const BUG_HINT: &str = "/bug sends a report to the developers";

fn error_reply(message: &str) -> FauxResponseStep {
    faux_assistant_message(
        Vec::new(),
        FauxMessageOptions {
            stop_reason: Some(pi_ai::types::StopReason::Error),
            error_message: Some(message.to_string()),
            ..Default::default()
        },
    )
    .into()
}

async fn bug_hint_after(reply: FauxResponseStep, error_text: &str) -> String {
    let mut fx = Fixture::new(FixtureOptions::default()).await;
    fx.harness.set_responses(vec![reply]);
    fx.start().await;
    fx.type_text("hello").await;
    fx.wait_for("the error", |s| s.contains(error_text)).await;
    let screen = fx.settle().await;
    fx.cleanup();
    screen
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn does_not_suggest_reports_for_retryable_provider_failures() {
    let _lock = LOCK.lock().await;
    for failure in [
        "500 Internal Server Error",
        "502 Bad Gateway",
        "503 Service Unavailable",
        "504 Gateway Timeout",
        "429 Too Many Requests",
        "Provider overloaded",
        "Network connection lost",
        "Request timed out",
    ] {
        let screen = bug_hint_after(error_reply(failure), failure).await;
        assert!(!screen.contains(BUG_HINT), "{failure}:\n{screen}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn does_not_suggest_reports_for_cancellations() {
    let _lock = LOCK.lock().await;
    for failure in ["This operation was aborted", "Request cancelled"] {
        let screen = bug_hint_after(error_reply(failure), failure).await;
        assert!(!screen.contains(BUG_HINT), "{failure}:\n{screen}");
    }
    let aborted = faux_assistant_message(
        Vec::new(),
        FauxMessageOptions { stop_reason: Some(pi_ai::types::StopReason::Aborted), ..Default::default() },
    );
    let screen = bug_hint_after(aborted.into(), "Operation aborted").await;
    assert!(!screen.contains(BUG_HINT), "{screen}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn suggests_reports_for_unexpected_errors() {
    let _lock = LOCK.lock().await;
    let screen = bug_hint_after(error_reply("Unexpected internal state"), "Unexpected internal state").await;
    assert_eq!(screen.matches(BUG_HINT).count(), 1, "{screen}");
}
