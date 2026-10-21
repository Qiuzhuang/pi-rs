//! Port of `packages/coding-agent/src/core/tools/edit-diff.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DetectLineEndingDetectLineEndingResult {
    #[serde(rename = "\r\n")]
    Empty,
    #[serde(rename = "\n")]
    Empty2,
}

impl DetectLineEndingDetectLineEndingResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Empty => "\r\n",
            Self::Empty2 => "\n",
        }
    }
}

pub fn detect_line_ending(content: &str) -> DetectLineEndingDetectLineEndingResult {
    todo!("port core/tools/edit-diff.ts:detectLineEnding")
}

pub fn normalize_to_lf(text: &str) -> String {
    todo!("port core/tools/edit-diff.ts:normalizeToLF")
}

pub fn restore_line_endings(text: &str, ending: DetectLineEndingDetectLineEndingResult) -> String {
    todo!("port core/tools/edit-diff.ts:restoreLineEndings")
}

/// Normalize text for fuzzy matching. Applies progressive transformations:
/// - Strip trailing whitespace from each line
/// - Normalize smart quotes to ASCII equivalents
/// - Normalize Unicode dashes/hyphens to ASCII hyphen
/// - Normalize special Unicode spaces to regular space
pub fn normalize_for_fuzzy_match(text: &str) -> String {
    todo!("port core/tools/edit-diff.ts:normalizeForFuzzyMatch")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineSpan {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchedEdit {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub edit_index: f64,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub match_index: f64,
    pub match_length: i64,
    pub new_text: String,
}

// TS: Pick<MatchedEdit, "matchIndex" | "matchLength" | "newText">
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextReplacement {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub match_index: f64,
    pub match_length: i64,
    pub new_text: String,
}

/// Apply replacements matched against `baseContent` to `originalContent` while
/// preserving unchanged line blocks from the original.
///
/// This is useful when `baseContent` is a normalized view of the original. Each
/// replacement is widened to the lines it actually touches, those touched lines
/// are rewritten from the normalized base, and all other lines are copied back
/// from `originalContent`. The actual replacement ranges drive preservation so
/// duplicate normalized lines cannot be aligned to the wrong occurrence.
pub fn apply_replacements_preserving_unchanged_lines(
    original_content: &str,
    base_content: &str,
    replacements: &[TextReplacement],
) -> anyhow::Result<String> {
    todo!("port core/tools/edit-diff.ts:applyReplacementsPreservingUnchangedLines")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuzzyMatchResult {
    /// Whether a match was found
    pub found: bool,
    /// The index where the match starts (in the content that should be used for replacement)
    pub index: i64,
    /// Length of the matched text
    pub match_length: i64,
    /// Whether fuzzy matching was used (false = exact match)
    pub used_fuzzy_match: bool,
    /// The content to use for replacement operations.
    /// When exact match: original content. When fuzzy match: normalized content.
    pub content_for_replacement: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedEditsResult {
    pub base_content: String,
    pub new_content: String,
}

/// Find oldText in content, trying exact match first, then fuzzy match.
/// When fuzzy matching is used, the returned contentForReplacement is the
/// fuzzy-normalized version of the content (trailing whitespace stripped,
/// Unicode quotes/dashes normalized to ASCII).
pub fn fuzzy_find_text(content: &str, old_text: &str) -> FuzzyMatchResult {
    todo!("port core/tools/edit-diff.ts:fuzzyFindText")
}

/// Apply one or more exact-text replacements to LF-normalized content.
///
/// All edits are matched against the same original content. Replacements are
/// then applied in reverse order so offsets remain stable. If any edit needs
/// fuzzy matching, the operation runs in fuzzy-normalized content space and then
/// overlays those line-level changes onto the original content so unchanged line
/// blocks keep their original bytes.
pub fn apply_edits_to_normalized_content(
    normalized_content: &str,
    edits: &[Edit],
    path: &str,
) -> anyhow::Result<AppliedEditsResult> {
    todo!("port core/tools/edit-diff.ts:applyEditsToNormalizedContent")
}

/// Generate a standard unified patch.
pub fn generate_unified_patch(
    path: &str,
    old_content: &str,
    new_content: &str,
    context_lines: Option<i64>,
) -> String {
    todo!("port core/tools/edit-diff.ts:generateUnifiedPatch")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDiffStringGenerateDiffStringResult {
    pub diff: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_changed_line: Option<i64>,
}

/// Generate a display-oriented diff string with line numbers and context.
/// Returns both the diff string and the first changed line number (in the new file).
pub fn generate_diff_string(
    old_content: &str,
    new_content: &str,
    context_lines: Option<i64>,
) -> GenerateDiffStringGenerateDiffStringResult {
    todo!("port core/tools/edit-diff.ts:generateDiffString")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDiffResult {
    pub diff: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_changed_line: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDiffError {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComputeEditsDiffComputeEditsDiffResult {
    EditDiffResult(EditDiffResult),
    EditDiffError(EditDiffError),
}

/// Compute the diff for one or more edit operations without applying them.
/// Used for preview rendering in the TUI before the tool executes.
pub async fn compute_edits_diff(
    path: &str,
    edits: &[Edit],
    cwd: &str,
) -> anyhow::Result<ComputeEditsDiffComputeEditsDiffResult> {
    todo!("port core/tools/edit-diff.ts:computeEditsDiff")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComputeEditDiffComputeEditDiffResult {
    EditDiffResult(EditDiffResult),
    EditDiffError(EditDiffError),
}

/// Compute the diff for a single edit operation without applying it.
/// Kept as a convenience wrapper for single-edit callers.
pub async fn compute_edit_diff(
    path: &str,
    old_text: &str,
    new_text: &str,
    cwd: &str,
) -> anyhow::Result<ComputeEditDiffComputeEditDiffResult> {
    todo!("port core/tools/edit-diff.ts:computeEditDiff")
}
