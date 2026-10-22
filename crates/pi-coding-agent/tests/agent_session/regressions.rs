//! Ports of the session regressions in `test/suite/regressions/` (one test per file unless noted).

use std::sync::Arc;

use futures::FutureExt;
use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, FnAgentTool};
use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session::AgentSessionNavigateTreeOptions;
use pi_coding_agent::core::extensions::types::{
    ExtensionAPIRef, ExtensionEventResult, MessageEndEvent, MessageEndEventResult, ToolCallEvent,
    ToolCallEventResult, on_event,
};
use pi_coding_agent::core::session_manager::SessionEntry;
use serde_json::{Value, json};

use crate::common::*;
use crate::retry_events::{error_response, retry_settings};
use crate::support::*;

async fn retries_once(error_message: &str, reply: &str) {
    let harness = create_harness(HarnessOptions {
        settings: retry_settings(3, 1),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![error_response(error_message), text(reply)]);

    harness.session.prompt("test", None).await.unwrap();

    assert_eq!(harness.faux.state().call_count, 2);
    let starts: Vec<Value> = harness
        .events_of_type("auto_retry_start")
        .iter()
        .map(|e| e["errorMessage"].clone())
        .collect();
    assert_eq!(starts, vec![json!(error_message)]);
    let ends: Vec<Value> = harness.events_of_type("auto_retry_end").iter().map(|e| e["success"].clone()).collect();
    assert_eq!(ends, vec![json!(true)]);
    assert!(harness.assistant_texts().contains(&reply.to_string()));
    harness.cleanup();
}

// #6904
#[tokio::test]
async fn retries_a_transient_dns_lookup_failure() {
    retries_once(
        "The pending stream has been canceled (caused by: getaddrinfo ENOTFOUND bedrock-runtime.us-east-1.amazonaws.com)",
        "recovered after DNS retry",
    )
    .await;
}

// #6019
#[tokio::test]
async fn retries_openai_explicit_retry_guidance() {
    retries_once(
        "An error occurred while processing your request. You can retry your request, or contact us through our help center at help.openai.com if the error persists. Please include the request ID req_******** in your message.",
        "recovered",
    )
    .await;
}

// #6019
#[tokio::test]
async fn retries_bedrock_explicit_retry_guidance() {
    retries_once(
        r#"{"message":"The system encountered an unexpected error during processing. Try your request again."}"#,
        "recovered",
    )
    .await;
}

// #3317
#[tokio::test]
async fn retries_transient_network_connection_lost_failures() {
    retries_once("Network connection lost.", "recovered after reconnect").await;
}

// tree-during-streaming
#[tokio::test]
async fn rejects_tree_navigation_during_an_active_response_without_changing_the_leaf() {
    let harness = Arc::new(create_harness(HarnessOptions::default()).await.unwrap());
    let target = harness.session_manager.append_message(user_msg("first")).unwrap();
    let observed = Arc::new(Mutex::new(None::<(Option<String>, bool)>));
    let (o, weak, t) = (observed.clone(), Arc::downgrade(&harness), target.clone());
    harness.set_responses(vec![respond_with(move |_| {
        let harness = weak.upgrade().unwrap();
        let active_leaf = harness.session_manager.get_leaf_id();
        // Navigate from inside the response, while the run is active.
        let result = harness
            .session
            .navigate_tree(
                &t,
                Some(&AgentSessionNavigateTreeOptions {
                    summarize: Some(false),
                    ..Default::default()
                }),
            )
            .now_or_never()
            .expect("navigation answers immediately while streaming");
        let unchanged =
            active_leaf.as_deref() != Some(t.as_str()) && harness.session_manager.get_leaf_id() == active_leaf;
        *o.lock() = Some((result.err().map(|e| e.to_string()), unchanged));
        faux_text_message("response")
    })]);

    harness.session.prompt("second", None).await.unwrap();

    let (error, unchanged) = observed.lock().clone().unwrap();
    assert_eq!(
        error.as_deref(),
        Some("Wait for the current response to finish before navigating the session tree.")
    );
    assert!(unchanged);
    harness.cleanup();
}

// #3688
#[tokio::test]
async fn clears_branch_summary_state_when_session_before_tree_cancels_navigation() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            pi.on(
                "session_before_tree",
                Arc::new(|_event, _ctx| {
                    Box::pin(async { Ok(Some(ExtensionEventResult::Json(json!({"cancel": true})))) })
                }),
            )?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    let sm = &harness.session_manager;
    let target = sm.append_message(user_msg("first")).unwrap();
    sm.append_message(assistant_msg("reply")).unwrap();
    let current = sm.append_message(user_msg("second")).unwrap();
    assert_eq!(sm.get_leaf_id().as_deref(), Some(current.as_str()));

    let result = harness
        .session
        .navigate_tree(
            &target,
            Some(&AgentSessionNavigateTreeOptions {
                summarize: Some(false),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    assert_eq!(serde_json::to_value(&result).unwrap(), json!({"cancelled": true}));
    assert!(!harness.session.is_compacting());
    assert_eq!(sm.get_leaf_id().as_deref(), Some(current.as_str()));
    harness.cleanup();
}

// #5996
#[tokio::test]
async fn filters_newlines_from_session_names() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.session.set_session_name("hello\nworld\r\nagain");
    assert_eq!(harness.session_manager.get_session_name().as_deref(), Some("hello world again"));
    let names: Vec<Value> = harness.events_of_type("session_info_changed").iter().map(|e| e["name"].clone()).collect();
    assert_eq!(names, vec![json!("hello world again")]);
    harness.cleanup();

    let api = Arc::new(Mutex::new(None::<ExtensionAPIRef>));
    let a = api.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            *a.lock() = Some(pi);
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    api.lock().clone().unwrap().set_session_name("from\nextension").unwrap();
    assert_eq!(harness.session_manager.get_session_name().as_deref(), Some("from extension"));
    let names: Vec<Value> = harness.events_of_type("session_info_changed").iter().map(|e| e["name"].clone()).collect();
    assert_eq!(names, vec![json!("from extension")]);
    harness.cleanup();
}

// #7048
#[tokio::test]
async fn does_not_persist_a_length_limited_compaction_summary() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    seed_compactable_session(&harness);
    harness.set_responses(vec![
        message(vec![faux_text("partial summar")], Some(StopReason::Length)).into(),
    ]);

    let error = harness.session.compact(None).await.unwrap_err();
    assert!(error.to_string().contains("generation hit the token cap"), "{error}");
    assert!(
        !harness
            .session_manager
            .get_entries()
            .iter()
            .any(|e| matches!(e, SessionEntry::Compaction(_)))
    );
    harness.cleanup();
}

// #5998
#[tokio::test]
async fn lets_a_tool_call_handler_terminate_the_run_after_blocking_execution() {
    let echo = FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        |_id, _p, _s, _u| async { anyhow::bail!("tool should have been blocked") },
    )
    .into_ref();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo]),
        extension_factories: vec![ext(|pi| {
            on_event::<ToolCallEvent, _, _>(&*pi, |_event, _ctx| async move {
                Ok(Some(ToolCallEventResult {
                    block: Some(true),
                    reason: Some("Blocked by terminating policy".into()),
                    terminate: Some(true),
                    input: None,
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![tool_use("echo", json!({"text": "hello"})), text("should not run")]);

    harness.session.prompt("hi", None).await.unwrap();

    assert_eq!(harness.get_pending_response_count(), 1);
    assert!(!harness.assistant_texts().contains(&"should not run".to_string()));
    assert_eq!(harness.events_of_type("tool_execution_end")[0]["result"]["terminate"], json!(true));
    assert!(
        harness
            .session
            .messages()
            .iter()
            .any(|m| matches!(m, AgentMessage::ToolResult(r) if r.is_error))
    );
    harness.cleanup();
}

// #3982
#[tokio::test]
async fn allows_extensions_to_replace_finalized_assistant_usage_cost() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| {
            on_event::<MessageEndEvent, _, _>(&*pi, |event, _ctx| async move {
                let AgentMessage::Assistant(mut message) = event.message else {
                    return Ok(None);
                };
                message.usage.cost.total = 0.123;
                Ok(Some(MessageEndEventResult {
                    message: Some(AgentMessage::Assistant(message)),
                }))
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("hello")]);

    harness.session.prompt("hi", None).await.unwrap();

    let assistant = harness
        .session
        .messages()
        .into_iter()
        .find_map(|m| match m {
            AgentMessage::Assistant(a) => Some(a),
            _ => None,
        })
        .expect("missing assistant message");
    assert_eq!(assistant.usage.cost.total, 0.123);
    let end = harness
        .events_of_type("message_end")
        .into_iter()
        .find(|e| e["message"]["role"] == json!("assistant"))
        .expect("missing assistant message_end event");
    assert_eq!(end["message"]["usage"]["cost"]["total"], json!(0.123));
    harness.cleanup();
}

// #3686
#[tokio::test]
async fn session_name_changes_emit_session_info_changed() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.session.set_session_name("hello world");
    assert_eq!(harness.session_manager.get_session_name().as_deref(), Some("hello world"));
    let names: Vec<Value> = harness.events_of_type("session_info_changed").iter().map(|e| e["name"].clone()).collect();
    assert_eq!(names, vec![json!("hello world")]);
    harness.cleanup();

    let api = Arc::new(Mutex::new(None::<ExtensionAPIRef>));
    let seen = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let (a, s) = (api.clone(), seen.clone());
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            *a.lock() = Some(pi.clone());
            let s = s.clone();
            on_event::<pi_coding_agent::core::extensions::types::SessionInfoChangedEvent, _, _>(
                &*pi,
                move |event, _ctx| {
                    let s = s.clone();
                    async move {
                        s.lock().push(event.name);
                        Ok(None)
                    }
                },
            )?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    api.lock().clone().unwrap().set_session_name("first").unwrap();
    harness.session.set_session_name("second");
    tick().await;
    assert_eq!(harness.session_manager.get_session_name().as_deref(), Some("second"));
    assert_eq!(*seen.lock(), vec![Some("first".to_string()), Some("second".to_string())]);
    harness.cleanup();
}

// #8964: extensions can stream responses from providers registered with pi.registerProvider().
async fn extension_command_streams_through_model_registry(simple: bool) {
    let faux = pi_ai::providers::faux::faux_provider(pi_ai::providers::faux::RegisterFauxProviderOptions {
        provider: Some("extension-provider".into()),
        api: Some("issue-8964-extension-api".into()),
        ..Default::default()
    });
    let received_key = Arc::new(Mutex::new(None::<String>));
    let rk = received_key.clone();
    faux.set_responses(vec![pi_ai::providers::faux::FauxResponseStep::factory(move |_c, options, _s, _m| {
        *rk.lock() = options.api_key.clone();
        faux_text_message("custom provider response")
    })]);
    let streamed = Arc::new(Mutex::new(String::new()));
    let result = Arc::new(Mutex::new(None::<pi_ai::types::AssistantMessage>));
    let provider = faux.provider.clone();
    let provider_id = provider.id().to_string();
    let model = faux.get_model();
    let api = faux.api().to_string();
    let models: Vec<pi_coding_agent::core::extensions::types::ProviderModelConfig> = faux
        .models()
        .iter()
        .map(|m| serde_json::from_value(serde_json::to_value(m).unwrap()).unwrap())
        .collect();
    let (pid, base_url) = (provider_id.clone(), model.base_url.clone());
    let (st, re, model_id) = (streamed.clone(), result.clone(), model.id.clone());
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![
            ext(move |pi| {
                let provider = provider.clone();
                pi.register_provider(pi_coding_agent::core::extensions::types::ProviderRegistration::Config {
                    name: pid.clone(),
                    config: pi_coding_agent::core::extensions::types::ProviderConfig {
                        api: Some(api.clone()),
                        base_url: Some(base_url.clone()),
                        api_key: Some("extension-key".into()),
                        models: Some(models.clone()),
                        stream_simple: Some(Arc::new(move |m, c, o| {
                            provider.stream_simple(&m, &c, o.unwrap_or_default())
                        })),
                        ..Default::default()
                    },
                })
            }),
            ext(move |pi| {
                let (st, re, provider_id, model_id) = (st.clone(), re.clone(), provider_id.clone(), model_id.clone());
                pi.register_command(
                    "stream-custom",
                    pi_coding_agent::core::extensions::types::CommandOptions {
                        description: Some("Stream a response from the custom provider".into()),
                        get_argument_completions: None,
                        handler: Arc::new(move |_args, ctx| {
                            let (st, re, provider_id, model_id) =
                                (st.clone(), re.clone(), provider_id.clone(), model_id.clone());
                            Box::pin(async move {
                                let registry = ctx.model_registry()?;
                                let model = registry.find(&provider_id, &model_id).unwrap();
                                let context = pi_ai::types::Context {
                                    system_prompt: None,
                                    messages: vec![pi_ai::types::Message::User(pi_ai::types::UserMessage::new(
                                        "Hello",
                                        chrono::Utc::now().timestamp_millis(),
                                    ))],
                                    tools: None,
                                };
                                let mut stream = if simple {
                                    registry.stream_simple(model, context, None)
                                } else {
                                    registry.stream(model, context, None)
                                };
                                while let Some(event) = stream.next_event().await {
                                    if let pi_ai::AssistantMessageEvent::TextDelta { delta, .. } = event {
                                        st.lock().push_str(&delta);
                                    }
                                }
                                *re.lock() = Some(stream.result().await);
                                Ok(())
                            })
                        }),
                    },
                )
            }),
        ],
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(pi_ai::compat::get_api_provider("issue-8964-extension-api").is_none());

    harness.session.prompt("/stream-custom", None).await.unwrap();

    assert_eq!(received_key.lock().as_deref(), Some("extension-key"));
    assert_eq!(*streamed.lock(), "custom provider response");
    let result = result.lock().clone().unwrap();
    assert_eq!(result.stop_reason, StopReason::Stop);
    assert_eq!(
        serde_json::to_value(&result.content).unwrap(),
        json!([{"type": "text", "text": "custom provider response"}])
    );
    assert!(pi_ai::compat::get_api_provider("issue-8964-extension-api").is_none());
    harness.cleanup();
}

#[tokio::test]
async fn allows_an_extension_command_to_use_model_registry_stream() {
    extension_command_streams_through_model_registry(false).await;
}

#[tokio::test]
async fn allows_an_extension_command_to_use_model_registry_stream_simple() {
    extension_command_streams_through_model_registry(true).await;
}

// #8935
#[tokio::test]
async fn does_not_start_prepared_tools_after_a_later_preflight_aborts() {
    let executions = Arc::new(Mutex::new(Vec::<String>::new()));
    let preflights = Arc::new(Mutex::new(Vec::<String>::new()));
    let result_hooks = Arc::new(Mutex::new(Vec::<String>::new()));
    let e = executions.clone();
    let tool = FnAgentTool::new(
        "external_write",
        "External write",
        "Perform an external write",
        json!({"type": "object", "properties": {"value": {"type": "string"}}, "required": ["value"]}),
        move |_id, params: Value, _s, _u| {
            let e = e.clone();
            async move {
                let value = params["value"].as_str().unwrap_or_default().to_string();
                e.lock().push(value.clone());
                Ok(pi_agent::types::AgentToolResult::text(value.clone(), Some(json!({"value": value}))))
            }
        },
    )
    .into_ref();
    let (p, r) = (preflights.clone(), result_hooks.clone());
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![tool]),
        extension_factories: vec![ext(move |pi| {
            let p = p.clone();
            on_event::<ToolCallEvent, _, _>(&*pi, move |event, ctx| {
                let p = p.clone();
                async move {
                    let value = event.input.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
                    p.lock().push(value.clone());
                    if value == "second" {
                        ctx.abort()?;
                    }
                    Ok(None)
                }
            })?;
            let r = r.clone();
            on_event::<pi_coding_agent::core::extensions::types::ToolResultEvent, _, _>(&*pi, move |event, _ctx| {
                let r = r.clone();
                async move {
                    r.lock().push(event.tool_call_id);
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(
            vec![
                tool_call("external_write", json!({"value": "first"})),
                tool_call("external_write", json!({"value": "second"})),
            ],
            Some(StopReason::ToolUse),
        )
        .into(),
    ]);

    harness.session.prompt("run both writes", None).await.unwrap();

    assert_eq!(*preflights.lock(), vec!["first", "second"]);
    assert!(executions.lock().is_empty());
    assert!(result_hooks.lock().is_empty());
    let starts = harness.events_of_type("tool_execution_start");
    let ends = harness.events_of_type("tool_execution_end");
    assert_eq!(starts.len(), 2);
    assert_eq!(ends.len(), 2);
    let mut start_ids: Vec<Value> = starts.iter().map(|e| e["toolCallId"].clone()).collect();
    let mut end_ids: Vec<Value> = ends.iter().map(|e| e["toolCallId"].clone()).collect();
    assert!(ends.iter().all(|e| e["isError"] == json!(true)));
    let results: Vec<(String, String)> = harness
        .session
        .messages()
        .iter()
        .filter_map(|m| match m {
            AgentMessage::ToolResult(r) => Some((r.tool_call_id.clone(), get_message_text(m))),
            _ => None,
        })
        .collect();
    assert_eq!(
        results.iter().map(|(id, _)| json!(id)).collect::<Vec<_>>(),
        start_ids.clone()
    );
    assert!(results.iter().all(|(_, text)| text == "Operation aborted"));
    start_ids.sort_by_key(|v| v.to_string());
    end_ids.sort_by_key(|v| v.to_string());
    assert_eq!(start_ids, end_ids);
    harness.cleanup();
}

// pre-prompt-compaction-no-continue
#[tokio::test]
async fn compacts_length_stop_overflow_before_a_new_prompt_without_continuing_from_an_assistant_message() {
    let harness = create_harness(HarnessOptions {
        models: vec![pi_ai::providers::faux::FauxModelDefinition {
            id: "faux-1".into(),
            context_window: Some(100),
            max_tokens: Some(100),
            ..Default::default()
        }],
        settings: Some(json!({"compaction": {"enabled": true, "keepRecentTokens": 1, "reserveTokens": 0}})),
        extension_factories: vec![crate::compaction::compactor("pre-prompt summary")],
        ..Default::default()
    })
    .await
    .unwrap();
    let now = now_ms();
    harness.session_manager.append_message(user_blocks("previous prompt", now - 1000)).unwrap();
    let mut length_stop = create_assistant(&harness, StopReason::Length, None, 100, now - 500);
    length_stop.content = vec![faux_text("length-stop assistant response")];
    harness.session_manager.append_message(AgentMessage::Assistant(length_stop)).unwrap();
    harness.session.agent().set_messages(harness.session_manager.build_session_context().messages);
    harness.set_responses(vec![text("answered next prompt")]);

    harness.session.prompt("next prompt", None).await.unwrap();

    let end = harness.events_of_type("compaction_end").last().cloned().unwrap();
    assert_eq!(end["reason"], json!("overflow"));
    assert_eq!(end["aborted"], json!(false));
    assert_eq!(end["willRetry"], json!(true));
    assert!(harness.user_texts().contains(&"next prompt".to_string()));
    // agent.continue() would have issued a second request.
    assert_eq!(harness.faux.state().call_count, 1);
    harness.cleanup();
}

// #2023
#[tokio::test]
async fn treats_extension_origin_queued_slash_command_follow_ups_as_raw_user_text() {
    let api = Arc::new(Mutex::new(None::<ExtensionAPIRef>));
    let runs = Arc::new(Mutex::new(Vec::<String>::new()));
    let (a, r) = (api.clone(), runs.clone());
    let mut waiting = create_waiting_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            *a.lock() = Some(pi.clone());
            let r = r.clone();
            pi.register_command(
                "testcmd",
                pi_coding_agent::core::extensions::types::CommandOptions {
                    description: Some("Test command".into()),
                    get_argument_completions: None,
                    handler: Arc::new(move |args, _ctx| {
                        let r = r.clone();
                        Box::pin(async move {
                            r.lock().push(args);
                            Ok(())
                        })
                    }),
                },
            )
        })],
        ..Default::default()
    })
    .await;
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        text("first turn complete"),
        text("queued follow-up handled by model"),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    tick().await;

    api.lock()
        .clone()
        .unwrap()
        .send_user_message(
            pi_ai::types::UserContent::Text("/testcmd queued".into()),
            pi_coding_agent::core::extensions::types::SendUserMessageOptions {
                deliver_as: Some(pi_coding_agent::core::extensions::types::StreamingBehavior::FollowUp),
                expand_prompt_templates: None,
            },
        )
        .unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    assert!(runs.lock().is_empty());
    assert_eq!(waiting.harness.user_texts(), vec!["start", "/testcmd queued"]);
    assert!(
        waiting
            .harness
            .assistant_texts()
            .contains(&"queued follow-up handled by model".to_string())
    );
    waiting.harness.cleanup();
}

// #8328
async fn zero_usage_check(user_text: String) -> Vec<(pi_coding_agent::core::extensions::types::CompactionReason, bool)> {
    let harness = create_harness(HarnessOptions {
        models: vec![pi_ai::providers::faux::FauxModelDefinition {
            id: "faux-1".into(),
            context_window: Some(100),
            max_tokens: Some(20),
            ..Default::default()
        }],
        settings: Some(json!({"compaction": {"enabled": true, "reserveTokens": 10}})),
        ..Default::default()
    })
    .await
    .unwrap();
    let mut assistant = create_assistant(&harness, StopReason::Stop, None, 0, now_ms());
    assistant.content = vec![faux_text("response")];
    harness.session.agent().set_messages(vec![
        user_blocks(&user_text, now_ms() - 1),
        AgentMessage::Assistant(assistant.clone()),
    ]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let c = calls.clone();
    harness.session.set_run_auto_compaction_override(Some(Arc::new(move |reason, will_retry| {
        c.lock().push((reason, will_retry));
        false
    })));

    harness.session.check_compaction_for_test(&assistant, true).await;

    harness.cleanup();
    let calls = calls.lock().clone();
    calls
}

#[tokio::test]
async fn uses_the_message_estimate_when_no_assistant_has_reported_usage() {
    let calls = zero_usage_check("x".repeat(400)).await;
    assert_eq!(
        calls,
        vec![(pi_coding_agent::core::extensions::types::CompactionReason::Threshold, false)]
    );
}

#[tokio::test]
async fn does_not_compact_when_the_zero_usage_message_estimate_is_below_the_threshold() {
    assert!(zero_usage_check("short".into()).await.is_empty());
}

// #2835: tool allowlists filter extension tools.
async fn allowlisted_session(dir: &str, allowed: Vec<String>) -> Arc<pi_coding_agent::core::agent_session::AgentSession> {
    use pi_coding_agent::core::extensions::types::{ExtensionFactory, InlineExtension, SessionStartEvent, ToolDefinition};
    use pi_coding_agent::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
    let agent_dir = format!("{dir}/agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    let settings_manager = pi_coding_agent::core::settings_manager::SettingsManager::create(dir, Some(&agent_dir), None);
    let factory: ExtensionFactory = Arc::new(|pi| {
        Box::pin(async move {
            let api = pi.clone();
            on_event::<SessionStartEvent, _, _>(&*pi, move |_event, _ctx| {
                let api = api.clone();
                async move {
                    let mut tool = ToolDefinition::new(
                        "dynamic_tool",
                        "Dynamic Tool",
                        "Tool registered from session_start",
                        json!({"type": "object", "properties": {}}),
                        |_id, _p, _s, _u, _c| async { Ok(pi_agent::types::AgentToolResult::text("ok", Some(json!({})))) },
                    );
                    tool.prompt_snippet = Some("Run dynamic test behavior".into());
                    api.register_tool(tool)?;
                    Ok(None)
                }
            })?;
            Ok(())
        })
    });
    let resource_loader = Arc::new(DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: dir.to_string(),
        agent_dir: agent_dir.clone(),
        settings_manager: Some(settings_manager.clone()),
        extension_factories: Some(vec![InlineExtension::Factory(factory)]),
        ..Default::default()
    }));
    resource_loader.reload(None).await.unwrap();
    let session = pi_coding_agent::core::sdk::create_agent_session(Some(&pi_coding_agent::core::sdk::CreateAgentSessionOptions {
        cwd: Some(dir.to_string()),
        agent_dir: Some(agent_dir),
        model: pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5"),
        settings_manager: Some(settings_manager),
        session_manager: Some(pi_coding_agent::core::session_manager::SessionManager::in_memory(Some(dir), None, None).unwrap()),
        resource_loader: Some(resource_loader as Arc<dyn ResourceLoader>),
        tools: Some(allowed),
        ..Default::default()
    }))
    .await
    .unwrap()
    .session;
    session
        .bind_extensions(&pi_coding_agent::core::agent_session::ExtensionBindings::default())
        .await
        .unwrap();
    session
}

#[tokio::test]
async fn allows_only_explicitly_listed_built_in_and_extension_tools() {
    let dir = std::env::temp_dir().join(format!("pi-tools-filter-{}", uuid::Uuid::new_v4()));
    let dir = dir.to_string_lossy().into_owned();
    let session = allowlisted_session(&dir, vec!["read".into(), "dynamic_tool".into()]).await;

    let mut all: Vec<String> = session.get_all_tools().into_iter().map(|t| t.name).collect();
    all.sort();
    assert_eq!(all, vec!["dynamic_tool", "read"]);
    let mut active = session.get_active_tool_names();
    active.sort();
    assert_eq!(active, vec!["dynamic_tool", "read"]);
    let prompt = session.system_prompt();
    assert!(prompt.contains("- read: Read file contents"));
    assert!(prompt.contains("- dynamic_tool: Run dynamic test behavior"));
    assert!(!prompt.contains("- bash:"));
    assert!(!prompt.contains("- edit:"));
    session.dispose();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn disables_all_tools_when_the_allowlist_is_empty() {
    let dir = std::env::temp_dir().join(format!("pi-tools-filter-{}", uuid::Uuid::new_v4()));
    let dir = dir.to_string_lossy().into_owned();
    let session = allowlisted_session(&dir, Vec::new()).await;

    assert!(session.get_all_tools().is_empty());
    assert!(session.get_active_tool_names().is_empty());
    let prompt = session.system_prompt();
    assert!(prompt.contains("<tools>\n(none)\n"), "{prompt}");
    assert!(!prompt.contains("dynamic_tool"));
    session.dispose();
    let _ = std::fs::remove_dir_all(&dir);
}

// #5217: compaction reason on extension events.
async fn compaction_reason_harness(recorded: Arc<Mutex<Vec<(String, Value, bool)>>>) -> Harness {
    use pi_coding_agent::core::extensions::types::{SessionBeforeCompactEvent, SessionBeforeCompactResult, SessionCompactEvent};
    let r = recorded.clone();
    let harness = create_harness(HarnessOptions {
        settings: Some(json!({"compaction": {"keepRecentTokens": 1}})),
        extension_factories: vec![ext(move |pi| {
            let r1 = r.clone();
            on_event::<SessionBeforeCompactEvent, _, _>(&*pi, move |event, _ctx| {
                let r = r1.clone();
                async move {
                    r.lock().push((
                        "session_before_compact".into(),
                        serde_json::to_value(event.reason)?,
                        event.will_retry,
                    ));
                    Ok(Some(SessionBeforeCompactResult {
                        cancel: None,
                        compaction: Some(pi_coding_agent::core::compaction::CompactionResult {
                            summary: "summary from extension".into(),
                            first_kept_entry_id: event.preparation.first_kept_entry_id.clone(),
                            tokens_before: event.preparation.tokens_before,
                            estimated_tokens_after: None,
                            usage: None,
                            details: Some(json!({})),
                        }),
                    }))
                }
            })?;
            let r2 = r.clone();
            on_event::<SessionCompactEvent, _, _>(&*pi, move |event, _ctx| {
                let r = r2.clone();
                async move {
                    r.lock()
                        .push(("session_compact".into(), serde_json::to_value(event.reason)?, event.will_retry));
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("one"), text("two")]);
    harness.session.prompt("first", None).await.unwrap();
    harness.session.prompt("second", None).await.unwrap();
    harness
}

fn reason_events(reason: &str, will_retry: bool) -> Vec<(String, Value, bool)> {
    vec![
        ("session_before_compact".into(), json!(reason), will_retry),
        ("session_compact".into(), json!(reason), will_retry),
    ]
}

#[tokio::test]
async fn reports_manual_reason_for_compact() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let harness = compaction_reason_harness(recorded.clone()).await;
    harness.session.compact(None).await.unwrap();
    assert_eq!(*recorded.lock(), reason_events("manual", false));
    harness.cleanup();
}

#[tokio::test]
async fn reports_threshold_reason_for_auto_compaction() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let harness = compaction_reason_harness(recorded.clone()).await;
    harness
        .session
        .run_auto_compaction_for_test(pi_coding_agent::core::extensions::types::CompactionReason::Threshold, false)
        .await;
    assert_eq!(*recorded.lock(), reason_events("threshold", false));
    harness.cleanup();
}

#[tokio::test]
async fn reports_overflow_reason_and_will_retry_for_overflow_recovery() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let harness = compaction_reason_harness(recorded.clone()).await;
    harness
        .session
        .run_auto_compaction_for_test(pi_coding_agent::core::extensions::types::CompactionReason::Overflow, true)
        .await;
    assert_eq!(*recorded.lock(), reason_events("overflow", true));
    harness.cleanup();
}

// #1717/#2113: agent session event settlement.
fn echo_tool() -> pi_agent::types::AgentToolRef {
    FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        |_id, params: Value, _s, _u| async move {
            let text = params["text"].as_str().unwrap_or_default().to_string();
            Ok(pi_agent::types::AgentToolResult::text(text.clone(), Some(json!({"text": text}))))
        },
    )
    .into_ref()
}

fn branch_roles(manager: &pi_coding_agent::core::session_manager::SessionManager) -> Vec<String> {
    manager
        .get_branch(None)
        .iter()
        .filter_map(|e| match e {
            SessionEntry::Message(m) => Some(m.message.role().to_string()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn keeps_persisted_message_order_when_extension_message_end_handlers_yield() {
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo_tool()]),
        extension_factories: vec![ext(|pi| {
            on_event::<MessageEndEvent, _, _>(&*pi, |event, _ctx| async move {
                if matches!(event.message, AgentMessage::Assistant(_)) {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Ok(None)
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![
        message(
            vec![tool_call("echo", json!({"text": "one"})), tool_call("echo", json!({"text": "two"}))],
            Some(StopReason::ToolUse),
        )
        .into(),
        text("done"),
    ]);

    harness.session.prompt("run tools", None).await.unwrap();

    assert_eq!(
        branch_roles(&harness.session_manager),
        ["system", "user", "assistant", "toolResult", "toolResult", "assistant"]
    );
    harness.cleanup();
}

#[tokio::test]
async fn runs_tool_call_handlers_after_the_assistant_tool_use_message_is_settled_in_the_session() {
    let roles = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let r = roles.clone();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo_tool()]),
        extension_factories: vec![ext(move |pi| {
            let r = r.clone();
            on_event::<ToolCallEvent, _, _>(&*pi, move |_event, ctx| {
                let r = r.clone();
                async move {
                    r.lock().push(branch_roles(&*ctx.session_manager()?));
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![tool_use("echo", json!({"text": "hello"})), text("done")]);

    harness.session.prompt("run tool", None).await.unwrap();

    assert_eq!(
        *roles.lock(),
        vec![vec!["system".to_string(), "user".into(), "assistant".into()]]
    );
    harness.cleanup();
}

// #5109: exclude tools.
fn tool_registering_extension() -> TestExtension {
    use pi_coding_agent::core::extensions::types::{SessionStartEvent, ToolDefinition};
    ext(|pi| {
        let api = pi.clone();
        on_event::<SessionStartEvent, _, _>(&*pi, move |_event, _ctx| {
            let api = api.clone();
            async move {
                for (name, label, description, snippet) in [
                    ("ask_question", "Ask Question", "Ask a question", "Ask a question"),
                    ("dynamic_tool", "Dynamic Tool", "Dynamic test tool", "Run dynamic test behavior"),
                ] {
                    let mut tool = ToolDefinition::new(
                        name,
                        label,
                        description,
                        json!({"type": "object", "properties": {}}),
                        |_id, _p, _s, _u, _c| async { Ok(pi_agent::types::AgentToolResult::text("ok", Some(json!({})))) },
                    );
                    tool.prompt_snippet = Some(snippet.into());
                    api.register_tool(tool)?;
                }
                Ok(None)
            }
        })?;
        Ok(())
    })
}

fn sorted_tool_names(harness: &Harness) -> Vec<String> {
    let mut names: Vec<String> = harness.session.get_all_tools().into_iter().map(|t| t.name).collect();
    names.sort();
    names
}

fn strings(values: &[&str]) -> Option<Vec<String>> {
    Some(values.iter().map(|s| s.to_string()).collect())
}

#[tokio::test]
async fn filters_built_in_and_extension_tools_from_available_and_active_tools() {
    let harness = create_harness(HarnessOptions {
        excluded_tool_names: strings(&["read", "ask_question"]),
        extension_factories: vec![tool_registering_extension()],
        ..Default::default()
    })
    .await
    .unwrap();
    harness
        .session
        .bind_extensions(&pi_coding_agent::core::agent_session::ExtensionBindings::default())
        .await
        .unwrap();

    let all = sorted_tool_names(&harness);
    assert!(!all.contains(&"read".to_string()));
    assert!(!all.contains(&"ask_question".to_string()));
    assert!(all.contains(&"bash".to_string()));
    assert!(all.contains(&"dynamic_tool".to_string()));
    let mut active = harness.session.get_active_tool_names();
    active.sort();
    assert_eq!(active, vec!["bash", "dynamic_tool", "edit", "write"]);
    let prompt = harness.session.system_prompt();
    assert!(!prompt.contains("- read:"));
    assert!(!prompt.contains("ask_question"));
    assert!(prompt.contains("- dynamic_tool: Run dynamic test behavior"));
    harness.cleanup();
}

#[tokio::test]
async fn lets_excluded_tools_override_the_allowlist() {
    let harness = create_harness(HarnessOptions {
        allowed_tool_names: strings(&["read", "bash", "ask_question"]),
        excluded_tool_names: strings(&["read", "ask_question"]),
        initial_active_tool_names: strings(&["read", "bash", "ask_question"]),
        extension_factories: vec![tool_registering_extension()],
        ..Default::default()
    })
    .await
    .unwrap();
    harness
        .session
        .bind_extensions(&pi_coding_agent::core::agent_session::ExtensionBindings::default())
        .await
        .unwrap();

    assert_eq!(sorted_tool_names(&harness), vec!["bash"]);
    assert_eq!(harness.session.get_active_tool_names(), vec!["bash"]);
    let prompt = harness.session.system_prompt();
    assert!(prompt.contains("- bash:"));
    assert!(!prompt.contains("- read:"));
    assert!(!prompt.contains("ask_question"));
    harness.cleanup();
}

#[tokio::test]
async fn matches_allowlist_and_denylist_patterns() {
    let harness = create_harness(HarnessOptions {
        allowed_tool_names: strings(&["*_tool", "ask_*", "re*"]),
        excluded_tool_names: strings(&["ask*"]),
        initial_active_tool_names: strings(&["*_tool", "ask_*", "re*"]),
        extension_factories: vec![tool_registering_extension()],
        ..Default::default()
    })
    .await
    .unwrap();
    harness
        .session
        .bind_extensions(&pi_coding_agent::core::agent_session::ExtensionBindings::default())
        .await
        .unwrap();

    // Patterns that match a tool activate it like its name.
    assert_eq!(sorted_tool_names(&harness), vec!["dynamic_tool", "read"]);
    let mut active = harness.session.get_active_tool_names();
    active.sort();
    assert_eq!(active, vec!["dynamic_tool", "read"]);
    harness.cleanup();
}

// #9178: tree navigation during manual compaction.
const BUSY: &str = "Wait for the current compaction or tree navigation to finish before navigating the session tree.";

fn no_summary() -> AgentSessionNavigateTreeOptions {
    AgentSessionNavigateTreeOptions {
        summarize: Some(false),
        ..Default::default()
    }
}

#[tokio::test]
async fn rejects_navigation_during_manual_compaction_before_the_active_leaf_can_change() {
    use pi_coding_agent::core::extensions::types::{SessionBeforeCompactEvent, SessionBeforeCompactResult};
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let release = Arc::new(tokio::sync::Notify::new());
    let rel = release.clone();
    let harness = Arc::new(
        create_harness(HarnessOptions {
            settings: Some(json!({"compaction": {"keepRecentTokens": 1}})),
            extension_factories: vec![ext(move |pi| {
                let (tx, rel) = (started_tx.clone(), rel.clone());
                on_event::<SessionBeforeCompactEvent, _, _>(&*pi, move |event, _ctx| {
                    let (tx, rel) = (tx.clone(), rel.clone());
                    async move {
                        if let Some(tx) = tx.lock().take() {
                            let _ = tx.send(());
                        }
                        rel.notified().await;
                        Ok(Some(SessionBeforeCompactResult {
                            cancel: None,
                            compaction: Some(pi_coding_agent::core::compaction::CompactionResult {
                                summary: "summary".into(),
                                first_kept_entry_id: event.preparation.first_kept_entry_id.clone(),
                                tokens_before: event.preparation.tokens_before,
                                estimated_tokens_after: None,
                                usage: None,
                                details: Some(json!({})),
                            }),
                        }))
                    }
                })?;
                Ok(())
            })],
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    let sm = &harness.session_manager;
    sm.append_message(user_msg("first user")).unwrap();
    let target = sm.append_message(assistant_msg("first assistant")).unwrap();
    sm.append_message(user_msg("second user")).unwrap();
    let original_leaf = sm.append_message(assistant_msg("second assistant")).unwrap();
    harness.session.agent().set_messages(sm.build_session_context().messages);

    let session = harness.session.clone();
    let compaction = tokio::spawn(async move { session.compact(None).await });
    started_rx.await.unwrap();

    assert!(harness.session.is_compacting());
    let error = harness.session.navigate_tree(&target, Some(&no_summary())).await.unwrap_err();
    assert_eq!(error.to_string(), BUSY);
    assert_eq!(sm.get_leaf_id().as_deref(), Some(original_leaf.as_str()));
    release.notify_one();
    compaction.await.unwrap().unwrap();

    let last = serde_json::to_value(sm.get_entries().last().unwrap()).unwrap();
    assert_eq!(last["type"], json!("compaction"));
    assert_eq!(last["parentId"], json!(original_leaf));
    assert!(harness.session.messages().iter().any(|m| get_message_text(m) == "second assistant"));
    harness.cleanup();
}

#[tokio::test]
async fn rejects_a_second_navigation_while_the_first_is_waiting() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let release = Arc::new(tokio::sync::Notify::new());
    let rel = release.clone();
    let harness = Arc::new(
        create_harness(HarnessOptions {
            extension_factories: vec![ext(move |pi| {
                let (tx, rel) = (started_tx.clone(), rel.clone());
                pi.on(
                    "session_before_tree",
                    Arc::new(move |_event, _ctx| {
                        let (tx, rel) = (tx.clone(), rel.clone());
                        Box::pin(async move {
                            if let Some(tx) = tx.lock().take() {
                                let _ = tx.send(());
                            }
                            rel.notified().await;
                            Ok(None)
                        })
                    }),
                )?;
                Ok(())
            })],
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    let sm = &harness.session_manager;
    let second_target = sm.append_message(user_msg("first user")).unwrap();
    let first_target = sm.append_message(assistant_msg("first assistant")).unwrap();
    sm.append_message(user_msg("second user")).unwrap();
    let original_leaf = sm.append_message(assistant_msg("second assistant")).unwrap();
    harness.session.agent().set_messages(sm.build_session_context().messages);

    let (session, t) = (harness.session.clone(), first_target.clone());
    let first = tokio::spawn(async move { session.navigate_tree(&t, Some(&no_summary())).await });
    started_rx.await.unwrap();
    let (session, t) = (harness.session.clone(), second_target.clone());
    let second = tokio::spawn(async move { session.navigate_tree(&t, Some(&no_summary())).await });
    tick().await;
    assert_eq!(sm.get_leaf_id().as_deref(), Some(original_leaf.as_str()));
    release.notify_one();

    let error = second.await.unwrap().unwrap_err();
    assert_eq!(error.to_string(), BUSY);
    first.await.unwrap().unwrap();
    assert_eq!(sm.get_leaf_id().as_deref(), Some(first_target.as_str()));
    harness.cleanup();
}

// copilot-request-size-overflow
#[tokio::test]
async fn compacts_an_image_heavy_history_and_retries_after_the_real_413() {
    const COPILOT_413: &str = r#"github-copilot API error (413): {"message":"failed to parse request","code":""}"#;
    const REQUEST_LIMIT: usize = 32 * 1024 * 1024;
    let harness = create_harness(HarnessOptions {
        provider: Some("github-copilot".into()),
        models: vec![pi_ai::providers::faux::FauxModelDefinition {
            id: "faux-copilot".into(),
            context_window: Some(1_000_000),
            ..Default::default()
        }],
        ..Default::default()
    })
    .await
    .unwrap();
    // Synthetic base64 only; no image decoding or network calls are involved.
    let image_data = "A".repeat(384 * 1024);
    let timestamp = now_ms() - 10_000;
    let model = harness.get_model();
    let with_model = |mut m: pi_ai::types::AssistantMessage| {
        m.provider = model.provider.clone();
        m.api = model.api.clone();
        m.model = model.id.clone();
        m.timestamp = timestamp;
        AgentMessage::Assistant(m)
    };
    let sm = &harness.session_manager;
    sm.append_message(agent_message(json!({"role": "user", "content": "Review the old images", "timestamp": timestamp})))
        .unwrap();
    sm.append_message(with_model(message(
        vec![faux_tool_call(
            "read",
            json!({"path": "old-images"}).as_object().cloned().unwrap(),
            Some("read-images"),
        )],
        Some(StopReason::ToolUse),
    )))
    .unwrap();
    let images: Vec<Value> = (0..89)
        .map(|_| json!({"type": "image", "mimeType": "image/png", "data": image_data}))
        .collect();
    sm.append_message(agent_message(json!({
        "role": "toolResult", "toolCallId": "read-images", "toolName": "read",
        "content": images, "isError": false, "timestamp": timestamp
    })))
    .unwrap();
    sm.append_message(with_model(faux_text_message("Finished reviewing the old images"))).unwrap();
    // Keep a completed recent turn so recovery does not summarize the whole current request.
    sm.append_message(agent_message(json!({
        "role": "user", "content": format!("Recent task notes: {}", "x".repeat(80_000)), "timestamp": timestamp
    })))
    .unwrap();
    sm.append_message(with_model(faux_text_message("Ready to continue"))).unwrap();
    harness.session.agent().set_messages(sm.build_session_context().messages);
    let oversized = Arc::new(Mutex::new(false));
    let retried_without_images = Arc::new(Mutex::new(false));
    let (o, r) = (oversized.clone(), retried_without_images.clone());
    harness.set_responses(vec![
        respond_with(move |context| {
            *o.lock() = serde_json::to_vec(context).unwrap().len() > REQUEST_LIMIT;
            // TS throws from the response factory, which the faux provider reports as a
            // zero-usage error response.
            faux_assistant_message(
                vec![],
                FauxMessageOptions {
                    stop_reason: Some(StopReason::Error),
                    error_message: Some(COPILOT_413.into()),
                    ..Default::default()
                },
            )
        }),
        // The overflow recovery summarization runs on the compacted, image-free context.
        text("The old images have been reviewed. Continue with the recent task."),
        respond_with(move |context| {
            *r.lock() = !serde_json::to_string(context).unwrap().contains(r#""type":"image""#);
            faux_text_message("Continued after compaction")
        }),
    ]);

    harness.session.prompt("Continue the task using the recent notes.", None).await.unwrap();

    assert!(*oversized.lock());
    assert!(*retried_without_images.lock());
    assert_eq!(harness.faux.state().call_count, 3);
    assert_eq!(
        harness.events_of_type("compaction_start"),
        vec![json!({"type": "compaction_start", "reason": "overflow"})]
    );
    let end = harness.events_of_type("compaction_end").last().cloned().unwrap();
    assert_eq!(end["reason"], json!("overflow"));
    assert_eq!(end["aborted"], json!(false));
    assert_eq!(end["willRetry"], json!(true));
    assert_eq!(
        harness.session.get_last_assistant_text().as_deref(),
        Some("Continued after compaction")
    );
    harness.cleanup();
}

// #7253: manual compaction during an active response.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persists_the_aborted_response_before_running_the_requested_manual_compaction() {
    use pi_coding_agent::core::extensions::types::{SessionBeforeCompactEvent, SessionBeforeCompactResult};
    let noop = FnAgentTool::new(
        "noop",
        "No-op",
        "Return immediately",
        json!({"type": "object", "properties": {}}),
        |_id, _p, _s, _u| async { Ok(pi_agent::types::AgentToolResult::text("done", Some(json!({})))) },
    )
    .into_ref();
    let harness = Arc::new(
        create_harness(HarnessOptions {
            models: vec![pi_ai::providers::faux::FauxModelDefinition {
                id: "faux-1".into(),
                context_window: Some(1000),
                max_tokens: Some(1000),
                ..Default::default()
            }],
            settings: Some(json!({"compaction": {"enabled": true, "reserveTokens": 200, "keepRecentTokens": 2}})),
            tools: Some(vec![noop]),
            extension_factories: vec![ext(|pi| {
                on_event::<SessionBeforeCompactEvent, _, _>(&*pi, |event, _ctx| async move {
                    let reason = serde_json::to_value(event.reason)?;
                    Ok(Some(SessionBeforeCompactResult {
                        cancel: None,
                        compaction: Some(pi_coding_agent::core::compaction::CompactionResult {
                            summary: format!("{} summary", reason.as_str().unwrap_or_default()),
                            first_kept_entry_id: event.preparation.first_kept_entry_id.clone(),
                            tokens_before: event.preparation.tokens_before,
                            estimated_tokens_after: None,
                            usage: None,
                            details: Some(json!({})),
                        }),
                    }))
                })?;
                Ok(())
            })],
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    // The second response blocks its (worker) thread until released, like the TS async factory.
    let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let started_tx = Mutex::new(Some(started_tx));
    let release_rx = Mutex::new(Some(release_rx));
    harness.set_responses(vec![
        tool_use("noop", json!({})),
        respond_with(move |_| {
            if let Some(tx) = started_tx.lock().take() {
                let _ = tx.send(());
            }
            if let Some(rx) = release_rx.lock().take() {
                let _ = rx.recv();
            }
            faux_text_message(format!("second response:{}", "x".repeat(4000)))
        }),
    ]);

    let session = harness.session.clone();
    let prompt = tokio::spawn(async move { session.prompt("Run the tool, then continue responding.", None).await });
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap()).await.unwrap();

    let session = harness.session.clone();
    let compact = tokio::spawn(async move { session.compact(None).await });
    tick().await;
    release_tx.send(()).unwrap();
    prompt.await.unwrap().unwrap();
    let result = compact.await.unwrap().unwrap();
    assert_eq!(result.summary, "manual summary");

    let reasons = |ty: &str| -> Vec<Value> { harness.events_of_type(ty).iter().map(|e| e["reason"].clone()).collect() };
    assert_eq!(reasons("compaction_start"), vec![json!("manual")]);
    assert_eq!(reasons("compaction_end"), vec![json!("manual")]);
    let entries = harness.session_manager.get_entries();
    let aborted_index = entries
        .iter()
        .position(|e| matches!(e, SessionEntry::Message(m) if matches!(&m.message, AgentMessage::Assistant(a) if a.stop_reason == StopReason::Aborted)))
        .expect("aborted response persisted");
    let compaction_index = entries
        .iter()
        .position(|e| matches!(e, SessionEntry::Compaction(_)))
        .expect("compaction persisted");
    assert!(compaction_index > aborted_index);
    assert_eq!(entries.iter().filter(|e| matches!(e, SessionEntry::Compaction(_))).count(), 1);
    harness.cleanup();
}
