//! Port of `packages/coding-agent/src/core/nested-tool-calls.ts`.

use futures::future::BoxFuture;
use pi_agent::types::AgentToolCall;
use pi_agent::types::AgentToolCallOutcome;
use pi_agent::types::AgentToolRef;
use pi_agent::types::AgentToolResult;
use pi_agent::types::AgentToolUpdateCallback;
use pi_ai::types::NestedToolCallRecord;
use pi_ai::types::NestedToolCalls;
use pi_ai::types::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio_util::sync::CancellationToken;

/// Limits of the nested-call record on a tool result: arguments
/// over the per-call or total size are omitted, calls beyond the count are dropped, and the record
/// is marked incomplete when any of that happens.
pub static NESTED_CALL_LIMITS: LazyLock<Value> =
    LazyLock::new(|| todo!("port core/nested-tool-calls.ts:NESTED_CALL_LIMITS"));

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

/// Collects the nested calls of one model-issued tool call, including calls made by nested tools.
/// The snapshot becomes `nestedCalls` on the tool result message.
pub struct NestedCallRecorder {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl NestedCallRecorder {
    pub fn new() -> Self {
        todo!("port core/nested-tool-calls.ts:NestedCallRecorder.constructor")
    }

    /// Record a call as it starts. Returns undefined when the call is dropped.
    pub fn start(&self, tool_call: AgentToolCall) -> Option<NestedToolCallRecord> {
        todo!("port core/nested-tool-calls.ts:NestedCallRecorder.start")
    }

    pub fn finish(&self, record: Option<NestedToolCallRecord>, is_error: bool, error_text: &str) {
        todo!("port core/nested-tool-calls.ts:NestedCallRecorder.finish")
    }

    pub fn add_usage(&self, usage: Usage) {
        todo!("port core/nested-tool-calls.ts:NestedCallRecorder.addUsage")
    }

    pub fn total_usage(&self) -> Option<Usage> {
        todo!("port core/nested-tool-calls.ts:NestedCallRecorder.totalUsage")
    }

    /// Copy of the record so far, or undefined when no nested call was made.
    pub fn snapshot(&self) -> Option<NestedToolCalls> {
        todo!("port core/nested-tool-calls.ts:NestedCallRecorder.snapshot")
    }
}

#[derive(Clone)]
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
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "pi_ai::serde_util::opt_js_f64"
        )]
        duration_ms: Option<f64>,
        parent_tool_call_id: String,
    },
}

#[async_trait::async_trait]
pub trait NestedToolCallHost: Send + Sync {
    /// Tools nested calls resolve against.
    fn get_tools(&self) -> Vec<AgentToolRef>;
    /// Whether every nested call runs exclusively, as when the agent executes tool calls sequentially.
    fn is_sequential(&self) -> bool;
    /// Run the call through the tool pipeline, with hooks that report `parentToolCallId`.
    async fn run_tool_call(
        &self,
        tool_call: AgentToolCall,
        parent_tool_call_id: &str,
        signal: Option<CancellationToken>,
        on_update: Arc<
            dyn Fn(AgentToolResult) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync,
        >,
    ) -> anyhow::Result<AgentToolCallOutcome>;
    async fn emit(&self, event: &NestedToolExecutionEvent) -> anyhow::Result<()>;
}

/// Calls below one model-issued call share its recorder.
#[derive(Clone)]
pub struct CallScope {
    pub recorder: Arc<NestedCallRecorder>,
    pub next_id: f64,
    /// Set inside a call that holds the exclusive queue, so its own nested calls do not wait on it.
    pub holds_queue: bool,
}

pub struct NestedToolCallRunner {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl NestedToolCallRunner {
    pub fn new(host: Arc<dyn NestedToolCallHost>) -> Self {
        todo!("port core/nested-tool-calls.ts:NestedToolCallRunner.constructor")
    }

    /// Run `name` on behalf of the call `callerId`. The nested call gets the id `<callerId>/<n>`.
    /// Never rejects for tool failures: they come back as `isError: true`.
    pub async fn execute(
        &self,
        caller_id: &str,
        name: &str,
        args: Value,
        options: Option<&NestedToolCallOptions>,
    ) -> anyhow::Result<AgentToolCallOutcome> {
        todo!("port core/nested-tool-calls.ts:NestedToolCallRunner.execute")
    }

    /// Remove and return the record of the nested calls a model-issued call made.
    pub fn take_record(&self, tool_call_id: &str) -> Option<NestedCallSummary> {
        todo!("port core/nested-tool-calls.ts:NestedToolCallRunner.takeRecord")
    }

    pub fn clear(&self) {
        todo!("port core/nested-tool-calls.ts:NestedToolCallRunner.clear")
    }
}
