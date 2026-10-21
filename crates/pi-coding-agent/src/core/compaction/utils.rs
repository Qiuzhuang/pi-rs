//! Port of `packages/coding-agent/src/core/compaction/utils.ts`.
//!
//! Shared utilities for compaction and branch summarization.

use indexmap::IndexSet;
use pi_agent::types::AgentMessage;
use pi_ai::types::{AssistantContent, Message};
use pi_ai::{assistant_content_text, content_text};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ============================================================================
// File Operation Tracking
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperations {
    pub read: IndexSet<String>,
    pub written: IndexSet<String>,
    pub edited: IndexSet<String>,
}

pub fn create_file_ops() -> FileOperations {
    FileOperations::default()
}

/// Extract file operations from tool calls in an assistant message, or from the nested calls
/// recorded on a tool result.
pub fn extract_file_ops_from_message(message: &AgentMessage, file_ops: &mut FileOperations) {
    match message {
        AgentMessage::ToolResult(result) => {
            // Calls made from codemode scripts are recorded on the script's result.
            if let Some(nested) = &result.nested_calls {
                for call in &nested.calls {
                    add_file_op(&call.name, call.arguments.as_ref(), file_ops);
                }
            }
        }
        AgentMessage::Assistant(assistant) => {
            for block in &assistant.content {
                if let AssistantContent::ToolCall(call) = block {
                    add_file_op(&call.name, Some(&call.arguments), file_ops);
                }
            }
        }
        _ => {}
    }
}

fn add_file_op(tool_name: &str, args: Option<&Map<String, Value>>, file_ops: &mut FileOperations) {
    let Some(path) = args
        .and_then(|args| args.get("path"))
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
    else {
        return;
    };
    match tool_name {
        "read" => {
            file_ops.read.insert(path.to_string());
        }
        "write" => {
            file_ops.written.insert(path.to_string());
        }
        "edit" => {
            file_ops.edited.insert(path.to_string());
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeFileListsResult {
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

/// JS default sort order: compare UTF-16 code units.
pub(crate) fn js_string_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Compute final file lists from file operations.
/// Returns readFiles (files only read, not modified) and modifiedFiles.
pub fn compute_file_lists(file_ops: &FileOperations) -> ComputeFileListsResult {
    let modified: IndexSet<&String> = file_ops.edited.iter().chain(file_ops.written.iter()).collect();
    let mut read_only: Vec<String> = file_ops
        .read
        .iter()
        .filter(|f| !modified.contains(f))
        .cloned()
        .collect();
    read_only.sort_by(|a, b| js_string_cmp(a, b));
    let mut modified_files: Vec<String> = modified.into_iter().cloned().collect();
    modified_files.sort_by(|a, b| js_string_cmp(a, b));
    ComputeFileListsResult {
        read_files: read_only,
        modified_files,
    }
}

/// Format file operations as XML tags for summary.
pub fn format_file_operations(read_files: &[String], modified_files: &[String]) -> String {
    let mut sections: Vec<String> = Vec::new();
    if !read_files.is_empty() {
        sections.push(format!("<read-files>\n{}\n</read-files>", read_files.join("\n")));
    }
    if !modified_files.is_empty() {
        sections.push(format!(
            "<modified-files>\n{}\n</modified-files>",
            modified_files.join("\n")
        ));
    }
    if sections.is_empty() {
        return String::new();
    }
    format!("\n\n{}", sections.join("\n\n"))
}

// ============================================================================
// Message Serialization
// ============================================================================

/// Maximum characters for a tool result in serialized summaries.
const TOOL_RESULT_MAX_CHARS: usize = 2000;

/// JS `string.length` (UTF-16 code units).
pub(crate) fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// JS `string.slice(0, max)` (UTF-16 code units; never splits a surrogate pair).
pub(crate) fn js_slice_prefix(text: &str, max: usize) -> &str {
    let mut units = 0usize;
    for (index, ch) in text.char_indices() {
        let width = ch.len_utf16();
        if units + width > max {
            return &text[..index];
        }
        units += width;
    }
    text
}

/// Truncate text to a maximum character length for summarization.
/// Keeps the beginning and appends a truncation marker.
fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    let length = js_len(text);
    if length <= max_chars {
        return text.to_string();
    }
    let truncated_chars = length - max_chars;
    format!(
        "{}\n\n[... {truncated_chars} more characters truncated]",
        js_slice_prefix(text, max_chars)
    )
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
    let mut parts: Vec<String> = Vec::new();

    for msg in messages {
        match msg {
            Message::User(user) => {
                let content = content_text(&user.content, "");
                if !content.is_empty() {
                    parts.push(format!("[User]: {content}"));
                }
            }
            Message::Assistant(assistant) => {
                let mut tool_calls: Vec<String> = Vec::new();
                for block in &assistant.content {
                    if let AssistantContent::ToolCall(call) = block {
                        let args_str = call
                            .arguments
                            .iter()
                            .map(|(k, v)| format!("{k}={}", serde_json::to_string(v).unwrap_or_default()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        tool_calls.push(format!("{}({args_str})", call.name));
                    }
                }

                if assistant.content.iter().any(|b| matches!(b, AssistantContent::Text(_))) {
                    parts.push(format!(
                        "[Assistant]: {}",
                        assistant_content_text(&assistant.content, "\n")
                    ));
                }
                if !tool_calls.is_empty() {
                    parts.push(format!("[Assistant tool calls]: {}", tool_calls.join("; ")));
                }
            }
            Message::ToolResult(result) => {
                let content = pi_ai::blocks_text(&result.content, "");
                if !content.is_empty() {
                    parts.push(format!(
                        "[Tool result]: {}",
                        truncate_for_summary(&content, TOOL_RESULT_MAX_CHARS)
                    ));
                }
            }
            Message::System(_) => {}
        }
    }

    parts.join("\n\n")
}

pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";
