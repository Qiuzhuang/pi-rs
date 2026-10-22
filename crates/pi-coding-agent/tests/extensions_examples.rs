//! Load pi's example extensions (from the TS reference checkout) through the real loader and the
//! pi-jsrt bridge, and exercise a few end to end. Skipped when the checkout is missing.

mod extensions_support;

use std::path::Path;
use std::sync::Arc;

use extensions_support::*;
use pi_coding_agent::core::extensions::loader::load_extensions;
use pi_coding_agent::core::extensions::runner::ExtensionRunner;
use pi_coding_agent::core::extensions::types::*;
use serde_json::{Value, json};

fn examples_or_skip() -> Option<std::path::PathBuf> {
    let dir = examples_dir();
    if dir.is_none() {
        eprintln!("skipping: reference TypeScript sources not found (set PI_TS_SOURCE)");
    }
    dir
}

async fn load(dir: &Path, files: &[&str]) -> (Arc<ExtensionRunner>, String) {
    let cwd = dir.display().to_string();
    let paths: Vec<String> = files
        .iter()
        .map(|f| dir.join(f).display().to_string())
        .collect();
    let result = load_extensions(&paths, &cwd, None, None).await.unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.extensions.len(), files.len());
    let runner = ExtensionRunner::new(
        &result.extensions,
        result.runtime.clone(),
        &cwd,
        session_manager(&cwd),
        model_registry().await,
    );
    (runner, cwd)
}

fn package_dependencies(dir: &Path) -> usize {
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
        return 0;
    };
    let manifest: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    manifest["dependencies"].as_object().map_or(0, |d| d.len())
}

/// Entry points of every example without npm dependencies (files and directory extensions).
fn dependency_free_examples(dir: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_file() && name.ends_with(".ts") {
            paths.push(path.display().to_string());
        } else if path.is_dir() && package_dependencies(&path) == 0 {
            let manifest: Value = std::fs::read_to_string(path.join("package.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            if let Some(declared) = manifest["pi"]["extensions"].as_array() {
                for p in declared.iter().filter_map(Value::as_str) {
                    paths.push(path.join(p).display().to_string());
                }
            } else if path.join("index.ts").is_file() {
                paths.push(path.join("index.ts").display().to_string());
            }
        }
    }
    paths
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_dependency_free_examples_load() {
    let Some(dir) = examples_or_skip() else {
        return;
    };
    let cwd = dir.display().to_string();
    let paths = dependency_free_examples(&dir);
    assert!(paths.len() > 50, "found only {} examples", paths.len());
    let result = load_extensions(&paths, &cwd, None, None).await.unwrap();
    assert!(
        result.errors.is_empty(),
        "examples failed to load: {:#?}",
        result.errors
    );
    assert_eq!(result.extensions.len(), paths.len());

    // Every example registered something.
    for ext in &result.extensions {
        let ext = ext.read();
        let registered = ext.handlers.len()
            + ext.tools.len()
            + ext.commands.len()
            + ext.flags.len()
            + ext.shortcuts.len()
            + ext.message_renderers.len()
            + ext.entry_renderers.len()
            + ext.tool_renderers.len();
        let providers = result
            .runtime
            .pending_provider_registrations
            .lock()
            .iter()
            .filter(|p| p.extension_path == ext.path)
            .count()
            + result
                .runtime
                .pending_virtual_model_registrations
                .lock()
                .iter()
                .filter(|p| p.extension_path == ext.path)
                .count();
        assert!(
            registered + providers > 0,
            "{} registered nothing",
            ext.path
        );
    }

    // A runner over all of them comes up and reports the registrations.
    let runner = ExtensionRunner::new(
        &result.extensions,
        result.runtime.clone(),
        &cwd,
        session_manager(&cwd),
        model_registry().await,
    );
    assert!(runner.get_tool_definition("todo").is_some());
    assert!(runner.get_command("commands").is_some());
    assert!(runner.has_handlers("tool_call"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hello_tool_executes() {
    let Some(dir) = examples_or_skip() else {
        return;
    };
    let (runner, _) = load(&dir, &["hello.ts"]).await;
    let tool = runner.get_tool_definition("hello").expect("hello tool");
    assert_eq!(tool.label, "Hello");
    assert_eq!(
        tool.parameters["properties"]["name"]["type"],
        json!("string")
    );
    let ctx = runner.create_tool_context("call-1", None);
    let out = (tool.execute)(
        "call-1".into(),
        json!({ "name": "Rust" }),
        None,
        None,
        Some(ctx),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&out).unwrap()["content"][0]["text"],
        json!("Hello, Rust!")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_command_runs_handler() {
    let Some(dir) = examples_or_skip() else {
        return;
    };
    let (runner, _) = load(&dir, &["commands.ts"]).await;
    let ui = Arc::new(TestUi::default());
    runner.bind_core(
        Arc::new(RecordingActions::default()),
        Arc::new(TestContextActions::default()),
        None,
    );
    runner.set_ui_context(Some(ui.clone()), Some(ExtensionMode::Tui));

    let command = runner.get_command("commands").expect("commands command");
    assert_eq!(
        command.description.as_deref(),
        Some("List available slash commands")
    );
    let completions = (command
        .get_argument_completions
        .clone()
        .expect("completions"))("ext".into())
    .await
    .unwrap();
    assert_eq!(
        completions
            .iter()
            .map(|c| c.value.as_str())
            .collect::<Vec<_>>(),
        vec!["extension"]
    );

    (command.handler)(String::new(), runner.create_command_context())
        .await
        .unwrap();
    wait_until(|| !ui.notifications.lock().is_empty()).await;
    assert_eq!(
        *ui.notifications.lock(),
        vec!["No commands found".to_string()]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn todo_tool_executes_and_renders_result() {
    let Some(dir) = examples_or_skip() else {
        return;
    };
    let (runner, cwd) = load(&dir, &["todo.ts"]).await;
    runner.bind_core(
        Arc::new(RecordingActions::default()),
        Arc::new(TestContextActions::default()),
        None,
    );
    let tool = runner.get_tool_definition("todo").expect("todo tool");
    assert_eq!(
        tool.parameters["properties"]["action"]["enum"],
        json!(["list", "add", "toggle", "clear"])
    );

    let exec = |args: Value| {
        let tool = tool.clone();
        let ctx = runner.create_tool_context("call", None);
        async move {
            (tool.execute)("call".into(), args, None, None, Some(ctx))
                .await
                .unwrap()
        }
    };
    exec(json!({ "action": "add", "text": "write tests" })).await;
    exec(json!({ "action": "add", "text": "ship it" })).await;
    exec(json!({ "action": "toggle", "id": 1 })).await;
    let listed = exec(json!({ "action": "list" })).await;
    let listed_json = serde_json::to_value(&listed).unwrap();
    assert_eq!(listed_json["details"]["todos"].as_array().unwrap().len(), 2);
    assert_eq!(listed_json["details"]["todos"][0]["done"], json!(true));

    let theme = pi_coding_agent::modes::interactive::theme::theme::theme();
    let render_ctx = ToolRenderContext {
        args: json!({ "action": "list" }),
        tool_call_id: "call".into(),
        invalidate: Arc::new(|| {}),
        last_component: None,
        state: Default::default(),
        cwd,
        execution_started: true,
        args_complete: true,
        is_partial: false,
        expanded: true,
        show_images: false,
        is_error: false,
        duration_ms: None,
        output_pad: 0,
    };
    let render_result = tool.render_result.clone().expect("renderResult");
    let component = render_result(
        &listed,
        ToolRenderResultOptions {
            expanded: true,
            is_partial: false,
        },
        &theme,
        &render_ctx,
    );
    let text = pi_coding_agent::utils::ansi::strip_ansi(&component.render(80).join("\n")).unwrap();
    assert!(text.contains("write tests"), "{text}");
    assert!(text.contains("ship it"), "{text}");

    let render_call = tool.render_call.clone().expect("renderCall");
    let call = render_call(
        &json!({ "action": "add", "text": "x" }),
        &theme,
        &render_ctx,
    );
    let text = pi_coding_agent::utils::ansi::strip_ansi(&call.render(80).join("\n")).unwrap();
    assert!(text.starts_with("todo add"), "{text}");
}

fn bash_call(command: &str) -> ToolCallEvent {
    ToolCallEvent {
        tool_call_id: "call-1".into(),
        parent_tool_call_id: None,
        tool_name: "bash".into(),
        input: serde_json::from_value(json!({ "command": command })).unwrap(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn permission_gate_blocks_dangerous_tool_calls() {
    let Some(dir) = examples_or_skip() else {
        return;
    };
    let (runner, _) = load(&dir, &["permission-gate.ts"]).await;
    runner.bind_core(
        Arc::new(RecordingActions::default()),
        Arc::new(TestContextActions::default()),
        None,
    );

    // Without UI: blocked by default.
    let result = runner
        .emit_tool_call(&bash_call("sudo rm -rf /"))
        .await
        .unwrap()
        .expect("blocked");
    assert_eq!(result.block, Some(true));
    assert_eq!(
        result.reason.as_deref(),
        Some("Dangerous command blocked (no UI for confirmation)")
    );

    // Harmless commands pass.
    assert!(
        runner
            .emit_tool_call(&bash_call("ls -la"))
            .await
            .unwrap()
            .is_none()
    );

    // With UI: the user decides.
    let ui = Arc::new(TestUi::default());
    runner.set_ui_context(Some(ui.clone()), Some(ExtensionMode::Tui));
    *ui.select_answer.lock() = Some("No".into());
    let result = runner
        .emit_tool_call(&bash_call("rm -rf build"))
        .await
        .unwrap()
        .expect("blocked");
    assert_eq!(result.reason.as_deref(), Some("Blocked by user"));
    assert_eq!(
        ui.selects.lock()[0].1,
        vec!["Yes".to_string(), "No".to_string()]
    );

    *ui.select_answer.lock() = Some("Yes".into());
    assert!(
        runner
            .emit_tool_call(&bash_call("rm -rf build"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn input_transform_rewrites_and_handles_input() {
    let Some(dir) = examples_or_skip() else {
        return;
    };
    let (runner, _) = load(&dir, &["input-transform.ts"]).await;
    runner.bind_core(
        Arc::new(RecordingActions::default()),
        Arc::new(TestContextActions::default()),
        None,
    );
    let ui = Arc::new(TestUi::default());
    runner.set_ui_context(Some(ui.clone()), Some(ExtensionMode::Tui));

    let result = runner
        .emit_input("?quick What is Rust?", None, InputSource::Interactive, None)
        .await
        .unwrap();
    match result {
        InputEventResult::Transform { text, .. } => {
            assert_eq!(text, "Respond briefly in 1-2 sentences: What is Rust?")
        }
        other => panic!("expected transform, got {other:?}"),
    }
    let result = runner
        .emit_input("ping", None, InputSource::Interactive, None)
        .await
        .unwrap();
    assert!(matches!(result, InputEventResult::Handled));
    // Notifications from the JS thread are delivered in order on the UI executor.
    wait_until(|| !ui.notifications.lock().is_empty()).await;
    assert_eq!(*ui.notifications.lock(), vec!["pong".to_string()]);

    let result = runner
        .emit_input("?quick x", None, InputSource::Extension, None)
        .await
        .unwrap();
    assert!(matches!(result, InputEventResult::Continue));
    let result = runner
        .emit_input("hello", None, InputSource::Interactive, None)
        .await
        .unwrap();
    assert!(matches!(result, InputEventResult::Continue));
}

const PROVIDER_FIXTURE: &str = r#"
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";

export default function (pi) {
	pi.registerProvider("echo-provider", {
		baseUrl: "http://localhost:1",
		apiKey: "echo-key",
		api: "echo-api",
		models: [
			{
				id: "echo-1",
				name: "Echo 1",
				reasoning: false,
				input: ["text"],
				cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
				contextWindow: 1000,
				maxTokens: 100,
			},
		],
		streamSimple(model, context, options) {
			const stream = createAssistantMessageEventStream();
			const last = context.messages[context.messages.length - 1];
			const text = typeof last.content === "string" ? last.content : last.content.map((c) => c.text).join("");
			const message = {
				role: "assistant",
				content: [{ type: "text", text: `echo: ${text}` }],
				api: model.api,
				provider: model.provider,
				model: model.id,
				usage: {
					input: 1,
					output: 1,
					cacheRead: 0,
					cacheWrite: 0,
					totalTokens: 2,
					cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
				},
				stopReason: "stop",
				timestamp: Date.now(),
			};
			queueMicrotask(() => {
				stream.push({ type: "start", partial: { ...message, content: [] } });
				stream.push({ type: "done", reason: "stop", message });
				stream.end();
			});
			return stream;
		},
	});
}
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn custom_provider_registers_and_streams() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("echo-provider.js");
    std::fs::write(&file, PROVIDER_FIXTURE).unwrap();
    let cwd = dir.path().display().to_string();
    let result = load_extensions(&[file.display().to_string()], &cwd, None, None)
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(
        result.runtime.pending_provider_registrations.lock().len(),
        1
    );

    let registry = model_registry().await;
    let runner = ExtensionRunner::new(
        &result.extensions,
        result.runtime.clone(),
        &cwd,
        session_manager(&cwd),
        registry.clone(),
    );
    let errors = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = errors.clone();
    runner.on_error(Arc::new(move |e| sink.lock().push(e.error)));
    runner.bind_core(
        Arc::new(RecordingActions::default()),
        Arc::new(TestContextActions::default()),
        None,
    );
    assert!(errors.lock().is_empty(), "{:?}", errors.lock());
    assert!(
        result
            .runtime
            .pending_provider_registrations
            .lock()
            .is_empty()
    );

    let model = registry
        .find("echo-provider", "echo-1")
        .expect("registered model");
    assert_eq!(model.name, "Echo 1");
    let context: pi_ai::types::Context = serde_json::from_value(json!({
        "messages": [{ "role": "user", "content": "hi there", "timestamp": 0 }]
    }))
    .unwrap();
    let message = registry.complete(model, context, None).await;
    let message = serde_json::to_value(&message).unwrap();
    assert_eq!(
        message["content"][0]["text"],
        json!("echo: hi there"),
        "{message}"
    );
}
