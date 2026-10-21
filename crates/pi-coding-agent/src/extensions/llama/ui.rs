//! Port of `packages/coding-agent/src/extensions/llama/ui.ts`.

use crate::core::extensions::types::ExtensionCommandContext;
use crate::core::keybindings::KeybindingsManager;
use crate::extensions::llama::client::LlamaModelInfo;
use crate::extensions::llama::client::LlamaProgress;
use crate::extensions::llama::huggingface::HuggingFaceModel;
use crate::modes::interactive::theme::theme::Theme;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
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

/// TS: `extends Container implements Focusable`
pub struct HuggingFaceSearch {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl HuggingFaceSearch {
    pub fn new(
        tui: Arc<Tui>,
        theme: Arc<Theme>,
        keybindings: Arc<KeybindingsManager>,
        search: Arc<
            dyn Fn(
                    String,
                    CancellationToken,
                ) -> BoxFuture<'static, anyhow::Result<Vec<HuggingFaceModel>>>
                + Send
                + Sync,
        >,
        cache: IndexMap<String, Vec<HuggingFaceModel>>,
        on_select_model: Arc<dyn Fn(Option<String>) + Send + Sync>,
    ) -> Self {
        todo!("port extensions/llama/ui.ts:HuggingFaceSearch.constructor")
    }
}

// TODO(port): impl pi-tui::Container for HuggingFaceSearch

impl Component for HuggingFaceSearch {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port extensions/llama/ui.ts:HuggingFaceSearch.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port extensions/llama/ui.ts:HuggingFaceSearch.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }
}

/// TS: `implements LlamaUi, Focusable`
pub struct LlamaView {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl LlamaView {
    pub fn new(tui: Arc<Tui>, theme: Arc<Theme>, keybindings: Arc<KeybindingsManager>) -> Self {
        todo!("port extensions/llama/ui.ts:LlamaView.constructor")
    }
}

#[async_trait::async_trait]
impl LlamaUi for LlamaView {
    async fn show_models(
        &self,
        server_url: &str,
        models: &[LlamaModelInfo],
    ) -> anyhow::Result<LlamaManagerAction> {
        todo!("port extensions/llama/ui.ts:LlamaView.show_models")
    }
    async fn select(&self, title: &str, options: &[String]) -> anyhow::Result<Option<String>> {
        todo!("port extensions/llama/ui.ts:LlamaView.select")
    }
    async fn confirm(&self, title: &str, message: &str) -> anyhow::Result<bool> {
        todo!("port extensions/llama/ui.ts:LlamaView.confirm")
    }
    async fn connection_error(
        &self,
        server_url: &str,
        message: &str,
    ) -> anyhow::Result<LlamaUiConnectionErrorResult> {
        todo!("port extensions/llama/ui.ts:LlamaView.connection_error")
    }
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
    ) -> anyhow::Result<Option<String>> {
        todo!("port extensions/llama/ui.ts:LlamaView.search_models")
    }
    fn show_status(&self, title: &str, message: &str) {
        todo!("port extensions/llama/ui.ts:LlamaView.show_status")
    }
    async fn progress(&self, state: &ProgressState) -> anyhow::Result<()> {
        todo!("port extensions/llama/ui.ts:LlamaView.progress")
    }
    fn update_progress(&self, state: &ProgressState) {
        todo!("port extensions/llama/ui.ts:LlamaView.update_progress")
    }
}

impl Component for LlamaView {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port extensions/llama/ui.ts:LlamaView.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port extensions/llama/ui.ts:LlamaView.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!("port extensions/llama/ui.ts:LlamaView.invalidate")
    }
}

pub async fn show_llama_ui(
    ctx: &ExtensionCommandContext,
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
pub struct RunWithProgressResultVariant1 {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunWithProgressResultVariant2 {
    pub cancelled: bool,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RunWithProgressResult {
    RunWithProgressResultVariant1(RunWithProgressResultVariant1),
    RunWithProgressResultVariant2(RunWithProgressResultVariant2),
}

pub async fn run_with_progress(
    ui: Arc<dyn LlamaUi>,
    options: &RunWithProgressOptions,
) -> anyhow::Result<RunWithProgressResult> {
    todo!("port extensions/llama/ui.ts:runWithProgress")
}
