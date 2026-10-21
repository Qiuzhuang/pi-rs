//! Port of `packages/coding-agent/src/modes/interactive/model-search.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSearchItem {
    pub id: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

pub fn get_model_search_text(item: &ModelSearchItem) -> String {
    todo!("port modes/interactive/model-search.ts:getModelSearchText")
}

/// The /model selector search should rank exact provider-prefixed queries before proxy-provider IDs
/// like openrouter/openai/gpt-5, so keep the bare model ID out of the leading position.
pub fn get_model_selector_search_text(item: &ModelSearchItem) -> String {
    todo!("port modes/interactive/model-search.ts:getModelSelectorSearchText")
}
