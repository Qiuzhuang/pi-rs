//! Ports of the remaining interactive component tests:
//! `tool-execution-component.test.ts` (fake clock, renderer slots, compact reads),
//! `auth-url-copy.test.ts` (fake clipboard), `first-time-setup.test.ts`,
//! `first-time-setup-fork.test.ts` and the config-selector cases of `package-command-paths.test.ts`.

use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};
use pi_agent::types::AgentToolResult;
use pi_coding_agent::config::get_readme_path;
use pi_coding_agent::core::extensions::types::{ToolDefinition, ToolRenderers};
use pi_coding_agent::core::keybindings::KeybindingsManager;
use pi_coding_agent::core::tools::bash::{
    BashOperations, BashOperationsResult, BashToolOptions, create_bash_tool_definition,
};
use pi_coding_agent::core::tools::read::{create_read_tool, create_read_tool_definition};
use pi_coding_agent::core::tools::render_utils::set_tool_timer_clock;
use pi_coding_agent::core::tools::renderers::with_built_in_renderers;
use pi_coding_agent::core::tools::write::create_write_tool_definition;
use pi_coding_agent::modes::interactive::components::tool_execution::{
    ToolExecutionComponent, ToolExecutionComponentToolDefinition, ToolExecutionComponentUpdateResultResult,
};
use pi_coding_agent::modes::interactive::theme::theme::{ThemeColor, init_theme, theme};
use pi_coding_agent::utils::ansi::strip_ansi;
use pi_tui::components::text::Text;
use pi_tui::event_loop::EventLoop;
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, ComponentRef, Tui, TuiOptions};
use serde_json::{Value, json};

static SERIAL: Mutex<()> = Mutex::new(());

fn setup() -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock();
    init_theme(Some("dark"), Some(false));
    pi_tui::keybindings::set_keybindings(KeybindingsManager::new(None, None).tui_manager());
    guard
}

fn fake_tui() -> Arc<Tui> {
    Tui::main_screen_with(
        Arc::new(VirtualTerminal::new(120, 24)),
        TuiOptions { event_loop: Some(EventLoop::new_manual()), ..Default::default() },
    )
}

fn cwd() -> String {
    std::env::current_dir().unwrap().to_string_lossy().into_owned()
}

fn plain(component: &dyn Component, width: usize) -> String {
    strip_ansi(&component.render(width).join("\n")).unwrap()
}

fn result(value: Value) -> ToolExecutionComponentUpdateResultResult {
    serde_json::from_value(value).unwrap()
}

fn text_result(text: &str, is_error: bool) -> ToolExecutionComponentUpdateResultResult {
    result(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
}

fn component(
    name: &str,
    args: Value,
    definition: Option<ToolExecutionComponentToolDefinition>,
) -> ToolExecutionComponent {
    ToolExecutionComponent::new(name, "tool-1", args, None, definition.as_ref(), fake_tui(), &cwd())
}

fn definition(def: ToolDefinition) -> Option<ToolExecutionComponentToolDefinition> {
    Some(ToolExecutionComponentToolDefinition::ToolDefinition(def))
}

fn renderers(r: Option<ToolRenderers>) -> Option<ToolExecutionComponentToolDefinition> {
    r.map(ToolExecutionComponentToolDefinition::ToolRenderers)
}

fn base_definition(name: &str) -> ToolDefinition {
    ToolDefinition::new(name, name, "custom tool", json!({}), |_id, _params, _signal, _update, _ctx| async {
        Ok(AgentToolResult::text("ok", Some(json!({}))))
    })
}

fn text_renderer(text: &'static str) -> ComponentRef {
    Arc::new(Text::new(text, 0, 0, None))
}

fn with_call(mut def: ToolDefinition, text: &'static str) -> ToolDefinition {
    def.render_call = Some(Arc::new(move |_, _, _| text_renderer(text)));
    def
}

fn with_result(mut def: ToolDefinition, text: &'static str) -> ToolDefinition {
    def.render_result = Some(Arc::new(move |_, _, _, _| text_renderer(text)));
    def
}

fn bash_definition() -> ToolDefinition {
    create_bash_tool_definition(
        &cwd(),
        Some(&BashToolOptions { expose_session_environment: Some(false), ..Default::default() }),
    )
}

/// TS `vi.useFakeTimers(); vi.setSystemTime(0)`: a settable clock for tool timers.
struct FakeClock(Arc<Mutex<i64>>);

impl FakeClock {
    fn install() -> FakeClock {
        let now = Arc::new(Mutex::new(0));
        let reader = now.clone();
        set_tool_timer_clock(Some(Arc::new(move || *reader.lock())));
        FakeClock(now)
    }

    fn advance(&self, ms: i64) {
        *self.0.lock() += ms;
    }
}

impl Drop for FakeClock {
    fn drop(&mut self) {
        set_tool_timer_clock(None);
    }
}

// ---------------------------------------------------------------------------------------------
// tool-execution-component.test.ts
// ---------------------------------------------------------------------------------------------

#[test]
fn uses_built_in_rendering_for_built_in_overrides_without_custom_renderers() {
    let _g = setup();
    let c = component(
        "edit",
        json!({ "path": "README.md", "oldText": "before", "newText": "after" }),
        renderers(with_built_in_renderers("edit", Some(base_definition("edit").renderers()))),
    );
    c.update_result(
        &result(json!({ "content": [], "details": { "diff": "+1 after", "firstChangedLine": 1 }, "isError": false })),
        None,
    );
    let rendered = plain(&c, 120);
    assert!(rendered.contains("edit"), "{rendered}");
    assert!(rendered.contains("README.md"), "{rendered}");
    assert!(!rendered.contains(":1"), "{rendered}");
}

#[test]
fn preserves_legacy_file_path_rendering_for_built_in_tools() {
    let _g = setup();
    let c = component("read", json!({ "file_path": "README.md" }), None);
    let rendered = plain(&c, 120);
    assert!(rendered.contains("read") && rendered.contains("README.md"), "{rendered}");
}

#[tokio::test]
async fn bash_execute_emits_an_initial_empty_partial_update_before_output_arrives() {
    let updates: Arc<Mutex<Vec<Value>>> = Arc::default();
    let operations = BashOperations {
        exec: Arc::new(|_command, _cwd, _options| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                Ok(BashOperationsResult { exit_code: Some(0) })
            })
        }),
    };
    let tool = create_bash_tool_definition(
        &cwd(),
        Some(&BashToolOptions { operations: Some(operations), expose_session_environment: Some(false), ..Default::default() }),
    );
    let sink = updates.clone();
    let on_update: pi_agent::types::AgentToolUpdateCallback =
        Arc::new(move |update: AgentToolResult| sink.lock().push(serde_json::to_value(&update).unwrap()));
    let promise = (tool.execute)("tool-bash-1".into(), json!({ "command": "sleep 10" }), None, Some(on_update), None);
    let handle = tokio::spawn(promise);
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    assert_eq!(updates.lock().clone(), vec![json!({ "content": [] })]);
    handle.await.unwrap().unwrap();
}

#[tokio::test]
async fn bash_renderer_does_not_duplicate_final_full_output_truncation_details() {
    let _g = setup();
    let operations = BashOperations {
        exec: Arc::new(|_command, _cwd, options| {
            Box::pin(async move {
                for i in 1..=4000 {
                    (options.on_data)(format!("line-{i:04}\n").into_bytes());
                }
                Ok(BashOperationsResult { exit_code: Some(0) })
            })
        }),
    };
    let tool = create_bash_tool_definition(
        &cwd(),
        Some(&BashToolOptions { operations: Some(operations), expose_session_environment: Some(false), ..Default::default() }),
    );
    let output = (tool.execute)("tool-bash-1b".into(), json!({ "command": "generate output" }), None, None, None)
        .await
        .unwrap();
    let c = component("bash", json!({ "command": "generate output" }), definition(tool));
    c.set_expanded(true);
    let mut value = serde_json::to_value(&output).unwrap();
    value["isError"] = json!(false);
    c.update_result(&result(value), Some(false));

    let rendered = plain(&c, 200);
    assert_eq!(rendered.matches("Full output:").count(), 1, "{rendered}");
    assert!(regex::Regex::new(r"line-4000[^\n]*\n[^\S\n]*\n \[Full output:").unwrap().is_match(&rendered), "{rendered}");
    assert!(
        !regex::Regex::new(r"line-4000[^\n]*\n[^\S\n]*\n[^\S\n]*\n \[Full output:").unwrap().is_match(&rendered),
        "{rendered}"
    );
    assert!(rendered.contains("Truncated: showing 2000 of 4000 lines"), "{rendered}");
    assert!(!rendered.contains("[Showing lines 2001-4000 of 4000. Full output:"), "{rendered}");
}

// Issue #9628: keep short durations precise and make long shell durations readable.
#[test]
fn bash_renderer_formats_running_and_completed_durations() {
    let _g = setup();
    for (ms, formatted) in [
        (0, "0.0s"),
        (4_200, "4.2s"),
        (59_900, "59.9s"),
        (59_999, "60.0s"),
        (60_000, "1m 0s"),
        (90_900, "1m 30s"),
        (1_592_200, "26m 32s"),
        (3_599_999, "59m 59s"),
        (3_600_000, "1h 0m 0s"),
        (7_384_900, "2h 3m 4s"),
    ] {
        let clock = FakeClock::install();
        let c = component("bash", json!({ "command": "long-running-command" }), definition(bash_definition()));
        c.mark_execution_started();
        c.update_result(&result(json!({ "content": [], "isError": false })), Some(true));

        clock.advance(ms);
        c.invalidate();
        let running = plain(&c, 120);

        c.update_result(&result(json!({ "content": [], "isError": false })), Some(false));
        let completed = plain(&c, 120);

        clock.advance(1_000);
        c.invalidate();
        assert_eq!(plain(&c, 120), completed, "{ms}");
        assert!(running.contains(&format!("Elapsed {formatted}")), "{ms}: {running}");
        assert!(completed.contains(&format!("Took {formatted}")), "{ms}: {completed}");
    }
}

// #10549
#[test]
fn bash_renderer_shows_a_results_recorded_duration_also_when_restored_without_a_live_start() {
    let _g = setup();
    let render = |live: bool| -> String {
        let clock = FakeClock::install();
        let c = component("bash", json!({ "command": "sleep 4" }), definition(bash_definition()));
        if live {
            c.mark_execution_started();
            c.update_result(&result(json!({ "content": [], "isError": false })), Some(true));
            // The wall clock jumps; the recorded duration does not.
            clock.advance(3_600_000);
        }
        c.update_result(&result(json!({ "content": [], "isError": false, "durationMs": 4_200 })), Some(false));
        plain(&c, 120)
    };
    assert!(render(true).contains("Took 4.2s"));
    assert!(render(false).contains("Took 4.2s"));
}

#[test]
fn does_not_duplicate_built_in_headers_when_passed_the_active_built_in_definition() {
    let _g = setup();
    let c = component("read", json!({ "path": "README.md" }), definition(create_read_tool_definition(&cwd(), None)));
    c.update_result(&text_result("hello", false), Some(false));
    let rendered = plain(&c, 120);
    assert_eq!(regex::Regex::new(r"\bread\b").unwrap().find_iter(&rendered).count(), 1, "{rendered}");
}

// Issue #9996: strict tool schemas make models send null for omitted optional fields.
#[test]
fn renders_read_calls_with_null_offset_and_limit_as_full_file_reads() {
    let _g = setup();
    let c = component(
        "read",
        json!({ "path": "src/example.ts", "offset": null, "limit": null }),
        definition(create_read_tool_definition(&cwd(), None)),
    );
    let rendered = plain(&c, 120);
    assert!(rendered.contains("read src/example.ts"), "{rendered}");
    assert!(!rendered.contains("src/example.ts:"), "{rendered}");
}

#[test]
fn inherits_missing_built_in_result_renderer_slot() {
    let _g = setup();
    let def = with_call(base_definition("read"), "override call");
    let c = component("read", json!({ "path": "notes.txt" }), renderers(with_built_in_renderers("read", Some(def.renderers()))));
    c.update_result(&text_result("hello", false), Some(false));
    c.set_expanded(true);
    let rendered = plain(&c, 120);
    assert!(rendered.contains("override call") && rendered.contains("hello"), "{rendered}");
}

#[test]
fn inherits_missing_built_in_call_renderer_slot() {
    let _g = setup();
    let def = with_result(base_definition("read"), "override result");
    let c = component("read", json!({ "path": "README.md" }), renderers(with_built_in_renderers("read", Some(def.renderers()))));
    c.update_result(&text_result("hello", false), Some(false));
    let rendered = plain(&c, 120);
    assert!(rendered.contains("read") && rendered.contains("README.md"), "{rendered}");
    assert!(rendered.contains("override result"), "{rendered}");
}

#[test]
fn uses_custom_renderers_for_overrides_that_reuse_built_in_definition_parameters() {
    let _g = setup();
    let def = with_result(with_call(create_read_tool_definition(&cwd(), None), "override call"), "override result");
    let c = component("read", json!({ "path": "README.md" }), definition(def));
    c.update_result(&text_result("hello", false), Some(false));
    let rendered = plain(&c, 120);
    assert!(rendered.contains("override call") && rendered.contains("override result"), "{rendered}");
    assert!(!rendered.contains("read README.md"), "{rendered}");
}

#[test]
fn uses_custom_renderers_for_overrides_that_reuse_wrapped_built_in_tool_parameters() {
    let _g = setup();
    let built_in = create_read_tool(&cwd(), None);
    let mut def = base_definition("read");
    def.parameters = built_in.parameters().clone();
    let def = with_result(with_call(def, "wrapped override call"), "wrapped override result");
    let c = component("read", json!({ "path": "README.md" }), definition(def));
    c.update_result(&text_result("hello", false), Some(false));
    let rendered = plain(&c, 120);
    assert!(rendered.contains("wrapped override call") && rendered.contains("wrapped override result"), "{rendered}");
}

#[test]
fn trims_trailing_blank_display_lines_from_write_previews() {
    let _g = setup();
    let c = component(
        "write",
        json!({ "path": "README.md", "content": "one\ntwo\n" }),
        definition(create_write_tool_definition(&cwd(), None)),
    );
    let rendered = plain(&c, 120);
    assert!(rendered.contains("one") && rendered.contains("two"), "{rendered}");
    assert!(!rendered.contains("two\n\n"), "{rendered}");
}

#[test]
fn trims_trailing_blank_display_lines_from_read_results() {
    let _g = setup();
    let c = component("read", json!({ "path": "notes.txt" }), definition(create_read_tool_definition(&cwd(), None)));
    c.update_result(&text_result("one\ntwo\n", false), Some(false));
    c.set_expanded(true);
    let rendered = plain(&c, 120);
    assert!(rendered.contains("one") && rendered.contains("two"), "{rendered}");
    assert!(!rendered.contains("two\n\n"), "{rendered}");
}

#[test]
fn does_not_syntax_highlight_read_errors_based_on_the_requested_file_path() {
    let _g = setup();
    let c = component(
        "read",
        json!({ "path": "config.exs", "offset": 120, "limit": 130 }),
        definition(create_read_tool_definition(&cwd(), None)),
    );
    let error = "Offset 120 is beyond end of file (96 lines total)";
    c.update_result(&text_result(error, true), Some(false));
    let rendered = c.render(120).join("\n");
    assert!(strip_ansi(&rendered).unwrap().contains(error));
    assert!(rendered.contains(&theme().fg(&ThemeColor::ToolOutput, error)), "{rendered:?}");
}

#[test]
fn collapses_ordinary_read_results_until_expanded() {
    let _g = setup();
    let c = component("read", json!({ "path": "notes.txt" }), definition(create_read_tool_definition(&cwd(), None)));
    c.update_result(&text_result("hidden content", false), Some(false));
    let collapsed = plain(&c, 120);
    assert!(collapsed.contains("read") && collapsed.contains("notes.txt"), "{collapsed}");
    assert!(!collapsed.contains("hidden content"), "{collapsed}");
    c.set_expanded(true);
    assert!(plain(&c, 120).contains("hidden content"));
}

#[test]
fn renders_resource_reads_compactly_until_expanded() {
    let _g = setup();
    let cwd = cwd();
    let parent_agents = std::path::Path::new(&cwd).parent().unwrap().join("AGENTS.md").to_string_lossy().replace('\\', "/");
    let scenarios: Vec<(String, &str, String, &str, Option<&str>)> = vec![
        (
            format!("{cwd}/attio/SKILL.md"),
            "---\nname: attio\ndescription: CRM helper\n---\n\n# Hidden skill instructions",
            "[skill] attio".into(),
            "Hidden skill instructions",
            Some("read skill attio"),
        ),
        (format!("{cwd}/.pi/AGENTS.md"), "Hidden resource instructions", "read resource .pi/AGENTS.md".into(), "Hidden resource instructions", None),
        (
            format!("{cwd}/.pi/AGENTS.override.md"),
            "Hidden override instructions",
            "read resource .pi/AGENTS.override.md".into(),
            "Hidden override instructions",
            None,
        ),
        (
            parent_agents.clone(),
            "Hidden outside resource instructions",
            format!("read resource {parent_agents}"),
            "Hidden outside resource instructions",
            None,
        ),
        (get_readme_path(), "Hidden docs content", "read docs README.md".into(), "Hidden docs content", None),
    ];
    for (path, content, compact, hidden, absent) in scenarios {
        let c = component("read", json!({ "path": path }), definition(create_read_tool_definition(&cwd, None)));
        c.update_result(&text_result(content, false), Some(false));
        let collapsed = plain(&c, 120);
        assert!(collapsed.contains(&compact), "{path}: {collapsed}");
        assert!(!collapsed.contains(hidden), "{path}: {collapsed}");
        if let Some(absent) = absent {
            assert!(!collapsed.contains(absent), "{path}: {collapsed}");
        }
        c.set_expanded(true);
        assert!(plain(&c, 120).contains(hidden), "{path}");
    }
}

#[test]
fn shows_the_read_line_range_in_compact_reads_before_the_expand_hint() {
    let _g = setup();
    let cwd = cwd();
    for (path, compact) in [
        (format!("{cwd}/attio/SKILL.md"), "[skill] attio:120-329"),
        (get_readme_path(), "read docs README.md:120-329"),
    ] {
        let c = component(
            "read",
            json!({ "path": path, "offset": 120, "limit": 210 }),
            definition(create_read_tool_definition(&cwd, None)),
        );
        let collapsed = plain(&c, 120);
        assert!(collapsed.contains(compact), "{collapsed}");
        assert!(collapsed.find(":120-329").unwrap() < collapsed.find("to expand").unwrap(), "{collapsed}");
    }
}
