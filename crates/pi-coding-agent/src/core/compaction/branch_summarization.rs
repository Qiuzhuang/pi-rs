//! Port of `packages/coding-agent/src/core/compaction/branch-summarization.ts`.
//!
//! Branch summarization for tree navigation.
//!
//! When navigating to a different point in the session tree, this generates
//! a summary of the branch being left so context isn't lost.

use crate::core::compaction::compaction::{
    complete_summarization, estimate_tokens, get_summarization_failure, to_provider_env,
    to_provider_headers,
};
use crate::core::compaction::utils::{
    SUMMARIZATION_SYSTEM_PROMPT, compute_file_lists, create_file_ops, extract_file_ops_from_message,
    format_file_operations, serialize_conversation,
};
use crate::core::messages::{
    convert_to_llm, create_branch_summary_message, create_compaction_summary_message,
    create_custom_message,
};
use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::SessionManager;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_agent::types::StreamFn;
use pi_ai::types::Model;
use pi_ai::types::Usage;
use pi_ai::types::{Context, Message, SimpleStreamOptions, StopReason, UserContent, UserContentBlock, UserMessage};
use pi_ai::utils::retry::RetryCallbacks;
use pi_ai::utils::retry::RetryPolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

pub use crate::core::compaction::utils::FileOperations;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aborted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Details stored in BranchSummaryEntry.details for file tracking
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryDetails {
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPreparation {
    /// Messages extracted for summarization, in chronological order
    pub messages: Vec<AgentMessage>,
    /// File operations extracted from tool calls
    pub file_ops: FileOperations,
    /// Total estimated tokens in messages
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectEntriesResult {
    /// Entries to summarize, in chronological order
    pub entries: Vec<SessionEntry>,
    /// Common ancestor between old and new position, if any
    #[serde(default)]
    pub common_ancestor_id: Option<String>,
}

pub struct GenerateBranchSummaryOptions {
    /// Model to use for summarization
    pub model: Model,
    /// API key for the model
    pub api_key: Option<String>,
    /// Request headers for the model
    pub headers: Option<IndexMap<String, String>>,
    /// Provider-scoped environment values for the model
    pub env: Option<IndexMap<String, String>>,
    /// Abort signal for cancellation
    pub signal: CancellationToken,
    /// Optional custom instructions for summarization
    pub custom_instructions: Option<String>,
    /// If true, customInstructions replaces the default prompt instead of being appended
    pub replace_instructions: Option<bool>,
    /// Tokens reserved when selecting branch history (default 16384)
    pub reserve_tokens: Option<u64>,
    /// Optional session stream function. Used to preserve SDK request behavior without mutating agent state.
    pub stream_fn: Option<StreamFn>,
    /// Retry policy for transient summarization errors. Reuses coding-agent's `settings.retry`.
    pub retry: Option<RetryPolicy>,
    /// Optional callbacks for retry reporting (e.g. TUI retry indicators).
    pub callbacks: Option<RetryCallbacks>,
}

// ============================================================================
// Entry Collection
// ============================================================================

/// Collect entries that should be summarized when navigating from one position to another.
///
/// Walks from oldLeafId back to the common ancestor with targetId, collecting entries
/// along the way. Does NOT stop at compaction boundaries - those are included and their
/// summaries become context.
///
/// @param session - Session manager (read-only access)
/// @param oldLeafId - Current position (where we're navigating from)
/// @param targetId - Target position (where we're navigating to)
/// @returns Entries to summarize and the common ancestor
pub fn collect_entries_for_branch_summary(
    session: &SessionManager,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> CollectEntriesResult {
    // If no old position, nothing to summarize
    let Some(old_leaf_id) = old_leaf_id.filter(|id| !id.is_empty()) else {
        return CollectEntriesResult {
            entries: Vec::new(),
            common_ancestor_id: None,
        };
    };

    // Find common ancestor (deepest node that's on both paths)
    let old_path: HashSet<String> = session
        .get_branch(Some(old_leaf_id))
        .iter()
        .map(|e| e.id().to_string())
        .collect();
    let target_path = session.get_branch(Some(target_id));

    // targetPath is root-first, so iterate backwards to find deepest common ancestor
    let common_ancestor_id = target_path
        .iter()
        .rev()
        .find(|entry| old_path.contains(entry.id()))
        .map(|entry| entry.id().to_string());

    // Collect entries from old leaf back to common ancestor
    let mut entries: Vec<SessionEntry> = Vec::new();
    let mut current: Option<String> = Some(old_leaf_id.to_string());

    while let Some(id) = current.filter(|id| !id.is_empty() && Some(id) != common_ancestor_id.as_ref()) {
        let Some(entry) = session.get_entry(&id) else {
            break;
        };
        current = entry.parent_id().map(str::to_string);
        entries.push(entry);
    }

    // Reverse to get chronological order
    entries.reverse();

    CollectEntriesResult {
        entries,
        common_ancestor_id,
    }
}

// ============================================================================
// Entry to Message Conversion
// ============================================================================

/// Extract AgentMessage from a session entry.
/// Similar to getMessageFromEntry in compaction.ts but also handles compaction entries.
fn get_message_from_entry(entry: &SessionEntry) -> Option<AgentMessage> {
    match entry {
        SessionEntry::Message(e) => {
            // Skip tool results - context is in assistant's tool call
            if e.message.role() == "toolResult" {
                return None;
            }
            Some(e.message.clone())
        }
        SessionEntry::CustomMessage(e) => Some(
            create_custom_message(
                &e.custom_type,
                e.content.clone(),
                e.display,
                e.details.clone(),
                &e.timestamp,
            )
            .into(),
        ),
        SessionEntry::BranchSummary(e) => {
            Some(create_branch_summary_message(&e.summary, &e.from_id, &e.timestamp).into())
        }
        SessionEntry::Compaction(e) => {
            Some(create_compaction_summary_message(&e.summary, e.tokens_before, &e.timestamp).into())
        }
        // These don't contribute to conversation content
        _ => None,
    }
}

fn string_array(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .map(|item| match item {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
        ),
        _ => None,
    }
}

/// Prepare entries for summarization with token budget.
///
/// Walks entries from NEWEST to OLDEST, adding messages until we hit the token budget.
/// This ensures we keep the most recent context when the branch is too long.
///
/// Also collects file operations from:
/// - Tool calls in assistant messages
/// - Existing branch_summary entries' details (for cumulative tracking)
///
/// @param entries - Entries in chronological order
/// @param tokenBudget - Maximum tokens to include (0 = no limit)
pub fn prepare_branch_entries(entries: &[SessionEntry], token_budget: Option<i64>) -> BranchPreparation {
    let token_budget = token_budget.unwrap_or(0);
    let mut messages: std::collections::VecDeque<AgentMessage> = std::collections::VecDeque::new();
    let mut file_ops = create_file_ops();
    let mut total_tokens: u64 = 0;

    // First pass: collect file ops from ALL entries (even if they don't fit in token budget)
    // This ensures we capture cumulative file tracking from nested branch summaries
    // Only extract from pi-generated summaries (fromHook !== true), not extension-generated ones
    for entry in entries {
        if let SessionEntry::BranchSummary(summary) = entry
            && !summary.from_hook.unwrap_or(false)
            && let Some(details) = summary.details.as_ref().filter(|d| !d.is_null())
        {
            if let Some(read_files) = string_array(details.get("readFiles")) {
                file_ops.read.extend(read_files);
            }
            if let Some(modified_files) = string_array(details.get("modifiedFiles")) {
                // Modified files go into both edited and written for proper deduplication
                file_ops.edited.extend(modified_files);
            }
        }
    }

    // Second pass: walk from newest to oldest, adding messages until token budget
    for entry in entries.iter().rev() {
        let Some(message) = get_message_from_entry(entry) else {
            continue;
        };

        // Extract file ops from assistant messages (tool calls)
        extract_file_ops_from_message(&message, &mut file_ops);

        let tokens = estimate_tokens(&message);

        // Check budget before adding
        if token_budget > 0 && (total_tokens + tokens) as f64 > token_budget as f64 {
            // If this is a summary entry, try to fit it anyway as it's important context
            if matches!(entry, SessionEntry::Compaction(_) | SessionEntry::BranchSummary(_))
                && (total_tokens as f64) < token_budget as f64 * 0.9
            {
                messages.push_front(message);
                total_tokens += tokens;
            }
            // Stop - we've hit the budget
            break;
        }

        messages.push_front(message);
        total_tokens += tokens;
    }

    BranchPreparation {
        messages: messages.into_iter().collect(),
        file_ops,
        total_tokens,
    }
}

// ============================================================================
// Summary Generation
// ============================================================================

const BRANCH_SUMMARY_PREAMBLE: &str =
    "The user explored a different conversation branch before returning here.\nSummary of that exploration:\n\n";

const BRANCH_SUMMARY_PROMPT: &str = r#"Create a structured summary of this conversation branch for context when returning later.

Use this EXACT format:

## Goal
[What was the user trying to accomplish in this branch?]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned]
- [Or "(none)" if none were mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Work that was started but not finished]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [What should happen next to continue this work]

Keep each section concise. Preserve exact file paths, function names, and error messages."#;

/// Generate a summary of abandoned branch entries.
///
/// @param entries - Session entries to summarize (chronological order)
/// @param options - Generation options
pub async fn generate_branch_summary(
    entries: &[SessionEntry],
    options: &GenerateBranchSummaryOptions,
) -> anyhow::Result<BranchSummaryResult> {
    let model = &options.model;
    let reserve_tokens = options.reserve_tokens.unwrap_or(16384);

    // Token budget = context window minus reserved space for prompt + response
    let context_window = if model.context_window == 0 {
        128000
    } else {
        model.context_window
    };
    let token_budget = context_window as i64 - reserve_tokens as i64;

    let preparation = prepare_branch_entries(entries, Some(token_budget));

    if preparation.messages.is_empty() {
        return Ok(BranchSummaryResult {
            summary: Some("No content to summarize".to_string()),
            ..Default::default()
        });
    }

    // Transform to LLM-compatible messages, then serialize to text
    // Serialization prevents the model from treating it as a conversation to continue
    let llm_messages = convert_to_llm(&preparation.messages);
    let conversation_text = serialize_conversation(&llm_messages);

    // Build prompt
    let custom_instructions = options.custom_instructions.as_deref().filter(|s| !s.is_empty());
    let instructions = match custom_instructions {
        Some(custom) if options.replace_instructions.unwrap_or(false) => custom.to_string(),
        Some(custom) => format!("{BRANCH_SUMMARY_PROMPT}\n\nAdditional focus: {custom}"),
        None => BRANCH_SUMMARY_PROMPT.to_string(),
    };
    let prompt_text = format!("<conversation>\n{conversation_text}\n</conversation>\n\n{instructions}");

    // Call LLM for summarization. Prefer the session stream function so SDK
    // request behavior (timeouts, retries, attribution headers) stays consistent
    // without running through agent state/events. Retried via completeSummarization
    // so transient stream drops reuse the configured retry policy.
    let context = pi_ai::normalize_context_owned(Context {
        system_prompt: Some(SUMMARIZATION_SYSTEM_PROMPT.to_string()),
        messages: vec![Message::User(UserMessage::new(
            UserContent::Blocks(vec![UserContentBlock::text(prompt_text)]),
            chrono::Utc::now().timestamp_millis(),
        ))],
        tools: None,
    });
    let request_options = SimpleStreamOptions {
        api_key: options.api_key.clone(),
        headers: to_provider_headers(options.headers.as_ref()),
        env: to_provider_env(options.env.as_ref()),
        signal: Some(options.signal.clone()),
        ..Default::default()
    };
    let response = complete_summarization(
        model,
        context,
        request_options,
        options.stream_fn.clone(),
        options.retry.as_ref(),
        options.callbacks.as_ref(),
    )
    .await?;

    // Check if aborted or errored
    if response.stop_reason == StopReason::Aborted {
        return Ok(BranchSummaryResult {
            aborted: Some(true),
            ..Default::default()
        });
    }
    if let Some(failure) = get_summarization_failure(&response, "Branch summarization") {
        return Ok(BranchSummaryResult {
            error: Some(failure),
            ..Default::default()
        });
    }
    if response.tool_calls().next().is_some() {
        return Ok(BranchSummaryResult {
            error: Some("Branch summarization attempted to call a tool".to_string()),
            ..Default::default()
        });
    }

    let mut summary = pi_ai::assistant_content_text(&response.content, "\n");

    // Prepend preamble to provide context about the branch summary
    summary = format!("{BRANCH_SUMMARY_PREAMBLE}{summary}");

    // Compute file lists and append to summary
    let lists = compute_file_lists(&preparation.file_ops);
    summary.push_str(&format_file_operations(&lists.read_files, &lists.modified_files));

    Ok(BranchSummaryResult {
        summary: Some(if summary.is_empty() {
            "No summary generated".to_string()
        } else {
            summary
        }),
        usage: Some(response.usage),
        read_files: Some(lists.read_files),
        modified_files: Some(lists.modified_files),
        aborted: None,
        error: None,
    })
}
