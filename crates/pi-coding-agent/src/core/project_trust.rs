//! Port of `packages/coding-agent/src/core/project-trust.ts`.

use crate::config::{APP_NAME, CONFIG_DIR_NAME};
use crate::core::extensions::runner::emit_project_trust_event;
use crate::core::extensions::types::{
    ExtensionUIDialogOptions, LoadExtensionsResult, ProjectTrustContext, ProjectTrustEvent,
    ProjectTrustEventDecision,
};
use crate::core::settings_schema::DefaultProjectTrust;
use crate::core::trust_manager::{
    GetProjectTrustOptionsOptions, ProjectTrustOption, ProjectTrustStore,
    get_project_trust_options, has_trust_requiring_project_resources,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppMode {
    #[serde(rename = "interactive")]
    Interactive,
    #[serde(rename = "print")]
    Print,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "rpc")]
    Rpc,
}

impl AppMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Print => "print",
            Self::Json => "json",
            Self::Rpc => "rpc",
        }
    }
}

#[derive(Clone)]
pub struct ResolveProjectTrustedOptions {
    pub cwd: String,
    pub trust_store: Arc<ProjectTrustStore>,
    pub trust_override: Option<bool>,
    pub default_project_trust: Option<DefaultProjectTrust>,
    pub extensions_result: Option<LoadExtensionsResult>,
    pub project_trust_context: ProjectTrustContext,
    pub on_extension_error: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

fn format_project_trust_prompt(cwd: &str) -> String {
    format!(
        "Trust project folder?\n{cwd}\n\nThis allows {APP_NAME} to load {CONFIG_DIR_NAME} settings and resources, install missing project packages, and execute project extensions."
    )
}

async fn select_project_trust_option(
    cwd: &str,
    ctx: &ProjectTrustContext,
) -> Option<ProjectTrustOption> {
    let options = get_project_trust_options(
        cwd,
        Some(&GetProjectTrustOptionsOptions {
            include_session_only: Some(true),
        }),
    );
    let selected = ctx
        .ui
        .select(
            &format_project_trust_prompt(cwd),
            options.iter().map(|o| o.label.clone()).collect(),
            ExtensionUIDialogOptions::default(),
        )
        .await?;
    options.into_iter().find(|o| o.label == selected)
}

fn save_project_trust_prompt_result(
    trust_store: &ProjectTrustStore,
    result: &ProjectTrustOption,
) -> anyhow::Result<()> {
    if !result.updates.is_empty() {
        trust_store.set_many(&result.updates)?;
    }
    Ok(())
}

pub async fn resolve_project_trusted(
    options: &ResolveProjectTrustedOptions,
) -> anyhow::Result<bool> {
    if let Some(trust_override) = options.trust_override {
        return Ok(trust_override);
    }
    if !has_trust_requiring_project_resources(&options.cwd) {
        return Ok(true);
    }

    if let Some(extensions_result) = &options.extensions_result {
        let emitted = emit_project_trust_event(
            extensions_result,
            &ProjectTrustEvent {
                cwd: options.cwd.clone(),
            },
            &options.project_trust_context,
        )
        .await?;
        for error in &emitted.errors {
            if let Some(on_error) = &options.on_extension_error {
                on_error(format!(
                    "Extension \"{}\" project_trust error: {}",
                    error.extension_path, error.error
                ));
            }
        }
        if let Some(result) = emitted.result {
            let trusted = result.trusted == ProjectTrustEventDecision::Yes;
            if result.remember == Some(true) {
                options.trust_store.set(&options.cwd, Some(trusted))?;
            }
            return Ok(trusted);
        }
    }

    if let Some(decision) = options.trust_store.get(&options.cwd)? {
        return Ok(decision);
    }

    match options
        .default_project_trust
        .unwrap_or(DefaultProjectTrust::Ask)
    {
        DefaultProjectTrust::Always => return Ok(true),
        DefaultProjectTrust::Never => return Ok(false),
        DefaultProjectTrust::Ask => {}
    }

    if !options.project_trust_context.has_ui {
        return Ok(false);
    }

    if let Some(selected) =
        select_project_trust_option(&options.cwd, &options.project_trust_context).await
    {
        save_project_trust_prompt_result(&options.trust_store, &selected)?;
        return Ok(selected.trusted);
    }
    Ok(false)
}
