//! Port of `packages/coding-agent/src/utils/frontmatter.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedFrontmatter {
    pub frontmatter: Value,
    pub body: String,
}

pub fn parse_frontmatter(content: &str) -> ParsedFrontmatter {
    todo!("port utils/frontmatter.ts:parseFrontmatter")
}

pub fn strip_frontmatter(content: &str) -> String {
    todo!("port utils/frontmatter.ts:stripFrontmatter")
}
