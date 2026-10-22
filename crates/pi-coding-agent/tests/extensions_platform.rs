//! Extension APIs that reach into pi's process-wide state like the TS modules extensions share
//! with pi: `setKeybindings()` (pi-tui's global keybindings), `getNativeClipboard()` (pi-tui's
//! native clipboard) and pi-ai telemetry for JavaScript `Models` collections.

mod extensions_support;
mod support;

use std::path::PathBuf;
use std::sync::Arc;

use extensions_support::*;
use pi_ai::types::{Context, Message, StopReason, StreamOptions, UserContent, UserMessage};
use pi_coding_agent::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
use pi_telemetry::{InMemoryTelemetryContext, TelemetryContextExt};
use serde_json::{Value, json};

/// Load `source` as the only extension and return its single tool's description as JSON.
async fn run_extension(source: &str) -> Value {
    let dir = ExtDir::new();
    dir.write("platform.ts", source);
    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:#?}", result.errors);
    let extension = result.extensions[0].read();
    serde_json::from_str(&extension.tools["out"].definition.description).unwrap()
}

/// TS: extensions share pi-tui's module state, so `setKeybindings()` replaces the bindings of the
/// whole app (pi's editor and selectors read pi-tui's global manager).
#[tokio::test(flavor = "multi_thread")]
async fn set_keybindings_replaces_the_global_keybindings() {
    let previous = pi_tui::keybindings::get_keybindings();
    let out = run_extension(
        r#"
        import { KeybindingsManager, TUI_KEYBINDINGS, setKeybindings, getKeybindings } from "@earendil-works/pi-tui";
        export default function (pi) {
            const manager = new KeybindingsManager(TUI_KEYBINDINGS, { "tui.select.up": "ctrl+k" });
            setKeybindings(manager);
            const out = { same: getKeybindings() === manager, js: getKeybindings().matches("\x0b", "tui.select.up") };
            // Later changes to the installed manager reach pi too.
            manager.setUserBindings({ "tui.select.up": ["ctrl+k", "ctrl+p"], "tui.select.down": "ctrl+j" });
            pi.registerTool({ name: "out", label: "out", description: JSON.stringify(out), parameters: { type: "object", properties: {} }, execute: async () => ({ content: [] }) });
        }
        "#,
    )
    .await;
    let global = pi_tui::keybindings::get_keybindings();
    let result = (
        out,
        global.matches("\x0b", "tui.select.up"),
        global.matches("\x10", "tui.select.up"),
        global.matches("\x0a", "tui.select.down"),
        global.get_keys("tui.select.down"),
    );
    pi_tui::keybindings::set_keybindings(previous);
    assert_eq!(result.0, json!({ "same": true, "js": true }));
    assert!(result.1, "ctrl+k selects up in pi's global keybindings");
    assert!(result.2, "setUserBindings() on the installed manager is mirrored");
    assert!(result.3);
    assert_eq!(result.4, vec!["ctrl+j".to_string()]);
}

/// `getNativeClipboard()` is pi-tui's native clipboard (arboard), present exactly where
/// `pi_tui::native_platform::get_native_clipboard()` is (TS: where the native addon loads).
#[tokio::test(flavor = "multi_thread")]
async fn native_clipboard_matches_pi_tui() {
    let out = run_extension(
        r#"
        import { getNativeClipboard } from "@earendil-works/pi-tui";
        export default function (pi) {
            const clipboard = getNativeClipboard();
            const out = clipboard === undefined
                ? { present: false }
                : { present: true, methods: ["getText", "getImage", "getFilePaths"].map((m) => typeof clipboard[m]), setText: typeof clipboard.setText };
            pi.registerTool({ name: "out", label: "out", description: JSON.stringify(out), parameters: { type: "object", properties: {} }, execute: async () => ({ content: [] }) });
        }
        "#,
    )
    .await;
    match pi_tui::native_platform::get_native_clipboard() {
        None => assert_eq!(out, json!({ "present": false })),
        Some(native) => {
            let set_text = if native.can_set_text() { "function" } else { "undefined" };
            assert_eq!(out, json!({ "present": true, "methods": ["function", "function", "function"], "setText": set_text }));
        }
    }
}

const TELEMETRY_EXTENSION: &str = r#"
import { createModels, createProvider, fauxAssistantMessage, fauxProvider } from "@earendil-works/pi-ai";
export default function (pi) {
    const inner = fauxProvider({ provider: "js-inner", models: [{ id: "inner-1" }] });
    inner.setResponses([fauxAssistantMessage("inner reply")]);
    const models = createModels();
    models.setProvider(inner.provider);
    const outer = fauxProvider({ provider: "js-outer", models: [{ id: "outer-1" }] });
    pi.registerProvider(createProvider({
        id: "js-outer",
        auth: { apiKey: { name: "Outer", resolve: async () => ({ auth: {} }) } },
        models: outer.models,
        api: {
            stream: (_model, context, options) => models.stream(inner.getModel(), context, options),
            streamSimple: (_model, context, options) => models.streamSimple(inner.getModel(), context, options),
        },
    }));
}
"#;

/// pi-ai `observeModelRequest`: a JavaScript `Models` collection that forwards the stream options
/// pi passed to its provider records its request under pi's telemetry parent, like TS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn js_models_requests_are_observed_by_pi_ai_telemetry() {
    let dir = std::env::temp_dir().join(format!("pi-ext-telemetry-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join("extension.ts");
    std::fs::write(&path, TELEMETRY_EXTENSION).unwrap();
    let loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: dir.display().to_string(),
        agent_dir: dir.display().to_string(),
        additional_extension_paths: Some(vec![path.display().to_string()]),
        no_skills: Some(true),
        no_prompt_templates: Some(true),
        no_context_files: Some(true),
        ..Default::default()
    }));
    loader.reload(None).await.unwrap();
    assert!(loader.get_extensions().errors.is_empty(), "{:#?}", loader.get_extensions().errors);
    let harness = support::create_harness(support::HarnessOptions { resource_loader: Some(loader), ..Default::default() })
        .await
        .unwrap();
    harness.session.bind_extensions(&Default::default()).await.unwrap();
    let runtime = harness.model_runtime.clone();
    let model = runtime.get_model("js-outer", "outer-1").expect("the extension registers its provider");

    let telemetry = InMemoryTelemetryContext::new();
    let message = telemetry
        .start_span("caller", |parent| async move {
            let context = Context {
                system_prompt: None,
                messages: vec![Message::User(UserMessage { content: UserContent::Text("hi".into()), timestamp: 0 })],
                tools: None,
            };
            let options = StreamOptions { telemetry: Some(parent), ..Default::default() };
            runtime.complete_simple(model, context, Some(options.into())).await
        })
        .await;
    assert_eq!(message.stop_reason, StopReason::Stop, "{:?}", message.error_message);
    for _ in 0..200 {
        if telemetry.get_spans().iter().all(|s| s.settled) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let spans = telemetry.get_spans();
    let requests: Vec<(String, Value, bool)> = spans
        .iter()
        .filter(|s| s.name == "pi.ai.request")
        .map(|s| {
            let attributes = serde_json::to_value(&s.attributes).unwrap();
            (attributes["provider"].as_str().unwrap_or_default().to_string(), attributes["stop"].clone(), s.settled)
        })
        .collect();
    // The extension's nested request (JavaScript Models), recorded by pi-ai's Rust telemetry.
    assert!(requests.contains(&("js-inner".to_string(), json!("stop"), true)), "{requests:?}");
    let inner = spans.iter().find(|s| s.name == "pi.ai.request" && serde_json::to_value(&s.attributes).unwrap()["provider"] == "js-inner").unwrap();
    let caller = spans.iter().find(|s| s.name == "caller").unwrap();
    assert_eq!(inner.parent_id, Some(caller.id), "the JS request is a child of pi's telemetry parent");
    let events: Vec<&str> = inner.events.iter().map(|e| e.name.as_str()).collect();
    assert!(events.contains(&"pi.ai.first_chunk") && events.contains(&"pi.ai.block"), "{events:?}");
    harness.cleanup();
    let _ = std::fs::remove_dir_all(dir);
}
