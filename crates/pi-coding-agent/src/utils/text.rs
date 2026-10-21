//! Port of `packages/coding-agent/src/utils/text.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitBomSplitBomResult {
    pub bom: String,
    pub text: String,
}

/// Split a leading UTF-8 byte order mark from decoded text.
pub fn split_bom(content: &str) -> SplitBomSplitBomResult {
    todo!("port utils/text.ts:splitBom")
}

/// Remove a leading UTF-8 byte order mark from decoded text.
pub fn strip_bom(content: &str) -> String {
    todo!("port utils/text.ts:stripBom")
}
