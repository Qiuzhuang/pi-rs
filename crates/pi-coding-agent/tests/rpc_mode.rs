//! RPC mode, print mode and JSON event tests (ports of `test/rpc-prompt-response-semantics.test.ts`,
//! `test/suite/regressions/5868-rpc-unknown-command-id.test.ts`,
//! `test/suite/regressions/7925-toolcall-start-metadata.test.ts`, `test/print-mode.test.ts` and the RPC
//! half of `test/suite/regressions/9068-user-bash-fail-closed.test.ts`),
//! driven through a real `AgentSession` on the faux provider.

mod support;

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use pi_ai::providers::faux::FauxResponseStep;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::core::agent_session::AgentSessionEvent;
use pi_coding_agent::modes::json_event::{JsonAgentSessionEvent, to_json_event};
use pi_coding_agent::modes::print_mode::{PrintModeOptions, PrintModeOptionsMode, run_print_mode};
use pi_coding_agent::modes::rpc::rpc_mode::{RpcIo, RpcOutput, run_rpc_session};
use serde_json::{Value, json};
use support::*;
use tokio::io::AsyncWriteExt;

fn runtime_for(harness: &Harness) -> Arc<AgentSessionRuntime> {
    let services = AgentSessionServices {
        cwd: harness.temp_dir.to_string_lossy().into_owned(),
        agent_dir: harness.temp_dir.to_string_lossy().into_owned(),
        model_runtime: harness.model_runtime.clone(),
        settings_manager: harness.settings_manager.clone(),
        resource_loader: harness.session.resource_loader(),
        diagnostics: Vec::new(),
    };
    let factory: CreateAgentSessionRuntimeFactory =
        Arc::new(|_options| Box::pin(async { anyhow::bail!("session replacement is not available in this test") }));
    Arc::new(AgentSessionRuntime::new(harness.session.clone(), &services, &factory, None, None))
}

struct RpcTest {
    input: tokio::io::DuplexStream,
    lines: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<anyhow::Result<pi_coding_agent::modes::rpc::rpc_mode::RpcModeExit>>,
}

impl RpcTest {
    fn start(runtime: Arc<AgentSessionRuntime>) -> Self {
        let (client, server) = tokio::io::duplex(1 << 16);
        let lines: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = lines.clone();
        let output: RpcOutput = Arc::new(move |line: String| {
            for part in line.split('\n').filter(|l| !l.trim().is_empty()) {
                sink.lock().push(serde_json::from_str(part).expect("output is JSON"));
            }
        });
        let task = tokio::task::spawn_local(async move {
            run_rpc_session(runtime, RpcIo { input: server, output, handle_signals: false }).await
        });
        Self { input: client, lines, task }
    }

    async fn send(&mut self, value: Value) {
        self.input.write_all(format!("{value}\n").as_bytes()).await.unwrap();
    }

    async fn wait_for(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..500 {
            if let Some(found) = self.lines.lock().iter().find(|l| predicate(l)).cloned() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out; output: {:#?}", self.lines.lock());
    }

    fn responses(&self, id: &str) -> Vec<Value> {
        self.lines
            .lock()
            .iter()
            .filter(|l| l["type"] == "response" && l["id"] == id)
            .cloned()
            .collect()
    }

    async fn finish(self) {
        drop(self.input);
        let exit = self.task.await.unwrap().unwrap();
        assert_eq!(exit.exit_code, 0);
    }
}

fn local<F: std::future::Future>(future: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, future)
}

#[test]
fn preserves_the_request_id_on_unknown_command_errors() {
    local(async {
        let harness = create_harness(HarnessOptions::default()).await.unwrap();
        let mut rpc = RpcTest::start(runtime_for(&harness));
        rpc.send(json!({ "id": "test", "type": "foobar" })).await;
        let response = rpc.wait_for(|l| l["id"] == "test").await;
        assert_eq!(
            response,
            json!({ "id": "test", "type": "response", "command": "foobar", "success": false, "error": "Unknown command: foobar" })
        );
        rpc.input.write_all(b"{not json\n").await.unwrap();
        let parse = rpc.wait_for(|l| l["command"] == "parse").await;
        assert_eq!(parse["success"], false);
        assert!(parse["error"].as_str().unwrap().starts_with("Failed to parse command: "));
        rpc.finish().await;
        harness.cleanup();
    });
}

#[test]
fn answers_commands_that_complete_without_waiting_in_command_order() {
    // TS starts each line's handler as the line arrives; handlers that finish without waiting
    // write their responses in the order the commands were sent.
    local(async {
        let harness = create_harness(HarnessOptions::default()).await.unwrap();
        let mut rpc = RpcTest::start(runtime_for(&harness));
        let mut batch = String::new();
        for round in 0..20 {
            batch.push_str(&format!("{}\n", json!({ "id": format!("s{round}"), "type": "get_state" })));
            batch.push_str(&format!("{}\n", json!({ "id": format!("u{round}"), "type": "nope" })));
            batch.push_str("{not json\n");
            batch.push_str(&format!("{}\n", json!({ "id": format!("m{round}"), "type": "get_messages" })));
        }
        rpc.input.write_all(batch.as_bytes()).await.unwrap();
        rpc.wait_for(|l| l["id"] == "m19").await;
        let order: Vec<String> = rpc
            .lines
            .lock()
            .iter()
            .filter(|l| l["type"] == "response")
            .map(|l| l["id"].as_str().map_or_else(|| l["command"].as_str().unwrap().to_string(), str::to_string))
            .collect();
        let expected: Vec<String> = (0..20)
            .flat_map(|round| [format!("s{round}"), format!("u{round}"), "parse".to_string(), format!("m{round}")])
            .collect();
        assert_eq!(order, expected);
        let parse = rpc.wait_for(|l| l["command"] == "parse").await;
        assert_eq!(
            parse["error"],
            "Failed to parse command: Expected property name or '}' in JSON at position 1 (line 1 column 2)"
        );
        rpc.finish().await;
        harness.cleanup();
    });
}

#[test]
fn emits_one_started_response_when_prompt_preflight_succeeds_and_streams_events() {
    local(async {
        let harness = create_harness(HarnessOptions::default()).await.unwrap();
        harness.set_responses(vec![FauxResponseStep::from(faux_text_message("hello there"))]);
        let mut rpc = RpcTest::start(runtime_for(&harness));
        rpc.send(json!({ "id": "p1", "type": "prompt", "message": "hi" })).await;
        rpc.wait_for(|l| l["type"] == "agent_settled").await;
        let responses = rpc.responses("p1");
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["success"], true);
        assert_eq!(responses[0]["data"]["disposition"], "started");
        // Streaming updates carry no cumulative snapshots.
        let updates: Vec<Value> =
            rpc.lines.lock().iter().filter(|l| l["type"] == "message_update").cloned().collect();
        assert!(!updates.is_empty());
        for update in &updates {
            assert!(update.get("message").is_none());
            assert!(update["assistantMessageEvent"].get("partial").is_none());
            assert!(update.get("usage").is_some());
        }

        rpc.send(json!({ "id": "s", "type": "get_state" })).await;
        let state = rpc.wait_for(|l| l["id"] == "s").await;
        assert_eq!(state["data"]["isStreaming"], false);
        assert!(state["data"]["messageCount"].as_u64().unwrap() >= 2);

        rpc.send(json!({ "id": "t", "type": "get_last_assistant_text" })).await;
        let text = rpc.wait_for(|l| l["id"] == "t").await;
        assert_eq!(text["data"], json!({ "text": "hello there" }));

        rpc.send(json!({ "id": "n", "type": "set_session_name", "name": "   " })).await;
        let name = rpc.wait_for(|l| l["id"] == "n").await;
        assert_eq!(name["error"], "Session name cannot be empty");

        rpc.send(json!({ "id": "e", "type": "get_entries", "since": "missing" })).await;
        let entries = rpc.wait_for(|l| l["id"] == "e").await;
        assert_eq!(entries["error"], "Entry not found: missing");

        rpc.send(json!({ "id": "q", "type": "clear_queue" })).await;
        let cleared = rpc.wait_for(|l| l["id"] == "q").await;
        assert_eq!(cleared["data"], json!({ "steering": [], "followUp": [] }));
        rpc.finish().await;
        harness.cleanup();
    });
}

#[test]
fn emits_one_failure_response_when_prompt_preflight_rejects() {
    local(async {
        // No model credentials: prompt preflight rejects before a run starts.
        let harness = create_harness(HarnessOptions { with_configured_auth: Some(false), ..Default::default() })
            .await
            .unwrap();
        let mut rpc = RpcTest::start(runtime_for(&harness));
        rpc.send(json!({ "id": "p", "type": "prompt", "message": "hi" })).await;
        let response = rpc.wait_for(|l| l["id"] == "p").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(rpc.responses("p").len(), 1);
        assert_eq!(response["success"], false);
        assert!(rpc.lines.lock().iter().all(|l| l["type"] != "agent_start"));
        rpc.finish().await;
        harness.cleanup();
    });
}

#[test]
fn tool_call_metadata_is_available_when_streaming_starts() {
    // Regression #7925.
    local(async {
        let harness = create_harness(HarnessOptions::default()).await.unwrap();
        harness.set_responses(vec![
            FauxResponseStep::from(faux_assistant_message(
                vec![faux_tool_call(
                    "write",
                    json!({ "path": "output.txt", "content": "x".repeat(100) }).as_object().unwrap().clone(),
                    Some("call_7925"),
                )],
                FauxMessageOptions { stop_reason: Some(pi_ai::types::StopReason::ToolUse), ..Default::default() },
            )),
            FauxResponseStep::from(faux_text_message("done")),
        ]);
        let _ = harness.session.prompt("write a file", None).await;
        let update = harness
            .events()
            .into_iter()
            .find(|e| {
                matches!(e, AgentSessionEvent::MessageUpdate { assistant_message_event, .. }
                    if assistant_message_event.type_name() == "toolcall_start")
            })
            .expect("toolcall_start update");
        let AgentSessionEvent::MessageUpdate { message: pi_agent::types::AgentMessage::Assistant(assistant), .. } =
            &update
        else {
            panic!("expected an assistant update");
        };
        let JsonAgentSessionEvent::JsonMessageUpdateEvent(wire) = to_json_event(&update).unwrap() else {
            panic!("expected a JSON message update");
        };
        assert_eq!(wire.usage, assistant.usage);
        assert_eq!(
            serde_json::to_value(&wire).unwrap()["assistantMessageEvent"],
            json!({ "type": "toolcall_start", "contentIndex": 0, "id": "call_7925", "toolName": "write" })
        );
        harness.cleanup();
    });
}

#[test]
fn print_mode_returns_exit_codes_and_disposes_the_runtime() {
    local(async {
        let harness = create_harness(HarnessOptions::default()).await.unwrap();
        harness.set_responses(vec![FauxResponseStep::from(faux_text_message("done"))]);
        let code = run_print_mode(
            runtime_for(&harness),
            &PrintModeOptions {
                mode: PrintModeOptionsMode::Text,
                messages: None,
                initial_message: Some("Say done".into()),
                initial_images: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(code, 0);
        assert_eq!(harness.user_texts(), vec!["Say done".to_string()]);
        harness.cleanup();

        // An assistant error returns a non-zero exit code.
        let harness = create_harness(HarnessOptions::default()).await.unwrap();
        harness.set_responses(vec![FauxResponseStep::from(faux_assistant_message(
            vec![],
            FauxMessageOptions {
                stop_reason: Some(pi_ai::types::StopReason::Error),
                error_message: Some("provider failure".into()),
                ..Default::default()
            },
        ))]);
        let code = run_print_mode(
            runtime_for(&harness),
            &PrintModeOptions {
                mode: PrintModeOptionsMode::Text,
                messages: Some(vec!["hello".into()]),
                initial_message: None,
                initial_images: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(code, 1);
        harness.cleanup();
    });
}

// ---------------------------------------------------------------------------
// #9068: RPC `bash` fails closed when a `user_bash` handler fails (the interactive half belongs
// to the interactive-mode audit). The TS test spies on `session.executeBash`; here the command
// creates a marker file, so a skipped execution leaves no file behind.
// ---------------------------------------------------------------------------

mod user_bash_fail_closed {
    use super::*;
    use pi_coding_agent::core::extensions::types::{
        ExtensionAPIRef, ExtensionEventResult, ExtensionFactory, ExtensionHandler,
    };

    fn extension(result: fn() -> anyhow::Result<Option<ExtensionEventResult>>) -> TestExtension {
        let factory: ExtensionFactory = Arc::new(move |pi: ExtensionAPIRef| {
            Box::pin(async move {
                let handler: ExtensionHandler = Arc::new(move |_event, _ctx| Box::pin(async move { result() }));
                pi.on("user_bash", handler)?;
                Ok(())
            })
        });
        TestExtension::new(factory)
    }

    fn run_case(result: fn() -> anyhow::Result<Option<ExtensionEventResult>>, error: Option<&str>) {
        local(async {
            let harness = create_harness(HarnessOptions {
                extension_factories: vec![extension(result)],
                ..Default::default()
            })
            .await
            .unwrap();
            let marker = harness.temp_dir.join("executed");
            let mut rpc = RpcTest::start(runtime_for(&harness));
            let command = format!("touch '{}' && printf 'local output'", marker.display());
            rpc.send(json!({ "id": "bash-request", "type": "bash", "command": command })).await;
            let response = rpc.wait_for(|l| l["type"] == "response" && l["id"] == "bash-request").await;
            assert_eq!(response["command"], "bash");
            assert_eq!(response["success"], error.is_none(), "{response}");
            match error {
                Some(error) => {
                    assert!(response["error"].as_str().unwrap().contains(error), "{response}");
                    let extension_error = rpc.wait_for(|l| l["type"] == "extension_error").await;
                    assert_eq!(extension_error["event"], "user_bash");
                    assert!(extension_error["error"].as_str().unwrap().contains(error), "{extension_error}");
                    assert!(!marker.exists(), "bash ran despite the failing handler");
                }
                None => {
                    assert_eq!(response["data"]["output"], "local output");
                    assert_eq!(response["data"]["exitCode"], 0);
                    assert!(marker.exists());
                }
            }
            rpc.finish().await;
            harness.cleanup();
        });
    }

    #[test]
    fn fails_the_request_without_executing_bash_when_a_handler_throws() {
        run_case(|| anyhow::bail!("Routing failed"), Some("Routing failed"));
    }

    #[test]
    fn fails_the_request_without_executing_bash_when_a_handler_returns_an_empty_result() {
        run_case(|| Ok(Some(ExtensionEventResult::Json(json!({})))), Some("Invalid user_bash handler result"));
    }

    #[test]
    fn executes_bash_normally_when_a_handler_returns_undefined() {
        run_case(|| Ok(None), None);
    }
}

#[test]
fn queue_modes_pass_unrecognized_values_through_like_ts() {
    // TS forwards `command.mode` unchecked and `get_state` reports the agent's raw mode.
    local(async {
        let harness = create_harness(HarnessOptions::default()).await.unwrap();
        let mut rpc = RpcTest::start(runtime_for(&harness));
        rpc.send(json!({ "id": "m", "type": "set_steering_mode", "mode": "bogus" })).await;
        let set = rpc.wait_for(|l| l["id"] == "m").await;
        assert_eq!(set["success"], true);
        rpc.send(json!({ "id": "s", "type": "get_state" })).await;
        let state = rpc.wait_for(|l| l["id"] == "s").await;
        assert_eq!(state["data"]["steeringMode"], "bogus");
        assert_eq!(state["data"]["followUpMode"], "one-at-a-time");
        assert_eq!(harness.settings_manager.get_global_settings_json()["steeringMode"], "bogus");
        rpc.finish().await;
        harness.cleanup();
    });
}
