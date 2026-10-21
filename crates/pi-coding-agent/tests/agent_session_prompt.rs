//! Port of `test/suite/agent-session-prompt.test.ts` (the parts that do not mock modules).

mod support;

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool};
use pi_ai::providers::faux::FauxResponseStep;
use pi_ai::types::{AssistantMessage, ImageContent, Message, StopReason, UserContent, UserContentBlock};
use pi_coding_agent::core::agent_session::{
    AgentSessionEvent, AgentSessionEventListener, AgentSessionSendCustomMessageMessage,
    AgentSessionSendCustomMessageOptions, AgentSessionSendUserMessageOptions, PromptOptions,
    PromptOptionsStreamingBehavior,
};
use pi_coding_agent::core::extensions::types::{
    BeforeAgentStartEvent, BeforeAgentStartEventResult, BeforeAgentStartKind, CommandOptions,
    ExtensionAPIRef, ExtensionFactory, InputEvent, InputEventResult, on_event,
};
use serde_json::json;
use support::*;

fn roles(messages: &[AgentMessage]) -> Vec<String> {
    messages.iter().map(|m| m.role().to_string()).collect()
}

fn message(content: Vec<pi_ai::types::AssistantContent>, stop_reason: Option<StopReason>) -> FauxResponseStep {
    faux_assistant_message(
        content,
        FauxMessageOptions {
            stop_reason,
            ..Default::default()
        },
    )
    .into()
}

fn text(text: &str) -> FauxResponseStep {
    faux_text_message(text).into()
}

fn tool_call(name: &str, args: serde_json::Value) -> pi_ai::types::AssistantContent {
    faux_tool_call(name, args.as_object().cloned().unwrap_or_default(), None)
}

fn factory<F, Fut>(f: F) -> TestExtension
where
    F: Fn(ExtensionAPIRef) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let f: ExtensionFactory = Arc::new(move |pi| Box::pin(f(pi)));
    TestExtension::new(f)
}

/// A tool that blocks until released, to observe the session while it streams.
fn wait_tool(release: Arc<tokio::sync::Notify>) -> pi_agent::types::AgentToolRef {
    FnAgentTool::new(
        "wait",
        "Wait",
        "Wait for release",
        json!({"type": "object", "properties": {}}),
        move |_id, _params, _signal, _update| {
            let release = release.clone();
            async move {
                release.notified().await;
                Ok(AgentToolResult::text("released", Some(json!({}))))
            }
        },
    )
    .into_ref()
}

/// Resolves once the session emits `tool_execution_start`.
fn wait_for_tool_start(harness: &Harness) -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if matches!(event, AgentSessionEvent::ToolExecutionStart { .. })
            && let Some(tx) = tx.lock().take()
        {
            let _ = tx.send(());
        }
    });
    harness.session.subscribe(&listener);
    rx
}

#[tokio::test]
async fn prompts_while_idle_and_records_a_single_text_response() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.set_responses(vec![text("hello")]);

    harness.session.prompt("hi", None).await.unwrap();

    assert_eq!(roles(&harness.session.messages()), ["system", "user", "assistant"]);
    assert_eq!(get_message_text(&harness.session.messages()[1]), "hi");
    assert_eq!(harness.get_pending_response_count(), 0);
    harness.cleanup();
}

#[tokio::test]
async fn handles_a_tool_call_turn_and_waits_for_the_follow_up_response() {
    let runs = Arc::new(Mutex::new(Vec::<String>::new()));
    let runs_clone = runs.clone();
    let echo = FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        move |_id, params: serde_json::Value, _signal, _update| {
            let runs = runs_clone.clone();
            async move {
                let text = params["text"].as_str().unwrap_or_default().to_string();
                runs.lock().push(text.clone());
                Ok(AgentToolResult::text(format!("echo:{text}"), Some(json!({"text": text}))))
            }
        },
    )
    .into_ref();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo]),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(vec![tool_call("echo", json!({"text": "hello"}))], Some(StopReason::ToolUse)),
        text("done"),
    ]);

    harness.session.prompt("start", None).await.unwrap();

    assert_eq!(*runs.lock(), vec!["hello".to_string()]);
    assert_eq!(
        roles(&harness.session.messages()),
        ["system", "user", "assistant", "toolResult", "assistant"]
    );
    harness.cleanup();
}

#[tokio::test]
async fn executes_multiple_tool_calls_and_continues_with_one_follow_up() {
    let runs = Arc::new(Mutex::new(Vec::<String>::new()));
    let make_tool = |name: &'static str, delay_ms: u64| {
        let runs = runs.clone();
        FnAgentTool::new(
            name,
            name,
            format!("{name} tool"),
            json!({"type": "object", "properties": {"value": {"type": "string"}}}),
            move |_id, params: serde_json::Value, _signal, _update| {
                let runs = runs.clone();
                async move {
                    let value = params["value"].as_str().unwrap_or_default().to_string();
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    runs.lock().push(format!("{name}:{value}"));
                    Ok(AgentToolResult::text(format!("{name}:{value}"), Some(json!({"value": value}))))
                }
            },
        )
        .into_ref()
    };
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![make_tool("slow", 25), make_tool("fast", 0)]),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(
            vec![
                tool_call("slow", json!({"value": "a"})),
                tool_call("fast", json!({"value": "b"})),
            ],
            Some(StopReason::ToolUse),
        ),
        FauxResponseStep::factory(|context, _options, _state, _model| {
            let results = context
                .messages
                .iter()
                .filter(|m| matches!(m, Message::ToolResult(_)))
                .count();
            faux_text_message(format!("tool results: {results}"))
        }),
    ]);

    harness.session.prompt("run tools", None).await.unwrap();

    let mut sorted = runs.lock().clone();
    sorted.sort();
    assert_eq!(sorted, vec!["fast:b".to_string(), "slow:a".to_string()]);
    let messages = harness.session.messages();
    assert_eq!(
        messages
            .iter()
            .filter(|m| matches!(m, AgentMessage::ToolResult(_)))
            .count(),
        2
    );
    assert_eq!(messages.last().unwrap().role(), "assistant");
    assert_eq!(get_message_text(messages.last().unwrap()), "tool results: 2");
    harness.cleanup();
}

#[tokio::test]
async fn send_user_message_while_idle_triggers_a_turn() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.set_responses(vec![text("response")]);

    harness
        .session
        .send_user_message(UserContent::Text("from extension".into()), None)
        .await
        .unwrap();

    assert_eq!(roles(&harness.session.messages()), ["system", "user", "assistant"]);
    assert_eq!(get_message_text(&harness.session.messages()[1]), "from extension");
    harness.cleanup();
}

#[tokio::test]
async fn preserves_image_attachments_in_the_provider_context() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let saw_image = Arc::new(Mutex::new(false));
    let saw = saw_image.clone();
    harness.set_responses(vec![FauxResponseStep::factory(move |context, _o, _s, _m| {
        let user = context.messages.iter().find_map(|m| match m {
            Message::User(u) => Some(u.clone()),
            _ => None,
        });
        *saw.lock() = user.is_some_and(|u| match &u.content {
            UserContent::Blocks(blocks) => blocks.iter().any(|b| matches!(b, UserContentBlock::Image(_))),
            UserContent::Text(_) => false,
        });
        faux_text_message("ok")
    })]);
    // Image processing (resize) needs the image module; disable auto-resize so the attachment
    // is passed through as given.
    harness.settings_manager.set_image_auto_resize(false);

    let result = harness
        .session
        .prompt(
            "describe",
            Some(&PromptOptions {
                images: Some(vec![ImageContent {
                    data: "ZmFrZQ==".into(),
                    mime_type: "image/png".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        )
        .await;
    result.unwrap();
    assert!(*saw_image.lock());
    harness.cleanup();
}

#[tokio::test]
async fn dispatches_extension_commands_without_consuming_a_provider_response() {
    let command_runs = Arc::new(Mutex::new(Vec::<String>::new()));
    let runs = command_runs.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![factory(move |pi| {
            let runs = runs.clone();
            async move {
                pi.register_command(
                    "testcmd",
                    CommandOptions {
                        description: Some("Test command".into()),
                        get_argument_completions: None,
                        handler: Arc::new(move |args, _ctx| {
                            let runs = runs.clone();
                            Box::pin(async move {
                                runs.lock().push(args);
                                Ok(())
                            })
                        }),
                    },
                )?;
                Ok(())
            }
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("should stay queued")]);

    harness.session.prompt("/testcmd hello world", None).await.unwrap();

    assert_eq!(*command_runs.lock(), vec!["hello world".to_string()]);
    assert!(harness.session.messages().is_empty());
    assert_eq!(harness.get_pending_response_count(), 1);
    harness.cleanup();
}

#[tokio::test]
async fn does_not_report_streaming_behavior_to_input_handlers_while_idle() {
    let input_events = Arc::new(Mutex::new(Vec::<InputEvent>::new()));
    let events = input_events.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![factory(move |pi| {
            let events = events.clone();
            async move {
                on_event::<InputEvent, _, _>(&*pi, move |event, _ctx| {
                    let events = events.clone();
                    async move {
                        events.lock().push(event);
                        Ok(None::<InputEventResult>)
                    }
                })?;
                Ok(())
            }
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("ok")]);

    harness
        .session
        .prompt(
            "idle",
            Some(&PromptOptions {
                streaming_behavior: Some(PromptOptionsStreamingBehavior::FollowUp),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let events = input_events.lock();
    assert_eq!(events.len(), 1);
    assert!(events[0].streaming_behavior.is_none());
    harness.cleanup();
}

#[tokio::test]
async fn throws_when_prompted_during_streaming_without_a_streaming_behavior() {
    let release = Arc::new(tokio::sync::Notify::new());
    let harness = Arc::new(
        create_harness(HarnessOptions {
            tools: Some(vec![wait_tool(release.clone())]),
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    harness.set_responses(vec![
        message(vec![tool_call("wait", json!({}))], Some(StopReason::ToolUse)),
        text("done"),
    ]);
    let saw_tool_start = wait_for_tool_start(&harness);
    let session = harness.session.clone();
    let run = tokio::spawn(async move { session.prompt("start", None).await });
    saw_tool_start.await.unwrap();

    let error = harness.session.prompt("second", None).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Agent is already processing. Specify streamingBehavior ('steer' or 'followUp') to queue the message."
    );

    release.notify_one();
    run.await.unwrap().unwrap();
    harness.cleanup();
}

#[tokio::test]
async fn reports_streaming_behavior_to_input_handlers_while_streaming() {
    let release = Arc::new(tokio::sync::Notify::new());
    let input_events = Arc::new(Mutex::new(Vec::<InputEvent>::new()));
    let events = input_events.clone();
    let harness = Arc::new(
        create_harness(HarnessOptions {
            tools: Some(vec![wait_tool(release.clone())]),
            extension_factories: vec![factory(move |pi| {
                let events = events.clone();
                async move {
                    on_event::<InputEvent, _, _>(&*pi, move |event, _ctx| {
                        let events = events.clone();
                        async move {
                            events.lock().push(event);
                            Ok(None::<InputEventResult>)
                        }
                    })?;
                    Ok(())
                }
            })],
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    harness.set_responses(vec![
        message(vec![tool_call("wait", json!({}))], Some(StopReason::ToolUse)),
        text("done"),
        text("follow-up answer"),
    ]);
    let saw_tool_start = wait_for_tool_start(&harness);
    let session = harness.session.clone();
    let run = tokio::spawn(async move { session.prompt("start", None).await });
    saw_tool_start.await.unwrap();
    harness
        .session
        .prompt(
            "queued",
            Some(&PromptOptions {
                streaming_behavior: Some(PromptOptionsStreamingBehavior::FollowUp),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let behaviors: Vec<_> = input_events
        .lock()
        .iter()
        .map(|e| e.streaming_behavior)
        .collect();
    assert_eq!(behaviors.len(), 2);
    assert!(behaviors[0].is_none());
    assert!(behaviors[1].is_some());

    release.notify_one();
    run.await.unwrap().unwrap();
    // The follow-up was delivered after the first answer.
    assert_eq!(harness.user_texts(), vec!["start".to_string(), "queued".to_string()]);
    harness.cleanup();
}

#[tokio::test]
async fn throws_when_prompting_without_configured_auth() {
    let harness = create_harness(HarnessOptions {
        with_configured_auth: Some(false),
        ..Default::default()
    })
    .await
    .unwrap();
    let error = harness.session.prompt("hi", None).await.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with(&format!("No API key found for {}.", harness.get_model().provider)),
        "{error}"
    );
    harness.cleanup();
}

#[tokio::test]
async fn custom_message_runs_keep_the_same_system_prompt_for_every_request() {
    let starts = Arc::new(Mutex::new(Vec::<String>::new()));
    let starts_clone = starts.clone();
    let noop = FnAgentTool::new(
        "noop",
        "Noop",
        "Does nothing",
        json!({"type": "object", "properties": {}}),
        |_id, _p, _s, _u| async { Ok(AgentToolResult::text("ok", Some(json!({})))) },
    )
    .into_ref();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![noop]),
        extension_factories: vec![factory(move |pi| {
            let starts = starts_clone.clone();
            async move {
                on_event::<BeforeAgentStartEvent, _, _>(&*pi, move |event, _ctx| {
                    let starts = starts.clone();
                    async move {
                        starts.lock().push(match &event.kind {
                            BeforeAgentStartKind::User { .. } => "user".into(),
                            BeforeAgentStartKind::Custom { message } => {
                                format!("custom:{}", message.custom_type)
                            }
                        });
                        let mut options = event.system_prompt_options.clone();
                        options
                            .sections
                            .insert("extension_rules".into(), "Answer in one word.".into());
                        Ok(Some(BeforeAgentStartEventResult {
                            system_prompt_options: Some(options),
                            ..Default::default()
                        }))
                    }
                })?;
                Ok(())
            }
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let record = |response: AssistantMessage| {
        let prompts = prompts.clone();
        FauxResponseStep::factory(move |context, _o, _s, _m| {
            prompts
                .lock()
                .push(pi_ai::get_current_system_prompt(&context.messages));
            response.clone()
        })
    };
    harness.set_responses(vec![
        record(faux_text_message("hi")),
        record(faux_assistant_message(
            vec![tool_call("noop", json!({}))],
            FauxMessageOptions {
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
        )),
        record(faux_text_message("done")),
    ]);

    harness.session.prompt("hello", None).await.unwrap();
    harness
        .session
        .send_custom_message(
            &AgentSessionSendCustomMessageMessage {
                custom_type: "task-done".into(),
                content: UserContent::Text("background task finished".into()),
                display: true,
                details: None,
            },
            Some(&AgentSessionSendCustomMessageOptions {
                trigger_turn: Some(true),
                deliver_as: None,
            }),
        )
        .await
        .unwrap();

    let prompts = prompts.lock().clone();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[0].contains("Answer in one word."));
    assert!(prompts.iter().all(|p| *p == prompts[0]));
    assert_eq!(
        *starts.lock(),
        vec!["user".to_string(), "custom:task-done".to_string()]
    );
    harness.cleanup();
}

#[tokio::test]
async fn send_user_message_can_opt_into_extension_command_dispatch() {
    let command_runs = Arc::new(Mutex::new(Vec::<String>::new()));
    let runs = command_runs.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![factory(move |pi| {
            let runs = runs.clone();
            async move {
                pi.register_command(
                    "testcmd",
                    CommandOptions {
                        description: Some("Test command".into()),
                        get_argument_completions: None,
                        handler: Arc::new(move |args, _ctx| {
                            let runs = runs.clone();
                            Box::pin(async move {
                                runs.lock().push(args);
                                Ok(())
                            })
                        }),
                    },
                )?;
                Ok(())
            }
        })],
        ..Default::default()
    })
    .await
    .unwrap();

    harness
        .session
        .send_user_message(
            UserContent::Text("/testcmd hello world".into()),
            Some(&AgentSessionSendUserMessageOptions {
                deliver_as: None,
                expand_prompt_templates: Some(true),
            }),
        )
        .await
        .unwrap();

    assert_eq!(*command_runs.lock(), vec!["hello world".to_string()]);
    assert!(harness.session.messages().is_empty());
    harness.cleanup();
}
