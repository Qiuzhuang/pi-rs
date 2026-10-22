//! Port of `test/suite/agent-session-tool-orchestration.test.ts`.

mod support;

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult};
use pi_ai::providers::faux::FauxResponseStep;
use pi_ai::types::{AssistantContent, Message, NestedToolCallStatus, StopReason, UserContentBlock};
use pi_ai::utils::transcript::get_current_tools;
use pi_coding_agent::core::agent_session::ExtensionBindings;
use pi_coding_agent::core::extensions::types::{
    ExecuteToolOptions, ExtensionAPIRef, ExtensionFactory, ToolCallEvent, ToolCallEventResult,
    ToolDefinition, ToolExposure, ToolLoadout, ToolLoadoutChanges, on_event,
};
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::extensions::codemode::create_codemode_extension;
use pi_coding_agent::extensions::tool_search::create_tool_search_extension;
use serde_json::{Value, json};
use support::*;

fn tool_call(name: &str, args: Value) -> AssistantContent {
    faux_tool_call(name, args.as_object().cloned().unwrap_or_default(), None)
}

fn tool_use(content: Vec<AssistantContent>) -> FauxResponseStep {
    faux_assistant_message(
        content,
        FauxMessageOptions {
            stop_reason: Some(StopReason::ToolUse),
            ..Default::default()
        },
    )
    .into()
}

fn text_result(text: String) -> AgentToolResult {
    AgentToolResult {
        content: vec![UserContentBlock::text(text)],
        details: Some(json!({})),
        ..Default::default()
    }
}

fn first_text(result: &AgentToolResult) -> String {
    match result.content.first() {
        Some(UserContentBlock::Text(t)) => t.text.clone(),
        _ => String::new(),
    }
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

/// A tool that calls other tools, built only on the extension API: its own name, exposure, loadout
/// hook, and ctx.execute_tool(). Codemode and tool search use the same mechanisms.
fn orchestrator_extension() -> TestExtension {
    let factory: ExtensionFactory = Arc::new(|pi: ExtensionAPIRef| {
        Box::pin(async move {
            pi.register_tool(ToolDefinition::new(
                "echo",
                "echo",
                "Echo text.",
                object_schema(json!({ "text": { "type": "string" } }), &["text"]),
                |_id, params: Value, _signal, _on_update, _ctx| async move {
                    let text = params.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
                    Ok(text_result(format!("echo: {text}")))
                },
            ))?;
            let mut helper = ToolDefinition::new(
                "helper",
                "helper",
                "Only reachable from other tools.",
                object_schema(json!({}), &[]),
                |_id, _params, _signal, _on_update, _ctx| async move { Ok(text_result("helped".into())) },
            );
            helper.exposure = Some(ToolExposure::Codemode);
            pi.register_tool(helper)?;
            let mut run_tools = ToolDefinition::new(
                "run_tools",
                "run_tools",
                "Runs tools.",
                object_schema(json!({}), &[]),
                |_id, _params, _signal, _on_update, ctx| async move {
                    let ctx = ctx.expect("tool context");
                    let helper = ctx.execute_tool("helper", json!({}), ExecuteToolOptions::default()).await?;
                    let echo = ctx.execute_tool("echo", json!({ "text": "hi" }), ExecuteToolOptions::default()).await?;
                    let this = ctx.execute_tool("run_tools", json!({}), ExecuteToolOptions::default()).await?;
                    let text = [helper, echo, this]
                        .iter()
                        .map(|outcome| first_text(&outcome.result))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    Ok(text_result(text))
                },
            );
            run_tools.exposure = Some(ToolExposure::ModelOnly);
            run_tools.prepare_loadout = Some(Arc::new(|loadout: &dyn ToolLoadout| {
                let callable = loadout.callable().iter().map(|t| t.name().to_string()).collect::<Vec<_>>();
                let mut descriptions = IndexMap::new();
                descriptions.insert("run_tools".to_string(), format!("Runs tools: {}", callable.join(", ")));
                descriptions.insert("echo".to_string(), "Echo text (also callable from run_tools).".to_string());
                Some(ToolLoadoutChanges {
                    descriptions: Some(descriptions),
                    hidden_declarations: Some(vec!["echo".to_string()]),
                })
            }));
            pi.register_tool(run_tools)?;
            Ok(())
        })
    });
    TestExtension::new(factory)
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}

fn tool_result_of(messages: &[AgentMessage]) -> Option<pi_ai::types::ToolResultMessage> {
    messages.iter().find_map(|m| match m {
        AgentMessage::ToolResult(r) => Some(r.clone()),
        _ => None,
    })
}

#[tokio::test]
async fn supports_tools_that_call_other_tools_under_any_name_through_the_extension_api() {
    let tool_calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = {
        let tool_calls = tool_calls.clone();
        let factory: ExtensionFactory = Arc::new(move |pi: ExtensionAPIRef| {
            let tool_calls = tool_calls.clone();
            Box::pin(async move {
                on_event::<ToolCallEvent, _, _>(&*pi, move |event, _ctx| {
                    let tool_calls = tool_calls.clone();
                    async move {
                        tool_calls.lock().push(format!(
                            "{}:{}",
                            event.tool_name,
                            event.parent_tool_call_id.as_deref().unwrap_or("top")
                        ));
                        Ok(None::<ToolCallEventResult>)
                    }
                })?;
                Ok(())
            })
        });
        TestExtension::new(factory)
    };
    let harness = create_harness(HarnessOptions {
        initial_active_tool_names: Some(Vec::new()),
        extension_factories: vec![orchestrator_extension(), recorder],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.session.bind_extensions(&ExtensionBindings::default()).await.unwrap();

    assert_eq!(harness.session.get_active_tool_names(), strings(&["echo", "run_tools"]));
    assert_eq!(harness.session.get_callable_tool_names(), strings(&["echo", "helper"]));
    let description = |name: &str| {
        harness
            .session
            .agent()
            .with_state(|s| s.tools.iter().find(|t| t.name() == name).map(|t| t.description().to_string()))
    };
    assert_eq!(description("run_tools").as_deref(), Some("Runs tools: echo, helper"));
    assert_eq!(description("echo").as_deref(), Some("Echo text (also callable from run_tools)."));

    let request_tools = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    {
        let request_tools = request_tools.clone();
        harness.set_responses(vec![
            FauxResponseStep::factory(move |context, _options, _state, _model| {
                let messages: &[Message] = &context.messages;
                request_tools
                    .lock()
                    .push(get_current_tools(messages).into_iter().map(|t| t.name).collect());
                faux_assistant_message(
                    vec![tool_call("run_tools", json!({}))],
                    FauxMessageOptions {
                        stop_reason: Some(StopReason::ToolUse),
                        ..Default::default()
                    },
                )
            }),
            faux_text_message("done").into(),
        ]);
    }
    harness.session.prompt("go", None).await.unwrap();

    // echo stays active, but its declaration is left out of requests.
    assert_eq!(request_tools.lock()[0], strings(&["run_tools"]));
    let result = tool_result_of(&harness.session.messages()).expect("No tool result");
    assert_eq!(
        result.content,
        vec![UserContentBlock::text("helped | echo: hi | Tool run_tools not found")]
    );
    let parent = result.tool_call_id.clone();
    assert_eq!(
        *tool_calls.lock(),
        vec![
            "run_tools:top".to_string(),
            format!("helper:{parent}"),
            format!("echo:{parent}"),
        ]
    );
    let nested = result.nested_calls.clone().expect("nested calls");
    let summary: Vec<(String, String, NestedToolCallStatus)> =
        nested.calls.iter().map(|c| (c.id.clone(), c.name.clone(), c.status)).collect();
    assert_eq!(
        summary,
        vec![
            (format!("{parent}/1"), "helper".to_string(), NestedToolCallStatus::Ok),
            (format!("{parent}/2"), "echo".to_string(), NestedToolCallStatus::Ok),
            (format!("{parent}/3"), "run_tools".to_string(), NestedToolCallStatus::Error),
        ]
    );
    // The record is persisted with the session.
    let persisted = harness.session_manager.get_branch(None).into_iter().find_map(|entry| match entry {
        SessionEntry::Message(e) => match e.message {
            AgentMessage::ToolResult(r) => Some(r),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(persisted.and_then(|r| r.nested_calls), Some(nested));
    harness.cleanup();
}

#[tokio::test]
async fn registers_codemode_and_tool_search_inactive_until_they_are_named() {
    let factories = || {
        vec![
            TestExtension::new(create_codemode_extension(None)),
            TestExtension::new(create_tool_search_extension()),
        ]
    };
    let plain = create_harness(HarnessOptions {
        extension_factories: factories(),
        ..Default::default()
    })
    .await
    .unwrap();
    let all: Vec<String> = plain.session.get_all_tools().into_iter().map(|t| t.name).collect();
    assert!(all.contains(&"codemode".to_string()), "{all:?}");
    assert!(all.contains(&"tool_search".to_string()), "{all:?}");
    assert_eq!(plain.session.get_active_tool_names(), strings(&["read", "bash", "edit", "write"]));
    plain.cleanup();

    // --tools and the defaultTools setting name them explicitly.
    let allowed = create_harness(HarnessOptions {
        allowed_tool_names: Some(strings(&["read", "codemode"])),
        extension_factories: factories(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(allowed.session.get_active_tool_names(), strings(&["read", "codemode"]));
    allowed.cleanup();
    let initial = create_harness(HarnessOptions {
        initial_active_tool_names: Some(strings(&["tool_search"])),
        extension_factories: factories(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(initial.session.get_active_tool_names(), strings(&["tool_search"]));
    initial.cleanup();
}

#[tokio::test]
async fn leaves_results_without_nested_calls_unchanged() {
    let harness = create_harness(HarnessOptions {
        initial_active_tool_names: Some(Vec::new()),
        extension_factories: vec![orchestrator_extension()],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.session.bind_extensions(&ExtensionBindings::default()).await.unwrap();
    harness.set_responses(vec![
        tool_use(vec![tool_call("echo", json!({ "text": "x" }))]),
        faux_text_message("done").into(),
    ]);
    harness.session.prompt("go", None).await.unwrap();
    let result = tool_result_of(&harness.session.messages()).expect("No tool result");
    assert!(result.nested_calls.is_none());
    harness.cleanup();
}
