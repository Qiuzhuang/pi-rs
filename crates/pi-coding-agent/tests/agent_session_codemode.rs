//! Port of the parts of `test/suite/agent-session-codemode.test.ts` that need no mocked model
//! catalog: tool presentation per `codemode.mode`, nested calls, store persistence, and options.

mod support;

use std::sync::Arc;

use pi_agent::types::{AgentMessage, AgentToolRef, AgentToolResult, FnAgentTool};
use pi_ai::providers::faux::FauxResponseStep;
use pi_ai::types::{StopReason, ToolResultMessage, UserContentBlock};
use pi_coding_agent::core::extensions::types::{ExtensionAPIRef, ExtensionFactory};
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::core::tools::tool_definition_wrapper::create_tool_definition_from_agent_tool;
use pi_coding_agent::extensions::codemode::create_codemode_extension;
use pi_coding_agent::extensions::codemode::tool::{CODEMODE_STORE_ENTRY_TYPE, CodemodeToolDetails};
use serde_json::{Value, json};
use support::*;

fn echo_tool() -> AgentToolRef {
    Arc::new(FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back.\n\nSecond paragraph.",
        json!({ "type": "object", "properties": { "text": { "type": "string", "description": "Text to echo" } }, "required": ["text"] }),
        |_, params: Value, _, _| async move {
            Ok(AgentToolResult::text(format!("echo: {}", params["text"].as_str().unwrap_or_default()), Some(json!({}))))
        },
    ))
}

fn stats_tool() -> AgentToolRef {
    Arc::new(
        FnAgentTool::new("stats", "Stats", "Return structured stats", json!({ "type": "object", "properties": {} }), |_, _, _, _| async {
            Ok(AgentToolResult {
                content: vec![UserContentBlock::text("2 files")],
                details: Some(json!({})),
                structured_content: Some(json!({ "files": 2, "names": ["a", "b"] })),
                ..Default::default()
            })
        })
        .with_output_schema(json!({
            "type": "object",
            "properties": { "files": { "type": "number" }, "names": { "type": "array", "items": { "type": "string" } } },
            "required": ["files", "names"]
        })),
    )
}

fn register_tools() -> TestExtension {
    let factory: ExtensionFactory = Arc::new(|pi: ExtensionAPIRef| {
        Box::pin(async move {
            for tool in [echo_tool(), stats_tool()] {
                pi.register_tool(create_tool_definition_from_agent_tool(tool))?;
            }
            Ok(())
        })
    });
    TestExtension::new(factory)
}

async fn setup(with_tools: bool) -> Harness {
    let mut extension_factories = vec![TestExtension::new(create_codemode_extension(None))];
    if with_tools {
        extension_factories.push(register_tools());
    }
    create_harness(HarnessOptions {
        initial_active_tool_names: Some(vec!["codemode".to_string()]),
        extension_factories,
        ..Default::default()
    })
    .await
    .unwrap()
}

async fn run(harness: &Harness, code: &str) -> ToolResultMessage {
    let call = faux_tool_call("codemode", json!({ "code": code }).as_object().cloned().unwrap(), None);
    harness.set_responses(vec![
        FauxResponseStep::from(faux_assistant_message(
            vec![call],
            FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
        )),
        faux_text_message("done").into(),
    ]);
    harness.session.prompt("go", None).await.unwrap();
    harness.tool_result("codemode")
}

/// The output after the script header, which is checked on the way.
fn result_text(message: &ToolResultMessage) -> String {
    let header = message.content[0].as_text().expect("header").text.clone();
    let header_re = regex::Regex::new(r"\AScript (completed|failed)\nWall time \d+\.\d seconds\nOutput:\n\z").unwrap();
    assert!(header_re.is_match(&header), "{header:?}");
    message.content[1..]
        .iter()
        .map(|block| match block.as_text() {
            Some(text) => text.text.clone(),
            None => "<image>".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn details(message: &ToolResultMessage) -> CodemodeToolDetails {
    serde_json::from_value(message.details.clone().expect("details")).unwrap()
}

fn store_entries(harness: &Harness) -> Vec<Value> {
    harness
        .session_manager
        .get_branch(None)
        .into_iter()
        .filter_map(|entry| match entry {
            SessionEntry::Custom(entry) if entry.custom_type == CODEMODE_STORE_ENTRY_TYPE => entry.data,
            _ => None,
        })
        .collect()
}

fn description(harness: &Harness, name: &str) -> String {
    harness
        .session
        .agent()
        .state()
        .tools
        .iter()
        .find(|tool| tool.name() == name)
        .map(|tool| tool.description().to_string())
        .unwrap_or_default()
}

const INCREMENT: &str = "const next = (load(\"count\") ?? 0) + 1;\nstore(\"count\", next);\nreturn next;";

#[tokio::test]
async fn presents_callable_tools_per_codemode_mode() {
    let harness = setup(true).await;
    let names = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    harness.session.set_active_tools_by_name(&names(&["read", "echo", "codemode"]));
    assert!(description(&harness, "echo").contains("Codemode: `tools.echo(args)` resolves to"));
    assert!(!description(&harness, "echo").contains("codemode tool declaration:"));
    assert!(!description(&harness, "codemode").contains("### `echo`"));

    harness.settings_manager.apply_overrides_json(json!({ "codemode": { "mode": "only" } }).as_object().unwrap());
    harness.session.set_active_tools_by_name(&names(&["read", "echo", "codemode"]));
    assert!(!description(&harness, "echo").contains("Codemode: `tools.echo"));
    assert!(description(&harness, "codemode").contains("### `echo`"));
    assert!(!description(&harness, "codemode").contains("### `stats`"));

    harness.session.set_active_tools_by_name(&names(&["echo"]));
    assert_eq!(description(&harness, "echo"), "Echo text back.\n\nSecond paragraph.");
    harness.cleanup();
}

#[tokio::test]
async fn runs_nested_calls_in_parallel_and_returns_only_the_script_result() {
    let harness = setup(true).await;
    let result = run(
        &harness,
        r#"
            const [a, b, stats] = await Promise.all([
                tools.echo({ text: "one" }),
                tools.echo({ text: "two" }),
                tools.stats({}),
            ]);
            console.log("files", stats.files);
            text(ALL_TOOLS.map((tool) => tool.name).join(","));
            return { a, b, names: stats.names };
        "#,
    )
    .await;
    assert!(!result.is_error);
    assert_eq!(
        result_text(&result),
        "==> text 1/2 <==\necho,stats\n==> text 2/2 <==\n{\"a\":\"echo: one\",\"b\":\"echo: two\",\"names\":[\"a\",\"b\"]}\n<console_output>\nfiles 2\n</console_output>"
    );
    let calls = details(&result).calls;
    assert_eq!(
        calls.iter().map(|c| (c.name.as_str(), c.status.as_str())).collect::<Vec<_>>(),
        vec![("echo", "ok"), ("echo", "ok"), ("stats", "ok")]
    );
    assert!(calls.iter().all(|call| call.id.starts_with(&format!("{}/", result.tool_call_id))));
    let tool_results = harness.session.messages().iter().filter(|m| matches!(m, AgentMessage::ToolResult(_))).count();
    assert_eq!(tool_results, 1);
    harness.cleanup();
}

#[tokio::test]
async fn reports_script_failures_with_partial_output_and_the_calls_that_ran() {
    let harness = setup(true).await;
    let result = run(
        &harness,
        "text(\"partial\");\nconsole.log(\"log\");\nawait tools.echo({ text: \"x\" });\nthrow new Error(\"boom\");",
    )
    .await;
    assert!(result.is_error);
    let text = result_text(&result);
    assert!(text.starts_with("partial\n<console_output>\nlog\n</console_output>\nScript error:\n"), "{text}");
    assert!(text.contains("boom"));
    assert!(text.contains("Tool calls made before the failure (they are not undone): echo (ok)"));
    assert_eq!(details(&result).calls.iter().map(|c| c.name.clone()).collect::<Vec<_>>(), vec!["echo"]);
    harness.cleanup();
}

#[tokio::test]
async fn applies_the_timeout_option_and_rejects_invalid_options() {
    let harness = setup(false).await;
    let timed_out = run(&harness, "// @options: {\"timeout_ms\": 200}\nwhile (true) {}").await;
    assert!(timed_out.is_error);
    assert!(result_text(&timed_out).contains("Script error:\nScript timed out"));

    let invalid = run(&harness, "// @options: {\"yield\": 1}\ntext(1)").await;
    assert!(invalid.is_error);
    assert_eq!(invalid.content.len(), 1);
    assert_eq!(
        invalid.content[0].as_text().unwrap().text,
        "@options only supports `max_output_tokens` and `timeout_ms`; got `yield`"
    );

    let misplaced = run(&harness, "// count\n// @options: {\"timeout_ms\": 200}\nwhile (true) {}").await;
    assert!(misplaced.is_error);
    assert_eq!(
        misplaced.content[0].as_text().unwrap().text,
        "The // @options: line must be the first line of the input; found one on line 2"
    );
    harness.cleanup();
}

#[tokio::test]
async fn persists_store_writes_as_custom_entries_for_later_calls() {
    let harness = setup(false).await;
    assert_eq!(result_text(&run(&harness, INCREMENT).await), "1");
    assert_eq!(result_text(&run(&harness, INCREMENT).await), "2");
    assert_eq!(
        store_entries(&harness),
        vec![json!({ "set": { "count": 1 }, "delete": [] }), json!({ "set": { "count": 2 }, "delete": [] })]
    );
    let deleted = run(&harness, "store(\"count\", undefined);\nreturn load(\"count\") === undefined;").await;
    assert_eq!(result_text(&deleted), "true");
    assert_eq!(store_entries(&harness).last().cloned(), Some(json!({ "set": {}, "delete": ["count"] })));
    harness.cleanup();
}

#[tokio::test]
async fn appends_nothing_for_failed_scripts_or_scripts_without_writes() {
    let harness = setup(false).await;
    assert!(run(&harness, "store(\"count\", 5);\nthrow new Error(\"boom\");").await.is_error);
    assert!(!run(&harness, "return load(\"count\") ?? \"missing\";").await.is_error);
    assert!(store_entries(&harness).is_empty());
    harness.cleanup();
}

#[tokio::test]
async fn loads_the_values_written_on_the_current_branch() {
    let harness = setup(false).await;
    run(&harness, INCREMENT).await;
    let first_prompt = harness
        .session_manager
        .get_branch(None)
        .into_iter()
        .find_map(|entry| match entry {
            SessionEntry::Message(entry) => Some(entry.id),
            _ => None,
        })
        .expect("first prompt");
    assert_eq!(result_text(&run(&harness, INCREMENT).await), "2");
    harness.session_manager.branch(&first_prompt).unwrap();
    assert_eq!(result_text(&run(&harness, INCREMENT).await), "1");
    harness.cleanup();
}

#[tokio::test]
async fn truncates_output_to_the_token_budget_and_spills_the_full_text() {
    let harness = setup(false).await;
    let result = run(&harness, "// @options: {\"max_output_tokens\": 30}\nfor (let i = 0; i < 100; i++) text(\"row \" + i);").await;
    let path = details(&result).full_output_path.expect("spill file");
    let text = result_text(&result);
    assert!(text.starts_with("Warning: truncated output"));
    assert!(text.contains("row 0\n"));
    assert!(text.contains("tokens truncated"));
    assert!(text.contains("row 99"));
    assert!(!text.contains("row 50\n"));
    assert!(text.contains(&format!("[Full output: {path} (read with offset/limit)]")));
    let expected = (0..100).map(|i| format!("==> text {}/100 <==\nrow {i}", i + 1)).collect::<Vec<_>>().join("\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    let _ = std::fs::remove_file(&path);

    let small = run(&harness, "return { ok: true };").await;
    assert!(details(&small).full_output_path.is_none());
    harness.cleanup();
}
