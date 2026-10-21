//! Port of `packages/coding-agent/src/core/messages.ts`.

use pi_agent::types::AgentMessage;
use pi_ai::types::Message;
use pi_ai::types::UserContent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const COMPACTION_SUMMARY_PREFIX: &str = "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";

pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";

pub const BRANCH_SUMMARY_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";

pub const BRANCH_SUMMARY_SUFFIX: &str = "</summary>";

/// Message type for bash executions via the ! command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashExecutionMessage {
    /// Always "bashExecution".
    pub role: String,
    pub command: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    pub cancelled: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
    pub timestamp: i64,
    /// If true, this message is excluded from LLM context (!! prefix)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_context: Option<bool>,
}

/// Message type for extension-injected messages via sendMessage().
/// These are custom messages that extensions can inject into the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomMessage {
    /// Always "custom".
    pub role: String,
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryMessage {
    /// Always "branchSummary".
    pub role: String,
    pub summary: String,
    #[serde(default)]
    pub from_id: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryMessage {
    /// Always "compactionSummary".
    pub role: String,
    pub summary: String,
    pub tokens_before: u64,
    pub timestamp: i64,
}

/// Convert a BashExecutionMessage to user message text for LLM context.
pub fn bash_execution_to_text(msg: &BashExecutionMessage) -> String {
    todo!("port core/messages.ts:bashExecutionToText")
}

pub fn create_branch_summary_message(
    summary: &str,
    from_id: &str,
    timestamp: &str,
) -> BranchSummaryMessage {
    todo!("port core/messages.ts:createBranchSummaryMessage")
}

pub fn create_compaction_summary_message(
    summary: &str,
    tokens_before: u64,
    timestamp: &str,
) -> CompactionSummaryMessage {
    todo!("port core/messages.ts:createCompactionSummaryMessage")
}

/// Convert CustomMessageEntry to AgentMessage format
pub fn create_custom_message(
    custom_type: &str,
    content: UserContent,
    display: bool,
    details: Option<Value>,
    timestamp: &str,
) -> CustomMessage {
    todo!("port core/messages.ts:createCustomMessage")
}

/// Transform AgentMessages (including custom types) to LLM-compatible Messages.
///
/// This is used by:
/// - Agent's transormToLlm option (for prompt calls and queued messages)
/// - Compaction's generateSummary (for summarization)
/// - Custom extensions and tools
pub fn convert_to_llm(messages: &[AgentMessage]) -> Vec<Message> {
    todo!("port core/messages.ts:convertToLlm")
}
