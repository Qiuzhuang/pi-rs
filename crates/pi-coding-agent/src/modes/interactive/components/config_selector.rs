//! Port of `packages/coding-agent/src/modes/interactive/components/config-selector.ts`.

use crate::core::package_manager::PathMetadata;
use crate::core::package_manager::ResolvedPaths;
use crate::core::package_manager::SourceScope;
use crate::core::settings_manager::SettingsManager;
use crate::core::source_info::SourceOrigin;
use indexmap::IndexMap;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    #[serde(rename = "extensions")]
    Extensions,
    #[serde(rename = "skills")]
    Skills,
    #[serde(rename = "prompts")]
    Prompts,
    #[serde(rename = "themes")]
    Themes,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Extensions => "extensions",
            Self::Skills => "skills",
            Self::Prompts => "prompts",
            Self::Themes => "themes",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfigWriteScope {
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "project")]
    Project,
}

impl ConfigWriteScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsScope {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "project")]
    Project,
}

impl SettingsScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectOverrideState {
    #[serde(rename = "inherit")]
    Inherit,
    #[serde(rename = "load")]
    Load,
    #[serde(rename = "unload")]
    Unload,
}

impl ProjectOverrideState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Load => "load",
            Self::Unload => "unload",
        }
    }
}

pub type ScopedResolvedPaths = IndexMap<ConfigWriteScope, ResolvedPaths>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceItem {
    pub path: String,
    pub enabled: bool,
    pub metadata: PathMetadata,
    pub resource_type: ResourceType,
    pub display_name: String,
    pub group_key: String,
    pub subgroup_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSubgroup {
    pub r#type: ResourceType,
    pub label: String,
    pub items: Vec<ResourceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGroup {
    pub key: String,
    pub label: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    pub source: String,
    pub subgroups: Vec<ResourceSubgroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum FlatEntry {
    #[serde(rename = "group")]
    Group { group: ResourceGroup },
    #[serde(rename = "subgroup")]
    Subgroup {
        subgroup: ResourceSubgroup,
        group: ResourceGroup,
    },
    #[serde(rename = "item")]
    Item { item: ResourceItem },
}

/// TS: `extends Container implements Focusable`
pub struct ConfigSelectorComponent {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ConfigSelectorComponent {
    pub fn new(
        resolved_paths: &ScopedResolvedPaths,
        settings_manager: Arc<SettingsManager>,
        cwd: &str,
        agent_dir: &str,
        on_close: Arc<dyn Fn() + Send + Sync>,
        on_exit: Arc<dyn Fn() + Send + Sync>,
        request_render: Arc<dyn Fn() + Send + Sync>,
        terminal_height: Option<i64>,
        write_scope: Option<&ConfigWriteScope>,
        project_mode_available: Option<bool>,
    ) -> Self {
        todo!(
            "port modes/interactive/components/config-selector.ts:ConfigSelectorComponent.constructor"
        )
    }

    pub fn get_resource_list(&self) -> Value /* TS: ResourceList */ {
        todo!(
            "port modes/interactive/components/config-selector.ts:ConfigSelectorComponent.getResourceList"
        )
    }
}

// TODO(port): impl pi-tui::Container for ConfigSelectorComponent

impl Component for ConfigSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/config-selector.ts:ConfigSelectorComponent.render")
    }
}
