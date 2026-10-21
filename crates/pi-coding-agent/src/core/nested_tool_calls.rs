//! Port of `packages/coding-agent/src/core/nested-tool-calls.ts`.
//!
//! Tool calls that a tool makes while it runs (`ctx.executeTool()`), for example from codemode
//! scripts. The agent loop does not know about them: the session runs each one through the agent's
//! tool pipeline (`runToolCall`) with its own hooks, emits `tool_execution_*` events with
//! `parentToolCallId`, and records the calls and their usage on the model-issued call's tool result
//! message.
//!
//! Nothing here runs until a tool calls `ctx.executeTool()`.

use crate::core::usage_totals::combine_usage;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_agent::types::AgentToolCall;
use pi_agent::types::AgentToolCallOutcome;
use pi_agent::types::AgentToolRef;
use pi_agent::types::AgentToolResult;
use pi_agent::types::AgentToolUpdateCallback;
use pi_agent::types::ToolExecutionMode;
use pi_ai::types::NestedToolCallRecord;
use pi_ai::types::NestedToolCallStatus;
use pi_ai::types::NestedToolCalls;
use pi_ai::types::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Limits of the nested-call record on a tool result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestedCallLimits {
    pub max_calls: usize,
    pub max_argument_bytes_per_call: usize,
    pub max_argument_bytes_total: usize,
    pub max_error_chars: usize,
}

/// Limits of the nested-call record on a tool result: arguments
/// over the per-call or total size are omitted, calls beyond the count are dropped, and the record
/// is marked incomplete when any of that happens.
pub const NESTED_CALL_LIMITS: NestedCallLimits = NestedCallLimits {
    max_calls: 256,
    max_argument_bytes_per_call: 8 * 1024,
    max_argument_bytes_total: 32 * 1024,
    max_error_chars: 500,
};

/// What the nested calls of one model-issued tool call leave on its tool result message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedCallSummary {
    /// Becomes `nestedCalls`. Undefined when no nested call was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<NestedToolCalls>,
    /// Summed `usage` of the nested results, added to the message's `usage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Handle of a recorded call, returned by [`NestedCallRecorder::start`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestedCallRecordHandle(usize);

#[derive(Default)]
struct RecorderState {
    calls: Vec<NestedToolCallRecord>,
    started_at: HashMap<usize, Instant>,
    complete: bool,
    argument_bytes: usize,
    /// Summed usage of every nested result, including calls dropped from the record.
    usage: Option<Usage>,
}

/// Collects the nested calls of one model-issued tool call, including calls made by nested tools.
/// The snapshot becomes `nestedCalls` on the tool result message.
pub struct NestedCallRecorder {
    state: Mutex<RecorderState>,
}

impl Default for NestedCallRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl NestedCallRecorder {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RecorderState {
                complete: true,
                ..Default::default()
            }),
        }
    }

    /// Record a call as it starts. Returns `None` when the call is dropped.
    pub fn start(&self, tool_call: &AgentToolCall) -> Option<NestedCallRecordHandle> {
        let mut state = self.state.lock();
        if state.calls.len() >= NESTED_CALL_LIMITS.max_calls {
            state.complete = false;
            return None;
        }
        let mut record = NestedToolCallRecord {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
            arguments: None,
            arguments_bytes: None,
            status: NestedToolCallStatus::Unfinished,
            duration_ms: None,
            error: None,
        };
        let json = serde_json::to_string(&tool_call.arguments).unwrap_or_else(|_| "{}".into());
        let bytes = json.len();
        if bytes > NESTED_CALL_LIMITS.max_argument_bytes_per_call
            || state.argument_bytes + bytes > NESTED_CALL_LIMITS.max_argument_bytes_total
        {
            record.arguments_bytes = Some(bytes as u64);
            state.complete = false;
        } else {
            record.arguments = Some(tool_call.arguments.clone());
            state.argument_bytes += bytes;
        }
        let index = state.calls.len();
        state.calls.push(record);
        state.started_at.insert(index, Instant::now());
        Some(NestedCallRecordHandle(index))
    }

    pub fn finish(&self, record: Option<NestedCallRecordHandle>, is_error: bool, error_text: &str) {
        let Some(NestedCallRecordHandle(index)) = record else {
            return;
        };
        let mut state = self.state.lock();
        let started = state.started_at.remove(&index);
        let Some(record) = state.calls.get_mut(index) else {
            return;
        };
        record.status = if is_error {
            NestedToolCallStatus::Error
        } else {
            NestedToolCallStatus::Ok
        };
        record.duration_ms = Some(
            started
                .map(|s| (s.elapsed().as_secs_f64() * 1000.0).round() as u64)
                .unwrap_or(0),
        );
        if is_error && !error_text.is_empty() {
            let units: Vec<u16> = error_text
                .encode_utf16()
                .take(NESTED_CALL_LIMITS.max_error_chars)
                .collect();
            record.error = Some(String::from_utf16_lossy(&units));
        }
    }

    pub fn add_usage(&self, usage: &Usage) {
        let mut state = self.state.lock();
        state.usage = Some(match &state.usage {
            Some(current) => combine_usage(current, usage),
            None => usage.clone(),
        });
    }

    pub fn total_usage(&self) -> Option<Usage> {
        self.state.lock().usage.clone()
    }

    /// Copy of the record so far, or `None` when no nested call was made.
    pub fn snapshot(&self) -> Option<NestedToolCalls> {
        let state = self.state.lock();
        if state.calls.is_empty() && state.complete {
            return None;
        }
        let calls = state.calls.clone();
        let complete = state.complete
            && calls
                .iter()
                .all(|call| call.status != NestedToolCallStatus::Unfinished);
        Some(NestedToolCalls { calls, complete })
    }
}

#[derive(Clone, Default)]
pub struct NestedToolCallOptions {
    /// Defaults to the calling tool's signal.
    pub signal: Option<CancellationToken>,
    /// Receives partial results of the nested tool, in addition to `tool_execution_update` events.
    pub on_update: Option<AgentToolUpdateCallback>,
}

/// `tool_execution_*` events of nested calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum NestedToolExecutionEvent {
    #[serde(rename = "tool_execution_start")]
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
        parent_tool_call_id: String,
    },
    #[serde(rename = "tool_execution_update")]
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        args: Value,
        partial_result: AgentToolResult,
        parent_tool_call_id: String,
    },
    #[serde(rename = "tool_execution_end")]
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: AgentToolResult,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        parent_tool_call_id: String,
    },
}

impl NestedToolExecutionEvent {
    pub fn tool_call_id(&self) -> &str {
        match self {
            Self::ToolExecutionStart { tool_call_id, .. }
            | Self::ToolExecutionUpdate { tool_call_id, .. }
            | Self::ToolExecutionEnd { tool_call_id, .. } => tool_call_id,
        }
    }

    pub fn parent_tool_call_id(&self) -> &str {
        match self {
            Self::ToolExecutionStart {
                parent_tool_call_id,
                ..
            }
            | Self::ToolExecutionUpdate {
                parent_tool_call_id,
                ..
            }
            | Self::ToolExecutionEnd {
                parent_tool_call_id,
                ..
            } => parent_tool_call_id,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::ToolExecutionStart { .. } => "tool_execution_start",
            Self::ToolExecutionUpdate { .. } => "tool_execution_update",
            Self::ToolExecutionEnd { .. } => "tool_execution_end",
        }
    }
}

/// Sink for partial results of a nested call.
pub type NestedUpdateSink = Arc<dyn Fn(AgentToolResult) -> BoxFuture<'static, ()> + Send + Sync>;

#[async_trait::async_trait]
pub trait NestedToolCallHost: Send + Sync {
    /// Tools nested calls resolve against.
    fn get_tools(&self) -> Vec<AgentToolRef>;
    /// Whether every nested call runs exclusively, as when the agent executes tool calls sequentially.
    fn is_sequential(&self) -> bool;
    /// Run the call through the tool pipeline, with hooks that report `parentToolCallId`.
    /// Tool failures come back as `is_error: true`.
    async fn run_tool_call(
        &self,
        tool_call: AgentToolCall,
        parent_tool_call_id: &str,
        signal: Option<CancellationToken>,
        on_update: NestedUpdateSink,
    ) -> AgentToolCallOutcome;
    async fn emit(&self, event: NestedToolExecutionEvent);
}

/// Calls below one model-issued call share its recorder.
pub struct CallScope {
    pub recorder: Arc<NestedCallRecorder>,
    pub next_id: AtomicU64,
    /// Set inside a call that holds the exclusive queue, so its own nested calls do not wait on it.
    pub holds_queue: bool,
}

fn text_of(result: &AgentToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text())
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct NestedToolCallRunner {
    host: Arc<dyn NestedToolCallHost>,
    /// Scopes by the id of the calling tool call.
    scopes: Mutex<HashMap<String, Arc<CallScope>>>,
    /// Serializes nested calls that must not run concurrently (fair, FIFO).
    queue: Arc<tokio::sync::Mutex<()>>,
}

impl NestedToolCallRunner {
    pub fn new(host: Arc<dyn NestedToolCallHost>) -> Self {
        Self {
            host,
            scopes: Mutex::new(HashMap::new()),
            queue: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Run `name` on behalf of the call `callerId`. The nested call gets the id `<callerId>/<n>`.
    /// Never fails for tool failures: they come back as `is_error: true`.
    pub async fn execute(
        &self,
        caller_id: &str,
        name: &str,
        args: Value,
        options: Option<&NestedToolCallOptions>,
    ) -> AgentToolCallOutcome {
        let options = options.cloned().unwrap_or_default();
        let scope = self
            .scopes
            .lock()
            .entry(caller_id.to_string())
            .or_insert_with(|| {
                Arc::new(CallScope {
                    recorder: Arc::new(NestedCallRecorder::new()),
                    next_id: AtomicU64::new(1),
                    holds_queue: false,
                })
            })
            .clone();
        let arguments = match args {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        let id = scope.next_id.fetch_add(1, Ordering::SeqCst);
        let tool_call = AgentToolCall {
            id: format!("{caller_id}/{id}"),
            name: name.to_string(),
            arguments,
            ..Default::default()
        };
        let record = scope.recorder.start(&tool_call);
        let args_value = Value::Object(tool_call.arguments.clone());
        self.host
            .emit(NestedToolExecutionEvent::ToolExecutionStart {
                tool_call_id: tool_call.id.clone(),
                tool_name: name.to_string(),
                args: args_value.clone(),
                parent_tool_call_id: caller_id.to_string(),
            })
            .await;

        let exclusive = !scope.holds_queue
            && (self.host.is_sequential()
                || self
                    .host
                    .get_tools()
                    .iter()
                    .find(|tool| tool.name() == name)
                    .and_then(|tool| tool.execution_mode())
                    == Some(ToolExecutionMode::Sequential));
        let guard = if exclusive {
            Some(self.queue.clone().lock_owned().await)
        } else {
            None
        };
        self.scopes.lock().insert(
            tool_call.id.clone(),
            Arc::new(CallScope {
                recorder: scope.recorder.clone(),
                next_id: AtomicU64::new(1),
                holds_queue: scope.holds_queue || exclusive,
            }),
        );

        let host = self.host.clone();
        let on_update_user = options.on_update.clone();
        let update_id = tool_call.id.clone();
        let update_name = name.to_string();
        let update_args = args_value.clone();
        let update_parent = caller_id.to_string();
        let on_update: NestedUpdateSink = Arc::new(move |partial_result: AgentToolResult| {
            if let Some(cb) = &on_update_user {
                cb(partial_result.clone());
            }
            let host = host.clone();
            let event = NestedToolExecutionEvent::ToolExecutionUpdate {
                tool_call_id: update_id.clone(),
                tool_name: update_name.clone(),
                args: update_args.clone(),
                partial_result,
                parent_tool_call_id: update_parent.clone(),
            };
            Box::pin(async move { host.emit(event).await })
        });

        // Remove the scope and release the queue even if the call panics or is cancelled.
        struct ScopeCleanup<'a> {
            scopes: &'a Mutex<HashMap<String, Arc<CallScope>>>,
            id: String,
        }
        impl Drop for ScopeCleanup<'_> {
            fn drop(&mut self) {
                self.scopes.lock().remove(&self.id);
            }
        }
        let outcome = {
            let _cleanup = ScopeCleanup {
                scopes: &self.scopes,
                id: tool_call.id.clone(),
            };
            let outcome = self
                .host
                .run_tool_call(tool_call.clone(), caller_id, options.signal.clone(), on_update)
                .await;
            drop(guard);
            outcome
        };

        scope
            .recorder
            .finish(record, outcome.is_error, &text_of(&outcome.result));
        // Nested results are not persisted, so their usage is only counted through the recorder.
        if let Some(usage) = &outcome.result.usage {
            scope.recorder.add_usage(usage);
        }
        self.host
            .emit(NestedToolExecutionEvent::ToolExecutionEnd {
                tool_call_id: tool_call.id.clone(),
                tool_name: name.to_string(),
                result: outcome.result.clone(),
                is_error: outcome.is_error,
                duration_ms: outcome.duration_ms,
                parent_tool_call_id: caller_id.to_string(),
            })
            .await;
        outcome
    }

    /// Remove and return the record of the nested calls a model-issued call made.
    pub fn take_record(&self, tool_call_id: &str) -> Option<NestedCallSummary> {
        let scope = self.scopes.lock().remove(tool_call_id)?;
        Some(NestedCallSummary {
            calls: scope.recorder.snapshot(),
            usage: scope.recorder.total_usage(),
        })
    }

    pub fn clear(&self) {
        self.scopes.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_agent::types::FnAgentTool;
    use pi_ai::types::{UsageCost, UserContentBlock};
    use serde_json::json;
    use std::sync::Weak;

    fn usage(input: u64, cost: f64) -> Usage {
        Usage {
            input,
            total_tokens: input,
            cost: UsageCost {
                input: cost,
                total: cost,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    struct TestHost {
        tools: Mutex<Vec<AgentToolRef>>,
        sequential: bool,
        events: Mutex<Vec<NestedToolExecutionEvent>>,
    }

    #[async_trait::async_trait]
    impl NestedToolCallHost for TestHost {
        fn get_tools(&self) -> Vec<AgentToolRef> {
            self.tools.lock().clone()
        }
        fn is_sequential(&self) -> bool {
            self.sequential
        }
        async fn run_tool_call(
            &self,
            tool_call: AgentToolCall,
            _parent_tool_call_id: &str,
            signal: Option<CancellationToken>,
            on_update: NestedUpdateSink,
        ) -> AgentToolCallOutcome {
            let tool = self
                .tools
                .lock()
                .iter()
                .find(|t| t.name() == tool_call.name)
                .cloned();
            let Some(tool) = tool else {
                return AgentToolCallOutcome {
                    result: AgentToolResult {
                        content: vec![UserContentBlock::text(format!(
                            "Tool {} not found",
                            tool_call.name
                        ))],
                        details: Some(json!({})),
                        ..Default::default()
                    },
                    tool_call,
                    is_error: true,
                    duration_ms: None,
                };
            };
            let on_update = on_update.clone();
            let callback: AgentToolUpdateCallback = Arc::new(move |partial| {
                let fut = on_update(partial);
                futures::executor::block_on(fut);
            });
            let result = tool
                .execute(
                    &tool_call.id,
                    Value::Object(tool_call.arguments.clone()),
                    signal.unwrap_or_default(),
                    callback,
                )
                .await
                .unwrap();
            let is_error = result.is_error == Some(true);
            AgentToolCallOutcome {
                tool_call,
                result,
                is_error,
                duration_ms: Some(0),
            }
        }
        async fn emit(&self, event: NestedToolExecutionEvent) {
            self.events.lock().push(event);
        }
    }

    fn create_runner(
        tools: Vec<AgentToolRef>,
        sequential: bool,
    ) -> (Arc<NestedToolCallRunner>, Arc<TestHost>) {
        let host = Arc::new(TestHost {
            tools: Mutex::new(tools),
            sequential,
            events: Mutex::new(Vec::new()),
        });
        (Arc::new(NestedToolCallRunner::new(host.clone())), host)
    }

    fn tool<F, Fut>(name: &str, execute: F) -> AgentToolRef
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<AgentToolResult>> + Send + 'static,
    {
        FnAgentTool::new(name, name, name, json!({"type": "object"}), move |id, _p, _s, _u| {
            execute(id)
        })
        .into_ref()
    }

    #[tokio::test]
    async fn assigns_ids_emits_events_and_records_calls() {
        let echo = FnAgentTool::new(
            "echo",
            "Echo",
            "Echo",
            json!({"type": "object"}),
            |_id, _p, _s, on_update: AgentToolUpdateCallback| async move {
                on_update(AgentToolResult::text("partial", Some(json!({}))));
                Ok(AgentToolResult::text("ok", Some(json!({}))))
            },
        )
        .into_ref();
        let (runner, host) = create_runner(vec![echo], false);
        let updates = Arc::new(Mutex::new(0));
        let u = updates.clone();
        let first = runner
            .execute(
                "call",
                "echo",
                json!({"a": 1}),
                Some(&NestedToolCallOptions {
                    signal: None,
                    on_update: Some(Arc::new(move |_| *u.lock() += 1)),
                }),
            )
            .await;
        let missing = runner.execute("call", "missing", json!({}), None).await;
        assert_eq!(first.tool_call.id, "call/1");
        assert_eq!(missing.tool_call.id, "call/2");
        assert!(missing.is_error);
        assert_eq!(*updates.lock(), 1);
        let events: Vec<(String, String, String)> = host
            .events
            .lock()
            .iter()
            .map(|e| {
                (
                    e.type_name().to_string(),
                    e.tool_call_id().to_string(),
                    e.parent_tool_call_id().to_string(),
                )
            })
            .collect();
        let expected: Vec<(String, String, String)> = [
            ("tool_execution_start", "call/1"),
            ("tool_execution_update", "call/1"),
            ("tool_execution_end", "call/1"),
            ("tool_execution_start", "call/2"),
            ("tool_execution_end", "call/2"),
        ]
        .iter()
        .map(|(t, id)| (t.to_string(), id.to_string(), "call".to_string()))
        .collect();
        assert_eq!(events, expected);
        let calls = runner.take_record("call").unwrap().calls.unwrap();
        assert!(calls.complete);
        assert_eq!(calls.calls.len(), 2);
        assert_eq!(calls.calls[0].arguments, Some(json!({"a": 1}).as_object().unwrap().clone()));
        assert_eq!(calls.calls[0].status, NestedToolCallStatus::Ok);
        assert!(calls.calls[0].duration_ms.is_some());
        assert_eq!(calls.calls[1].status, NestedToolCallStatus::Error);
        assert_eq!(calls.calls[1].error.as_deref(), Some("Tool missing not found"));
        assert!(runner.take_record("call").is_none());
        assert!(runner.take_record("other").is_none());
    }

    #[tokio::test]
    async fn records_calls_of_nested_tools_and_sums_usage() {
        let leaf = tool("leaf", |_| async {
            Ok(AgentToolResult {
                usage: Some(usage(10, 0.01)),
                ..AgentToolResult::text("", Some(json!({})))
            })
        });
        let plain = tool("plain", |_| async { Ok(AgentToolResult::text("", Some(json!({})))) });
        let (runner, host) = create_runner(vec![leaf, plain], false);
        let weak: Weak<NestedToolCallRunner> = Arc::downgrade(&runner);
        let middle = tool("middle", move |id| {
            let weak = weak.clone();
            async move {
                let runner = weak.upgrade().unwrap();
                runner.execute(&id, "leaf", json!({}), None).await;
                Ok(AgentToolResult {
                    usage: Some(usage(5, 0.005)),
                    ..AgentToolResult::text("", Some(json!({})))
                })
            }
        });
        host.tools.lock().push(middle);

        runner.execute("call", "middle", json!({}), None).await;
        runner.execute("call", "leaf", json!({}), None).await;
        runner.execute("call", "plain", json!({}), None).await;
        runner.execute("free", "plain", json!({}), None).await;

        let summary = runner.take_record("call").unwrap();
        let ids: Vec<String> = summary
            .calls
            .as_ref()
            .unwrap()
            .calls
            .iter()
            .map(|c| c.id.clone())
            .collect();
        assert_eq!(ids, vec!["call/1", "call/1/1", "call/2", "call/3"]);
        let usage = summary.usage.unwrap();
        assert_eq!(usage.input, 25);
        assert!((usage.cost.total - 0.025).abs() < 1e-10);
        let free = runner.take_record("free").unwrap();
        assert!(free.calls.unwrap().complete);
        assert!(free.usage.is_none());
    }

    #[tokio::test]
    async fn serializes_concurrent_calls_to_sequential_tools() {
        let active = Arc::new(Mutex::new((0usize, 0usize, 0usize))); // active, max seq, max par
        let make = |name: &'static str, sequential: bool| {
            let active = active.clone();
            let t = FnAgentTool::new(name, name, name, json!({"type": "object"}), move |_i, _p, _s, _u| {
                let active = active.clone();
                async move {
                    {
                        let mut a = active.lock();
                        a.0 += 1;
                        if sequential {
                            a.1 = a.1.max(a.0);
                        } else {
                            a.2 = a.2.max(a.0);
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    active.lock().0 -= 1;
                    Ok(AgentToolResult::text("", Some(json!({}))))
                }
            });
            if sequential {
                t.with_execution_mode(ToolExecutionMode::Sequential).into_ref()
            } else {
                t.into_ref()
            }
        };
        let (runner, _host) = create_runner(vec![make("sequential", true), make("parallel", false)], false);
        futures::future::join_all((0..3).map(|_| runner.execute("call", "sequential", json!({}), None))).await;
        futures::future::join_all((0..3).map(|_| runner.execute("call", "parallel", json!({}), None))).await;
        let a = active.lock();
        assert_eq!((a.1, a.2), (1, 3));
    }

    fn call(id: &str, args: Value) -> AgentToolCall {
        AgentToolCall {
            id: id.into(),
            name: "t".into(),
            arguments: args.as_object().unwrap().clone(),
            ..Default::default()
        }
    }

    #[test]
    fn recorder_omits_oversized_arguments_and_drops_calls_beyond_limit() {
        let recorder = NestedCallRecorder::new();
        assert!(recorder.snapshot().is_none());
        let small = recorder.start(&call("a", json!({"x": 1})));
        recorder.finish(small, false, "");
        let snapshot = recorder.snapshot().unwrap();
        assert!(snapshot.complete);
        assert_eq!(snapshot.calls.len(), 1);
        assert_eq!(snapshot.calls[0].status, NestedToolCallStatus::Ok);

        let big = recorder.start(&call(
            "b",
            json!({"text": "x".repeat(NESTED_CALL_LIMITS.max_argument_bytes_per_call)}),
        ));
        recorder.finish(big, true, &"e".repeat(1000));
        let snapshot = recorder.snapshot().unwrap();
        assert!(!snapshot.complete);
        assert_eq!(snapshot.calls[1].status, NestedToolCallStatus::Error);
        assert!(snapshot.calls[1].arguments.is_none());
        assert!(
            snapshot.calls[1].arguments_bytes.unwrap()
                > NESTED_CALL_LIMITS.max_argument_bytes_per_call as u64
        );
        assert_eq!(
            snapshot.calls[1].error.as_ref().unwrap().len(),
            NESTED_CALL_LIMITS.max_error_chars
        );
        for i in 0..NESTED_CALL_LIMITS.max_calls {
            let r = recorder.start(&call(&format!("c{i}"), json!({})));
            recorder.finish(r, false, "");
        }
        assert_eq!(
            recorder.snapshot().unwrap().calls.len(),
            NESTED_CALL_LIMITS.max_calls
        );
    }

    #[test]
    fn recorder_caps_total_argument_size_and_marks_unfinished_incomplete() {
        let recorder = NestedCallRecorder::new();
        let chunk = json!({"text": "x".repeat(7000)});
        let records: Vec<_> = (0..6)
            .map(|i| recorder.start(&call(&format!("c{i}"), chunk.clone())))
            .collect();
        let snapshot = recorder.snapshot().unwrap();
        assert_eq!(
            snapshot
                .calls
                .iter()
                .filter(|c| c.arguments.is_some())
                .count(),
            4
        );
        assert!(
            snapshot
                .calls
                .iter()
                .all(|c| c.status == NestedToolCallStatus::Unfinished)
        );
        assert!(!snapshot.complete);
        assert_eq!(records.len(), 6);
    }
}
