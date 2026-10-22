//! RPC mode's extension UI protocol driven end to end with example extensions from the TS
//! reference checkout (`question.ts`, `rpc-demo.ts`): extensions load through the real resource
//! loader and the pi-jsrt bridge, dialogs become `extension_ui_request` lines and are answered
//! with `extension_ui_response` commands. Skipped when the checkout is missing.

mod extensions_support;
mod support;

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use pi_ai::providers::faux::FauxResponseStep;
use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
use pi_coding_agent::modes::rpc::rpc_mode::{RpcIo, RpcOutput, run_rpc_session};
use serde_json::{Value, json};
use support::*;
use tokio::io::AsyncWriteExt;

struct Rpc {
    harness: Harness,
    input: tokio::io::DuplexStream,
    lines: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<anyhow::Result<pi_coding_agent::modes::rpc::rpc_mode::RpcModeExit>>,
    agent_dir: std::path::PathBuf,
}

impl Rpc {
    async fn start(files: &[&str]) -> Option<Rpc> {
        let examples = extensions_support::examples_dir()?;
        let agent_dir = std::env::temp_dir().join(format!("pi-ext-rpc-agent-{}", uuid::Uuid::new_v4()));
        let cwd = agent_dir.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
            cwd: cwd.display().to_string(),
            agent_dir: agent_dir.display().to_string(),
            additional_extension_paths: Some(files.iter().map(|f| examples.join(f).display().to_string()).collect()),
            no_skills: Some(true),
            no_prompt_templates: Some(true),
            no_context_files: Some(true),
            ..Default::default()
        }));
        loader.reload(None).await.unwrap();
        assert!(loader.get_extensions().errors.is_empty(), "{:#?}", loader.get_extensions().errors);
        let harness = create_harness(HarnessOptions { resource_loader: Some(loader), ..Default::default() }).await.unwrap();
        let services = AgentSessionServices {
            cwd: harness.temp_dir.to_string_lossy().into_owned(),
            agent_dir: agent_dir.to_string_lossy().into_owned(),
            model_runtime: harness.model_runtime.clone(),
            settings_manager: harness.settings_manager.clone(),
            resource_loader: harness.session.resource_loader(),
            diagnostics: Vec::new(),
        };
        let factory: CreateAgentSessionRuntimeFactory =
            Arc::new(|_| Box::pin(async { anyhow::bail!("session replacement is not available in this test") }));
        let runtime = Arc::new(AgentSessionRuntime::new(harness.session.clone(), &services, &factory, None, None));

        let (client, server) = tokio::io::duplex(1 << 16);
        let lines: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = lines.clone();
        let output: RpcOutput = Arc::new(move |line: String| {
            for part in line.split('\n').filter(|l| !l.trim().is_empty()) {
                sink.lock().push(serde_json::from_str(part).expect("output is JSON"));
            }
        });
        let task = tokio::task::spawn_local(async move { run_rpc_session(runtime, RpcIo { input: server, output, handle_signals: false }).await });
        Some(Rpc { harness, input: client, lines, task, agent_dir })
    }

    async fn send(&mut self, value: Value) {
        self.input.write_all(format!("{value}\n").as_bytes()).await.unwrap();
    }

    async fn wait_for(&self, what: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..1000 {
            if let Some(found) = self.lines.lock().iter().find(|l| predicate(l)).cloned() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting for {what}; output: {:#?}", self.lines.lock());
    }

    async fn ui_request(&self, method: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        self.wait_for(&format!("extension_ui_request {method}"), |l| {
            l["type"] == "extension_ui_request" && l["method"] == method && predicate(l)
        })
        .await
    }

    async fn notified(&self, message: &str) -> Value {
        self.ui_request("notify", |l| l["message"] == message).await
    }

    async fn finish(self) {
        drop(self.input);
        let exit = self.task.await.unwrap().unwrap();
        assert_eq!(exit.exit_code, 0);
        self.harness.cleanup();
        let _ = std::fs::remove_dir_all(&self.agent_dir);
    }
}

fn tool_call(name: &str, args: Value) -> FauxResponseStep {
    let Value::Object(args) = args else { panic!("tool args must be an object") };
    faux_assistant_message(
        vec![faux_tool_call(name, args, None)],
        FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
    )
    .into()
}

fn tool_result_text(harness: &Harness, tool: &str) -> String {
    get_message_text(&pi_agent::types::AgentMessage::ToolResult(harness.tool_result(tool)))
}

/// RPC mode runs on a `LocalSet` (its command loop is not `Send`), like the binary.
fn local<F: std::future::Future>(future: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, future)
}

#[test]
fn question_tool_reports_missing_tui_in_rpc_mode() {
    local(question_tool_reports_missing_tui_in_rpc_mode_impl());
}

async fn question_tool_reports_missing_tui_in_rpc_mode_impl() {
    let Some(mut rpc) = Rpc::start(&["question.ts"]).await else { return };
    rpc.harness.set_responses(vec![
        tool_call("question", json!({ "question": "Pick one", "options": [{ "label": "A" }, { "label": "B" }] })),
        faux_text_message("understood").into(),
    ]);
    rpc.send(json!({ "id": "p1", "type": "prompt", "message": "ask" })).await;
    rpc.wait_for("agent_end", |l| l["type"] == "agent_end").await;
    // question.ts needs the TUI (`ctx.mode !== "tui"`), so in RPC mode it answers with an error.
    assert_eq!(tool_result_text(&rpc.harness, "question"), "Error: UI not available (running in non-interactive mode)");
    assert!(!rpc.lines.lock().iter().any(|l| l["type"] == "extension_ui_request" && l["method"] == "select"));
    rpc.finish().await;
}

#[test]
fn rpc_demo_exercises_the_extension_ui_protocol() {
    local(rpc_demo_exercises_the_extension_ui_protocol_impl());
}

async fn rpc_demo_exercises_the_extension_ui_protocol_impl() {
    let Some(mut rpc) = Rpc::start(&["rpc-demo.ts"]).await else { return };

    // Fire-and-forget UI calls from session_start.
    let title = rpc.ui_request("setTitle", |_| true).await;
    assert_eq!(title["title"], "pi RPC Demo");
    let widget = rpc.ui_request("setWidget", |_| true).await;
    assert_eq!(widget["widgetKey"], "rpc-demo");
    assert_eq!(widget["widgetLines"], json!(["--- RPC Extension UI Demo ---", "Loaded and ready."]));
    rpc.ui_request("setStatus", |l| l["statusText"] == "Turns: 0").await;

    // select() from a tool_call handler, answered by the client.
    rpc.harness.set_responses(vec![
        tool_call("bash", json!({ "command": "sudo rm -rf /tmp/pi-rpc-demo-nothing" })),
        faux_text_message("blocked then").into(),
    ]);
    rpc.send(json!({ "id": "p1", "type": "prompt", "message": "clean" })).await;
    let select = rpc.ui_request("select", |_| true).await;
    assert_eq!(select["title"], "Dangerous command: sudo rm -rf /tmp/pi-rpc-demo-nothing");
    assert_eq!(select["options"], json!(["Allow", "Block"]));
    rpc.send(json!({ "type": "extension_ui_response", "id": select["id"], "value": "Block" })).await;
    rpc.notified("Command blocked by user").await;
    rpc.wait_for("agent_end", |l| l["type"] == "agent_end").await;
    assert!(tool_result_text(&rpc.harness, "bash").contains("Blocked by user"));
    rpc.ui_request("setStatus", |l| l["statusText"] == "Turn 2 done").await;

    // input() from a command.
    rpc.send(json!({ "id": "c1", "type": "prompt", "message": "/rpc-input" })).await;
    let input = rpc.ui_request("input", |_| true).await;
    assert_eq!(input["title"], "Enter a value");
    assert_eq!(input["placeholder"], "type something...");
    rpc.send(json!({ "type": "extension_ui_response", "id": input["id"], "value": "forty-two" })).await;
    rpc.notified("You entered: forty-two").await;

    // A cancelled input.
    rpc.send(json!({ "id": "c2", "type": "prompt", "message": "/rpc-input" })).await;
    let input = rpc.ui_request("input", |l| l["id"] != input["id"]).await;
    rpc.send(json!({ "type": "extension_ui_response", "id": input["id"], "cancelled": true })).await;
    rpc.notified("Input cancelled").await;

    // editor() with a prefill.
    rpc.send(json!({ "id": "c3", "type": "prompt", "message": "/rpc-editor" })).await;
    let editor = rpc.ui_request("editor", |_| true).await;
    assert_eq!(editor["prefill"], "Line 1\nLine 2\nLine 3");
    rpc.send(json!({ "type": "extension_ui_response", "id": editor["id"], "value": "a\nb" })).await;
    rpc.notified("Editor submitted (2 lines)").await;

    // setEditorText().
    rpc.send(json!({ "id": "c4", "type": "prompt", "message": "/rpc-prefill" })).await;
    let prefill = rpc.ui_request("set_editor_text", |_| true).await;
    assert_eq!(prefill["text"], "This text was set by the rpc-demo extension.");
    rpc.notified("Editor prefilled").await;

    // confirm() from session_before_switch: declining cancels the new session.
    rpc.send(json!({ "id": "n1", "type": "new_session" })).await;
    let confirm = rpc.ui_request("confirm", |_| true).await;
    assert_eq!(confirm["title"], "Clear session?");
    assert_eq!(confirm["message"], "All messages will be lost.");
    rpc.send(json!({ "type": "extension_ui_response", "id": confirm["id"], "confirmed": false })).await;
    rpc.notified("Clear cancelled").await;
    let response = rpc.wait_for("new_session response", |l| l["type"] == "response" && l["id"] == "n1").await;
    assert_eq!(response["success"], true);
    assert_eq!(response["data"]["cancelled"], true);
    rpc.finish().await;
}
