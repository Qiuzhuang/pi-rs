//! Port of `packages/coding-agent/src/core/compaction/branch-summarization.ts`.

use crate::core::session_manager::ReadonlySessionManager;
use crate::core::session_manager::SessionEntry;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_agent::types::StreamFn;
use pi_ai::types::Model;
use pi_ai::types::Usage;
use pi_ai::utils::retry::RetryCallbacks;
use pi_ai::utils::retry::RetryPolicy;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub use crate::core::compaction::utils::FileOperations;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryDetails {
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPreparation {
    /// Messages extracted for summarization, in chronological order
    pub messages: Vec<AgentMessage>,
    /// File operations extracted from tool calls
    pub file_ops: FileOperations,
    /// Total estimated tokens in messages
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    session: &ReadonlySessionManager,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> CollectEntriesResult {
    todo!("port core/compaction/branch-summarization.ts:collectEntriesForBranchSummary")
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
pub fn prepare_branch_entries(
    entries: &[SessionEntry],
    token_budget: Option<i64>,
) -> BranchPreparation {
    todo!("port core/compaction/branch-summarization.ts:prepareBranchEntries")
}

/// Generate a summary of abandoned branch entries.
///
/// @param entries - Session entries to summarize (chronological order)
/// @param options - Generation options
pub async fn generate_branch_summary(
    entries: &[SessionEntry],
    options: &GenerateBranchSummaryOptions,
) -> anyhow::Result<BranchSummaryResult> {
    todo!("port core/compaction/branch-summarization.ts:generateBranchSummary")
}
