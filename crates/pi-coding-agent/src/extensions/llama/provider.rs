//! Port of `packages/coding-agent/src/extensions/llama/provider.ts`.

use crate::extensions::llama::client::LlamaModelInfo;
use pi_ai::models::Provider;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const LLAMA_PROVIDER_ID: &str = "llama.cpp";

pub const DEFAULT_LLAMA_SERVER_URL: &str = "http://127.0.0.1:8080";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LlamaClassifierApi {
    #[serde(rename = "llama-cpp-classify")]
    LlamaCppClassify,
    #[serde(rename = "typesafe-system-one")]
    TypesafeSystemOne,
}

impl LlamaClassifierApi {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LlamaCppClassify => "llama-cpp-classify",
            Self::TypesafeSystemOne => "typesafe-system-one",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaProviderControllerSetCatalogOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_autoload: Option<bool>,
}

pub trait LlamaProviderController: Send + Sync {
    fn provider(&self) -> Arc<dyn Provider>;
    fn set_provider(&self, value: Arc<dyn Provider>);
    fn set_catalog(
        &self,
        models: &[LlamaModelInfo],
        server_url: &str,
        options: Option<&LlamaProviderControllerSetCatalogOptions>,
    );
}

pub fn create_llama_provider() -> Arc<dyn LlamaProviderController> {
    todo!("port extensions/llama/provider.ts:createLlamaProvider")
}
