//! Port of `packages/coding-agent/src/extensions/llama/index.ts`.
//!
//! The llama.cpp provider and the `/llama` command that manages the models of a llama.cpp router.

pub mod client;
pub mod huggingface;
pub mod provider;
pub mod ui;

#[cfg(test)]
mod tests;

use crate::core::extensions::types::{
    CommandOptions, ExtensionAPI, ExtensionCommandContext, ExtensionMode, NotifyType, ProviderRegistration,
};
use client::{LlamaClient, LlamaClientListOptions, LlamaModelInfo, LlamaModelStatus, format_bytes, normalize_llama_server_url};
use huggingface::{HuggingFaceClient, find_hugging_face_token};
use pi_ai::models::ModelsRefreshOptions;
use provider::{LLAMA_PROVIDER_ID, LlamaProviderController, create_llama_provider};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use ui::{LlamaManagerAction, LlamaUi, LlamaUiConnectionErrorResult, RunWithProgressOptions, RunWithProgressResult};

fn model_is_loaded(model: &LlamaModelInfo) -> bool {
    matches!(model.status.value, LlamaModelStatus::Loaded | LlamaModelStatus::Sleeping)
}

fn is_connection_error(error: &anyhow::Error) -> bool {
    let message = format!("Error {error}").to_lowercase();
    message.contains("fetch failed") || message.contains("timeout") || message.contains("network")
}

fn connection_error_message(error: &anyhow::Error) -> String {
    if is_connection_error(error) { "Could not connect to the server.".to_string() } else { error.to_string() }
}

struct ParsedHuggingFaceModel {
    repository: String,
    quantization: Option<String>,
}

fn parse_hugging_face_model(value: &str) -> ParsedHuggingFaceModel {
    let slash = value.find('/').map(|i| i + 1).unwrap_or(0);
    match value[slash..].find(':').map(|i| i + slash) {
        None => ParsedHuggingFaceModel { repository: value.to_string(), quantization: None },
        Some(colon) => ParsedHuggingFaceModel {
            repository: value[..colon].to_string(),
            quantization: Some(value[colon + 1..].to_string()),
        },
    }
}

async fn configured_client(ctx: &ExtensionCommandContext) -> anyhow::Result<Option<LlamaClient>> {
    let Some(result) = ctx.model_registry()?.get_provider_auth(LLAMA_PROVIDER_ID).await? else {
        ctx.ui()?.notify(&format!("Configure llama.cpp with /login {LLAMA_PROVIDER_ID}"), NotifyType::Warning);
        return Ok(None);
    };
    let configured_url = result.env.as_ref().and_then(|env| env.get("LLAMA_BASE_URL")).filter(|u| !u.is_empty());
    let server_url = normalize_llama_server_url(match configured_url {
        Some(url) => url,
        None => result.auth.base_url.as_deref().unwrap_or(""),
    })?;
    Ok(Some(LlamaClient::try_new(&server_url, result.auth.api_key.as_deref())?))
}

/// A signal that fires after `duration` (`AbortSignal.timeout()`).
fn timeout_signal(duration: Duration) -> CancellationToken {
    let token = CancellationToken::new();
    let timer = token.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(duration) => timer.cancel(),
            _ = timer.cancelled() => {}
        }
    });
    token
}

/// The state the `/llama` command works with.
#[derive(Clone)]
struct Llama {
    provider: Arc<dyn LlamaProviderController>,
    ctx: ExtensionCommandContext,
    ui: Arc<dyn LlamaUi>,
    client: LlamaClient,
}

impl Llama {
    fn notify(&self, message: &str, kind: NotifyType) {
        if let Ok(ui) = self.ctx.ui() {
            ui.notify(message, kind);
        }
    }

    async fn sync_catalog(&self, catalog: Option<Vec<LlamaModelInfo>>) -> anyhow::Result<Vec<LlamaModelInfo>> {
        let signal = timeout_signal(Duration::from_secs(15));
        let result = async {
            let current = match catalog {
                Some(catalog) => catalog,
                None => {
                    self.client
                        .list(Some(&LlamaClientListOptions { reload: None, signal: Some(signal.clone()) }))
                        .await?
                }
            };
            self.provider.set_catalog(&current, &self.client.server_url(), None);
            let result = self
                .ctx
                .model_registry()?
                .refresh(Some(ModelsRefreshOptions {
                    providers: Some(vec![LLAMA_PROVIDER_ID.to_string()]),
                    // /llama already contacted the configured llama.cpp server, so keep this refresh
                    // live even in PI_OFFLINE.
                    allow_network: Some(true),
                    force: None,
                    signal: Some(signal.clone()),
                }))
                .await?;
            if result.aborted {
                anyhow::bail!("Model catalog refresh timed out.");
            }
            if let Some(error) = result.errors.get(LLAMA_PROVIDER_ID) {
                anyhow::bail!(error.clone());
            }
            Ok(current)
        }
        .await;
        signal.cancel();
        result
    }

    async fn restore_loaded(&self, loaded: &[LlamaModelInfo]) -> anyhow::Result<()> {
        self.notify("Restoring previously loaded models", NotifyType::Info);
        for model in loaded {
            self.client.load_and_wait(&model.id, Arc::new(|_| {}), None).await?;
        }
        self.sync_catalog(None).await?;
        Ok(())
    }

    async fn load_model(&self, catalog: &[LlamaModelInfo], target: &LlamaModelInfo) -> anyhow::Result<()> {
        let loaded: Vec<LlamaModelInfo> =
            catalog.iter().filter(|m| m.id != target.id && model_is_loaded(m)).cloned().collect();
        let mut replace = false;
        if !loaded.is_empty() {
            let title = format!(
                "{} model{} loaded",
                loaded.len(),
                if loaded.len() == 1 { " is" } else { "s are" }
            );
            let choice = self
                .ui
                .select(
                    &title,
                    &["Unload all and load".to_string(), "Keep loaded and load".to_string(), "Cancel".to_string()],
                )
                .await?;
            match choice.as_deref() {
                None | Some("Cancel") => return Ok(()),
                Some(choice) => replace = choice == "Unload all and load",
            }
        }
        if replace {
            for model in &loaded {
                self.client.unload_and_wait(&model.id, None).await?;
            }
        }

        let outcome = async {
            let (run_client, cancel_client) = (self.client.clone(), self.client.clone());
            let (run_id, cancel_id) = (target.id.clone(), target.id.clone());
            let result = ui::run_with_progress(
                self.ui.clone(),
                &RunWithProgressOptions {
                    title: "Loading model".to_string(),
                    model: target.id.clone(),
                    initial_message: "Starting…".to_string(),
                    cancel_title: "Stop loading?".to_string(),
                    cancel_message: target.id.clone(),
                    run: Arc::new(move |signal, update| {
                        let (client, id) = (run_client.clone(), run_id.clone());
                        Box::pin(async move { client.load_and_wait(&id, update, Some(signal)).await })
                    }),
                    cancel: Arc::new(move || {
                        let (client, id) = (cancel_client.clone(), cancel_id.clone());
                        Box::pin(async move { client.unload(&id, None).await })
                    }),
                },
            )
            .await?;
            if matches!(result, RunWithProgressResult::Cancelled) {
                if replace {
                    self.restore_loaded(&loaded).await?;
                }
                return Ok(());
            }
            let refreshed = self.sync_catalog(None).await?;
            let loaded_model = refreshed.iter().find(|m| m.id == target.id);
            self.notify(
                &if loaded_model.is_some_and(|m| m.status.value == LlamaModelStatus::Loaded) {
                    format!("Loaded {}", target.id)
                } else {
                    format!("Load started for {}", target.id)
                },
                NotifyType::Info,
            );
            anyhow::Ok(())
        }
        .await;
        if let Err(error) = outcome {
            if replace {
                // Preserve the original load error.
                let _ = self.restore_loaded(&loaded).await;
            }
            return Err(error);
        }
        Ok(())
    }

    async fn unload_model(&self, model: &LlamaModelInfo) -> anyhow::Result<()> {
        if !self.ui.confirm("Unload model?", &model.id).await? {
            return Ok(());
        }
        self.client.unload_and_wait(&model.id, None).await?;
        self.sync_catalog(None).await?;
        self.notify(&format!("Unloaded {}", model.id), NotifyType::Info);
        Ok(())
    }

    async fn download_model(&self) -> anyhow::Result<()> {
        let token = find_hugging_face_token(None).await?;
        let hugging_face = Arc::new(HuggingFaceClient::new(token.as_deref(), None));
        let search_client = hugging_face.clone();
        let Some(selected) = self
            .ui
            .search_models(Arc::new(move |query, signal| {
                let client = search_client.clone();
                Box::pin(async move { client.search(&query, Some(signal)).await })
            }))
            .await?
        else {
            return Ok(());
        };
        let parsed = parse_hugging_face_model(&selected);
        self.ui.show_status("Loading model details", &parsed.repository);
        let details = hugging_face.details(&parsed.repository, None).await?;
        if details.gated.is_gated() {
            let approval = if details.gated == huggingface::HuggingFaceModelDetailsGated::String("manual".into()) {
                "Manual approval is required"
            } else {
                "Accept the access terms"
            };
            let choice = self
                .ui
                .select(
                    &format!(
                        "Hugging Face access required\n{}\n\n{approval} at:\nhttps://huggingface.co/{}\n\nThe llama.cpp server needs HF_TOKEN with access.",
                        details.id, details.id
                    ),
                    &["Continue".to_string(), "Back".to_string()],
                )
                .await?;
            if choice.as_deref() != Some("Continue") {
                return Ok(());
            }
        }
        let mut quantization = parsed.quantization.filter(|q| !q.is_empty());
        if quantization.is_none() && !details.quantizations.is_empty() {
            let options: Vec<String> = details
                .quantizations
                .iter()
                .map(|entry| {
                    let detail = [
                        entry.size.map(format_bytes),
                        (entry.name == "Q4_K_M").then(|| "recommended".to_string()),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                    if detail.is_empty() { entry.name.clone() } else { format!("{} · {detail}", entry.name) }
                })
                .collect();
            let Some(choice) = self.ui.select(&format!("Select quantization\n{}", details.id), &options).await? else {
                return Ok(());
            };
            let Some(index) = options.iter().position(|o| *o == choice) else { return Ok(()) };
            quantization = details.quantizations.get(index).map(|q| q.name.clone());
            if quantization.is_none() {
                return Ok(());
            }
        }
        let model = match &quantization {
            Some(q) => format!("{}:{q}", details.id),
            None => details.id.clone(),
        };
        let (run_client, cancel_client) = (self.client.clone(), self.client.clone());
        let (run_model, cancel_model) = (model.clone(), model.clone());
        let result = ui::run_with_progress(
            self.ui.clone(),
            &RunWithProgressOptions {
                title: "Downloading model".to_string(),
                model: model.clone(),
                initial_message: "Starting…".to_string(),
                cancel_title: "Stop download?".to_string(),
                cancel_message: model.clone(),
                run: Arc::new(move |signal, update| {
                    let (client, model) = (run_client.clone(), run_model.clone());
                    Box::pin(async move { client.download_and_wait(&model, update, Some(signal)).await })
                }),
                cancel: Arc::new(move || {
                    let (client, model) = (cancel_client.clone(), cancel_model.clone());
                    Box::pin(async move { client.unload(&model, None).await })
                }),
            },
        )
        .await?;
        let RunWithProgressResult::Completed(catalog) = result else { return Ok(()) };
        self.sync_catalog(Some(catalog)).await?;
        self.notify(&format!("Downloaded {model}"), NotifyType::Info);
        Ok(())
    }

    async fn read_catalog(&self) -> anyhow::Result<Option<Vec<LlamaModelInfo>>> {
        loop {
            match self.sync_catalog(None).await {
                Ok(catalog) => return Ok(Some(catalog)),
                Err(error) => {
                    let choice =
                        self.ui.connection_error(&self.client.server_url(), &connection_error_message(&error)).await?;
                    if choice == LlamaUiConnectionErrorResult::Close {
                        return Ok(None);
                    }
                }
            }
        }
    }

    async fn run(&self) -> anyhow::Result<()> {
        let Some(mut catalog) = self.read_catalog().await? else { return Ok(()) };
        loop {
            let action = self.ui.show_models(&self.client.server_url(), &catalog).await?;
            let outcome = match &action {
                LlamaManagerAction::Close {} => return Ok(()),
                LlamaManagerAction::Download {} => self.download_model().await,
                LlamaManagerAction::Model { model } if model_is_loaded(model) => self.unload_model(model).await,
                LlamaManagerAction::Model { model } if model.status.value == LlamaModelStatus::Unloaded => {
                    self.load_model(&catalog, model).await
                }
                LlamaManagerAction::Model { model } => {
                    self.notify(&format!("{} is {}", model.id, model.status.value.as_str()), NotifyType::Warning);
                    Ok(())
                }
            };
            let Some(refreshed) = self.read_catalog().await? else { return Ok(()) };
            catalog = refreshed;
            if let Err(error) = outcome
                && !is_connection_error(&error)
            {
                self.notify(&error.to_string(), NotifyType::Error);
            }
        }
    }
}

pub fn llama_extension(pi: Arc<dyn ExtensionAPI>) -> anyhow::Result<()> {
    let provider = create_llama_provider();
    pi.register_provider(ProviderRegistration::Native(provider.provider()))?;

    pi.register_command(
        "llama",
        CommandOptions {
            description: Some("Manage llama.cpp router models".to_string()),
            get_argument_completions: None,
            handler: Arc::new(move |_args: String, ctx: ExtensionCommandContext| {
                let provider = provider.clone();
                Box::pin(async move {
                    if ctx.mode()? != ExtensionMode::Tui {
                        ctx.ui()?.notify("/llama is available in interactive mode", NotifyType::Warning);
                        return Ok(());
                    }
                    let Some(client) = configured_client(&ctx).await? else { return Ok(()) };
                    let run_ctx = ctx.clone();
                    ui::show_llama_ui(
                        &ctx,
                        Arc::new(move |ui: Arc<dyn LlamaUi>| {
                            let llama = Llama {
                                provider: provider.clone(),
                                ctx: run_ctx.clone(),
                                ui,
                                client: client.clone(),
                            };
                            Box::pin(async move { llama.run().await })
                        }),
                    )
                    .await
                })
            }),
        },
    )?;
    Ok(())
}
