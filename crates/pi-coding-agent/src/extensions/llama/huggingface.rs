//! Port of `packages/coding-agent/src/extensions/llama/huggingface.ts`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceModel {
    pub id: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub downloads: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceQuantization {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HuggingFaceModelDetailsGated {
    Bool(bool),
    String(String),
    String2(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceModelDetails {
    pub id: String,
    pub gated: HuggingFaceModelDetailsGated,
    pub quantizations: Vec<HuggingFaceQuantization>,
}

pub async fn find_hugging_face_token(env: Option<Value>) -> anyhow::Result<Option<String>> {
    todo!("port extensions/llama/huggingface.ts:findHuggingFaceToken")
}

pub struct HuggingFaceClient {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl HuggingFaceClient {
    pub fn new(token: Option<&str>, base_url: Option<&str>) -> Self {
        todo!("port extensions/llama/huggingface.ts:HuggingFaceClient.constructor")
    }

    pub async fn search(
        &self,
        query: &str,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<Vec<HuggingFaceModel>> {
        todo!("port extensions/llama/huggingface.ts:HuggingFaceClient.search")
    }

    pub async fn details(
        &self,
        id: &str,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<HuggingFaceModelDetails> {
        todo!("port extensions/llama/huggingface.ts:HuggingFaceClient.details")
    }
}
