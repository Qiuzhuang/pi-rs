//! Rust-specific coverage of the TS in-place mutations that the Rust extension API returns as
//! replacements: `tool_call` input (`ToolCallEventResult::input`) must reach the executed tool and
//! the `tool_result` event like a mutated `event.input` does in TS, while the assistant message
//! keeps the model's original arguments (TS validates a clone).

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::{AgentMessage, AgentToolResult, FnAgentTool};
use pi_coding_agent::core::extensions::types::{
    ToolCallEvent, ToolCallEventResult, ToolResultEvent, on_event,
};
use serde_json::{Value, json};

use crate::common::*;
use crate::support::*;

#[tokio::test]
async fn tool_call_input_mutations_reach_the_tool_and_later_handlers() {
    let executed = Arc::new(Mutex::new(Vec::<Value>::new()));
    let e = executed.clone();
    let echo = FnAgentTool::new(
        "echo",
        "Echo",
        "Echo",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        move |_id, params: Value, _s, _u| {
            let e = e.clone();
            async move {
                e.lock().push(params.clone());
                Ok(AgentToolResult::text("ok", Some(json!({}))))
            }
        },
    )
    .into_ref();
    let seen_by_second = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen_by_result = Arc::new(Mutex::new(Vec::<Value>::new()));
    let (s2, sr) = (seen_by_second.clone(), seen_by_result.clone());
    let harness = create_harness(HarnessOptions {
        tools: Some(vec![echo]),
        extension_factories: vec![
            ext(|pi| {
                on_event::<ToolCallEvent, _, _>(&*pi, |event, _ctx| async move {
                    let mut input = event.input.clone();
                    input.insert("text".into(), json!("mutated"));
                    Ok(Some(ToolCallEventResult {
                        input: Some(input),
                        ..Default::default()
                    }))
                })?;
                Ok(())
            }),
            ext(move |pi| {
                let s2 = s2.clone();
                on_event::<ToolCallEvent, _, _>(&*pi, move |event, _ctx| {
                    let s2 = s2.clone();
                    async move {
                        s2.lock().push(Value::Object(event.input.clone()));
                        Ok(None)
                    }
                })?;
                let sr = sr.clone();
                on_event::<ToolResultEvent, _, _>(&*pi, move |event, _ctx| {
                    let sr = sr.clone();
                    async move {
                        sr.lock().push(Value::Object(event.input.clone()));
                        Ok(None)
                    }
                })?;
                Ok(())
            }),
        ],
        ..Default::default()
    })
    .await
    .unwrap();
    harness.set_responses(vec![tool_use("echo", json!({"text": "original"})), text("done")]);

    harness.session.prompt("go", None).await.unwrap();

    assert_eq!(*executed.lock(), vec![json!({"text": "mutated"})]);
    assert_eq!(*seen_by_second.lock(), vec![json!({"text": "mutated"})]);
    assert_eq!(*seen_by_result.lock(), vec![json!({"text": "mutated"})]);
    // The transcript keeps what the model sent.
    let call_args = harness
        .session
        .messages()
        .iter()
        .find_map(|m| match m {
            AgentMessage::Assistant(a) => a.tool_calls().next().map(|c| c.arguments.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(serde_json::to_value(call_args).unwrap(), json!({"text": "original"}));
    harness.cleanup();
}
