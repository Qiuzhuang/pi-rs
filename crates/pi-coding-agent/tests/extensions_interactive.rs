//! Example extensions (from the TS reference checkout) driven end to end in the real
//! `InteractiveMode` on pi-tui's `VirtualTerminal`, with the faux provider. Each test loads the
//! example through the real loader and the pi-jsrt bridge, types into the terminal and asserts on
//! the rendered viewport. Skipped when the checkout is missing.

#![allow(dead_code)]

mod extensions_support;
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::core::extensions::loader::load_extensions;
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
use pi_tui::terminal::Terminal;
use pi_tui::testing::VirtualTerminal;
use serde_json::{Value, json};
use support::*;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const ESC: &str = "\x1b";
const ENTER: &str = "\r";
const DOWN: &str = "\x1b[B";
const UP: &str = "\x1b[A";

struct Fixture {
    harness: Harness,
    mode: InteractiveMode,
    terminal: VirtualTerminal,
    agent_dir: PathBuf,
    run: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

fn examples() -> Option<PathBuf> {
    let dir = extensions_support::examples_dir();
    if dir.is_none() {
        eprintln!("skipping: reference TypeScript sources not found (set PI_TS_SOURCE)");
    }
    dir
}

/// Strip ANSI escape sequences (SGR, OSC, APC) from rendered text.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') | Some('_') | Some('P') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

struct FixtureOptions {
    settings: Value,
    columns: usize,
    rows: usize,
    flags: Vec<(String, pi_coding_agent::core::extensions::types::FlagValue)>,
    /// Faux model ids (default: one model).
    models: Vec<&'static str>,
    /// Files written into the agent dir before loading (relative path, content).
    agent_files: Vec<(&'static str, String)>,
}

impl Default for FixtureOptions {
    fn default() -> Self {
        Self { settings: json!({ "quietStartup": true }), columns: 100, rows: 40, flags: Vec::new(), models: Vec::new(), agent_files: Vec::new() }
    }
}

impl Fixture {
    async fn new(examples: &Path, files: &[&str]) -> Fixture {
        Self::with_options(examples, files, FixtureOptions::default()).await
    }

    async fn with_options(examples: &Path, files: &[&str], options: FixtureOptions) -> Fixture {
        let agent_dir = std::env::temp_dir().join(format!("pi-ext-interactive-agent-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent_dir).unwrap();
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
            std::env::set_var("PI_OFFLINE", "1");
            std::env::set_var("PI_SKIP_VERSION_CHECK", "1");
        }
        for (path, content) in &options.agent_files {
            let path = agent_dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let cwd = agent_dir.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd = cwd.display().to_string();
        let paths: Vec<String> = files.iter().map(|f| examples.join(f).display().to_string()).collect();
        let loaded = load_extensions(&paths, &cwd, None, None).await.unwrap();
        assert!(loaded.errors.is_empty(), "extensions failed to load: {:#?}", loaded.errors);
        for (name, value) in &options.flags {
            loaded.runtime.flag_values.lock().insert(name.clone(), value.clone());
        }
        let harness = create_harness(HarnessOptions {
            settings: Some(options.settings.clone()),
            resource_loader: Some(TestResourceLoader::new(Some(loaded))),
            models: options
                .models
                .iter()
                .map(|id| pi_ai::providers::faux::FauxModelDefinition { id: id.to_string(), ..Default::default() })
                .collect(),
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
        let terminal = VirtualTerminal::new(options.columns, options.rows);
        let mode = InteractiveMode::new(
            runtime,
            Some(&InteractiveModeOptions {
                terminal: Some(Arc::new(terminal.clone()) as Arc<dyn Terminal>),
                ..Default::default()
            }),
        );
        mode.init().await.unwrap();
        let run_mode = mode.clone();
        let run = Some(tokio::spawn(async move { run_mode.run().await }));
        let fx = Fixture { harness, mode, terminal, agent_dir, run };
        fx.wait_for("the editor", |s| s.contains('─')).await;
        fx
    }

    /// The viewport without ANSI styling.
    fn screen(&self) -> String {
        plain(&self.terminal.get_viewport().join("\n"))
    }

    /// Wait until the plain viewport satisfies `predicate`, rendering as needed.
    async fn wait_for(&self, what: &str, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.mode.tui().request_render(false);
            tokio::time::sleep(Duration::from_millis(40)).await;
            let screen = self.screen();
            if predicate(&screen) {
                return screen;
            }
            if Instant::now() > deadline {
                self.mode.tui().request_render(true);
                tokio::time::sleep(Duration::from_millis(300)).await;
                panic!("timed out waiting for {what}; screen:\n{screen}\nafter a full redraw:\n{}", self.screen());
            }
        }
    }

    async fn wait_text(&self, text: &str) -> String {
        self.wait_for(text, |s| s.contains(text)).await
    }

    async fn wait_gone(&self, text: &str) -> String {
        self.wait_for(&format!("{text} to disappear"), |s| !s.contains(text)).await
    }

    /// Wait for a condition outside the screen (session state, files, ...).
    async fn wait_until(&self, what: &str, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !predicate() {
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}; screen:\n{}", self.screen());
            }
            self.mode.tui().request_render(false);
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }

    fn keys(&self, data: &str) {
        self.terminal.send_input(data);
    }

    /// Type text into the focused component, one key at a time.
    async fn type_text(&self, text: &str) {
        for c in text.chars() {
            self.terminal.send_input(&c.to_string());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    /// Submit a line through the editor (like typing it and pressing Enter).
    fn submit(&self, text: &str) {
        self.mode.submit(text);
    }

    fn respond(&self, steps: Vec<FauxResponseStep>) {
        self.harness.set_responses(steps);
    }

    async fn wait_idle(&self) {
        self.wait_until("the agent to be idle", || !self.harness.session.is_streaming()).await;
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

use pi_ai::providers::faux::FauxResponseStep;

fn tool_call(name: &str, args: Value) -> FauxResponseStep {
    let Value::Object(args) = args else { panic!("tool args must be an object") };
    faux_assistant_message(
        vec![faux_tool_call(name, args, None)],
        FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
    )
    .into()
}

fn text(text: &str) -> FauxResponseStep {
    faux_text_message(text).into()
}

type Captured = Arc<parking_lot::Mutex<Vec<pi_ai::types::TranscriptContext>>>;

/// A text reply that records the request context it answers.
fn capturing(reply: &str, sink: &Captured) -> FauxResponseStep {
    let reply = reply.to_string();
    let sink = sink.clone();
    FauxResponseStep::factory(move |context, _, _, _| {
        sink.lock().push(context.clone());
        faux_text_message(reply.clone())
    })
}

fn system_prompt_of(context: &pi_ai::types::TranscriptContext) -> String {
    pi_ai::utils::transcript::get_current_system_prompt(&context.messages)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn todo_tool_renderer_and_custom_ui() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["todo.ts"]).await;

    fx.respond(vec![tool_call("todo", json!({ "action": "add", "text": "Buy milk" })), text("Added it.")]);
    fx.submit("add a todo");
    let screen = fx.wait_text("Added it.").await;
    assert!(screen.contains("todo add \"Buy milk\""), "renderCall missing:\n{screen}");
    assert!(screen.contains("✓ Added #1 Buy milk"), "renderResult missing:\n{screen}");

    fx.submit("/todos");
    let screen = fx.wait_text("Press Escape to close").await;
    assert!(screen.contains("0/1 completed"), "{screen}");
    assert!(screen.contains("○ #1 Buy milk"), "{screen}");
    fx.keys(ESC);
    fx.wait_gone("Press Escape to close").await;
    fx.cleanup();
}

fn custom_entries(fx: &Fixture, custom_type: &str) -> Vec<Value> {
    fx.harness
        .session_manager
        .get_entries()
        .iter()
        .filter_map(|e| serde_json::to_value(e).ok())
        .filter(|e| e["type"] == "custom" && e["customType"] == custom_type)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snake_custom_component_with_input_and_timers() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["snake.ts"]).await;

    fx.submit("/snake");
    let first = fx.wait_text("SNAKE").await;
    assert!(first.contains("Score: 0"), "{first}");
    // The game advances on its interval timer.
    fx.wait_for("the snake to move", |s| s.contains("SNAKE") && s != first).await;
    fx.keys("w");
    // Escape pauses: the state is saved in the session and the game closes.
    fx.keys(ESC);
    fx.wait_gone("SNAKE").await;
    fx.wait_until("the saved game", || !custom_entries(&fx, "snake-save").is_empty()).await;

    // Reopening resumes the saved game paused.
    fx.submit("/snake");
    fx.wait_text("PAUSED").await;
    fx.keys("q");
    fx.wait_gone("SNAKE").await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlay_test_overlay_input_and_result() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["overlay-test.ts"]).await;

    fx.submit("/overlay-test");
    let screen = fx.wait_text("Overlay Test").await;
    assert!(screen.contains("Edge Cases"), "{screen}");
    fx.type_text("abc").await;
    fx.wait_text("Search: abc").await;
    fx.keys(ENTER);
    fx.wait_gone("Overlay Test").await;
    fx.wait_text("Search: \"abc\"").await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn modal_editor_replaces_the_editor_and_keeps_app_actions() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["modal-editor.ts"]).await;

    fx.wait_text(" INSERT ").await;
    fx.type_text("hello").await;
    fx.wait_text("hello").await;
    fx.keys(ESC);
    fx.wait_text(" NORMAL ").await;
    // Normal mode: `0` goes to the line start, `x` deletes, printable keys are ignored.
    fx.keys("0");
    fx.keys("x");
    fx.keys("z");
    fx.wait_for("the edited text", |s| s.contains("ello") && !s.contains("hello")).await;
    fx.keys("i");
    fx.wait_text(" INSERT ").await;

    // App actions reach the interactive mode through the CustomEditor action handlers.
    fx.keys("\x0c"); // ctrl+l: model selector
    fx.wait_text("Only showing models").await;
    fx.keys(ESC);
    fx.wait_gone("Only showing models").await;

    fx.respond(vec![text("modal reply")]);
    fx.keys(ENTER);
    fx.wait_text("modal reply").await;
    assert_eq!(fx.harness.user_texts(), vec!["ello".to_string()]);
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn custom_footer_toggles() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["custom-footer.ts"]).await;

    fx.submit("/footer");
    fx.wait_text("Custom footer enabled").await;
    let screen = fx.wait_text("↑0 ↓0 $0.000").await;
    assert!(screen.contains("faux-1"), "{screen}");
    fx.submit("/footer");
    fx.wait_text("Default footer restored").await;
    fx.wait_gone("↑0 ↓0 $0.000").await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn custom_header_replaces_and_restores_the_header() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["custom-header.ts"]).await;

    let screen = fx.wait_text("shitty coding agent").await;
    assert!(screen.contains("██████████████"), "mascot missing:\n{screen}");
    fx.submit("/builtin-header");
    fx.wait_text("Built-in header restored").await;
    fx.wait_gone("shitty coding agent").await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_line_follows_turns() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["status-line.ts"]).await;

    fx.wait_text("Ready").await;
    fx.respond(vec![text("ok")]);
    fx.submit("hi");
    fx.wait_text("✓ Turn 1 complete").await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn widget_placement_above_and_below_the_editor() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["widget-placement.ts"]).await;

    let screen = fx.wait_for("both widgets", |s| s.contains("Above editor widget") && s.contains("Below editor widget")).await;
    let lines: Vec<&str> = screen.lines().collect();
    let above = lines.iter().position(|l| l.contains("Above editor widget")).unwrap();
    let below = lines.iter().position(|l| l.contains("Below editor widget")).unwrap();
    let borders: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.starts_with("────")).map(|(i, _)| i).collect();
    assert!(borders.len() >= 2, "{screen}");
    assert!(above < borders[0] && below > borders[1], "widgets misplaced:\n{screen}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn titlebar_spinner_animates_and_restores_the_title() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["titlebar-spinner.ts"]).await;

    fx.respond(vec![text("done")]);
    fx.submit("hi");
    fx.wait_text("done").await;
    fx.wait_until("the base title", || fx.terminal.get_title().as_deref() == Some("π - project")).await;
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_status_notifies_on_model_change() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx =
        Fixture::with_options(&dir, &["model-status.ts"], FixtureOptions { models: vec!["faux-1", "faux-2"], ..Default::default() })
            .await;

    fx.submit("/model");
    fx.wait_text("Only showing models").await;
    fx.type_text("faux-2").await;
    fx.wait_text("Model Name: faux-2").await;
    fx.keys(ENTER);
    // The interactive mode's own "Model: faux-2" status coalesces with the extension's
    // notification (both are status lines), as in TS.
    let screen = fx.wait_text("🤖 faux-2").await;
    eprintln!("{screen}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notify_runs_on_agent_settled() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["notify.ts"]).await;

    fx.respond(vec![text("settled")]);
    fx.submit("hi");
    fx.wait_text("settled").await;
    fx.wait_idle().await;
    let screen = fx.screen();
    assert!(!screen.contains("Extension error") && !screen.contains("error"), "{screen}");
    fx.cleanup();
}


#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn question_tool_dialog_selects_and_types_answers() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["question.ts"]).await;
    let options = json!([{ "label": "Red" }, { "label": "Blue", "description": "the sky" }]);

    fx.respond(vec![tool_call("question", json!({ "question": "Favorite color?", "options": options })), text("noted")]);
    fx.submit("ask me");
    let screen = fx.wait_text("↑↓ navigate • Enter to select • Esc to cancel").await;
    assert!(screen.contains("Favorite color?") && screen.contains("1. Red") && screen.contains("2. Blue"), "{screen}");
    assert!(screen.contains("the sky") && screen.contains("3. Type something."), "{screen}");
    fx.keys(DOWN);
    fx.keys(ENTER);
    fx.wait_text("noted").await;
    let result = get_message_text(&pi_agent::types::AgentMessage::ToolResult(fx.harness.tool_result("question")));
    assert_eq!(result, "User selected: 2. Blue");

    // "Type something." opens the inline editor.
    fx.respond(vec![tool_call("question", json!({ "question": "Favorite color?", "options": options })), text("noted again")]);
    fx.submit("ask again");
    fx.wait_text("↑↓ navigate • Enter to select • Esc to cancel").await;
    fx.keys(DOWN);
    fx.keys(DOWN);
    fx.keys(ENTER);
    fx.wait_text("Your answer:").await;
    fx.type_text("Green").await;
    fx.keys(ENTER);
    fx.wait_text("noted again").await;
    let result = get_message_text(&pi_agent::types::AgentMessage::ToolResult(fx.harness.tool_result("question")));
    assert_eq!(result, "User wrote: Green");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn questionnaire_single_question() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["questionnaire.ts"]).await;
    let questions = json!([{
        "id": "scope",
        "prompt": "Which scope?",
        "options": [{ "value": "small", "label": "Small" }, { "value": "big", "label": "Big" }]
    }]);
    fx.respond(vec![tool_call("questionnaire", json!({ "questions": questions })), text("thanks")]);
    fx.submit("ask");
    fx.wait_text("Which scope?").await;
    fx.keys(DOWN);
    fx.keys(ENTER);
    fx.wait_text("thanks").await;
    let result = get_message_text(&pi_agent::types::AgentMessage::ToolResult(fx.harness.tool_result("questionnaire")));
    assert!(result.contains("Big"), "{result}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn permission_gate_asks_before_dangerous_commands() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["permission-gate.ts"]).await;

    fx.respond(vec![tool_call("bash", json!({ "command": "sudo rm -rf /tmp/pi-nothing-here" })), text("ok, not done")]);
    fx.submit("clean up");
    let screen = fx.wait_text("Dangerous command:").await;
    assert!(screen.contains("sudo rm -rf /tmp/pi-nothing-here") && screen.contains("Yes") && screen.contains("No"), "{screen}");
    fx.keys(DOWN);
    fx.keys(ENTER);
    fx.wait_text("ok, not done").await;
    let result = fx.harness.tool_result("bash");
    assert!(result.is_error, "blocked tool call should be an error result");
    assert!(get_message_text(&pi_agent::types::AgentMessage::ToolResult(result)).contains("Blocked by user"));
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tools_settings_list_toggles_active_tools() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["tools.ts"]).await;
    assert!(fx.harness.session.get_active_tool_names().contains(&"read".to_string()));

    fx.submit("/tools");
    let screen = fx.wait_text("Tool Configuration").await;
    assert!(screen.contains("enabled"), "{screen}");
    // The first tool is `read`: Enter cycles enabled -> disabled.
    fx.keys(ENTER);
    fx.wait_until("read to be disabled", || !fx.harness.session.get_active_tool_names().contains(&"read".to_string())).await;
    fx.keys(ESC);
    fx.wait_gone("Tool Configuration").await;
    let saved = custom_entries(&fx, "tools-config");
    assert!(!saved.is_empty());
    assert!(!saved.last().unwrap()["data"]["enabledTools"].as_array().unwrap().contains(&json!("read")));
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plan_mode_plans_then_executes_with_progress() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let fx = Fixture::new(&dir, &["plan-mode/index.ts"]).await;

    fx.submit("/plan");
    fx.wait_text("Plan mode enabled").await;
    fx.wait_text("⏸ plan").await;
    let tools = fx.harness.session.get_active_tool_names();
    assert!(!tools.contains(&"edit".to_string()) && !tools.contains(&"write".to_string()), "{tools:?}");

    fx.respond(vec![text("Plan:\n1. Inspect the parser module\n2. Write the failing test")]);
    fx.submit("fix the bug");
    fx.wait_text("Plan mode - what next?").await;
    fx.respond(vec![text("Working. [DONE:1] [DONE:2]")]);
    fx.keys(ENTER); // Execute the plan
    fx.wait_text("Plan Complete!").await;
    let tools = fx.harness.session.get_active_tool_names();
    assert!(tools.contains(&"edit".to_string()) && tools.contains(&"write".to_string()), "{tools:?}");
    fx.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preset_selects_model_tools_and_instructions() {
    let Some(dir) = examples() else { return };
    let _lock = LOCK.lock().await;
    let presets = json!({
        "explore": { "provider": "faux", "model": "faux-2", "tools": ["read", "grep"], "instructions": "PRESET-INSTRUCTIONS" }
    });
    let fx = Fixture::with_options(
        &dir,
        &["preset.ts"],
        FixtureOptions { models: vec!["faux-1", "faux-2"], agent_files: vec![("presets.json", presets.to_string())], ..Default::default() },
    )
    .await;

    fx.submit("/preset");
    fx.wait_text("Select Preset").await;
    fx.keys(ENTER);
    fx.wait_text("Preset \"explore\" activated").await;
    fx.wait_text("preset:explore").await;
    assert_eq!(fx.harness.session.model().map(|m| m.id), Some("faux-2".to_string()));
    assert_eq!(fx.harness.session.get_active_tool_names(), vec!["read".to_string(), "grep".to_string()]);
    let captured = Captured::default();
    fx.respond(vec![capturing("terse", &captured)]);
    fx.submit("hi");
    fx.wait_text("terse").await;
    let prompt = system_prompt_of(&captured.lock()[0]);
    assert!(prompt.ends_with("\n\nPRESET-INSTRUCTIONS"), "{prompt}");
    fx.cleanup();
}
