//! Port of `packages/coding-agent/src/core/tools/edit-diff.ts`.

use crate::core::tools::jsdiff;
use crate::core::tools::path_utils::resolve_to_cwd;
use crate::utils::text::split_bom;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DetectLineEndingResult {
    #[serde(rename = "\r\n")]
    Crlf,
    #[serde(rename = "\n")]
    Lf,
}

impl DetectLineEndingResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Crlf => "\r\n",
            Self::Lf => "\n",
        }
    }
}

pub fn detect_line_ending(content: &str) -> DetectLineEndingResult {
    let crlf_idx = content.find("\r\n");
    let lf_idx = content.find('\n');
    match (crlf_idx, lf_idx) {
        (Some(crlf), Some(lf)) if crlf < lf => DetectLineEndingResult::Crlf,
        _ => DetectLineEndingResult::Lf,
    }
}

pub fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn restore_line_endings(text: &str, ending: DetectLineEndingResult) -> String {
    match ending {
        DetectLineEndingResult::Crlf => text.replace('\n', "\r\n"),
        DetectLineEndingResult::Lf => text.to_string(),
    }
}

/// Normalize text for fuzzy matching. Applies progressive transformations:
/// - Strip trailing whitespace from each line
/// - Normalize smart quotes to ASCII equivalents
/// - Normalize Unicode dashes/hyphens to ASCII hyphen
/// - Normalize special Unicode spaces to regular space
pub fn normalize_for_fuzzy_match(text: &str) -> String {
    use crate::core::tools::js_util::js_trim_end;
    use unicode_normalization::UnicodeNormalization;
    let nfkc: String = text.nfkc().collect();
    nfkc.split('\n')
        .map(js_trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .map(|c| match c {
            // Smart single quotes → '
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            // Smart double quotes → "
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            // Various dashes/hyphens → -
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' | '\u{2212}' => '-',
            // Special spaces → regular space
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
            c => c,
        })
        .collect()
}

/// `content.match(/[^\n]*\n|[^\n]+/g) ?? []`
fn split_lines_with_endings(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, _) in content.match_indices('\n') {
        lines.push(&content[start..=index]);
        start = index + 1;
    }
    if start < content.len() {
        lines.push(&content[start..]);
    }
    lines
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
    pub edit_index: i64,
    pub match_index: i64,
    pub match_length: i64,
    pub new_text: String,
}

// TS: Pick<MatchedEdit, "matchIndex" | "matchLength" | "newText">
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextReplacement {
    pub match_index: i64,
    pub match_length: i64,
    pub new_text: String,
}

impl From<&MatchedEdit> for TextReplacement {
    fn from(edit: &MatchedEdit) -> Self {
        Self {
            match_index: edit.match_index,
            match_length: edit.match_length,
            new_text: edit.new_text.clone(),
        }
    }
}

fn get_line_spans(content: &str) -> Vec<LineSpan> {
    let mut offset = 0i64;
    split_lines_with_endings(content)
        .into_iter()
        .map(|line| {
            let span = LineSpan {
                start: offset,
                end: offset + line.len() as i64,
            };
            offset = span.end;
            span
        })
        .collect()
}

fn get_replacement_line_range(lines: &[LineSpan], replacement: &TextReplacement) -> anyhow::Result<(usize, usize)> {
    let replacement_start = replacement.match_index;
    let replacement_end = replacement.match_index + replacement.match_length;
    let start_line = lines
        .iter()
        .position(|line| replacement_start >= line.start && replacement_start < line.end)
        .ok_or_else(|| anyhow::anyhow!("Replacement range is outside the base content."))?;
    let mut end_line = start_line;
    while end_line < lines.len() && lines[end_line].end < replacement_end {
        end_line += 1;
    }
    if end_line >= lines.len() {
        anyhow::bail!("Replacement range is outside the base content.");
    }
    Ok((start_line, end_line + 1))
}

fn apply_replacements(content: &str, replacements: &[TextReplacement], offset: i64) -> String {
    let mut result = content.to_string();
    for replacement in replacements.iter().rev() {
        let match_index = (replacement.match_index - offset) as usize;
        let match_end = match_index + replacement.match_length as usize;
        result = format!("{}{}{}", &result[..match_index], replacement.new_text, &result[match_end..]);
    }
    result
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
    let original_lines = split_lines_with_endings(original_content);
    let base_lines = get_line_spans(base_content);
    if original_lines.len() != base_lines.len() {
        anyhow::bail!("Cannot preserve unchanged lines because the base content has a different line count.");
    }

    struct Group {
        start_line: usize,
        end_line: usize,
        replacements: Vec<TextReplacement>,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut sorted = replacements.to_vec();
    sorted.sort_by_key(|r| r.match_index);
    for replacement in sorted {
        let (start_line, end_line) = get_replacement_line_range(&base_lines, &replacement)?;
        if let Some(current) = groups.last_mut()
            && start_line < current.end_line
        {
            current.end_line = current.end_line.max(end_line);
            current.replacements.push(replacement);
            continue;
        }
        groups.push(Group {
            start_line,
            end_line,
            replacements: vec![replacement],
        });
    }

    let mut original_line_index = 0usize;
    let mut result = String::new();
    for group in &groups {
        result.push_str(&original_lines[original_line_index..group.start_line].concat());
        let group_start_offset = base_lines[group.start_line].start;
        let group_end_offset = base_lines[group.end_line - 1].end;
        result.push_str(&apply_replacements(
            &base_content[group_start_offset as usize..group_end_offset as usize],
            &group.replacements,
            group_start_offset,
        ));
        original_line_index = group.end_line;
    }
    result.push_str(&original_lines[original_line_index..].concat());
    Ok(result)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
///
/// Offsets are byte offsets into `content_for_replacement`.
pub fn fuzzy_find_text(content: &str, old_text: &str) -> FuzzyMatchResult {
    if let Some(exact_index) = content.find(old_text) {
        return FuzzyMatchResult {
            found: true,
            index: exact_index as i64,
            match_length: old_text.len() as i64,
            used_fuzzy_match: false,
            content_for_replacement: content.to_string(),
        };
    }

    let fuzzy_content = normalize_for_fuzzy_match(content);
    let fuzzy_old_text = normalize_for_fuzzy_match(old_text);
    match fuzzy_content.find(&fuzzy_old_text) {
        None => FuzzyMatchResult {
            found: false,
            index: -1,
            match_length: 0,
            used_fuzzy_match: false,
            content_for_replacement: content.to_string(),
        },
        Some(fuzzy_index) => FuzzyMatchResult {
            found: true,
            index: fuzzy_index as i64,
            match_length: fuzzy_old_text.len() as i64,
            used_fuzzy_match: true,
            content_for_replacement: fuzzy_content,
        },
    }
}

/// `content.split(needle).length - 1` (non-overlapping occurrences; an empty needle splits into
/// UTF-16 code units).
fn js_split_count(content: &str, needle: &str) -> i64 {
    if needle.is_empty() {
        return crate::core::tools::js_util::utf16_len(content) as i64 - 1;
    }
    content.matches(needle).count() as i64
}

fn count_occurrences(content: &str, old_text: &str) -> i64 {
    let fuzzy_content = normalize_for_fuzzy_match(content);
    let fuzzy_old_text = normalize_for_fuzzy_match(old_text);
    js_split_count(&fuzzy_content, &fuzzy_old_text)
}

fn get_not_found_error(path: &str, edit_index: usize, total_edits: usize) -> anyhow::Error {
    if total_edits == 1 {
        return anyhow::anyhow!(
            "Could not find the exact text in {path}. The old text must match exactly including all whitespace and newlines."
        );
    }
    anyhow::anyhow!(
        "Could not find edits[{edit_index}] in {path}. The oldText must match exactly including all whitespace and newlines."
    )
}

fn get_duplicate_error(path: &str, edit_index: usize, total_edits: usize, occurrences: i64) -> anyhow::Error {
    if total_edits == 1 {
        return anyhow::anyhow!(
            "Found {occurrences} occurrences of the text in {path}. The text must be unique. Please provide more context to make it unique."
        );
    }
    anyhow::anyhow!(
        "Found {occurrences} occurrences of edits[{edit_index}] in {path}. Each oldText must be unique. Please provide more context to make it unique."
    )
}

fn get_empty_old_text_error(path: &str, edit_index: usize, total_edits: usize) -> anyhow::Error {
    if total_edits == 1 {
        return anyhow::anyhow!("oldText must not be empty in {path}.");
    }
    anyhow::anyhow!("edits[{edit_index}].oldText must not be empty in {path}.")
}

fn get_no_change_error(path: &str, total_edits: usize) -> anyhow::Error {
    if total_edits == 1 {
        return anyhow::anyhow!(
            "No changes made to {path}. The replacement produced identical content. This might indicate an issue with special characters or the text not existing as expected."
        );
    }
    anyhow::anyhow!("No changes made to {path}. The replacements produced identical content.")
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
    let normalized_edits: Vec<Edit> = edits
        .iter()
        .map(|edit| Edit {
            old_text: normalize_to_lf(&edit.old_text),
            new_text: normalize_to_lf(&edit.new_text),
        })
        .collect();

    for (i, edit) in normalized_edits.iter().enumerate() {
        if edit.old_text.is_empty() {
            return Err(get_empty_old_text_error(path, i, normalized_edits.len()));
        }
    }

    let used_fuzzy_match = normalized_edits
        .iter()
        .any(|edit| fuzzy_find_text(normalized_content, &edit.old_text).used_fuzzy_match);
    let replacement_base_content = if used_fuzzy_match {
        normalize_for_fuzzy_match(normalized_content)
    } else {
        normalized_content.to_string()
    };

    let mut matched_edits: Vec<MatchedEdit> = Vec::new();
    for (i, edit) in normalized_edits.iter().enumerate() {
        let match_result = fuzzy_find_text(&replacement_base_content, &edit.old_text);
        if !match_result.found {
            return Err(get_not_found_error(path, i, normalized_edits.len()));
        }
        let occurrences = count_occurrences(&replacement_base_content, &edit.old_text);
        if occurrences > 1 {
            return Err(get_duplicate_error(path, i, normalized_edits.len(), occurrences));
        }
        matched_edits.push(MatchedEdit {
            edit_index: i as i64,
            match_index: match_result.index,
            match_length: match_result.match_length,
            new_text: edit.new_text.clone(),
        });
    }

    matched_edits.sort_by_key(|edit| edit.match_index);
    for pair in matched_edits.windows(2) {
        let (previous, current) = (&pair[0], &pair[1]);
        if previous.match_index + previous.match_length > current.match_index {
            anyhow::bail!(
                "edits[{}] and edits[{}] overlap in {path}. Merge them into one edit or target disjoint regions.",
                previous.edit_index,
                current.edit_index
            );
        }
    }

    let replacements: Vec<TextReplacement> = matched_edits.iter().map(TextReplacement::from).collect();
    let base_content = normalized_content.to_string();
    let new_content = if used_fuzzy_match {
        apply_replacements_preserving_unchanged_lines(normalized_content, &replacement_base_content, &replacements)?
    } else {
        apply_replacements(&replacement_base_content, &replacements, 0)
    };

    if base_content == new_content {
        return Err(get_no_change_error(path, normalized_edits.len()));
    }

    Ok(AppliedEditsResult {
        base_content,
        new_content,
    })
}

/// Generate a standard unified patch.
pub fn generate_unified_patch(
    path: &str,
    old_content: &str,
    new_content: &str,
    context_lines: Option<i64>,
) -> String {
    let context = context_lines.unwrap_or(4).max(0) as usize;
    jsdiff::create_two_files_patch(
        path,
        path,
        old_content,
        new_content,
        None,
        None,
        context,
        jsdiff::FILE_HEADERS_ONLY,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDiffStringResult {
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
) -> GenerateDiffStringResult {
    let context_lines = context_lines.unwrap_or(4).max(0) as usize;
    let parts = jsdiff::diff_lines(old_content, new_content);
    let mut output: Vec<String> = Vec::new();

    let old_line_count = old_content.split('\n').count();
    let new_line_count = new_content.split('\n').count();
    let max_line_num = old_line_count.max(new_line_count);
    let width = max_line_num.to_string().len();
    let num = |n: usize| format!("{n:>width$}");
    let blank = " ".repeat(width);

    let mut old_line_num = 1usize;
    let mut new_line_num = 1usize;
    let mut last_was_change = false;
    let mut first_changed_line: Option<i64> = None;

    for (i, part) in parts.iter().enumerate() {
        let mut raw: Vec<&str> = part.value.split('\n').collect();
        if raw.last() == Some(&"") {
            raw.pop();
        }

        if part.added || part.removed {
            if first_changed_line.is_none() {
                first_changed_line = Some(new_line_num as i64);
            }
            for line in &raw {
                if part.added {
                    output.push(format!("+{} {line}", num(new_line_num)));
                    new_line_num += 1;
                } else {
                    output.push(format!("-{} {line}", num(old_line_num)));
                    old_line_num += 1;
                }
            }
            last_was_change = true;
        } else {
            let next_part_is_change = i + 1 < parts.len() && (parts[i + 1].added || parts[i + 1].removed);
            let has_leading_change = last_was_change;
            let has_trailing_change = next_part_is_change;

            let mut push_context = |line: &str, old_line_num: &mut usize, new_line_num: &mut usize| {
                output.push(format!(" {} {line}", num(*old_line_num)));
                *old_line_num += 1;
                *new_line_num += 1;
            };

            if has_leading_change && has_trailing_change {
                if raw.len() <= context_lines * 2 {
                    for line in &raw {
                        push_context(line, &mut old_line_num, &mut new_line_num);
                    }
                } else {
                    let leading = &raw[..context_lines];
                    let trailing = &raw[raw.len() - context_lines..];
                    let skipped = raw.len() - leading.len() - trailing.len();
                    for line in leading {
                        push_context(line, &mut old_line_num, &mut new_line_num);
                    }
                    output.push(format!(" {blank} ..."));
                    old_line_num += skipped;
                    new_line_num += skipped;
                    for line in trailing {
                        output.push(format!(" {} {line}", num(old_line_num)));
                        old_line_num += 1;
                        new_line_num += 1;
                    }
                }
            } else if has_leading_change {
                let shown = &raw[..raw.len().min(context_lines)];
                let skipped = raw.len() - shown.len();
                for line in shown {
                    push_context(line, &mut old_line_num, &mut new_line_num);
                }
                if skipped > 0 {
                    output.push(format!(" {blank} ..."));
                    old_line_num += skipped;
                    new_line_num += skipped;
                }
            } else if has_trailing_change {
                let skipped = raw.len().saturating_sub(context_lines);
                if skipped > 0 {
                    output.push(format!(" {blank} ..."));
                    old_line_num += skipped;
                    new_line_num += skipped;
                }
                for line in &raw[skipped..] {
                    output.push(format!(" {} {line}", num(old_line_num)));
                    old_line_num += 1;
                    new_line_num += 1;
                }
            } else {
                // Skip these context lines entirely.
                old_line_num += raw.len();
                new_line_num += raw.len();
            }

            last_was_change = false;
        }
    }

    GenerateDiffStringResult {
        diff: output.join("\n"),
        first_changed_line,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDiffResult {
    pub diff: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_changed_line: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDiffError {
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComputeEditsDiffResult {
    EditDiffResult(EditDiffResult),
    EditDiffError(EditDiffError),
}

/// Compute the diff for one or more edit operations without applying them.
/// Used for preview rendering in the TUI before the tool executes.
pub async fn compute_edits_diff(
    path: &str,
    edits: &[Edit],
    cwd: &str,
) -> anyhow::Result<ComputeEditsDiffResult> {
    use crate::core::tools::node_fs;
    let absolute_path = resolve_to_cwd(path, cwd);
    let error = |message: String| Ok(ComputeEditsDiffResult::EditDiffError(EditDiffError { error: message }));

    // Check if file exists and is readable.
    if let Err(err) = node_fs::access(&absolute_path, node_fs::R_OK).await {
        return error(format!("Could not edit file: {path}. Error code: {}.", err.code));
    }

    let raw = match node_fs::read_file(&absolute_path).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(err) => return error(err.message),
    };

    // Strip BOM before matching (LLM won't include invisible BOM in oldText).
    let content = split_bom(&raw).text;
    let normalized_content = normalize_to_lf(&content);
    match apply_edits_to_normalized_content(&normalized_content, edits, path) {
        Ok(applied) => {
            let result = generate_diff_string(&applied.base_content, &applied.new_content, None);
            Ok(ComputeEditsDiffResult::EditDiffResult(EditDiffResult {
                diff: result.diff,
                first_changed_line: result.first_changed_line,
            }))
        }
        Err(err) => error(err.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComputeEditDiffResult {
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
) -> anyhow::Result<ComputeEditDiffResult> {
    let edits = [Edit {
        old_text: old_text.to_string(),
        new_text: new_text.to_string(),
    }];
    Ok(match compute_edits_diff(path, &edits, cwd).await? {
        ComputeEditsDiffResult::EditDiffResult(result) => ComputeEditDiffResult::EditDiffResult(result),
        ComputeEditsDiffResult::EditDiffError(error) => ComputeEditDiffResult::EditDiffError(error),
    })
}
