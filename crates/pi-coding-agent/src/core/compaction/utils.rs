//! Port of `packages/coding-agent/src/core/compaction/utils.ts`.

use indexmap::IndexSet;
use pi_agent::types::AgentMessage;
use pi_ai::types::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperations {
    pub read: IndexSet<String>,
    pub written: IndexSet<String>,
    pub edited: IndexSet<String>,
}

pub fn create_file_ops() -> FileOperations {
    todo!("port core/compaction/utils.ts:createFileOps")
}

/// Extract file operations from tool calls in an assistant message, or from the nested calls
/// recorded on a tool result.
pub fn extract_file_ops_from_message(message: AgentMessage, file_ops: &FileOperations) {
    todo!("port core/compaction/utils.ts:extractFileOpsFromMessage")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyComputeFileListsResult {
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

/// Compute final file lists from file operations.
/// Returns readFiles (files only read, not modified) and modifiedFiles.
pub fn compute_file_lists(file_ops: &FileOperations) -> EmptyComputeFileListsResult {
    todo!("port core/compaction/utils.ts:computeFileLists")
}

/// Format file operations as XML tags for summary.
pub fn format_file_operations(read_files: &[String], modified_files: &[String]) -> String {
    todo!("port core/compaction/utils.ts:formatFileOperations")
}

/// Serialize LLM messages to text for summarization.
/// This prevents the model from treating it as a conversation to continue.
/// Call convertToLlm() first to handle custom message types.
///
/// Tool results are truncated to keep the summarization request within
/// reasonable token budgets. Full content is not needed for summarization.
///
/// Thinking is omitted. Providers return summaries of the real reasoning, and
/// replayed thinking is often not part of the model's live context: for example,
/// a block that does not match its request is dropped. Including it can make the
/// summarization request larger than the context it summarizes. Visible text,
/// tool calls, and tool results record what happened.
pub fn serialize_conversation(messages: &[Message]) -> String {
    todo!("port core/compaction/utils.ts:serializeConversation")
}

pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";
