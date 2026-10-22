//! Port of `extensions-discovery.test.ts` and `extensions-input-event.test.ts`: discovery rules
//! and loading of JS extensions through the pi-jsrt bridge.

mod extensions_support;

use std::sync::Arc;

use extensions_support::*;
use parking_lot::Mutex;
use pi_coding_agent::core::event_bus::{EventBus, create_event_bus};
use pi_coding_agent::core::extensions::loader::load_extensions;
use pi_coding_agent::core::extensions::types::*;
use serde_json::{Value, json};

const EXTENSION_CODE: &str = r#"
export default function(pi) {
	pi.registerCommand("test", { handler: async () => {} });
}
"#;

fn extension_code_with_tool(name: &str) -> String {
    format!(
        r#"
import {{ Type }} from "typebox";
export default function(pi) {{
	pi.registerTool({{
		name: "{name}",
		label: "{name}",
		description: "Test tool",
		parameters: Type.Object({{}}),
		execute: async () => ({{ content: [{{ type: "text", text: "ok" }}] }}),
	}});
}}
"#
    )
}

fn paths(result: &LoadExtensionsResult) -> Vec<String> {
    result
        .extensions
        .iter()
        .map(|e| e.read().path.clone())
        .collect()
}

fn basename(p: &str) -> String {
    std::path::Path::new(p)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn discovers_direct_ts_files() {
    let dir = ExtDir::new();
    dir.write("foo.ts", EXTENSION_CODE);
    dir.write("bar.ts", EXTENSION_CODE);
    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let mut names: Vec<_> = paths(&result).iter().map(|p| basename(p)).collect();
    names.sort();
    assert_eq!(names, ["bar.ts", "foo.ts"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn loads_coding_agent_entrypoint_and_pi_ai_subpaths() {
    let dir = ExtDir::new();
    dir.write(
        "coding-agent-import.ts",
        r#"
import { getAgentDir } from "@earendil-works/pi-coding-agent";
import * as oauth from "@earendil-works/pi-ai/oauth";
void getAgentDir; void oauth;
export default function(pi) { pi.registerCommand("test", { handler: async () => {} }); }
"#,
    );
    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.extensions.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn discovers_direct_js_files() {
    let dir = ExtDir::new();
    dir.write("foo.js", EXTENSION_CODE);
    let result = dir.discover().await;
    assert!(result.errors.is_empty());
    assert_eq!(basename(&paths(&result)[0]), "foo.js");
}

#[tokio::test(flavor = "multi_thread")]
async fn discovers_subdirectories_with_index() {
    let dir = ExtDir::new();
    dir.write("my-extension/index.ts", EXTENSION_CODE);
    dir.write("other/index.js", EXTENSION_CODE);
    dir.write("both/index.ts", EXTENSION_CODE);
    dir.write("both/index.js", EXTENSION_CODE);
    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let mut found = paths(&result);
    found.sort();
    assert_eq!(found.len(), 3);
    assert!(found.iter().any(|p| p.ends_with("my-extension/index.ts")));
    assert!(found.iter().any(|p| p.ends_with("other/index.js")));
    // index.ts wins over index.js.
    assert!(found.iter().any(|p| p.ends_with("both/index.ts")));
}

#[tokio::test(flavor = "multi_thread")]
async fn discovers_package_json_pi_entries() {
    let dir = ExtDir::new();
    dir.write("my-package/src/main.ts", EXTENSION_CODE);
    dir.write(
        "my-package/package.json",
        &json!({ "name": "my-package", "pi": { "extensions": ["./src/main.ts"] } }).to_string(),
    );
    // Multiple entries, missing ones skipped.
    dir.write("multi/ext1.ts", EXTENSION_CODE);
    dir.write("multi/ext2.ts", EXTENSION_CODE);
    dir.write(
        "multi/package.json",
        &json!({ "pi": { "extensions": ["./ext1.ts", "./ext2.ts", "./missing.ts"] } }).to_string(),
    );
    // pi field takes precedence over index.ts.
    dir.write(
        "precedence/index.ts",
        &extension_code_with_tool("from-index"),
    );
    dir.write(
        "precedence/custom.ts",
        &extension_code_with_tool("from-custom"),
    );
    dir.write(
        "precedence/package.json",
        &json!({ "pi": { "extensions": ["./custom.ts"] } }).to_string(),
    );
    // No pi field: falls back to index.ts.
    dir.write("plain/index.ts", EXTENSION_CODE);
    dir.write(
        "plain/package.json",
        &json!({ "name": "plain", "version": "1.0.0" }).to_string(),
    );

    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let found = paths(&result);
    assert_eq!(found.len(), 5, "{found:?}");
    assert!(found.iter().any(|p| p.ends_with("my-package/src/main.ts")));
    assert!(found.iter().any(|p| p.ends_with("plain/index.ts")));
    assert!(!found.iter().any(|p| p.ends_with("missing.ts")));
    let precedence = result
        .extensions
        .iter()
        .find(|e| e.read().path.ends_with("custom.ts"))
        .unwrap();
    assert!(precedence.read().tools.contains_key("from-custom"));
    assert!(!found.iter().any(|p| p.ends_with("precedence/index.ts")));
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_tilde_entries_package_relative() {
    let dir = ExtDir::new();
    let direct = dir.write("tilde-package/~entry.ts", EXTENSION_CODE);
    let slash = dir.write("tilde-package/~/entry.ts", EXTENSION_CODE);
    dir.write(
        "tilde-package/package.json",
        &json!({ "pi": { "extensions": ["~entry.ts", "~/entry.ts"] } }).to_string(),
    );
    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let mut found = paths(&result);
    found.sort();
    let mut expected = vec![direct, slash];
    expected.sort();
    assert_eq!(found, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn ignores_non_extension_directories_and_does_not_recurse() {
    let dir = ExtDir::new();
    dir.write("not-an-extension/helper.ts", EXTENSION_CODE);
    dir.write("container/nested/index.ts", EXTENSION_CODE);
    let result = dir.discover().await;
    assert!(result.errors.is_empty());
    assert!(result.extensions.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn handles_mixed_files_and_subdirectories() {
    let dir = ExtDir::new();
    dir.write("direct.ts", EXTENSION_CODE);
    dir.write("with-index/index.ts", EXTENSION_CODE);
    dir.write("with-manifest/entry.ts", EXTENSION_CODE);
    dir.write(
        "with-manifest/package.json",
        &json!({ "pi": { "extensions": ["./entry.ts"] } }).to_string(),
    );
    let result = dir.discover().await;
    assert!(result.errors.is_empty());
    assert_eq!(result.extensions.len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn registers_commands_tools_renderers_handlers_shortcuts_and_flags() {
    let dir = ExtDir::new();
    dir.write("with-command.ts", EXTENSION_CODE);
    dir.write("tool-a.ts", &extension_code_with_tool("tool-a"));
    dir.write("tool-b.ts", &extension_code_with_tool("tool-b"));
    dir.write(
        "with-everything.ts",
        r#"
export default function(pi) {
	pi.registerMarkdownTransformer((markdown) => markdown);
	pi.registerMessageRenderer("my-custom-type", () => null);
	pi.registerEntryRenderer("my-entry-type", () => null);
	pi.on("agent_start", async () => {});
	pi.on("tool_call", async () => undefined);
	pi.on("agent_end", async () => {});
	pi.registerShortcut("ctrl+t", { description: "Test shortcut", handler: async () => {} });
	pi.registerFlag("my-flag", { description: "My custom flag" });
}
"#,
    );
    let result = dir.discover().await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.extensions.len(), 4);
    let find = |suffix: &str| {
        result
            .extensions
            .iter()
            .find(|e| e.read().path.ends_with(suffix))
            .unwrap()
            .clone()
    };
    assert!(find("with-command.ts").read().commands.contains_key("test"));
    assert!(find("tool-a.ts").read().tools.contains_key("tool-a"));
    assert!(find("tool-b.ts").read().tools.contains_key("tool-b"));
    let ext = find("with-everything.ts");
    let ext = ext.read();
    assert!(ext.markdown_transformer.is_some());
    assert!(ext.message_renderers.contains_key("my-custom-type"));
    assert!(ext.entry_renderers.contains_key("my-entry-type"));
    for event in ["agent_start", "tool_call", "agent_end"] {
        assert!(ext.handlers.contains_key(event), "{event}");
    }
    assert!(ext.shortcuts.contains_key("ctrl+t"));
    assert!(ext.flags.contains_key("my-flag"));
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_load_errors() {
    let dir = ExtDir::new();
    dir.write("invalid.ts", "this is not valid typescript export");
    dir.write(
        "throws.ts",
        "export default function(pi) { throw new Error(\"Initialization failed!\"); }",
    );
    dir.write(
        "no-default.ts",
        "export function notDefault(pi) { pi.registerCommand(\"test\", { handler: async () => {} }); }",
    );
    let result = dir.discover().await;
    assert!(result.extensions.is_empty());
    assert_eq!(result.errors.len(), 3, "{:?}", result.errors);
    let error_for = |name: &str| {
        result
            .errors
            .iter()
            .find(|e| e.path.ends_with(name))
            .unwrap()
            .error
            .clone()
    };
    assert!(error_for("invalid.ts").starts_with("Failed to load extension: "));
    assert!(error_for("throws.ts").contains("Initialization failed!"));
    assert!(error_for("no-default.ts").contains("does not export a valid factory function"));
}

#[tokio::test(flavor = "multi_thread")]
async fn handles_explicitly_configured_paths() {
    let dir = ExtDir::new();
    let custom = dir.tmp.path().join("custom-location/my-ext.ts");
    std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
    std::fs::write(&custom, EXTENSION_CODE).unwrap();
    let result = dir
        .discover_with(&[custom.display().to_string()], None)
        .await;
    assert!(result.errors.is_empty());
    assert!(paths(&result)[0].ends_with("my-ext.ts"));
}

#[tokio::test(flavor = "multi_thread")]
async fn load_extensions_only_loads_explicit_paths() {
    let dir = ExtDir::new();
    dir.write("discovered.ts", &extension_code_with_tool("discovered"));
    let explicit = dir.tmp.path().join("explicit.ts");
    std::fs::write(&explicit, extension_code_with_tool("explicit")).unwrap();

    let result = load_extensions(&[explicit.display().to_string()], &dir.root(), None, None)
        .await
        .unwrap();
    assert!(result.errors.is_empty());
    assert_eq!(result.extensions.len(), 1);
    assert!(result.extensions[0].read().tools.contains_key("explicit"));

    let result = load_extensions(&[], &dir.root(), None, None).await.unwrap();
    assert!(result.errors.is_empty());
    assert!(result.extensions.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_tools_without_parameter_schema() {
    let dir = ExtDir::new();
    let path = dir.write(
        "missing-parameters.js",
        r#"export default function(pi) {
	pi.registerTool({ name: "noop", label: "No-op", description: "Do nothing", execute: async () => ({ content: [] }) });
}"#,
    );
    let result = load_extensions(std::slice::from_ref(&path), &dir.root(), None, None)
        .await
        .unwrap();
    assert!(result.extensions.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].path, path);
    assert_eq!(
        result.errors[0].error,
        format!(
            "Failed to load extension: Tool \"noop\" registered by extension \"{path}\" must define an object parameter schema."
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_flag_defaults_of_the_wrong_type() {
    let dir = ExtDir::new();
    dir.write(
        "bad-flag-default.ts",
        r#"export default function(pi) { pi.registerFlag("safe-mode", { type: "boolean", default: "false" }); }"#,
    );
    let result = dir.discover().await;
    assert!(result.extensions.is_empty());
    assert!(
        result.errors[0]
            .error
            .contains("Invalid default for flag \"safe-mode\": expected boolean, got string"),
        "{}",
        result.errors[0].error
    );
    assert!(!result.runtime.flag_values.lock().contains_key("safe-mode"));
}

// ----------------------------------------------------------------------------
// Input event (extensions-input-event.test.ts). `globalThis.testVar` observations are reported
// over `pi.events` instead, since JS globals are not visible from Rust.
// ----------------------------------------------------------------------------

struct InputHarness {
    _dir: ExtDir,
    runner: Arc<pi_coding_agent::core::extensions::runner::ExtensionRunner>,
    observed: Arc<Mutex<Vec<Value>>>,
}

async fn input_runner(extensions: &[&str]) -> InputHarness {
    let dir = ExtDir::new();
    for (i, code) in extensions.iter().enumerate() {
        dir.write(&format!("e{i}.ts"), code);
    }
    let bus: Arc<dyn EventBus> = create_event_bus();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let sink = observed.clone();
    bus.on("test", Arc::new(move |v| sink.lock().push(v)));
    let result = dir.discover_with(&[], Some(bus)).await;
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let runner = dir.runner(&result).await;
    InputHarness {
        _dir: dir,
        runner,
        observed,
    }
}

fn image(data: &str, mime: &str) -> pi_ai::types::ImageContent {
    serde_json::from_value(json!({ "type": "image", "data": data, "mimeType": mime })).unwrap()
}

fn action(result: &InputEventResult) -> Value {
    serde_json::to_value(result).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn input_continue_without_handlers_or_results() {
    for code in [
        None,
        Some(r#"export default p => p.on("input", async () => {});"#),
        Some(r#"export default p => p.on("input", async () => ({ action: "continue" }));"#),
    ] {
        let h = input_runner(code.as_slice()).await;
        let r = h
            .runner
            .emit_input("x", None, InputSource::Interactive, None)
            .await
            .unwrap();
        assert_eq!(action(&r)["action"], json!("continue"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn input_transform_preserves_or_replaces_images() {
    let h = input_runner(&[r#"export default p => p.on("input", async e => ({ action: "transform", text: "T:" + e.text }));"#]).await;
    let imgs = vec![image("orig", "image/png")];
    let r = h
        .runner
        .emit_input("hi", Some(imgs.clone()), InputSource::Interactive, None)
        .await
        .unwrap();
    let r = action(&r);
    assert_eq!(r["text"], json!("T:hi"));
    assert_eq!(r["images"][0]["data"], json!("orig"));
    assert_eq!(r["images"][0]["mimeType"], json!("image/png"));

    let h = input_runner(&[
        r#"export default p => p.on("input", async () => ({ action: "transform", text: "X", images: [{ type: "image", data: "new", mimeType: "image/jpeg" }] }));"#,
    ])
    .await;
    let r = h
        .runner
        .emit_input("hi", Some(imgs), InputSource::Interactive, None)
        .await
        .unwrap();
    let r = action(&r);
    assert_eq!(r["text"], json!("X"));
    assert_eq!(r["images"][0]["data"], json!("new"));
    assert_eq!(r["images"][0]["mimeType"], json!("image/jpeg"));
}

#[tokio::test(flavor = "multi_thread")]
async fn input_chains_transforms() {
    let h = input_runner(&[
        r#"export default p => p.on("input", async e => ({ action: "transform", text: e.text + "[1]" }));"#,
        r#"export default p => p.on("input", async e => ({ action: "transform", text: e.text + "[2]" }));"#,
    ])
    .await;
    let r = h
        .runner
        .emit_input("X", None, InputSource::Interactive, None)
        .await
        .unwrap();
    assert_eq!(
        action(&r),
        json!({ "action": "transform", "text": "X[1][2]" })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn input_handled_short_circuits() {
    let h = input_runner(&[
        r#"export default p => p.on("input", async () => ({ action: "handled" }));"#,
        r#"export default p => p.on("input", async () => { p.events.emit("test", true); });"#,
    ])
    .await;
    let r = h
        .runner
        .emit_input("X", None, InputSource::Interactive, None)
        .await
        .unwrap();
    assert_eq!(action(&r), json!({ "action": "handled" }));
    assert!(h.observed.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn input_passes_source_and_streaming_behavior() {
    let h = input_runner(&[
        r#"export default p => p.on("input", async e => { p.events.emit("test", { source: e.source, sb: e.streamingBehavior ?? null }); return { action: "continue" }; });"#,
    ])
    .await;
    for source in [
        InputSource::Interactive,
        InputSource::Rpc,
        InputSource::Extension,
    ] {
        h.runner.emit_input("x", None, source, None).await.unwrap();
    }
    h.runner
        .emit_input(
            "x",
            None,
            InputSource::Interactive,
            Some(StreamingBehavior::Steer),
        )
        .await
        .unwrap();
    h.runner
        .emit_input(
            "x",
            None,
            InputSource::Interactive,
            Some(StreamingBehavior::FollowUp),
        )
        .await
        .unwrap();
    assert_eq!(
        *h.observed.lock(),
        vec![
            json!({ "source": "interactive", "sb": null }),
            json!({ "source": "rpc", "sb": null }),
            json!({ "source": "extension", "sb": null }),
            json!({ "source": "interactive", "sb": "steer" }),
            json!({ "source": "interactive", "sb": "followUp" }),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn input_handler_errors_are_reported_and_continue() {
    let h = input_runner(&[
        r#"export default p => p.on("input", async () => { throw new Error("boom"); });"#,
    ])
    .await;
    let errors = collect_errors(&h.runner);
    let r = h
        .runner
        .emit_input("x", None, InputSource::Interactive, None)
        .await
        .unwrap();
    assert_eq!(action(&r)["action"], json!("continue"));
    assert!(
        errors.lock().iter().any(|e| e.error == "boom"),
        "{:?}",
        errors.lock()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn input_has_handlers() {
    let h = input_runner(&[]).await;
    assert!(!h.runner.has_handlers("input"));
    let h = input_runner(&[r#"export default p => p.on("input", async () => {});"#]).await;
    assert!(h.runner.has_handlers("input"));
}
