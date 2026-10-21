//! Port of `packages/coding-agent/src/utils/ansi.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptySplitIncompleteAnsiSuffixResult {
    pub complete: String,
    pub pending: String,
}

/// Split streamed text into a part that is safe to pass to stripAnsi now and a trailing
/// unfinished escape sequence that should be prepended to the next chunk.
pub fn split_incomplete_ansi_suffix(value: &str) -> EmptySplitIncompleteAnsiSuffixResult {
    todo!("port utils/ansi.ts:splitIncompleteAnsiSuffix")
}

pub fn strip_ansi(value: &str) -> anyhow::Result<String> {
    todo!("port utils/ansi.ts:stripAnsi")
}
