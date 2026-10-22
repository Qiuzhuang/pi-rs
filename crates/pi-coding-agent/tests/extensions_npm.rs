//! Example extensions with npm dependencies (from the TS reference checkout): each example is
//! copied to a temp directory, its dependencies are installed with pi's built-in (pure-Rust) npm
//! client, and it is loaded through the real loader and exercised. Skipped when the checkout is
//! missing or the npm registry is unreachable.

mod extensions_support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use extensions_support::*;
use pi_coding_agent::core::extensions::loader::load_extensions;
use pi_coding_agent::core::extensions::runner::ExtensionRunner;
use pi_coding_agent::core::package_manager::install_dependencies_with_builtin_npm;
use serde_json::{Value, json};

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            if entry.file_name() != "node_modules" {
                copy_dir(&path, &target);
            }
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

/// Copy an example directory and install its npm dependencies. `None` skips the test.
async fn prepare(example: &str) -> Option<PathBuf> {
    let Some(examples) = examples_dir() else {
        eprintln!("skipping: reference TypeScript sources not found (set PI_TS_SOURCE)");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("pi-npm-example-{}", uuid::Uuid::new_v4())).join(example);
    copy_dir(&examples.join(example), &dir);
    if let Err(error) = install_dependencies_with_builtin_npm(&dir).await {
        eprintln!("skipping {example}: npm install failed (offline?): {error:#}");
        return None;
    }
    Some(dir)
}

async fn load(dir: &Path, entry: &str) -> Arc<ExtensionRunner> {
    let cwd = dir.display().to_string();
    let path = dir.join(entry).display().to_string();
    let result = load_extensions(&[path], &cwd, None, None).await.unwrap();
    assert!(result.errors.is_empty(), "{:#?}", result.errors);
    ExtensionRunner::new(&result.extensions, result.runtime.clone(), &cwd, session_manager(&cwd), model_registry().await)
}

async fn execute(runner: &ExtensionRunner, tool: &str, args: Value) -> anyhow::Result<pi_agent::types::AgentToolResult> {
    let definition = runner.get_tool_definition(tool).unwrap_or_else(|| panic!("tool {tool} is not registered"));
    (definition.execute)("call-1".into(), args, None, None, None).await
}

fn text_of(result: &pi_agent::types::AgentToolResult) -> String {
    serde_json::to_value(&result.content).unwrap()[0]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_deps_resolves_its_own_node_modules() {
    let Some(dir) = prepare("with-deps").await else { return };
    assert!(dir.join("node_modules/ms/package.json").is_file());
    let runner = load(&dir, "index.ts").await;
    let result = execute(&runner, "parse_duration", json!({ "duration": "2 days" })).await.unwrap();
    assert_eq!(text_of(&result), "2 days = 172800000 milliseconds");
    let error = execute(&runner, "parse_duration", json!({ "duration": "soon" })).await.unwrap_err();
    assert!(error.to_string().contains("Invalid duration: \"soon\""), "{error:#}");
}
