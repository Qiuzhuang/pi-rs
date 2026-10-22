//! Port of `test/suite/agent-session-bash-persistence.test.ts`.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool};
use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session::{
    AgentSessionEvent, AgentSessionEventListener, AgentSessionExecuteBashOptions,
};
use pi_coding_agent::core::bash_executor::BashResult;
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::core::tools::bash::{BashOperations, BashOperationsOptions, BashOperationsResult};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::common::*;
use crate::support::*;

fn entry_types(harness: &Harness) -> Vec<String> {
    harness
        .session_manager
        .get_entries()
        .iter()
        .map(|e| serde_json::to_value(e).unwrap()["type"].as_str().unwrap().to_string())
        .collect()
}

fn hi_result() -> BashResult {
    BashResult {
        output: "hi".into(),
        exit_code: Some(0),
        cancelled: false,
        truncated: false,
        full_output_path: None,
    }
}

fn last_role(harness: &Harness) -> String {
    harness.session.messages().last().map(|m| m.role().to_string()).unwrap_or_default()
}

fn ops(
    exec: impl Fn(String, String, BashOperationsOptions) -> futures::future::BoxFuture<'static, anyhow::Result<BashOperationsResult>>
    + Send
    + Sync
    + 'static,
) -> BashOperations {
    BashOperations { exec: Arc::new(exec) }
}

fn with_ops(operations: BashOperations) -> AgentSessionExecuteBashOptions {
    AgentSessionExecuteBashOptions {
        exclude_from_context: None,
        id: None,
        operations: Some(operations),
    }
}

/// TS `createControlledBashOperations`: each invocation waits until `finish` is called.
#[derive(Clone)]
struct Invocation {
    signal: Option<CancellationToken>,
    finish: Arc<tokio::sync::Notify>,
}

fn controlled_ops(invocations: Arc<Mutex<Vec<Invocation>>>) -> BashOperations {
    ops(move |_command, _cwd, options| {
        let finish = Arc::new(tokio::sync::Notify::new());
        invocations.lock().push(Invocation {
            signal: options.signal.clone(),
            finish: finish.clone(),
        });
        Box::pin(async move {
            finish.notified().await;
            Ok(BashOperationsResult { exit_code: Some(0) })
        })
    })
}

#[tokio::test]
async fn records_bash_results_immediately_while_idle() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();

    harness.session.record_bash_result("echo hi", &hi_result(), None);

    assert!(!harness.session.has_pending_bash_messages());
    assert_eq!(last_role(&harness), "bashExecution");
    assert!(entry_types(&harness).contains(&"message".to_string()));
    harness.cleanup();
}

#[tokio::test]
async fn defers_bash_results_while_streaming_and_flushes_them_before_the_next_prompt() {
    let release = Arc::new(tokio::sync::Notify::new());
    let harness = Arc::new(
        create_harness(HarnessOptions {
            tools: Some(vec![wait_tool(release.clone())]),
            ..Default::default()
        })
        .await
        .unwrap(),
    );
    harness.set_responses(vec![tool_use("wait", json!({})), text("done"), text("after flush")]);
    let saw_tool_start = wait_for_tool_start(&harness, "wait");

    let session = harness.session.clone();
    let first = tokio::spawn(async move { session.prompt("start", None).await });
    saw_tool_start.await.unwrap();
    harness.session.record_bash_result("echo hi", &hi_result(), None);

    let has_bash = |h: &Harness| h.session.messages().iter().any(|m| m.role() == "bashExecution");
    assert!(harness.session.has_pending_bash_messages());
    assert!(!has_bash(&harness));

    release.notify_one();
    first.await.unwrap().unwrap();

    assert!(!harness.session.has_pending_bash_messages());
    assert!(has_bash(&harness));

    harness.session.prompt("next turn", None).await.unwrap();

    assert!(!harness.session.has_pending_bash_messages());
    assert!(has_bash(&harness));
    assert!(entry_types(&harness).iter().any(|t| t == "message"));
    harness.cleanup();
}

#[tokio::test]
async fn executes_bash_commands_and_records_the_result() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();

    let result = harness.session.execute_bash("printf 'hello'", None, None).await.unwrap();

    assert!(result.output.contains("hello"));
    assert_eq!(last_role(&harness), "bashExecution");
    harness.cleanup();
}

#[tokio::test]
async fn cancels_running_bash_commands_with_abort_bash() {
    let harness = Arc::new(create_harness(HarnessOptions::default()).await.unwrap());
    let operations = ops(|_command, _cwd, options| {
        Box::pin(async move {
            if let Some(signal) = options.signal {
                signal.cancelled().await;
            }
            Err(anyhow::anyhow!("aborted"))
        })
    });

    let session = harness.session.clone();
    let bash = tokio::spawn(async move { session.execute_bash("sleep", None, Some(&with_ops(operations))).await });
    tick().await;
    assert!(harness.session.is_bash_running());
    harness.session.abort_bash();

    let result = bash.await.unwrap().unwrap();
    assert!(result.cancelled);
    assert!(!harness.session.is_bash_running());
    harness.cleanup();
}

#[tokio::test]
async fn keeps_newer_bash_execution_tracked_when_an_older_execution_finishes() {
    let harness = Arc::new(create_harness(HarnessOptions::default()).await.unwrap());
    let invocations = Arc::new(Mutex::new(Vec::<Invocation>::new()));
    let operations = controlled_ops(invocations.clone());

    let (s1, o1) = (harness.session.clone(), operations.clone());
    let first = tokio::spawn(async move { s1.execute_bash("first", None, Some(&with_ops(o1))).await });
    tick().await;
    let (s2, o2) = (harness.session.clone(), operations.clone());
    let second = tokio::spawn(async move { s2.execute_bash("second", None, Some(&with_ops(o2))).await });
    tick().await;
    assert_eq!(invocations.lock().len(), 2);

    invocations.lock()[0].finish.notify_one();
    let first_result = first.await.unwrap().unwrap();
    let running_after_first = harness.session.is_bash_running();

    harness.session.abort_bash();
    let second_aborted = invocations.lock()[1].signal.as_ref().is_some_and(|s| s.is_cancelled());
    invocations.lock()[1].finish.notify_one();
    let second_result = second.await.unwrap().unwrap();

    assert!(!first_result.cancelled);
    assert!(running_after_first);
    assert!(second_aborted);
    assert!(second_result.cancelled);
    assert!(!harness.session.is_bash_running());
    harness.cleanup();
}

#[tokio::test]
async fn aborts_all_active_bash_executions() {
    let harness = Arc::new(create_harness(HarnessOptions::default()).await.unwrap());
    let invocations = Arc::new(Mutex::new(Vec::<Invocation>::new()));
    let operations = controlled_ops(invocations.clone());

    let mut runs = Vec::new();
    for name in ["first", "second"] {
        let (s, o) = (harness.session.clone(), operations.clone());
        runs.push(tokio::spawn(async move { s.execute_bash(name, None, Some(&with_ops(o))).await }));
        tick().await;
    }

    harness.session.abort_bash();
    let aborted: Vec<bool> = invocations
        .lock()
        .iter()
        .map(|i| i.signal.as_ref().is_some_and(|s| s.is_cancelled()))
        .collect();
    for invocation in invocations.lock().iter() {
        invocation.finish.notify_one();
    }
    let mut cancelled = Vec::new();
    for run in runs {
        cancelled.push(run.await.unwrap().unwrap().cancelled);
    }

    assert_eq!(aborted, vec![true, true]);
    assert_eq!(cancelled, vec![true, true]);
    assert!(!harness.session.is_bash_running());
    harness.cleanup();
}

#[tokio::test]
async fn persists_user_assistant_tool_result_and_custom_messages_in_order() {
    let echo = FnAgentTool::new(
        "echo",
        "Echo",
        "Echo text back",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        |_id, params: Value, _s, _u| async move {
            let text = params["text"].as_str().unwrap_or_default().to_string();
            Ok(AgentToolResult::text(format!("echo:{text}"), Some(json!({"text": text}))))
        },
    )
    .into_ref();
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo]),
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![tool_use("echo", json!({"text": "hello"})), text("done")]);

    harness
        .session
        .send_custom_message(
            &pi_coding_agent::core::agent_session::AgentSessionSendCustomMessageMessage {
                custom_type: "note".into(),
                content: pi_ai::types::UserContent::Text("hello".into()),
                display: true,
                details: Some(json!({"a": 1})),
            },
            None,
        )
        .await
        .unwrap();
    harness.session.prompt("start", None).await.unwrap();

    // The prompt is declared by the first request, after the queued custom message.
    assert_eq!(
        entry_types(&harness),
        ["custom_message", "message", "message", "message", "message", "message"]
    );
    let roles: Vec<_> = harness.session.messages().iter().map(|m| m.role().to_string()).collect();
    assert_eq!(roles, ["custom", "system", "user", "assistant", "toolResult", "assistant"]);
    harness.cleanup();
}

#[tokio::test]
async fn does_not_emit_message_end_for_bash_execution_messages() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let roles = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = roles.clone();
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if let AgentSessionEvent::MessageEnd { message } = &event {
            r.lock().push(message.role().to_string());
        }
    });
    harness.session.subscribe(&listener);

    harness.session.record_bash_result("echo hi", &hi_result(), None);

    assert!(roles.lock().is_empty());
    harness.cleanup();
}

#[tokio::test]
async fn persists_aborted_assistant_messages() {
    let harness = Arc::new(create_harness(HarnessOptions::default()).await.unwrap());
    harness.set_responses(vec![text(&"x".repeat(20_000))]);
    let saw_update = wait_for_event(&harness, |e| matches!(e, AgentSessionEvent::MessageUpdate { .. }));

    let session = harness.session.clone();
    let run = tokio::spawn(async move { session.prompt("hi", None).await });
    saw_update.await.unwrap();
    harness.session.abort().await.unwrap();
    run.await.unwrap().unwrap();

    match harness.session_manager.get_entries().last() {
        Some(SessionEntry::Message(entry)) => match &entry.message {
            AgentMessage::Assistant(a) => assert_eq!(a.stop_reason, StopReason::Aborted),
            other => panic!("expected an assistant message, got {other:?}"),
        },
        other => panic!("expected a message entry, got {other:?}"),
    }
    harness.cleanup();
}

#[tokio::test]
async fn records_bash_output_through_custom_operations() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let operations = ops(|_command, _cwd, options| {
        (options.on_data)(b"hello from custom ops".to_vec());
        Box::pin(async { Ok(BashOperationsResult { exit_code: Some(0) }) })
    });

    let result = harness
        .session
        .execute_bash("custom", None, Some(&with_ops(operations)))
        .await
        .unwrap();

    assert!(result.output.contains("hello from custom ops"));
    assert_eq!(last_role(&harness), "bashExecution");
    harness.cleanup();
}

#[tokio::test]
async fn streams_bash_output_to_the_callback_and_session_events() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let callback = Arc::new(Mutex::new(Vec::<String>::new()));
    let updates = Arc::new(Mutex::new(Vec::<(Option<String>, String)>::new()));
    let u = updates.clone();
    let listener: AgentSessionEventListener = Arc::new(move |event| {
        if let AgentSessionEvent::BashExecutionUpdate { id, delta } = &event {
            u.lock().push((id.clone(), delta.clone()));
        }
    });
    let unsubscribe = harness.session.subscribe(&listener);
    let operations = ops(|_command, _cwd, options| {
        (options.on_data)(b"hello ".to_vec());
        (options.on_data)(b"world".to_vec());
        Box::pin(async { Ok(BashOperationsResult { exit_code: Some(0) }) })
    });
    let c = callback.clone();

    harness
        .session
        .execute_bash(
            "custom",
            Some(Arc::new(move |delta: String| c.lock().push(delta))),
            Some(&AgentSessionExecuteBashOptions {
                exclude_from_context: None,
                id: Some("bash-1".into()),
                operations: Some(operations),
            }),
        )
        .await
        .unwrap();
    unsubscribe();

    assert_eq!(*callback.lock(), vec!["hello ", "world"]);
    assert_eq!(
        *updates.lock(),
        vec![
            (Some("bash-1".to_string()), "hello ".to_string()),
            (Some("bash-1".to_string()), "world".to_string()),
        ]
    );
    harness.cleanup();
}

// Regression tests for https://github.com/earendil-works/pi/issues/10504 (escape sequences split
// across output chunks).

struct Chunks {
    output: String,
    streamed: String,
    recorded: Option<String>,
    streamed_before_exit: String,
}

async fn run_chunks(chunks: Vec<Vec<u8>>) -> Chunks {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    let deltas = Arc::new(Mutex::new(Vec::<String>::new()));
    let before_exit = Arc::new(Mutex::new(String::new()));
    let (d, b) = (deltas.clone(), before_exit.clone());
    let operations = ops(move |_command, _cwd, options| {
        for chunk in &chunks {
            (options.on_data)(chunk.clone());
        }
        *b.lock() = d.lock().concat();
        Box::pin(async { Ok(BashOperationsResult { exit_code: Some(0) }) })
    });
    let d = deltas.clone();
    let result = harness
        .session
        .execute_bash(
            "custom",
            Some(Arc::new(move |delta: String| d.lock().push(delta))),
            Some(&with_ops(operations)),
        )
        .await
        .unwrap();
    let recorded = harness
        .session
        .messages()
        .last()
        .filter(|m| m.role() == "bashExecution")
        .and_then(|m| m.to_value()["output"].as_str().map(str::to_string));
    let chunks = Chunks {
        output: result.output,
        streamed: deltas.lock().concat(),
        recorded,
        streamed_before_exit: before_exit.lock().clone(),
    };
    harness.cleanup();
    chunks
}

#[tokio::test]
async fn strips_a_color_reset_split_inside_its_parameters() {
    let r = run_chunks(vec![b"\x1b[31mERROR: file.py:1\x1b[0".to_vec(), b"m\n".to_vec()]).await;
    assert_eq!(r.output, "ERROR: file.py:1\n");
    assert_eq!(r.streamed, "ERROR: file.py:1\n");
    assert_eq!(r.recorded.as_deref(), Some("ERROR: file.py:1\n"));
}

#[tokio::test]
async fn strips_a_color_code_split_right_after_esc() {
    let r = run_chunks(vec![b"before\x1b".to_vec(), b"[32mafter\n".to_vec()]).await;
    assert_eq!(r.output, "beforeafter\n");
    assert_eq!(r.streamed, "beforeafter\n");
}

#[tokio::test]
async fn strips_an_osc_sequence_split_before_its_terminator() {
    let r = run_chunks(vec![
        b"a\x1b]0;window ".to_vec(),
        b"title\x1b".to_vec(),
        b"\\b\n".to_vec(),
    ])
    .await;
    assert_eq!(r.output, "ab\n");
}

#[tokio::test]
async fn flushes_an_incomplete_multi_byte_character_at_the_end_of_output() {
    let e_acute = "\u{e9}".as_bytes()[..1].to_vec();
    let r = run_chunks(vec![b"ok".to_vec(), e_acute]).await;
    assert_eq!(r.output, "ok\u{FFFD}");
    assert_eq!(r.streamed, "ok\u{FFFD}");
}

#[tokio::test]
async fn does_not_hold_back_output_behind_a_long_unterminated_sequence() {
    let long = "x".repeat(300);
    let r = run_chunks(vec![format!("\x1b]{long}").into_bytes()]).await;
    assert_eq!(r.streamed_before_exit, format!("]{long}"));
}
