//! Port of `test/suite/agent-session-queue.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::types::{Message, UserContent};
use pi_coding_agent::core::agent_session::{
    AgentSessionEvent, AgentSessionEventListener, AgentSessionFollowUpOptions,
    AgentSessionSendCustomMessageMessage, AgentSessionSendCustomMessageOptions,
    AgentSessionSendCustomMessageOptionsDeliverAs, AgentSessionSteerOptions,
    AgentSessionSteeringMode,
};
use pi_coding_agent::core::extensions::types::{
    AgentEndEvent, CommandOptions, ExtensionAPIRef, InputEvent, InputEventResult, InputSource,
    SendUserMessageOptions, StreamingBehavior, on_event,
};
use serde_json::json;

use crate::common::*;
use crate::support::*;

fn command(pi: &ExtensionAPIRef, runs: Option<Arc<Mutex<Vec<String>>>>) -> anyhow::Result<()> {
    pi.register_command(
        "testcmd",
        CommandOptions {
            description: Some("Test command".into()),
            get_argument_completions: None,
            handler: Arc::new(move |args, _ctx| {
                let runs = runs.clone();
                Box::pin(async move {
                    if let Some(runs) = runs {
                        runs.lock().push(args);
                    }
                    Ok(())
                })
            }),
        },
    )
}

fn has_user_text(context: &pi_ai::types::TranscriptContext, text: &str) -> bool {
    context.messages.iter().any(|m| match m {
        Message::User(u) => match &u.content {
            UserContent::Blocks(blocks) => blocks
                .iter()
                .any(|b| b.as_text().is_some_and(|t| t.text == text)),
            UserContent::Text(_) => false,
        },
        _ => false,
    })
}

#[tokio::test]
async fn dispatches_extension_commands_immediately_when_prompted_while_idle() {
    let runs = Arc::new(Mutex::new(Vec::new()));
    let r = runs.clone();
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| command(&pi, Some(r.clone())))],
        ..Default::default()
    })
    .await
    .unwrap();

    harness.session.prompt("/testcmd hello world", None).await.unwrap();

    assert_eq!(*runs.lock(), vec!["hello world".to_string()]);
    assert_eq!(harness.get_pending_response_count(), 0);
    assert!(harness.session.messages().is_empty());
    harness.cleanup();
}

#[tokio::test]
async fn delivers_extension_origin_steering_messages_before_the_next_llm_call() {
    let api: Arc<Mutex<Option<ExtensionAPIRef>>> = Arc::new(Mutex::new(None));
    let a = api.clone();
    let mut waiting = create_waiting_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            *a.lock() = Some(pi);
            Ok(())
        })],
        ..Default::default()
    })
    .await;
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        respond_with(|context| {
            let saw = user_llm_texts(context).iter().any(|t| t == "steer now");
            faux_text_message(if saw { "saw steer" } else { "missing steer" })
        }),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    tick().await;

    let pi = api.lock().clone().unwrap();
    pi.send_user_message(
        UserContent::Text("steer now".into()),
        SendUserMessageOptions {
            deliver_as: Some(StreamingBehavior::Steer),
            expand_prompt_templates: None,
        },
    )
    .unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    let harness = &waiting.harness;
    assert_eq!(harness.user_texts(), vec!["start", "steer now"]);
    assert!(harness.assistant_texts().contains(&"saw steer".to_string()));
    harness.cleanup();
}

#[tokio::test]
async fn delivers_follow_up_messages_only_after_the_current_run_finishes() {
    let mut waiting = create_waiting_harness(HarnessOptions::default()).await;
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let s = seen.clone();
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        respond_with(move |context| {
            s.lock().extend(
                context
                    .messages
                    .iter()
                    .filter(|m| matches!(m, Message::Assistant(_)))
                    .map(llm_text),
            );
            faux_text_message("follow-up response")
        }),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    waiting
        .harness
        .session
        .follow_up("after current run", None, None)
        .await
        .unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    let harness = &waiting.harness;
    assert_eq!(harness.user_texts(), vec!["start", "after current run"]);
    assert!(seen.lock().contains(&String::new()));
    assert!(harness.assistant_texts().contains(&"follow-up response".to_string()));
    harness.cleanup();
}

// Regression test for #8718.
#[tokio::test]
async fn runs_direct_steering_and_follow_up_messages_through_input_handlers() {
    type Seen = (String, InputSource, Option<StreamingBehavior>);
    let input_events = Arc::new(Mutex::new(Vec::<Seen>::new()));
    let events = input_events.clone();
    let mut waiting = create_waiting_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let events = events.clone();
            on_event::<InputEvent, _, _>(&*pi, move |event, _ctx| {
                let events = events.clone();
                async move {
                    events
                        .lock()
                        .push((event.text.clone(), event.source, event.streaming_behavior));
                    if event.text.starts_with("handle") {
                        return Ok(Some(InputEventResult::Handled));
                    }
                    Ok(Some(InputEventResult::Transform {
                        text: format!("transformed: {}", event.text),
                        images: None,
                    }))
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await;
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        text("steered"),
        text("followed up"),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    input_events.lock().clear();
    let session = waiting.harness.session.clone();
    let steer = AgentSessionSteerOptions {
        source: Some(InputSource::Rpc),
    };
    let follow = AgentSessionFollowUpOptions {
        source: Some(InputSource::Rpc),
    };
    session.steer("steer me", None, Some(&steer)).await.unwrap();
    session.steer("handle steer", None, Some(&steer)).await.unwrap();
    session.follow_up("follow me", None, Some(&follow)).await.unwrap();
    session.follow_up("handle follow", None, Some(&follow)).await.unwrap();

    assert_eq!(
        *input_events.lock(),
        vec![
            ("steer me".to_string(), InputSource::Rpc, Some(StreamingBehavior::Steer)),
            ("handle steer".to_string(), InputSource::Rpc, Some(StreamingBehavior::Steer)),
            ("follow me".to_string(), InputSource::Rpc, Some(StreamingBehavior::FollowUp)),
            ("handle follow".to_string(), InputSource::Rpc, Some(StreamingBehavior::FollowUp)),
        ]
    );
    assert_eq!(session.get_steering_messages(), vec!["transformed: steer me"]);
    assert_eq!(session.get_follow_up_messages(), vec!["transformed: follow me"]);
    waiting.release();
    waiting.finish().await.unwrap();
    waiting.harness.cleanup();
}

#[tokio::test]
async fn delivers_multiple_steering_messages_in_order_in_one_at_a_time_mode() {
    let mut waiting = create_waiting_harness(HarnessOptions::default()).await;
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        text("handled steer 1"),
        text("handled steer 2"),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    let session = waiting.harness.session.clone();
    session.steer("steer 1", None, None).await.unwrap();
    session.steer("steer 2", None, None).await.unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    let harness = &waiting.harness;
    assert_eq!(harness.user_texts(), vec!["start", "steer 1", "steer 2"]);
    assert_eq!(harness.assistant_texts(), vec!["", "handled steer 1", "handled steer 2"]);
    harness.cleanup();
}

#[tokio::test]
async fn delivers_multiple_follow_up_messages_in_order_in_one_at_a_time_mode() {
    let mut waiting = create_waiting_harness(HarnessOptions::default()).await;
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        text("original turn complete"),
        text("handled follow-up 1"),
        text("handled follow-up 2"),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    let session = waiting.harness.session.clone();
    session.follow_up("follow-up 1", None, None).await.unwrap();
    session.follow_up("follow-up 2", None, None).await.unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    let harness = &waiting.harness;
    assert_eq!(harness.user_texts(), vec!["start", "follow-up 1", "follow-up 2"]);
    assert_eq!(
        harness.assistant_texts(),
        vec!["", "original turn complete", "handled follow-up 1", "handled follow-up 2"]
    );
    harness.cleanup();
}

#[tokio::test]
async fn delivers_all_steering_messages_in_one_batch_in_all_mode() {
    let mut waiting = create_waiting_harness(HarnessOptions::default()).await;
    waiting.harness.session.set_steering_mode(AgentSessionSteeringMode::All);
    let batched = Arc::new(Mutex::new(Vec::<String>::new()));
    let b = batched.clone();
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        respond_with(move |context| {
            *b.lock() = user_llm_texts(context);
            faux_text_message("batched steer response")
        }),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    let session = waiting.harness.session.clone();
    session.steer("steer 1", None, None).await.unwrap();
    session.steer("steer 2", None, None).await.unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    assert_eq!(*batched.lock(), vec!["start", "steer 1", "steer 2"]);
    assert_eq!(waiting.harness.assistant_texts(), vec!["", "batched steer response"]);
    waiting.harness.cleanup();
}

#[tokio::test]
async fn delivers_all_follow_up_messages_in_one_batch_in_all_mode() {
    let mut waiting = create_waiting_harness(HarnessOptions::default()).await;
    waiting.harness.session.set_follow_up_mode(AgentSessionSteeringMode::All);
    let batched = Arc::new(Mutex::new(Vec::<String>::new()));
    let b = batched.clone();
    waiting.harness.set_responses(vec![
        tool_use("wait", json!({})),
        text("original turn complete"),
        respond_with(move |context| {
            *b.lock() = user_llm_texts(context);
            faux_text_message("batched follow-up response")
        }),
    ]);
    waiting.start();
    waiting.wait_for_tool_start().await;
    let session = waiting.harness.session.clone();
    session.follow_up("follow-up 1", None, None).await.unwrap();
    session.follow_up("follow-up 2", None, None).await.unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    assert_eq!(*batched.lock(), vec!["start", "follow-up 1", "follow-up 2"]);
    assert_eq!(
        waiting.harness.assistant_texts(),
        vec!["", "original turn complete", "batched follow-up response"]
    );
    waiting.harness.cleanup();
}

async fn queue_custom_message(
    deliver_as: AgentSessionSendCustomMessageOptionsDeliverAs,
    content: &'static str,
    extra_turn: bool,
) {
    let mut waiting = create_waiting_harness(HarnessOptions::default()).await;
    let saw = Arc::new(Mutex::new(false));
    let s = saw.clone();
    let mut responses = vec![tool_use("wait", json!({}))];
    if extra_turn {
        responses.push(text("original turn complete"));
    }
    responses.push(respond_with(move |context| {
        *s.lock() = has_user_text(context, content);
        faux_text_message("done")
    }));
    waiting.harness.set_responses(responses);
    waiting.start();
    waiting.wait_for_tool_start().await;
    waiting
        .harness
        .session
        .send_custom_message(
            &AgentSessionSendCustomMessageMessage {
                custom_type: "queue-test".into(),
                content: UserContent::Text(content.into()),
                display: true,
                details: Some(json!({"value": 1})),
            },
            Some(&AgentSessionSendCustomMessageOptions {
                trigger_turn: None,
                deliver_as: Some(deliver_as),
            }),
        )
        .await
        .unwrap();
    waiting.release();
    waiting.finish().await.unwrap();

    assert!(*saw.lock());
    assert!(waiting.harness.session.messages().iter().any(|m| matches!(
        m,
        AgentMessage::Custom(c) if c.get("customType").and_then(|v| v.as_str()) == Some("queue-test")
    )));
    waiting.harness.cleanup();
}

#[tokio::test]
async fn queues_custom_messages_with_deliver_as_steer_while_streaming() {
    queue_custom_message(
        AgentSessionSendCustomMessageOptionsDeliverAs::Steer,
        "steer custom",
        false,
    )
    .await;
}

#[tokio::test]
async fn queues_custom_messages_with_deliver_as_follow_up_while_streaming() {
    queue_custom_message(
        AgentSessionSendCustomMessageOptionsDeliverAs::FollowUp,
        "follow-up custom",
        true,
    )
    .await;
}

#[tokio::test]
async fn injects_next_turn_custom_messages_into_the_next_prompt() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let saw = Arc::new(Mutex::new(false));
    harness
        .session
        .send_custom_message(
            &AgentSessionSendCustomMessageMessage {
                custom_type: "next-turn".into(),
                content: UserContent::Text("carry this".into()),
                display: true,
                details: Some(json!({})),
            },
            Some(&AgentSessionSendCustomMessageOptions {
                trigger_turn: None,
                deliver_as: Some(AgentSessionSendCustomMessageOptionsDeliverAs::NextTurn),
            }),
        )
        .await
        .unwrap();
    let s = saw.clone();
    harness.set_responses(vec![respond_with(move |context| {
        *s.lock() = has_user_text(context, "carry this");
        faux_text_message("done")
    })]);

    harness.session.prompt("normal prompt", None).await.unwrap();

    assert!(*saw.lock());
    let roles: Vec<_> = harness.session.messages().iter().map(|m| m.role().to_string()).collect();
    assert_eq!(roles, ["system", "user", "custom", "assistant"]);
    harness.cleanup();
}

#[tokio::test]
async fn updates_pending_message_count_and_removes_queued_text_before_message_start() {
    let mut waiting = create_waiting_harness(HarnessOptions::default()).await;
    waiting
        .harness
        .set_responses(vec![tool_use("wait", json!({})), text("done")]);
    let counts = Arc::new(Mutex::new(Vec::<i64>::new()));
    let weak = Arc::downgrade(&waiting.harness.session);
    let c = counts.clone();
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if let AgentSessionEvent::MessageStart {
            message: message @ AgentMessage::User(_),
        } = &event
            && get_message_text(message) == "queued"
            && let Some(session) = weak.upgrade()
        {
            c.lock().push(session.pending_message_count());
        }
    });
    waiting.harness.session.subscribe(&listener);
    waiting.start();
    waiting.wait_for_tool_start().await;
    waiting.harness.session.steer("queued", None, None).await.unwrap();
    assert_eq!(waiting.harness.session.pending_message_count(), 1);
    waiting.release();
    waiting.finish().await.unwrap();

    assert_eq!(*counts.lock(), vec![0]);
    assert_eq!(waiting.harness.session.pending_message_count(), 0);
    waiting.harness.cleanup();
}

#[tokio::test]
async fn throws_when_queueing_an_extension_command_with_steer_or_follow_up() {
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(|pi| command(&pi, None))],
        ..Default::default()
    })
    .await
    .unwrap();
    let expected = "Extension command \"/testcmd\" cannot be queued. Use prompt() or execute the command when not streaming.";
    let error = harness.session.steer("/testcmd queued", None, None).await.unwrap_err();
    assert_eq!(error.to_string(), expected);
    let error = harness
        .session
        .follow_up("/testcmd queued", None, None)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), expected);
    harness.cleanup();
}

#[tokio::test]
async fn delivers_follow_ups_queued_during_agent_end() {
    let sent = Arc::new(Mutex::new(false));
    let harness = create_harness(HarnessOptions {
        extension_factories: vec![ext(move |pi| {
            let sent = sent.clone();
            let api = pi.clone();
            on_event::<AgentEndEvent, _, _>(&*pi, move |_event, _ctx| {
                let sent = sent.clone();
                let api = api.clone();
                async move {
                    if std::mem::replace(&mut *sent.lock(), true) {
                        return Ok(None);
                    }
                    api.send_user_message(
                        UserContent::Text("conflict report".into()),
                        SendUserMessageOptions {
                            deliver_as: Some(StreamingBehavior::FollowUp),
                            expand_prompt_templates: None,
                        },
                    )?;
                    Ok(None)
                }
            })?;
            Ok(())
        })],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![text("reply"), text("follow-up reply")]);

    harness.session.prompt("hello", None).await.unwrap();
    harness.session.agent().wait_for_idle().await;

    assert_eq!(harness.user_texts(), vec!["hello", "conflict report"]);
    harness.cleanup();
}
