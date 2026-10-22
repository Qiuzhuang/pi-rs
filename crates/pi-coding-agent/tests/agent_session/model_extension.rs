//! Port of `test/suite/agent-session-model-extension.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool, ThinkingLevel};
use pi_ai::providers::faux::FauxModelDefinition;
use pi_ai::types::{Message, TextContent, UserContent, UserContentBlock};
use pi_coding_agent::core::agent_session::{
    AgentSessionSetScopedModelsScopedModels, ExtensionBindings, ModelMutationOptions,
};
use pi_coding_agent::core::extensions::types::{
    BeforeAgentStartEvent, BeforeAgentStartEventResult, CommandOptions, ContextEvent,
    ContextEventResult, CustomMessageInput, InputEvent, InputEventResult, ModelSelectEvent,
    SessionShutdownEvent, SessionStartEvent, ToolCallEvent, ToolCallEventResult, ToolResultEvent,
    ToolResultEventResult, on_event,
};
use pi_coding_agent::core::session_manager::SessionEntry;
use serde_json::{Value, json};

use crate::common::*;
use crate::support::*;

fn model(id: &str, name: Option<&str>, reasoning: bool) -> FauxModelDefinition {
    FauxModelDefinition {
        id: id.into(),
        name: name.map(str::to_string),
        reasoning: Some(reasoning),
        ..Default::default()
    }
}

fn two_models() -> Vec<FauxModelDefinition> {
    vec![model("faux-1", Some("One"), true), model("faux-2", Some("Two"), true)]
}

fn persist() -> ModelMutationOptions {
    ModelMutationOptions { persist: Some(true) }
}

fn tool_result_text(context: &pi_ai::types::TranscriptContext) -> String {
    context
        .messages
        .iter()
        .find(|m| matches!(m, Message::ToolResult(_)))
        .map(llm_text)
        .unwrap_or_default()
}

fn first_user_text(context: &pi_ai::types::TranscriptContext) -> String {
    context
        .messages
        .iter()
        .find(|m| matches!(m, Message::User(_)))
        .map(llm_text)
        .unwrap_or_default()
}

#[tokio::test]
async fn set_model_saves_the_model_to_the_session_and_emits_model_select() {
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let e = events.clone();
    let harness = create_harness(HarnessOptions {
        models: two_models(),
        extension_factories: vec![ext(move |pi| {
            let e = e.clone();
            on_event::<ModelSelectEvent, _, _>(&*pi, move |event, _ctx| {
                let e = e.clone();
                async move {
                    let source = serde_json::to_value(event.source).unwrap();
                    e.lock().push(format!(
                        "{}->{}:{}",
                        event.previous_model.as_ref().map(|m| m.id.as_str()).unwrap_or("none"),
                        event.model.id,
                        source.as_str().unwrap()
                    ));
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let next = harness.get_model_by_id("faux-2").unwrap();

    harness.session.set_model(next.clone(), None).await.unwrap();

    assert_eq!(harness.session.model().unwrap().id, "faux-2");
    assert_eq!(*events.lock(), vec!["faux-1->faux-2:set"]);
    let changes: Vec<String> = harness
        .session_manager
        .get_entries()
        .iter()
        .filter_map(|e| match e {
            SessionEntry::ModelChange(c) => Some(format!("{}/{}", c.provider, c.model_id)),
            _ => None,
        })
        .collect();
    assert_eq!(changes, vec![format!("{}/{}", next.provider, next.id)]);
    assert!(harness.settings_manager.get_default_provider().is_none());
    assert!(harness.settings_manager.get_default_model().is_none());
    harness.cleanup();
}

#[tokio::test]
async fn only_persists_model_and_thinking_defaults_when_requested() {
    let harness = create_harness(HarnessOptions {
        models: two_models(),
        ..Default::default()
    })
    .await
    .unwrap();
    let next = harness.get_model_by_id("faux-2").unwrap();
    let settings = &harness.settings_manager;

    harness.session.set_model(next.clone(), None).await.unwrap();
    assert!(settings.get_default_provider().is_none());
    assert!(settings.get_default_model().is_none());

    harness.session.set_thinking_level(ThinkingLevel::Low, None);
    assert!(settings.get_default_thinking_level().is_none());

    harness.session.set_model(next.clone(), Some(&persist())).await.unwrap();
    assert_eq!(settings.get_default_provider(), Some(next.provider.clone()));
    assert_eq!(settings.get_default_model(), Some(next.id.clone()));

    harness.session.set_thinking_level(ThinkingLevel::High, Some(&persist()));
    assert_eq!(settings.get_default_thinking_level(), Some(ThinkingLevel::High));
    harness.cleanup();
}

#[tokio::test]
async fn persists_the_requested_default_thinking_level_even_when_the_current_model_clamps_it() {
    let harness = create_harness(HarnessOptions {
        models: vec![model("faux-1", None, true)],
        ..Default::default()
    })
    .await
    .unwrap();

    harness.session.set_thinking_level(ThinkingLevel::Max, Some(&persist()));

    assert_eq!(harness.session.thinking_level(), ThinkingLevel::High);
    assert_eq!(harness.settings_manager.get_default_thinking_level(), Some(ThinkingLevel::Max));
    harness.cleanup();
}

#[tokio::test]
async fn cycle_model_and_cycle_thinking_level_are_session_only_by_default() {
    let harness = create_harness(HarnessOptions {
        models: two_models(),
        settings: Some(json!({"defaultProvider": "faux", "defaultModel": "faux-1", "defaultThinkingLevel": "low"})),
        ..Default::default()
    })
    .await
    .unwrap();

    harness.session.cycle_model(None, None).await.unwrap();
    assert_eq!(harness.session.model().unwrap().id, "faux-2");
    assert_eq!(harness.settings_manager.get_default_model().as_deref(), Some("faux-1"));

    harness.session.set_thinking_level(ThinkingLevel::Off, None);
    assert_eq!(harness.session.cycle_thinking_level(None), Some(ThinkingLevel::Minimal));
    assert_eq!(harness.settings_manager.get_default_thinking_level(), Some(ThinkingLevel::Low));
    harness.cleanup();
}

#[tokio::test]
async fn applies_per_model_thinking_level_override_on_model_switch() {
    let harness = create_harness(HarnessOptions {
        models: two_models(),
        settings: Some(json!({"defaultThinkingLevel": "medium"})),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.settings_manager.set_model_thinking_level("faux", "faux-2", ThinkingLevel::Low);

    harness.session.set_thinking_level(ThinkingLevel::High, None);
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::High);

    harness.session.set_model(harness.get_model_by_id("faux-2").unwrap(), None).await.unwrap();
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::Low);

    harness.session.set_model(harness.get_model_by_id("faux-1").unwrap(), None).await.unwrap();
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::Medium);
    harness.cleanup();
}

#[tokio::test]
async fn falls_back_to_current_session_thinking_level_when_no_per_model_or_global_default_is_configured() {
    let harness = create_harness(HarnessOptions {
        models: two_models(),
        ..Default::default()
    })
    .await
    .unwrap();

    harness.session.set_thinking_level(ThinkingLevel::High, None);
    harness.session.set_model(harness.get_model_by_id("faux-2").unwrap(), None).await.unwrap();
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::High);
    harness.cleanup();
}

#[tokio::test]
async fn per_model_override_takes_priority_over_global_default_during_model_switch() {
    let harness = create_harness(HarnessOptions {
        models: two_models(),
        settings: Some(json!({"defaultThinkingLevel": "high", "modelThinkingLevels": {"faux/faux-2": "minimal"}})),
        ..Default::default()
    })
    .await
    .unwrap();

    harness.session.set_model(harness.get_model_by_id("faux-2").unwrap(), None).await.unwrap();
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::Minimal);
    harness.cleanup();
}

#[tokio::test]
async fn cycles_through_scoped_models_and_preserves_the_scoped_thinking_preference() {
    let harness = create_harness(HarnessOptions {
        models: vec![model("faux-1", Some("One"), true), model("faux-2", Some("Two"), false)],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.session.set_scoped_models(&[
        AgentSessionSetScopedModelsScopedModels {
            model: harness.get_model_by_id("faux-1").unwrap(),
            thinking_level: Some(ThinkingLevel::High),
        },
        AgentSessionSetScopedModelsScopedModels {
            model: harness.get_model_by_id("faux-2").unwrap(),
            thinking_level: None,
        },
    ]);
    harness.session.set_thinking_level(ThinkingLevel::High, None);

    harness.session.cycle_model(None, None).await.unwrap();
    assert_eq!(harness.session.model().unwrap().id, "faux-2");
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::Off);

    harness.session.cycle_model(None, None).await.unwrap();
    assert_eq!(harness.session.model().unwrap().id, "faux-1");
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::High);
    harness.cleanup();
}

#[tokio::test]
async fn clamps_thinking_levels_to_model_capabilities_and_cycles_available_levels() {
    let harness = create_harness(HarnessOptions {
        models: vec![model("faux-1", None, false)],
        ..Default::default()
    })
    .await
    .unwrap();

    harness.session.set_thinking_level(ThinkingLevel::High, None);
    assert_eq!(harness.session.thinking_level(), ThinkingLevel::Off);
    assert_eq!(harness.session.cycle_thinking_level(None), None);
    harness.cleanup();
}

#[tokio::test]
async fn cycles_xhigh_before_max_when_both_are_supported() {
    let harness = create_harness(HarnessOptions {
        models: vec![model("faux-1", None, true)],
        ..Default::default()
    })
    .await
    .unwrap();
    // TS mutates the model object the agent holds.
    let mut current = harness.session.agent().model();
    current.thinking_level_map =
        Some(serde_json::from_value(json!({"xhigh": "xhigh", "max": "max"})).unwrap());
    harness.session.agent().set_model(current);

    assert_eq!(
        harness.session.get_available_thinking_levels(),
        vec![
            ThinkingLevel::Off,
            ThinkingLevel::Minimal,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::Xhigh,
            ThinkingLevel::Max,
        ]
    );
    harness.session.set_thinking_level(ThinkingLevel::High, None);
    assert_eq!(harness.session.cycle_thinking_level(None), Some(ThinkingLevel::Xhigh));
    assert_eq!(harness.session.cycle_thinking_level(None), Some(ThinkingLevel::Max));
    assert_eq!(harness.session.cycle_thinking_level(None), Some(ThinkingLevel::Off));
    harness.cleanup();
}

#[tokio::test]
async fn throws_when_set_model_is_called_without_configured_auth() {
    let harness = create_harness(HarnessOptions {
        models: two_models(),
        with_configured_auth: Some(false),
        ..Default::default()
    })
    .await
    .unwrap();

    let error = harness
        .session
        .set_model(harness.get_model_by_id("faux-2").unwrap(), None)
        .await
        .unwrap_err();
    let expected = format!("No API key for {}/faux-2", harness.get_model().provider);
    assert!(error.to_string().contains(&expected), "{error}");
    harness.cleanup();
}

fn echo_tool(fail: bool) -> pi_agent::types::AgentToolRef {
    FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        move |_id, params: Value, _s, _u| async move {
            if fail {
                anyhow::bail!("tool should have been blocked");
            }
            let text = params["text"].as_str().unwrap_or_default().to_string();
            let mut result = AgentToolResult::text(text.clone(), Some(json!({"text": text})));
            result.usage = Some(
                serde_json::from_value(json!({
                    "input": 1, "output": 2, "cacheRead": 3, "cacheWrite": 4, "totalTokens": 10,
                    "cost": {"input": 0.1, "output": 0.2, "cacheRead": 0.3, "cacheWrite": 0.4, "total": 1}
                }))
                .unwrap(),
            );
            Ok(result)
        },
    )
    .into_ref()
}

#[tokio::test]
async fn allows_extension_tool_call_handlers_to_block_tool_execution() {
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo_tool(true)]),
        extension_factories: vec![ext(|pi| {
            on_event::<ToolCallEvent, _, _>(&*pi, |_event, _ctx| async move {
                Ok(Some(ToolCallEventResult {
                    block: Some(true),
                    reason: Some("Blocked by test".into()),
                    ..Default::default()
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        tool_use("echo", json!({"text": "hello"})),
        respond_with(|context| faux_text_message(tool_result_text(context))),
    ]);

    harness.session.prompt("hi", None).await.unwrap();

    assert!(harness.assistant_texts().contains(&"Blocked by test".to_string()));
    assert!(
        harness
            .session
            .messages()
            .iter()
            .any(|m| matches!(m, AgentMessage::ToolResult(r) if r.is_error))
    );
    harness.cleanup();
}

#[tokio::test]
async fn allows_extension_tool_result_handlers_to_modify_tool_results() {
    let patched_usage: pi_ai::types::Usage = serde_json::from_value(json!({
        "input": 5, "output": 6, "cacheRead": 7, "cacheWrite": 8, "totalTokens": 26,
        "cost": {"input": 0.5, "output": 0.6, "cacheRead": 0.7, "cacheWrite": 0.8, "total": 2.6}
    }))
    .unwrap();
    let observed = Arc::new(Mutex::new(None::<pi_ai::types::Usage>));
    let (o, pu) = (observed.clone(), patched_usage.clone());
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo_tool(false)]),
        extension_factories: vec![ext(move |pi| {
            let (o, pu) = (o.clone(), pu.clone());
            on_event::<ToolResultEvent, _, _>(&*pi, move |event, _ctx| {
                let (o, pu) = (o.clone(), pu.clone());
                async move {
                    *o.lock() = event.usage.clone();
                    Ok(Some(ToolResultEventResult {
                        content: Some(vec![UserContentBlock::Text(TextContent::new("patched result"))]),
                        details: Some(json!({"patched": true})),
                        usage: Some(pu),
                        ..Default::default()
                    }))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        tool_use("echo", json!({"text": "hello"})),
        respond_with(|context| faux_text_message(tool_result_text(context))),
    ]);

    harness.session.prompt("hi", None).await.unwrap();

    assert!(harness.assistant_texts().contains(&"patched result".to_string()));
    let result = harness
        .session
        .messages()
        .into_iter()
        .find_map(|m| match m {
            AgentMessage::ToolResult(r) if r.details.as_ref().is_some_and(|d| d["patched"] == json!(true)) => {
                Some(r)
            }
            _ => None,
        })
        .expect("patched tool result");
    let expected_tool_usage: pi_ai::types::Usage = serde_json::from_value(json!({
        "input": 1, "output": 2, "cacheRead": 3, "cacheWrite": 4, "totalTokens": 10,
        "cost": {"input": 0.1, "output": 0.2, "cacheRead": 0.3, "cacheWrite": 0.4, "total": 1}
    }))
    .unwrap();
    assert_eq!(*observed.lock(), Some(expected_tool_usage));
    assert_eq!(result.usage, Some(patched_usage));
    harness.cleanup();
}

#[tokio::test]
async fn allows_extension_context_handlers_to_modify_messages_before_the_llm_call() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            on_event::<ContextEvent, _, _>(&*pi, |event, _ctx| async move {
                let messages = event
                    .messages
                    .into_iter()
                    .map(|m| match m {
                        AgentMessage::User(mut user) => {
                            user.content = UserContent::Blocks(vec![UserContentBlock::Text(TextContent::new("rewritten"))]);
                            AgentMessage::User(user)
                        }
                        other => other,
                    })
                    .collect();
                Ok(Some(ContextEventResult { messages: Some(messages) }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let provider_text = Arc::new(Mutex::new(String::new()));
    let p = provider_text.clone();
    harness.set_responses(vec![respond_with(move |context| {
        *p.lock() = first_user_text(context);
        faux_text_message("done")
    })]);

    harness.session.prompt("original", None).await.unwrap();

    assert_eq!(*provider_text.lock(), "rewritten");
    let stored = harness
        .session
        .messages()
        .into_iter()
        .find_map(|m| match m {
            AgentMessage::User(u) => Some(u),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(&stored.content).unwrap(),
        json!([{"type": "text", "text": "original"}])
    );
    harness.cleanup();
}

#[tokio::test]
async fn allows_extension_input_handlers_to_transform_or_handle_input() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            on_event::<InputEvent, _, _>(&*pi, |event, _ctx| async move {
                if event.text == "ping" {
                    return Ok(Some(InputEventResult::Handled));
                }
                Ok(Some(InputEventResult::Transform {
                    text: format!("transformed:{}", event.text),
                    images: None,
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let provider_text = Arc::new(Mutex::new(String::new()));
    let p = provider_text.clone();
    harness.set_responses(vec![respond_with(move |context| {
        *p.lock() = first_user_text(context);
        faux_text_message("done")
    })]);

    harness.session.prompt("hello", None).await.unwrap();
    harness.session.prompt("ping", None).await.unwrap();

    assert_eq!(*provider_text.lock(), "transformed:hello");
    assert_eq!(
        harness
            .session
            .messages()
            .iter()
            .filter(|m| matches!(m, AgentMessage::User(_)))
            .count(),
        1
    );
    harness.cleanup();
}

/// TS asserts that `ctx.getSystemPromptOptions()` returns the same live object both times, so a
/// command's mutation is visible to the next call. Rust returns copies; the port checks the values.
#[tokio::test]
async fn allows_extension_commands_to_inspect_live_system_prompt_options() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let s = seen.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let s = s.clone();
            pi.register_command(
                "inspect-options",
                CommandOptions {
                    description: Some("Inspect system prompt options".into()),
                    get_argument_completions: None,
                    handler: Arc::new(move |_args, ctx| {
                        let s = s.clone();
                        Box::pin(async move {
                            let options = ctx.get_system_prompt_options()?;
                            s.lock().push(serde_json::to_value(&options)?);
                            Ok(())
                        })
                    }),
                },
            )
        })],
        ..Default::default()
    })
    .await
    .unwrap();

    harness.session.prompt("/inspect-options", None).await.unwrap();
    harness.session.prompt("/inspect-options", None).await.unwrap();

    let seen = seen.lock();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0], seen[1]);
    assert_eq!(seen[0]["cwd"], json!(harness.temp_dir.to_string_lossy()));
    assert!(seen[0]["selectedTools"].as_array().unwrap().contains(&json!("read")));
    harness.cleanup();
}

#[tokio::test]
async fn allows_before_agent_start_handlers_to_inject_custom_messages_and_modify_the_system_prompt() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            on_event::<BeforeAgentStartEvent, _, _>(&*pi, |event, _ctx| async move {
                Ok(Some(BeforeAgentStartEventResult {
                    message: Some(CustomMessageInput {
                        custom_type: "before-start".into(),
                        content: UserContent::Text("injected".into()),
                        display: true,
                        details: Some(json!({"injected": true})),
                    }),
                    system_prompt: Some(format!("{}\n\nextra instructions", event.system_prompt)),
                    system_prompt_options: None,
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let captured = Arc::new(Mutex::new((String::new(), false)));
    let c = captured.clone();
    harness.set_responses(vec![respond_with(move |context| {
        let prompt = pi_ai::get_current_system_prompt(&context.messages);
        let injected = context.messages.iter().any(|m| match m {
            Message::User(u) => matches!(&u.content, UserContent::Blocks(b)
                if b.iter().any(|p| p.as_text().is_some_and(|t| t.text == "injected"))),
            _ => false,
        });
        *c.lock() = (prompt, injected);
        faux_text_message("done")
    })]);

    harness.session.prompt("hello", None).await.unwrap();

    let (prompt, injected) = captured.lock().clone();
    assert!(prompt.contains("extra instructions"));
    assert!(injected);
    assert!(harness.session.messages().iter().any(|m| matches!(
        m,
        AgentMessage::Custom(c) if c.get("customType").and_then(Value::as_str) == Some("before-start")
    )));
    harness.cleanup();
}

#[tokio::test]
async fn bind_extensions_emits_session_start_and_reload_emits_session_shutdown_then_session_start() {
    // `reload()` resets the global api registry.
    let _exclusive = exclusive_api_registry().await;
    let lifecycle = Arc::new(Mutex::new(Vec::<String>::new()));
    let l = lifecycle.clone();
    let harness = create_harness(HarnessOptions {
        exclusive_api_registry: true,
        extension_factories: vec![ext(move |pi| {
            let l1 = l.clone();
            on_event::<SessionStartEvent, _, _>(&*pi, move |event, _ctx| {
                let l = l1.clone();
                async move {
                    let reason = serde_json::to_value(event.reason).unwrap();
                    l.lock().push(format!("start:{}", reason.as_str().unwrap()));
                    Ok(None)
                }
            })?;
            let l2 = l.clone();
            on_event::<SessionShutdownEvent, _, _>(&*pi, move |event, _ctx| {
                let l = l2.clone();
                async move {
                    let reason = serde_json::to_value(event.reason).unwrap();
                    l.lock().push(format!("shutdown:{}", reason.as_str().unwrap()));
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();

    harness
        .session
        .bind_extensions(&ExtensionBindings {
            shutdown_handler: Some(Arc::new(|| {})),
            ..Default::default()
        })
        .await
        .unwrap();
    harness.session.reload(None).await.unwrap();

    assert_eq!(*lifecycle.lock(), vec!["start:startup", "shutdown:reload", "start:reload"]);
    harness.cleanup();
}
