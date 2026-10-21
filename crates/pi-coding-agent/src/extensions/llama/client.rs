//! Port of `packages/coding-agent/src/extensions/llama/client.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LlamaModelStatus {
    #[serde(rename = "unloaded")]
    Unloaded,
    #[serde(rename = "loading")]
    Loading,
    #[serde(rename = "loaded")]
    Loaded,
    #[serde(rename = "downloading")]
    Downloading,
    #[serde(rename = "sleeping")]
    Sleeping,
}

impl LlamaModelStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unloaded => "unloaded",
            Self::Loading => "loading",
            Self::Loaded => "loaded",
            Self::Downloading => "downloading",
            Self::Sleeping => "sleeping",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoStatusProgress {
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub done: f64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoStatus {
    pub value: LlamaModelStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<bool>,
    #[serde(rename = "exit_code", default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<IndexMap<String, LlamaModelInfoStatusProgress>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoArchitecture {
    #[serde(
        rename = "input_modalities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub input_modalities: Option<Vec<String>>,
    #[serde(
        rename = "output_modalities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_modalities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfoMeta {
    #[serde(
        rename = "n_ctx",
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub n_ctx: Option<f64>,
    #[serde(
        rename = "n_ctx_train",
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub n_ctx_train: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ftype: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    pub status: LlamaModelInfoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<LlamaModelInfoArchitecture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<LlamaModelInfoMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelsResponse {
    pub data: Vec<LlamaModelInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaServerProps {
    #[serde(
        rename = "models_autoload",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub models_autoload: Option<bool>,
    #[serde(
        rename = "chat_template",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaModelEvent {
    pub model: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaProgress {
    pub message: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn format_bytes(bytes: i64) -> String {
    todo!("port extensions/llama/client.ts:formatBytes")
}

pub fn normalize_llama_server_url(value: &str) -> anyhow::Result<String> {
    todo!("port extensions/llama/client.ts:normalizeLlamaServerUrl")
}

pub fn llama_inference_url(server_url: &str) -> String {
    todo!("port extensions/llama/client.ts:llamaInferenceUrl")
}

#[derive(Debug, Clone)]
pub struct LlamaClientListOptions {
    pub reload: Option<bool>,
    pub signal: Option<CancellationToken>,
}

#[derive(Debug, Clone)]
pub struct LlamaClientPropsOptions {
    pub model: Option<String>,
    pub signal: Option<CancellationToken>,
}

pub struct LlamaClient {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl LlamaClient {
    pub fn new(server_url: &str, api_key: Option<&str>) -> Self {
        todo!("port extensions/llama/client.ts:LlamaClient.constructor")
    }

    pub fn server_url(&self) -> String {
        todo!("port extensions/llama/client.ts:LlamaClient.serverUrl")
    }

    pub async fn list(
        &self,
        options: Option<&LlamaClientListOptions>,
    ) -> anyhow::Result<Vec<LlamaModelInfo>> {
        todo!("port extensions/llama/client.ts:LlamaClient.list")
    }

    pub async fn props(
        &self,
        options: Option<&LlamaClientPropsOptions>,
    ) -> anyhow::Result<LlamaServerProps> {
        todo!("port extensions/llama/client.ts:LlamaClient.props")
    }

    pub async fn load(&self, model: &str, signal: Option<CancellationToken>) -> anyhow::Result<()> {
        todo!("port extensions/llama/client.ts:LlamaClient.load")
    }

    pub async fn unload(
        &self,
        model: &str,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<()> {
        todo!("port extensions/llama/client.ts:LlamaClient.unload")
    }

    pub async fn unload_and_wait(
        &self,
        model: &str,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<()> {
        todo!("port extensions/llama/client.ts:LlamaClient.unloadAndWait")
    }

    pub async fn download(
        &self,
        model: &str,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<()> {
        todo!("port extensions/llama/client.ts:LlamaClient.download")
    }

    pub async fn watch(
        &self,
        on_event: Arc<dyn Fn(LlamaModelEvent) + Send + Sync>,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<()> {
        todo!("port extensions/llama/client.ts:LlamaClient.watch")
    }

    pub async fn load_and_wait(
        &self,
        model: &str,
        on_progress: Arc<dyn Fn(LlamaProgress) + Send + Sync>,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<LlamaModelInfo> {
        todo!("port extensions/llama/client.ts:LlamaClient.loadAndWait")
    }

    pub async fn download_and_wait(
        &self,
        model: &str,
        on_progress: Arc<dyn Fn(LlamaProgress) + Send + Sync>,
        signal: Option<CancellationToken>,
    ) -> anyhow::Result<Vec<LlamaModelInfo>> {
        todo!("port extensions/llama/client.ts:LlamaClient.downloadAndWait")
    }
}
