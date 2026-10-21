//! Port of `packages/coding-agent/src/main.ts`.

use crate::cli::args::Args;
use crate::core::extensions::types::InlineExtension;
use crate::core::session_manager::SessionManager;
use crate::core::settings_manager::SettingsManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Result from resolving a session argument
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum ResolvedSession {
    #[serde(rename = "path")]
    Path { path: String },
    #[serde(rename = "local")]
    Local { path: String },
    #[serde(rename = "global")]
    Global { path: String, cwd: String },
    #[serde(rename = "not_found")]
    NotFound { arg: String },
}

pub async fn create_session_manager(
    parsed: &Args,
    cwd: &str,
    session_dir: Option<&str>,
    settings_manager: Arc<SettingsManager>,
) -> anyhow::Result<Arc<SessionManager>> {
    todo!("port main.ts:createSessionManager")
}

#[derive(Clone)]
pub struct MainOptions {
    pub extension_factories: Option<Vec<InlineExtension>>,
}

pub async fn main(args: &[String], options: Option<&MainOptions>) -> anyhow::Result<()> {
    todo!("port main.ts:main")
}
