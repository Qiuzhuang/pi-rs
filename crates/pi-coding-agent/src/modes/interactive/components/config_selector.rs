//! Port of `packages/coding-agent/src/modes/interactive/components/config-selector.ts`.

use crate::core::package_manager::PathMetadata;
use crate::core::package_manager::ResolvedPaths;
use crate::core::package_manager::SourceScope;
use crate::core::settings_manager::SettingsManager;
use crate::core::source_info::SourceOrigin;
use indexmap::IndexMap;
use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
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

/// TS: `implements Component`
pub struct ConfigSelectorHeader {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ConfigSelectorHeader {
    pub fn new(write_scope: &ConfigWriteScope, project_mode_available: bool) -> Self {
        todo!(
            "port modes/interactive/components/config-selector.ts:ConfigSelectorHeader.constructor"
        )
    }

    pub fn set_write_scope(&self, write_scope: &ConfigWriteScope) {
        todo!(
            "port modes/interactive/components/config-selector.ts:ConfigSelectorHeader.setWriteScope"
        )
    }
}

impl Component for ConfigSelectorHeader {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/config-selector.ts:ConfigSelectorHeader.render")
    }

    fn invalidate(&self) {
        todo!(
            "port modes/interactive/components/config-selector.ts:ConfigSelectorHeader.invalidate"
        )
    }
}

/// TS: `implements Component, Focusable`
pub struct ResourceList {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl ResourceList {
    pub fn new(
        groups_by_scope: IndexMap<ConfigWriteScope, Vec<ResourceGroup>>,
        settings_manager: Arc<SettingsManager>,
        cwd: &str,
        agent_dir: &str,
        terminal_height: Option<i64>,
        write_scope: Option<&ConfigWriteScope>,
    ) -> Self {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.constructor")
    }

    pub fn on_cancel(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onCancel")
    }

    pub fn set_on_cancel(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onCancel")
    }

    pub fn on_exit(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onExit")
    }

    pub fn set_on_exit(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onExit")
    }

    pub fn on_toggle(&self) -> Option<Arc<dyn Fn(ResourceItem, bool) + Send + Sync>> {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onToggle")
    }

    pub fn set_on_toggle(&self, value: Option<Arc<dyn Fn(ResourceItem, bool) + Send + Sync>>) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onToggle")
    }

    pub fn on_switch_mode(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onSwitchMode")
    }

    pub fn set_on_switch_mode(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.onSwitchMode")
    }

    pub fn set_write_scope(&self, write_scope: &ConfigWriteScope) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.setWriteScope")
    }

    pub fn update_item(&self, item: &ResourceItem, enabled: bool) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.updateItem")
    }
}

impl Component for ResourceList {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.render")
    }

    fn handle_input(&self, data: &str) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.handleInput")
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/config-selector.ts:ResourceList.invalidate")
    }
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

    pub fn get_resource_list(&self) -> Arc<ResourceList> {
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
