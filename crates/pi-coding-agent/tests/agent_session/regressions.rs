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
