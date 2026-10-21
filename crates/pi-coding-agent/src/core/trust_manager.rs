//! Port of `packages/coding-agent/src/core/trust-manager.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub type ProjectTrustDecision = Option<bool>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustStoreEntry {
    pub path: String,
    pub decision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustUpdate {
    pub path: String,
    pub decision: ProjectTrustDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustOption {
    pub label: String,
    pub trusted: bool,
    pub updates: Vec<ProjectTrustUpdate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
}

pub type TrustFile = IndexMap<String, Option<bool>>;

pub fn get_project_trust_parent_path(cwd: &str) -> Option<String> {
    todo!("port core/trust-manager.ts:getProjectTrustParentPath")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProjectTrustOptionsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_session_only: Option<bool>,
}

pub fn get_project_trust_options(
    cwd: &str,
    options: Option<&GetProjectTrustOptionsOptions>,
) -> Vec<ProjectTrustOption> {
    todo!("port core/trust-manager.ts:getProjectTrustOptions")
}

/// Returns true when cwd has project-local resources that must be gated by
/// project trust: trust-requiring entries under cwd/.pi, or .agents/skills in
/// cwd or one of its ancestors. Returns false when no such project resources
/// exist. The user/global ~/.agents/skills directory is always treated as a
/// trusted user resource and is ignored here, even when cwd is $HOME.
pub fn has_trust_requiring_project_resources(cwd: &str) -> bool {
    todo!("port core/trust-manager.ts:hasTrustRequiringProjectResources")
}

pub struct ProjectTrustStore {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ProjectTrustStore {
    pub fn new(agent_dir: &str) -> Self {
        todo!("port core/trust-manager.ts:ProjectTrustStore.constructor")
    }

    pub fn get(&self, cwd: &str) -> ProjectTrustDecision {
        todo!("port core/trust-manager.ts:ProjectTrustStore.get")
    }

    pub fn get_entry(&self, cwd: &str) -> Option<ProjectTrustStoreEntry> {
        todo!("port core/trust-manager.ts:ProjectTrustStore.getEntry")
    }

    pub fn set(&self, cwd: &str, decision: &ProjectTrustDecision) {
        todo!("port core/trust-manager.ts:ProjectTrustStore.set")
    }

    pub fn set_many(&self, decisions: &[ProjectTrustUpdate]) {
        todo!("port core/trust-manager.ts:ProjectTrustStore.setMany")
    }
}
