//! Port of `packages/coding-agent/src/core/compaction/compaction.ts`.

use crate::core::compaction::utils::FileOperations;
use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::SessionProjection;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_agent::types::StreamFn;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::AssistantMessage;
use pi_ai::types::Model;
use pi_ai::types::SimpleStreamOptions;
use pi_ai::types::TranscriptContext;
use pi_ai::types::Usage;
use pi_ai::utils::retry::RetryCallbacks;
use pi_ai::utils::retry::RetryPolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;
use tokio_util::sync::CancellationToken;

/// Details stored in CompactionEntry.details for file tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionDetails {
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

/// Result from compact() - SessionManager adds uuid/parentUuid when saving
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_tokens_after: Option<u64>,
    /// Usage from the LLM call(s) that generated this summary, if available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Extension-specific data (e.g., ArtifactIndex, version markers for structured compaction)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSettings {
    pub enabled: bool,
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}

pub static DEFAULT_COMPACTION_SETTINGS: LazyLock<CompactionSettings> =
    LazyLock::new(|| todo!("port core/compaction/compaction.ts:DEFAULT_COMPACTION_SETTINGS"));

/// Calculate total context tokens from usage.
/// Uses the native totalTokens field when available, falls back to computing from components.
pub fn calculate_context_tokens(usage: Usage) -> u64 {
    todo!("port core/compaction/compaction.ts:calculateContextTokens")
}

/// Find the last valid assistant message usage from session entries.
pub fn get_last_assistant_usage(entries: &[SessionEntry]) -> Option<Usage> {
    todo!("port core/compaction/compaction.ts:getLastAssistantUsage")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsageEstimate {
    pub tokens: u64,
    pub usage_tokens: u64,
    pub trailing_tokens: u64,
    #[serde(default, with = "pi_ai::serde_util::opt_js_f64")]
    pub last_usage_index: Option<f64>,
}

/// Estimate context tokens from messages, using the last assistant usage when available.
/// If there are messages after the last usage, estimate their tokens with estimateTokens.
pub fn estimate_context_tokens(messages: &[AgentMessage]) -> ContextUsageEstimate {
    todo!("port core/compaction/compaction.ts:estimateContextTokens")
}

/// Estimate projected context without trusting usage captured before a later edit or compaction.
pub fn estimate_projected_context_tokens(
    projection: &SessionProjection,
    branch_entries: &[SessionEntry],
) -> ContextUsageEstimate {
    todo!("port core/compaction/compaction.ts:estimateProjectedContextTokens")
}

/// Check if compaction should trigger based on context usage.
pub fn should_compact(
    context_tokens: u64,
    context_window: u64,
    settings: &CompactionSettings,
) -> bool {
    todo!("port core/compaction/compaction.ts:shouldCompact")
}

/// Estimate token count for a message using chars/4 heuristic.
/// This is conservative (overestimates tokens).
pub fn estimate_tokens(message: AgentMessage) -> u64 {
    todo!("port core/compaction/compaction.ts:estimateTokens")
}

/// Find the context-visible user-role message that starts the turn containing the given entry index.
/// Returns -1 if no turn start found before the index.
pub fn find_turn_start_index(entries: &[SessionEntry], entry_index: f64, start_index: i64) -> i64 {
    todo!("port core/compaction/compaction.ts:findTurnStartIndex")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutPointResult {
    /// Index of first entry to keep
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub first_kept_entry_index: f64,
    /// Index of user message that starts the turn being split, or -1 if not splitting
    pub turn_start_index: i64,
    /// Whether this cut splits a turn (cut point is not a user message)
    pub is_split_turn: bool,
}

/// Find the cut point in session entries that keeps approximately `keepRecentTokens`.
///
/// Algorithm: Walk backwards from newest, accumulating estimated message sizes.
/// Stop when we've accumulated >= keepRecentTokens. Cut at that point.
///
/// Can cut at user OR assistant messages (never tool results). When cutting at an
/// assistant message with tool calls, its tool results come after and will be kept.
///
/// Returns CutPointResult with:
/// - firstKeptEntryIndex: the entry index to start keeping from
/// - turnStartIndex: if cutting mid-turn, the user message that started that turn
/// - isSplitTurn: whether we're cutting in the middle of a turn
///
/// Only considers entries between `startIndex` and `endIndex` (exclusive).
pub fn find_cut_point(
    entries: &[SessionEntry],
    start_index: i64,
    end_index: i64,
    keep_recent_tokens: u64,
) -> CutPointResult {
    todo!("port core/compaction/compaction.ts:findCutPoint")
}

/// Returns an error message when a summarization response cannot safely be persisted.
/// A length stop contains partial text and must not become a session checkpoint.
pub fn get_summarization_failure(response: AssistantMessage, label: &str) -> Option<String> {
    todo!("port core/compaction/compaction.ts:getSummarizationFailure")
}

/// Shared choke point for every compaction/branch-summary summarization call. Wraps the
/// single LLM call in {@link retryAssistantCall} so transient stream drops (e.g.
/// `terminated`, socket close) honor the configured retry policy instead of failing
/// the whole compaction on the first attempt. Deterministic errors and aborts return
/// immediately (see {@link retryAssistantCall}).
pub async fn complete_summarization(
    model: Model,
    context: TranscriptContext,
    options: SimpleStreamOptions,
    stream_fn: Option<StreamFn>,
    retry: Option<RetryPolicy>,
    callbacks: Option<RetryCallbacks>,
) -> anyhow::Result<AssistantMessage> {
    todo!("port core/compaction/compaction.ts:completeSummarization")
}

/// Generate a summary of the conversation using the LLM.
/// If previousSummary is provided, uses the update prompt to merge.
pub async fn generate_summary(
    current_messages: &[AgentMessage],
    model: Model,
    reserve_tokens: Option<u64>,
    api_key: Option<&str>,
    headers: Option<IndexMap<String, String>>,
    signal: Option<CancellationToken>,
    custom_instructions: Option<&str>,
    previous_summary: Option<&str>,
    thinking_level: Option<ThinkingLevel>,
    stream_fn: Option<StreamFn>,
    env: Option<IndexMap<String, String>>,
    retry: Option<RetryPolicy>,
    callbacks: Option<RetryCallbacks>,
    session_id: Option<&str>,
) -> anyhow::Result<String> {
    todo!("port core/compaction/compaction.ts:generateSummary")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateSummaryWithUsageGenerateSummaryWithUsageResult {
    pub text: String,
    pub usage: Usage,
}

/// Generate or update a conversation summary and return its provider usage.
pub async fn generate_summary_with_usage(
    current_messages: &[AgentMessage],
    model: Model,
    reserve_tokens: Option<u64>,
    api_key: Option<&str>,
    headers: Option<IndexMap<String, String>>,
    signal: Option<CancellationToken>,
    custom_instructions: Option<&str>,
    previous_summary: Option<&str>,
    thinking_level: Option<ThinkingLevel>,
    stream_fn: Option<StreamFn>,
    env: Option<IndexMap<String, String>>,
    retry: Option<RetryPolicy>,
    callbacks: Option<RetryCallbacks>,
    session_id: Option<&str>,
) -> anyhow::Result<GenerateSummaryWithUsageGenerateSummaryWithUsageResult> {
    todo!("port core/compaction/compaction.ts:generateSummaryWithUsage")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPreparation {
    /// UUID of first entry to keep
    pub first_kept_entry_id: String,
    /// Messages that will be summarized and discarded
    pub messages_to_summarize: Vec<AgentMessage>,
    /// Messages that will be turned into turn prefix summary (if splitting)
    pub turn_prefix_messages: Vec<AgentMessage>,
    /// Whether this is a split turn (cut point in middle of turn)
    pub is_split_turn: bool,
    pub tokens_before: u64,
    /// Summary from previous compaction, for iterative update
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_summary: Option<String>,
    /// File operations extracted from messagesToSummarize
    pub file_ops: FileOperations,
    /// Compaction settions from settings.jsonl
    pub settings: CompactionSettings,
}

pub fn prepare_compaction(
    path_entries: &[SessionEntry],
    settings: &CompactionSettings,
) -> Option<CompactionPreparation> {
    todo!("port core/compaction/compaction.ts:prepareCompaction")
}

/// Generate summaries for compaction using prepared data.
/// Returns CompactionResult - SessionManager adds uuid/parentUuid when saving.
///
/// @param preparation - Pre-calculated preparation from prepareCompaction()
/// @param customInstructions - Optional custom focus for the summary
/// @param sessionId - Optional routing session ID forwarded without enabling prompt caching
pub async fn compact(
    preparation: &CompactionPreparation,
    model: Model,
    api_key: Option<&str>,
    headers: Option<IndexMap<String, String>>,
    custom_instructions: Option<&str>,
    signal: Option<CancellationToken>,
    thinking_level: Option<ThinkingLevel>,
    stream_fn: Option<StreamFn>,
    env: Option<IndexMap<String, String>>,
    retry: Option<RetryPolicy>,
    callbacks: Option<RetryCallbacks>,
    session_id: Option<&str>,
) -> anyhow::Result<CompactionResult> {
    todo!("port core/compaction/compaction.ts:compact")
}
