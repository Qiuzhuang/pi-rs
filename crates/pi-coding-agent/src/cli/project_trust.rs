//! Port of `packages/coding-agent/src/cli/project-trust.ts`.

use crate::core::extensions::types::ProjectTrustContext;
use crate::core::project_trust::AppMode;
use crate::core::settings_manager::SettingsManager;
use std::sync::Arc;

#[derive(Clone)]
pub struct CreateProjectTrustContextOptions {
    pub cwd: String,
    pub mode: AppMode,
    pub settings_manager: Arc<SettingsManager>,
    pub has_ui: bool,
}

pub fn create_project_trust_context(
    options: &CreateProjectTrustContextOptions,
) -> ProjectTrustContext {
    todo!("port cli/project-trust.ts:createProjectTrustContext")
}
