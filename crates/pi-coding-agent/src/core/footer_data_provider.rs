//! Port of `packages/coding-agent/src/core/footer-data-provider.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPaths {
    pub repo_dir: String,
    pub common_git_dir: String,
    pub head_path: String,
}

/// Find git metadata paths by walking up from cwd.
/// Handles both regular git repos (.git is a directory) and worktrees (.git is a file).
pub fn find_git_paths(cwd: &str) -> Option<GitPaths> {
    todo!("port core/footer-data-provider.ts:findGitPaths")
}

/// Provides git branch and extension statuses - data not otherwise accessible to extensions.
/// Context usage on ctx.getContextUsage(), token stats on ctx.sessionManager.getEntries(), model info on ctx.model.
pub struct FooterDataProvider {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl FooterDataProvider {
    pub fn new(cwd: &str) -> Self {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.constructor")
    }

    /// Current git branch, null if not in repo, "detached" if detached HEAD
    pub fn get_git_branch(&self) -> Option<String> {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.getGitBranch")
    }

    /// Extension status texts set via ctx.ui.setStatus()
    pub fn get_extension_statuses(&self) -> IndexMap<String, String> {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.getExtensionStatuses")
    }

    /// Subscribe to git branch changes. Returns unsubscribe function.
    pub fn on_branch_change(
        &self,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Arc<dyn Fn() + Send + Sync> {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.onBranchChange")
    }

    /// Internal: set extension status
    pub fn set_extension_status(&self, key: &str, text: Option<&str>) {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.setExtensionStatus")
    }

    /// Internal: clear extension statuses
    pub fn clear_extension_statuses(&self) {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.clearExtensionStatuses")
    }

    /// Number of unique providers with available models (for footer display)
    pub fn get_available_provider_count(&self) -> i64 {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.getAvailableProviderCount")
    }

    /// Internal: update available provider count
    pub fn set_available_provider_count(&self, count: i64) {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.setAvailableProviderCount")
    }

    pub fn set_cwd(&self, cwd: &str) {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.setCwd")
    }

    /// Internal: cleanup
    pub fn dispose(&self) {
        todo!("port core/footer-data-provider.ts:FooterDataProvider.dispose")
    }
}

/// Read-only view for extensions - excludes setExtensionStatus, setAvailableProviderCount and dispose
/// TS: Pick< FooterDataProvider, "getGitBranch" | "getExtensionStatuses" | "getAvailableProviderCount" | "onBranchChange" >
pub type ReadonlyFooterDataProvider = Arc<FooterDataProvider>;
