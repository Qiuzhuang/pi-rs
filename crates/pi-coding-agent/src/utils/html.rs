//! Port of `packages/coding-agent/src/utils/html.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedHtmlEntity {
    pub text: String,
    pub length: i64,
}

pub fn decode_html_entity(entity: &str) -> Option<String> {
    todo!("port utils/html.ts:decodeHtmlEntity")
}

pub fn decode_html_entity_at(html: &str, index: i64) -> Option<DecodedHtmlEntity> {
    todo!("port utils/html.ts:decodeHtmlEntityAt")
}
