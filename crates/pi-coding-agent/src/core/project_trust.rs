//! Port of `packages/coding-agent/src/core/project-trust.ts`.

use crate::core::extensions::types::LoadExtensionsResult;
use crate::core::extensions::types::ProjectTrustContext;
use crate::core::settings_schema::DefaultProjectTrust;
use crate::core::trust_manager::ProjectTrustStore;
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

pub async fn resolve_project_trusted(
    options: &ResolveProjectTrustedOptions,
) -> anyhow::Result<bool> {
    todo!("port core/project-trust.ts:resolveProjectTrusted")
}
