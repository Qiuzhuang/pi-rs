//! Port of `test/tool-renderer-examples.test.ts` (regression tests for #10072). The example
//! extensions are loaded from the TS reference checkout through the pi-jsrt bridge; the tests are
//! skipped when the checkout is missing (set `PI_TS_SOURCE`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pi_coding_agent::core::extensions::types::ToolDefinition;
use pi_coding_agent::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
use pi_coding_agent::core::sdk::{CreateAgentSessionOptions, create_agent_session};
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::core::settings_manager::SettingsManager;
use pi_coding_agent::modes::interactive::components::tool_execution::{
    ToolExecutionComponent, ToolExecutionComponentToolDefinition,
};
use pi_coding_agent::modes::interactive::theme::theme::init_theme;
use pi_tui::event_loop::EventLoop;
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, Tui, TuiOptions};
use serde_json::json;

const DEFAULT_SOURCE: &str = "/tmp/claude-0/-home-user-pi-rs/3420df64-2251-57a4-b9aa-4eb6ba35ed96/scratchpad/src/pi";

fn examples_dir() -> Option<PathBuf> {
    let root = std::env::var("PI_TS_SOURCE").unwrap_or_else(|_| DEFAULT_SOURCE.to_string());
    let dir = Path::new(&root).join("packages/coding-agent/examples/extensions");
    if !dir.is_dir() {
        eprintln!("skipping: reference TypeScript sources not found (set PI_TS_SOURCE)");
        return None;
    }
    Some(dir)
}

struct SessionState {
    system_prompt: String,
    edit_tool_definition: Option<ToolDefinition>,
}

async fn get_session_state(dir: &Path, extension_path: Option<PathBuf>, tools: &[&str]) -> SessionState {
    let cwd = dir.to_string_lossy().into_owned();
    let agent_dir = dir.join("agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    let agent_dir = agent_dir.to_string_lossy().into_owned();
    let settings_manager = SettingsManager::in_memory(None, None);
    let resource_loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: cwd.clone(),
        agent_dir: agent_dir.clone(),
        settings_manager: Some(settings_manager.clone()),
        no_extensions: Some(true),
        no_skills: Some(true),
        no_prompt_templates: Some(true),
        no_themes: Some(true),
        no_context_files: Some(true),
        additional_extension_paths: Some(extension_path.iter().map(|p| p.to_string_lossy().into_owned()).collect()),
        ..Default::default()
    }));
    resource_loader.reload(None).await.unwrap();
    let extensions = resource_loader.get_extensions();
    assert!(extensions.errors.is_empty(), "{:?}", extensions.errors);
    assert_eq!(extensions.extensions.len(), usize::from(extension_path.is_some()));
    let model = pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5");
    let result = create_agent_session(Some(&CreateAgentSessionOptions {
        cwd: Some(cwd.clone()),
        agent_dir: Some(agent_dir),
        model,
        settings_manager: Some(settings_manager),
        session_manager: Some(SessionManager::in_memory(Some(&cwd), None, None).unwrap()),
        resource_loader: Some(resource_loader),
        tools: Some(tools.iter().map(|t| t.to_string()).collect()),
        ..Default::default()
    }))
    .await
    .unwrap();
    let state = SessionState {
        system_prompt: result.session.system_prompt(),
        edit_tool_definition: result.session.get_tool_definition("edit"),
    };
    result.session.dispose();
    state
}

#[tokio::test]
async fn keeps_the_system_prompt_unchanged_for_the_built_in_tool_renderer_and_minimal_mode_examples() {
    let Some(examples) = examples_dir() else { return };
    // Regression test for https://github.com/earendil-works/pi/issues/10072
    for (example, tools) in [
        ("built-in-tool-renderer.ts", vec!["read", "bash", "edit", "write"]),
        ("minimal-mode.ts", vec!["read", "bash", "write", "edit", "find", "grep", "ls"]),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let baseline = get_session_state(dir.path(), None, &tools).await;
        let with_renderer = get_session_state(dir.path(), Some(examples.join(example)), &tools).await;
        assert_eq!(with_renderer.system_prompt, baseline.system_prompt, "{example}");
    }
}

fn render_edit_tool(definition: &ToolDefinition, cwd: &str) -> Vec<String> {
    let tui = Tui::main_screen_with(
        Arc::new(VirtualTerminal::new(80, 24)),
        TuiOptions { event_loop: Some(EventLoop::new_manual()), ..Default::default() },
    );
    let component = ToolExecutionComponent::new(
        "edit",
        "edit-shell-test",
        json!({ "path": "notes.txt", "oldText": "before", "newText": "after" }),
        None,
        Some(&ToolExecutionComponentToolDefinition::ToolDefinition(definition.clone())),
        tui,
        cwd,
    );
    component.render(40)
}

#[tokio::test]
async fn keeps_minimal_modes_edit_tool_in_the_default_shell() {
    let Some(examples) = examples_dir() else { return };
    init_theme(Some("dark"), Some(false));
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_string_lossy().into_owned();
    let minimal_mode = get_session_state(dir.path(), Some(examples.join("minimal-mode.ts")), &["edit"]).await;
    let edit = minimal_mode.edit_tool_definition.expect("minimal mode registers the edit tool");
    let default_shell = ToolDefinition { render_shell: None, ..edit.clone() };
    assert_eq!(render_edit_tool(&edit, &cwd), render_edit_tool(&default_shell, &cwd));
}
