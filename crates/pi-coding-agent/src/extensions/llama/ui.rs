//! Port of `packages/coding-agent/src/extensions/llama/ui.ts`.

use crate::core::extensions::types::ExtensionCommandContext;
use crate::extensions::llama::client::LlamaModelInfo;
use crate::extensions::llama::client::LlamaProgress;
use crate::extensions::llama::huggingface::HuggingFaceModel;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum LlamaManagerAction {
    #[serde(rename = "model")]
    Model { model: LlamaModelInfo },
    #[serde(rename = "download")]
    Download {},
    #[serde(rename = "close")]
    Close {},
}

// TS: extends LlamaProgress (inherited fields inlined)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressState {
    pub message: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "pi_ai::serde_util::opt_js_f64"
    )]
    pub ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub title: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LlamaUiConnectionErrorResult {
    #[serde(rename = "retry")]
    Retry,
    #[serde(rename = "close")]
    Close,
}

impl LlamaUiConnectionErrorResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Close => "close",
        }
    }
}

#[async_trait::async_trait]
pub trait LlamaUi: Send + Sync {
    async fn show_models(
        &self,
        server_url: &str,
        models: &[LlamaModelInfo],
    ) -> anyhow::Result<LlamaManagerAction>;
    async fn select(&self, title: &str, options: &[String]) -> anyhow::Result<Option<String>>;
    async fn confirm(&self, title: &str, message: &str) -> anyhow::Result<bool>;
    async fn connection_error(
        &self,
        server_url: &str,
        message: &str,
    ) -> anyhow::Result<LlamaUiConnectionErrorResult>;
    async fn search_models(
        &self,
        search: Arc<
            dyn Fn(
                    String,
                    CancellationToken,
                ) -> BoxFuture<'static, anyhow::Result<Vec<HuggingFaceModel>>>
                + Send
                + Sync,
        >,
    ) -> anyhow::Result<Option<String>>;
    fn show_status(&self, title: &str, message: &str);
    async fn progress(&self, state: &ProgressState) -> anyhow::Result<()>;
    fn update_progress(&self, state: &ProgressState);
}

pub async fn show_llama_ui(
    ctx: Arc<dyn ExtensionCommandContext>,
    run: Arc<dyn Fn(Arc<dyn LlamaUi>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
) -> anyhow::Result<()> {
    todo!("port extensions/llama/ui.ts:showLlamaUi")
}

#[derive(Clone)]
pub struct RunWithProgressOptions {
    pub title: String,
    pub model: String,
    pub initial_message: String,
    pub cancel_title: String,
    pub cancel_message: String,
    pub run: Arc<
        dyn Fn(
                CancellationToken,
                Arc<dyn Fn(LlamaProgress) + Send + Sync>,
            ) -> BoxFuture<'static, anyhow::Result<Value>>
            + Send
            + Sync,
    >,
    pub cancel: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunWithProgressRunWithProgressResultVariant1 {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunWithProgressRunWithProgressResultVariant2 {
    pub cancelled: bool,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RunWithProgressRunWithProgressResult {
    RunWithProgressRunWithProgressResultVariant1(RunWithProgressRunWithProgressResultVariant1),
    RunWithProgressRunWithProgressResultVariant2(RunWithProgressRunWithProgressResultVariant2),
}

pub async fn run_with_progress(
    ui: Arc<dyn LlamaUi>,
    options: &RunWithProgressOptions,
) -> anyhow::Result<RunWithProgressRunWithProgressResult> {
    todo!("port extensions/llama/ui.ts:runWithProgress")
}
