//! Builders for `screens.json` (scripts/golden/gen-screens.mjs): full InteractiveMode screens on a
//! virtual terminal with the faux provider.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pi_ai::providers::faux::FauxResponseStep;
use pi_ai::types::AssistantMessage;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::core::tools::ls::create_ls_tool;
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
use pi_tui::terminal::Terminal;
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::Component;
use serde_json::{Value, json};

use super::harness::*;
use super::support::{HarnessOptions, create_harness};

const SCREEN_ROOT: &str = "/tmp/pi-golden-screens";

async fn wait_for(mode: &InteractiveMode, terminal: &VirtualTerminal, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        mode.tui().request_render(false);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let screen = terminal.get_viewport().join("\n");
        if screen.contains(what) {
            return;
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for {what}:\n{screen}");
        }
    }
}

async fn capture(input: &Value, theme_name: &str) -> Vec<String> {
    let root = Path::new(SCREEN_ROOT);
    let agent_dir = root.join("agent");
    let cwd = root.join("project");
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(cwd.join("README.md"), "# Golden project\n\nUsed by the golden screen tests.\n").unwrap();
    std::fs::write(cwd.join("main.ts"), "console.log(\"hi\");\n").unwrap();
    let cwd = cwd.to_str().unwrap().to_string();
    let agent_dir = agent_dir.to_str().unwrap().to_string();
    // SAFETY: golden tests are serialized by the harness lock.
    unsafe {
        std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
        std::env::set_var("PI_SKIP_VERSION_CHECK", "1");
    }

    let mut settings = json!({ "theme": theme_name });
    for (k, v) in input["settings"].as_object().unwrap() {
        settings[k] = v.clone();
    }
    let harness = create_harness(HarnessOptions {
        settings: Some(settings),
        session_manager: Some(SessionManager::in_memory(Some(&cwd), None, None).unwrap()),
        tools: Some(vec![create_ls_tool(&cwd, None)]),
        ..Default::default()
    })
    .await
    .unwrap();
    let session = harness.session.clone();
    let services = AgentSessionServices {
        cwd: cwd.clone(),
        agent_dir: agent_dir.clone(),
        model_runtime: harness.model_runtime.clone(),
        settings_manager: harness.settings_manager.clone(),
        resource_loader: session.resource_loader(),
        diagnostics: Vec::new(),
    };
    let factory: CreateAgentSessionRuntimeFactory = Arc::new(|_| {
        Box::pin(async { Err(anyhow::anyhow!("session replacement is not supported in golden screens")) })
    });
    let runtime = Arc::new(AgentSessionRuntime::new(session, &services, &factory, None, None));
    let columns = input["columns"].as_u64().unwrap() as usize;
    let rows = input["rows"].as_u64().unwrap() as usize;
    let terminal = VirtualTerminal::new(columns, rows);
    let mode = InteractiveMode::new(
        runtime,
        Some(&InteractiveModeOptions {
            terminal: Some(Arc::new(terminal.clone()) as Arc<dyn Terminal>),
            ..Default::default()
        }),
    );
    mode.init().await.unwrap();
    wait_for(&mode, &terminal, "─").await;

    let mut running = None;
    for step in input.get("steps").and_then(Value::as_array).into_iter().flatten() {
        if let Some(responses) = step.get("responses") {
            let messages: Vec<AssistantMessage> = serde_json::from_value(responses.clone()).unwrap();
            harness.set_responses(messages.into_iter().map(FauxResponseStep::from).collect());
        }
        if let Some(text) = opt_s(step, "type") {
            if running.is_none() {
                let mode = mode.clone();
                running = Some(tokio::spawn(async move { mode.run().await }));
                // Let the main loop reach its first getUserInput().
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            terminal.send_input(text);
        }
        if let Some(what) = opt_s(step, "wait") {
            wait_for(&mode, &terminal, what).await;
        }
        if b(step, "idle") {
            let deadline = Instant::now() + Duration::from_secs(15);
            while harness.session.is_streaming() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    mode.tui().request_render(false);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut lines = mode.tui().render(columns);
    let mut buffer: Vec<String> = terminal.get_scroll_buffer().iter().map(|l| l.trim_end().to_string()).collect();
    while buffer.last().is_some_and(|l| l.is_empty()) {
        buffer.pop();
    }
    if let Some(running) = running {
        running.abort();
    }
    mode.stop(None);
    harness.cleanup();
    let _ = std::fs::remove_dir_all(root);
    lines.push("\u{0}terminal".to_string());
    lines.extend(buffer);
    lines.iter().map(|l| normalize_usage(l)).collect()
}

/// Same masking as `normalizeUsage` in gen-screens.mjs: the footer's input tokens and context
/// percentage depend on machine-specific paths in the system prompt.
fn normalize_usage(line: &str) -> String {
    let up = regex::Regex::new(r"↑[0-9.]+[kM]?").unwrap();
    let pct = regex::Regex::new(r"[0-9.]+%/").unwrap();
    pct.replace_all(&up.replace_all(line, "↑#"), "#%/").into_owned()
}

pub fn build(kind: &str, input: &Value) -> Option<Built> {
    if kind != "screen" {
        return None;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap();
    let lines = runtime.block_on(capture(input, &current_theme()));
    runtime.shutdown_background();
    Some(Built::Render(Box::new(move |_| lines.clone())))
}
