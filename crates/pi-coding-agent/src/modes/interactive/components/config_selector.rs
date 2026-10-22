//! Port of `packages/coding-agent/src/modes/interactive/components/config-selector.ts`.

use crate::core::package_manager::PathMetadata;
use crate::core::package_manager::ResolvedPaths;
use crate::core::package_manager::SourceScope;
use crate::config::CONFIG_DIR_NAME;
use crate::core::package_manager::ResolvedResource;
use crate::core::settings_manager::SettingsManager;
use crate::core::settings_schema::{PackageSource, PackageSourceVariant2, Settings};
use crate::core::source_info::BUILTIN_PATH_PREFIX;
use crate::core::source_info::SourceOrigin;
use crate::core::tools::js_util::locale_compare;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::{key_hint, raw_key_hint};
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use crate::utils::paths::{
    PathInputOptions, canonicalize_path, is_local_path, node_basename, node_dirname, node_join, node_relative,
    resolve_path,
};
use indexmap::IndexMap;
use parking_lot::Mutex;
use pi_tui::components::input::Input;
use pi_tui::components::spacer::Spacer;
use pi_tui::keybindings::get_keybindings;
use pi_tui::keys::matches_key;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult};
use pi_tui::utils::{js_trim, truncate_to_width, visible_width};
use std::collections::{HashMap, HashSet};
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

const RESOURCE_TYPES: [ResourceType; 4] =
    [ResourceType::Extensions, ResourceType::Skills, ResourceType::Prompts, ResourceType::Themes];

fn resource_type_label(t: ResourceType) -> &'static str {
    match t {
        ResourceType::Extensions => "Extensions",
        ResourceType::Skills => "Skills",
        ResourceType::Prompts => "Prompts",
        ResourceType::Themes => "Themes",
    }
}

fn type_order(t: ResourceType) -> u8 {
    match t {
        ResourceType::Extensions => 0,
        ResourceType::Skills => 1,
        ResourceType::Prompts => 2,
        ResourceType::Themes => 3,
    }
}

fn format_base_dir(base_dir: &str) -> String {
    let home_dir = dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();
    let display_path = if base_dir == home_dir {
        "~".to_string()
    } else if !home_dir.is_empty() && base_dir.starts_with(&home_dir) {
        // Replace home prefix with ~, normalize separators for display
        format!("~{}", base_dir[home_dir.len()..].replace('\\', "/"))
    } else {
        base_dir.replace('\\', "/")
    };
    if display_path.ends_with('/') { display_path } else { format!("{display_path}/") }
}

fn get_group_label(metadata: &PathMetadata, agent_dir: &str) -> String {
    if metadata.origin == SourceOrigin::Package {
        return format!("{} ({})", metadata.source, metadata.scope.as_str());
    }
    if metadata.source == "builtin" {
        return if metadata.scope == SourceScope::User {
            "Built-in".to_string()
        } else {
            "Built-in (project override)".to_string()
        };
    }
    // Top-level resources
    if metadata.source == "auto" {
        if let Some(base_dir) = metadata.base_dir.as_deref().filter(|b| !b.is_empty()) {
            return if metadata.scope == SourceScope::User {
                format!("User ({})", format_base_dir(base_dir))
            } else {
                format!("Project ({})", format_base_dir(base_dir))
            };
        }
        return if metadata.scope == SourceScope::User {
            format!("User ({})", format_base_dir(agent_dir))
        } else {
            format!("Project ({CONFIG_DIR_NAME}/)")
        };
    }
    if metadata.scope == SourceScope::User { "User settings".to_string() } else { "Project settings".to_string() }
}

fn build_groups(resolved: &ResolvedPaths, agent_dir: &str) -> Vec<ResourceGroup> {
    let mut group_map: IndexMap<String, ResourceGroup> = IndexMap::new();

    let mut add_to_group = |resources: &[ResolvedResource], resource_type: ResourceType| {
        for res in resources {
            let metadata = &res.metadata;
            let group_key = format!(
                "{}:{}:{}:{}",
                metadata.origin.as_str(),
                metadata.scope.as_str(),
                metadata.source,
                metadata.base_dir.as_deref().unwrap_or("")
            );
            let group = group_map.entry(group_key.clone()).or_insert_with(|| ResourceGroup {
                key: group_key.clone(),
                label: get_group_label(metadata, agent_dir),
                scope: metadata.scope,
                origin: metadata.origin,
                source: metadata.source.clone(),
                subgroups: Vec::new(),
            });
            let subgroup_key = format!("{group_key}:{}", resource_type.as_str());
            let subgroup_index = match group.subgroups.iter().position(|sg| sg.r#type == resource_type) {
                Some(i) => i,
                None => {
                    group.subgroups.push(ResourceSubgroup {
                        r#type: resource_type,
                        label: resource_type_label(resource_type).to_string(),
                        items: Vec::new(),
                    });
                    group.subgroups.len() - 1
                }
            };

            let path = &res.path;
            let file_name = node_basename(path);
            let parent_folder = node_basename(&node_dirname(path));
            let display_name = if metadata.source == "builtin" {
                path.get(BUILTIN_PATH_PREFIX.len()..).unwrap_or("").to_string()
            } else if resource_type == ResourceType::Extensions && parent_folder != "extensions" {
                format!("{parent_folder}/{file_name}")
            } else if resource_type == ResourceType::Skills && file_name == "SKILL.md" {
                parent_folder
            } else {
                file_name
            };
            group.subgroups[subgroup_index].items.push(ResourceItem {
                path: path.clone(),
                enabled: res.enabled,
                metadata: metadata.clone(),
                resource_type,
                display_name,
                group_key: group_key.clone(),
                subgroup_key,
            });
        }
    };

    add_to_group(&resolved.extensions, ResourceType::Extensions);
    add_to_group(&resolved.skills, ResourceType::Skills);
    add_to_group(&resolved.prompts, ResourceType::Prompts);
    add_to_group(&resolved.themes, ResourceType::Themes);

    // Sort groups: packages first, then top-level; user before project
    let mut groups: Vec<ResourceGroup> = group_map.into_values().collect();
    groups.sort_by(|a, b| {
        use std::cmp::Ordering;
        if a.origin != b.origin {
            return if a.origin == SourceOrigin::Package { Ordering::Less } else { Ordering::Greater };
        }
        if a.scope != b.scope {
            return if a.scope == SourceScope::User { Ordering::Less } else { Ordering::Greater };
        }
        locale_compare(&a.source, &b.source)
    });

    // Sort subgroups within each group by type order, and items by name
    for group in &mut groups {
        group.subgroups.sort_by_key(|sg| type_order(sg.r#type));
        for subgroup in &mut group.subgroups {
            subgroup.items.sort_by(|a, b| locale_compare(&a.display_name, &b.display_name));
        }
    }

    groups
}

/// TS: `implements Component`
pub struct ConfigSelectorHeader {
    write_scope: Mutex<ConfigWriteScope>,
    project_mode_available: bool,
}

impl ConfigSelectorHeader {
    pub fn new(write_scope: &ConfigWriteScope, project_mode_available: bool) -> Self {
        Self { write_scope: Mutex::new(*write_scope), project_mode_available }
    }

    pub fn set_write_scope(&self, write_scope: &ConfigWriteScope) {
        *self.write_scope.lock() = *write_scope;
    }
}

impl Component for ConfigSelectorHeader {
    fn render(&self, width: usize) -> Vec<String> {
        let t = theme();
        let write_scope = *self.write_scope.lock();
        let title = t.bold(if write_scope == ConfigWriteScope::Project {
            "Project Local Resources"
        } else {
            "Global Resources"
        });
        let sep = t.fg(&ThemeColor::Muted, " · ");
        let switch_hint = if self.project_mode_available {
            key_hint("tui.input.tab".into(), "switch mode") + &sep
        } else {
            String::new()
        };
        let action_hint = if write_scope == ConfigWriteScope::Project {
            raw_key_hint("space", "cycle inherit/+/-")
        } else {
            raw_key_hint("space", "toggle")
        };
        let hint = format!("{switch_hint}{action_hint}{sep}{}", raw_key_hint("esc", "close"));
        let spacing = (width as i64 - visible_width(&title) as i64 - visible_width(&hint) as i64).max(1) as usize;
        let scope_hint = if write_scope == ConfigWriteScope::Project {
            t.fg(
                &ThemeColor::Muted,
                &format!("{CONFIG_DIR_NAME}/settings.json · inherited global resources are dimmed"),
            )
        } else {
            t.fg(&ThemeColor::Muted, &format!("~/{CONFIG_DIR_NAME}/agent/settings.json"))
        };
        vec![
            truncate_to_width(&format!("{title}{}{hint}", " ".repeat(spacing)), width, "", false),
            truncate_to_width(&scope_hint, width, "", false),
        ]
    }

    fn invalidate(&self) {}
}

/// Reference into the current scope's groups: group, subgroup and item indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryRef {
    Group(usize),
    Subgroup(usize, usize),
    Item(usize, usize, usize),
}

#[derive(Default)]
struct ResourceListCallbacks {
    on_cancel: Option<Arc<dyn Fn() + Send + Sync>>,
    on_exit: Option<Arc<dyn Fn() + Send + Sync>>,
    on_toggle: Option<Arc<dyn Fn(ResourceItem, bool) + Send + Sync>>,
    on_switch_mode: Option<Arc<dyn Fn() + Send + Sync>>,
}

struct ResourceListState {
    groups_by_scope: IndexMap<ConfigWriteScope, Vec<ResourceGroup>>,
    flat_items: Vec<EntryRef>,
    filtered_items: Vec<EntryRef>,
    selected_index: usize,
    write_scope: ConfigWriteScope,
    focused: bool,
}

impl ResourceListState {
    fn groups(&self) -> &[ResourceGroup] {
        self.groups_by_scope.get(&self.write_scope).map(|g| g.as_slice()).unwrap_or(&[])
    }

    fn item(&self, r: EntryRef) -> Option<&ResourceItem> {
        match r {
            EntryRef::Item(g, s, i) => self.groups().get(g)?.subgroups.get(s)?.items.get(i),
            _ => None,
        }
    }

    fn build_flat_list(&mut self) {
        let mut flat = Vec::new();
        for (gi, group) in self.groups().iter().enumerate() {
            flat.push(EntryRef::Group(gi));
            for (si, subgroup) in group.subgroups.iter().enumerate() {
                flat.push(EntryRef::Subgroup(gi, si));
                for ii in 0..subgroup.items.len() {
                    flat.push(EntryRef::Item(gi, si, ii));
                }
            }
        }
        self.flat_items = flat;
        // Start selection on first item (not header)
        self.selected_index = self.flat_items.iter().position(|e| matches!(e, EntryRef::Item(..))).unwrap_or(0);
    }

    fn find_next_item(&self, from_index: usize, direction: i64) -> usize {
        let mut idx = from_index as i64 + direction;
        while idx >= 0 && (idx as usize) < self.filtered_items.len() {
            if matches!(self.filtered_items[idx as usize], EntryRef::Item(..)) {
                return idx as usize;
            }
            idx += direction;
        }
        from_index // Stay at current if no item found
    }

    fn select_first_item(&mut self) {
        self.selected_index =
            self.filtered_items.iter().position(|e| matches!(e, EntryRef::Item(..))).unwrap_or(0);
    }

    fn filter_items(&mut self, query: &str) {
        if js_trim(query).is_empty() {
            self.filtered_items = self.flat_items.clone();
            self.select_first_item();
            return;
        }

        let lower_query = query.to_lowercase();
        let mut matching_items: HashSet<(usize, usize, usize)> = HashSet::new();
        for entry in &self.flat_items {
            if let EntryRef::Item(g, s, i) = *entry {
                let Some(item) = self.item(*entry) else { continue };
                if item.display_name.to_lowercase().contains(&lower_query)
                    || item.resource_type.as_str().to_lowercase().contains(&lower_query)
                    || item.path.to_lowercase().contains(&lower_query)
                {
                    matching_items.insert((g, s, i));
                }
            }
        }
        // Find which subgroups and groups contain matching items
        let matching_subgroups: HashSet<(usize, usize)> = matching_items.iter().map(|(g, s, _)| (*g, *s)).collect();
        let matching_groups: HashSet<usize> = matching_items.iter().map(|(g, _, _)| *g).collect();

        self.filtered_items = self
            .flat_items
            .iter()
            .copied()
            .filter(|entry| match *entry {
                EntryRef::Group(g) => matching_groups.contains(&g),
                EntryRef::Subgroup(g, s) => matching_subgroups.contains(&(g, s)),
                EntryRef::Item(g, s, i) => matching_items.contains(&(g, s, i)),
            })
            .collect();
        self.select_first_item();
    }
}

fn get_pattern_entry_target(entry: &str) -> &str {
    if entry.starts_with('!') || entry.starts_with('+') || entry.starts_with('-') { &entry[1..] } else { entry }
}

fn get_resource_item_key(item: &ResourceItem) -> String {
    format!("{}:{}", item.resource_type.as_str(), canonicalize_path(&item.path))
}

fn get_item_scope(item: &ResourceItem) -> SettingsScope {
    if item.metadata.scope == SourceScope::Project { SettingsScope::Project } else { SettingsScope::User }
}

fn settings_list(settings: &Settings, resource_type: ResourceType) -> Vec<String> {
    match resource_type {
        ResourceType::Extensions => settings.extensions.clone(),
        ResourceType::Skills => settings.skills.clone(),
        ResourceType::Prompts => settings.prompts.clone(),
        ResourceType::Themes => settings.themes.clone(),
    }
    .unwrap_or_default()
}

fn package_list(pkg: &PackageSourceVariant2, resource_type: ResourceType) -> Option<&Vec<String>> {
    match resource_type {
        ResourceType::Extensions => pkg.extensions.as_ref(),
        ResourceType::Skills => pkg.skills.as_ref(),
        ResourceType::Prompts => pkg.prompts.as_ref(),
        ResourceType::Themes => pkg.themes.as_ref(),
    }
}

fn set_package_list(pkg: &mut PackageSourceVariant2, resource_type: ResourceType, value: Option<Vec<String>>) {
    match resource_type {
        ResourceType::Extensions => pkg.extensions = value,
        ResourceType::Skills => pkg.skills = value,
        ResourceType::Prompts => pkg.prompts = value,
        ResourceType::Themes => pkg.themes = value,
    }
}

fn package_source_str(pkg: &PackageSource) -> &str {
    match pkg {
        PackageSource::String(s) => s,
        PackageSource::PackageSourceVariant2(p) => &p.source,
    }
}

fn has_package_filters(pkg: &PackageSourceVariant2) -> bool {
    RESOURCE_TYPES.iter().any(|t| package_list(pkg, *t).is_some())
}

fn object_package(source: String) -> PackageSourceVariant2 {
    PackageSourceVariant2 { source, autoload: None, extensions: None, skills: None, prompts: None, themes: None }
}

/// TS: `implements Component, Focusable`
pub struct ResourceList {
    state: Mutex<ResourceListState>,
    callbacks: Mutex<ResourceListCallbacks>,
    search_input: Input,
    max_visible: usize,
    settings_manager: Arc<SettingsManager>,
    cwd: String,
    agent_dir: String,
    inherited_enabled_by_key: HashMap<String, bool>,
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
        let mut inherited_enabled_by_key = HashMap::new();
        if let Some(global) = groups_by_scope.get(&ConfigWriteScope::Global) {
            for group in global {
                for subgroup in &group.subgroups {
                    for item in &subgroup.items {
                        inherited_enabled_by_key.insert(get_resource_item_key(item), item.enabled);
                    }
                }
            }
        }
        // 8 lines of chrome: top spacer + top border + spacer + header (2 lines) + spacer + bottom spacer + bottom border
        let chrome = 8;
        let max_visible = (terminal_height.unwrap_or(24) - chrome).max(5) as usize;
        let mut state = ResourceListState {
            groups_by_scope,
            flat_items: Vec::new(),
            filtered_items: Vec::new(),
            selected_index: 0,
            write_scope: write_scope.copied().unwrap_or(ConfigWriteScope::Global),
            focused: false,
        };
        state.build_flat_list();
        state.filtered_items = state.flat_items.clone();
        Self {
            state: Mutex::new(state),
            callbacks: Mutex::new(ResourceListCallbacks::default()),
            search_input: Input::default(),
            max_visible,
            settings_manager,
            cwd: cwd.to_string(),
            agent_dir: agent_dir.to_string(),
            inherited_enabled_by_key,
        }
    }

    pub fn on_cancel(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.callbacks.lock().on_cancel.clone()
    }

    pub fn set_on_cancel(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.callbacks.lock().on_cancel = value;
    }

    pub fn on_exit(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.callbacks.lock().on_exit.clone()
    }

    pub fn set_on_exit(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.callbacks.lock().on_exit = value;
    }

    pub fn on_toggle(&self) -> Option<Arc<dyn Fn(ResourceItem, bool) + Send + Sync>> {
        self.callbacks.lock().on_toggle.clone()
    }

    pub fn set_on_toggle(&self, value: Option<Arc<dyn Fn(ResourceItem, bool) + Send + Sync>>) {
        self.callbacks.lock().on_toggle = value;
    }

    pub fn on_switch_mode(&self) -> Option<Arc<dyn Fn() + Send + Sync>> {
        self.callbacks.lock().on_switch_mode.clone()
    }

    pub fn set_on_switch_mode(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.callbacks.lock().on_switch_mode = value;
    }

    pub fn set_write_scope(&self, write_scope: &ConfigWriteScope) {
        let query = self.search_input.get_value();
        let mut s = self.state.lock();
        s.write_scope = *write_scope;
        s.build_flat_list();
        s.filter_items(&query);
    }

    pub fn update_item(&self, item: &ResourceItem, enabled: bool) {
        let mut s = self.state.lock();
        let scope = s.write_scope;
        if let Some(groups) = s.groups_by_scope.get_mut(&scope) {
            for group in groups.iter_mut() {
                for subgroup in group.subgroups.iter_mut() {
                    if let Some(found) = subgroup
                        .items
                        .iter_mut()
                        .find(|i| i.path == item.path && i.resource_type == item.resource_type)
                    {
                        found.enabled = enabled;
                        return;
                    }
                }
            }
        }
    }

    fn get_top_level_base_dir(&self, scope: SettingsScope) -> String {
        if scope == SettingsScope::Project { node_join(&[&self.cwd, CONFIG_DIR_NAME]) } else { self.agent_dir.clone() }
    }

    fn get_resource_pattern(&self, item: &ResourceItem) -> String {
        if item.metadata.source == "builtin" {
            return item.path.clone();
        }
        let scope = get_item_scope(item);
        let base_dir = item.metadata.base_dir.clone().unwrap_or_else(|| self.get_top_level_base_dir(scope));
        node_relative(&base_dir, &item.path)
    }

    fn get_package_resource_pattern(&self, item: &ResourceItem) -> String {
        let base_dir = item.metadata.base_dir.clone().unwrap_or_else(|| node_dirname(&item.path));
        node_relative(&base_dir, &item.path)
    }

    fn get_resource_pattern_for_scope(&self, item: &ResourceItem, scope: SettingsScope) -> String {
        let source_scope = get_item_scope(item);
        if scope != source_scope || item.metadata.source == "builtin" {
            return item.path.clone();
        }
        let base_dir = item.metadata.base_dir.clone().unwrap_or_else(|| self.get_top_level_base_dir(source_scope));
        node_relative(&base_dir, &item.path)
    }

    fn get_top_level_override_patterns(&self, item: &ResourceItem, scope: SettingsScope) -> HashSet<String> {
        let base_dir = self.get_top_level_base_dir(scope);
        let mut patterns = HashSet::new();
        patterns.insert(self.get_resource_pattern_for_scope(item, scope));
        patterns.insert(item.path.clone());
        patterns.insert(node_relative(&base_dir, &item.path));
        if let Some(base) = item.metadata.base_dir.as_deref().filter(|b| !b.is_empty()) {
            patterns.insert(node_relative(base, &item.path));
        }
        patterns
    }

    fn trim_options() -> PathInputOptions {
        PathInputOptions { trim: Some(true), ..Default::default() }
    }

    fn package_source_string_matches(
        &self,
        left_source: &str,
        left_scope: SettingsScope,
        right_source: &str,
        right_scope: SettingsScope,
    ) -> bool {
        if left_source == right_source {
            return true;
        }
        if !is_local_path(left_source) || !is_local_path(right_source) {
            return false;
        }
        let opts = Self::trim_options();
        let left = resolve_path(left_source, Some(&self.get_top_level_base_dir(left_scope)), Some(&opts));
        let right = resolve_path(right_source, Some(&self.get_top_level_base_dir(right_scope)), Some(&opts));
        left == right
    }

    fn find_matching_package_source(&self, item: &ResourceItem, target_scope: SettingsScope) -> Option<PackageSource> {
        let settings = if target_scope == SettingsScope::Project {
            self.settings_manager.get_project_settings()
        } else {
            self.settings_manager.get_global_settings()
        };
        settings.packages.unwrap_or_default().into_iter().find(|pkg| {
            self.package_source_string_matches(
                &item.metadata.source,
                get_item_scope(item),
                package_source_str(pkg),
                target_scope,
            )
        })
    }

    fn create_package_override_source(&self, item: &ResourceItem) -> PackageSource {
        let source = item.metadata.source.clone();
        if !is_local_path(&source) {
            return PackageSource::PackageSourceVariant2(PackageSourceVariant2 {
                autoload: Some(false),
                ..object_package(source)
            });
        }
        let source_path = resolve_path(
            &source,
            Some(&self.get_top_level_base_dir(get_item_scope(item))),
            Some(&Self::trim_options()),
        );
        let rel = node_relative(&self.get_top_level_base_dir(SettingsScope::Project), &source_path);
        PackageSource::PackageSourceVariant2(PackageSourceVariant2 {
            autoload: Some(false),
            ..object_package(if rel.is_empty() { ".".to_string() } else { rel })
        })
    }

    fn get_inherited_enabled(&self, item: &ResourceItem) -> bool {
        self.inherited_enabled_by_key
            .get(&get_resource_item_key(item))
            .copied()
            .unwrap_or(if get_item_scope(item) == SettingsScope::User { item.enabled } else { true })
    }

    fn is_inherited_global_item(&self, item: &ResourceItem) -> bool {
        get_item_scope(item) == SettingsScope::User
            || self.inherited_enabled_by_key.contains_key(&get_resource_item_key(item))
    }

    fn get_override_state_from_entries(
        entries: &[String],
        patterns: &HashSet<String>,
        empty_array_is_unload: bool,
    ) -> ProjectOverrideState {
        if entries.is_empty() && empty_array_is_unload {
            return ProjectOverrideState::Unload;
        }
        let mut state = ProjectOverrideState::Inherit;
        for entry in entries {
            if !patterns.contains(get_pattern_entry_target(entry)) {
                continue;
            }
            state = if entry.starts_with('!') || entry.starts_with('-') {
                ProjectOverrideState::Unload
            } else {
                ProjectOverrideState::Load
            };
        }
        state
    }

    fn get_project_override_state(&self, item: &ResourceItem, write_scope: ConfigWriteScope) -> ProjectOverrideState {
        if write_scope != ConfigWriteScope::Project {
            return ProjectOverrideState::Inherit;
        }
        if item.metadata.origin == SourceOrigin::TopLevel {
            let settings = self.settings_manager.get_project_settings();
            return Self::get_override_state_from_entries(
                &settings_list(&settings, item.resource_type),
                &self.get_top_level_override_patterns(item, SettingsScope::Project),
                false,
            );
        }
        let Some(PackageSource::PackageSourceVariant2(pkg)) =
            self.find_matching_package_source(item, SettingsScope::Project)
        else {
            return ProjectOverrideState::Inherit;
        };
        let Some(entries) = package_list(&pkg, item.resource_type) else {
            return ProjectOverrideState::Inherit;
        };
        let mut patterns = HashSet::new();
        patterns.insert(self.get_package_resource_pattern(item));
        Self::get_override_state_from_entries(entries, &patterns, pkg.autoload != Some(false))
    }

    fn get_next_override_state(&self, item: &ResourceItem, write_scope: ConfigWriteScope) -> ProjectOverrideState {
        let state = self.get_project_override_state(item, write_scope);
        let inherited_enabled = self.get_inherited_enabled(item);
        match state {
            ProjectOverrideState::Inherit => {
                if inherited_enabled { ProjectOverrideState::Unload } else { ProjectOverrideState::Load }
            }
            ProjectOverrideState::Unload => {
                if inherited_enabled { ProjectOverrideState::Load } else { ProjectOverrideState::Inherit }
            }
            ProjectOverrideState::Load => {
                if inherited_enabled { ProjectOverrideState::Inherit } else { ProjectOverrideState::Unload }
            }
        }
    }

    fn set_project_top_level_paths(&self, key: ResourceType, paths: &[String]) {
        let _ = match key {
            ResourceType::Extensions => self.settings_manager.set_project_extension_paths(paths),
            ResourceType::Skills => self.settings_manager.set_project_skill_paths(paths),
            ResourceType::Prompts => self.settings_manager.set_project_prompt_template_paths(paths),
            ResourceType::Themes => self.settings_manager.set_project_theme_paths(paths),
        };
    }

    fn set_project_top_level_override(&self, item: &ResourceItem, state: ProjectOverrideState) -> bool {
        let current = settings_list(&self.settings_manager.get_project_settings(), item.resource_type);
        let inherited = self.is_inherited_global_item(item);
        let pattern =
            if inherited { item.path.clone() } else { self.get_resource_pattern_for_scope(item, SettingsScope::Project) };
        let patterns = self.get_top_level_override_patterns(item, SettingsScope::Project);
        let mut updated: Vec<String> = current
            .into_iter()
            .filter(|entry| {
                let target = get_pattern_entry_target(entry);
                if (entry.starts_with('!') || entry.starts_with('+') || entry.starts_with('-'))
                    && patterns.contains(target)
                {
                    return false;
                }
                !(state == ProjectOverrideState::Inherit && inherited && target == pattern)
            })
            .collect();
        if state != ProjectOverrideState::Inherit {
            // Project entries name inherited files to override them. Built-in paths need no entry.
            if inherited && item.metadata.source != "builtin" && !updated.contains(&pattern) {
                updated.push(pattern.clone());
            }
            updated.push(format!("{}{pattern}", if state == ProjectOverrideState::Load { "+" } else { "-" }));
        }
        self.set_project_top_level_paths(item.resource_type, &updated);
        true
    }

    fn set_project_package_override(&self, item: &ResourceItem, state: ProjectOverrideState) -> bool {
        let mut packages = self.settings_manager.get_project_settings().packages.unwrap_or_default();
        let mut pkg_index = packages.iter().position(|pkg| {
            self.package_source_string_matches(
                &item.metadata.source,
                get_item_scope(item),
                package_source_str(pkg),
                SettingsScope::Project,
            )
        });
        if pkg_index.is_none() {
            if state == ProjectOverrideState::Inherit {
                return false;
            }
            packages.push(self.create_package_override_source(item));
            pkg_index = Some(packages.len() - 1);
        }
        let Some(pkg_index) = pkg_index else { return false };
        let mut pkg = match packages[pkg_index].clone() {
            PackageSource::String(s) => object_package(s),
            PackageSource::PackageSourceVariant2(p) => p,
        };
        let pattern = self.get_package_resource_pattern(item);
        let mut updated: Vec<String> = package_list(&pkg, item.resource_type)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| get_pattern_entry_target(entry) != pattern)
            .collect();
        if state != ProjectOverrideState::Inherit {
            updated.push(format!("{}{pattern}", if state == ProjectOverrideState::Load { "+" } else { "-" }));
        }
        set_package_list(&mut pkg, item.resource_type, if updated.is_empty() { None } else { Some(updated) });
        if !has_package_filters(&pkg) {
            if pkg.autoload == Some(false) {
                packages.remove(pkg_index);
            } else {
                packages[pkg_index] = PackageSource::String(pkg.source);
            }
        } else {
            packages[pkg_index] = PackageSource::PackageSourceVariant2(pkg);
        }
        let _ = self.settings_manager.set_project_packages(&packages);
        true
    }

    fn set_project_resource_override(&self, item: &ResourceItem, state: ProjectOverrideState) -> bool {
        if item.metadata.origin == SourceOrigin::TopLevel {
            self.set_project_top_level_override(item, state)
        } else {
            self.set_project_package_override(item, state)
        }
    }

    fn toggle_top_level_resource(&self, item: &ResourceItem, enabled: bool) {
        let scope = get_item_scope(item);
        let settings = if scope == SettingsScope::Project {
            self.settings_manager.get_project_settings()
        } else {
            self.settings_manager.get_global_settings()
        };
        let current = settings_list(&settings, item.resource_type);

        // Generate pattern for this resource
        let pattern = self.get_resource_pattern(item);

        // Filter out existing patterns for this resource
        let mut updated: Vec<String> =
            current.into_iter().filter(|p| get_pattern_entry_target(p) != pattern).collect();
        updated.push(format!("{}{pattern}", if enabled { "+" } else { "-" }));

        if scope == SettingsScope::Project {
            self.set_project_top_level_paths(item.resource_type, &updated);
        } else {
            match item.resource_type {
                ResourceType::Extensions => self.settings_manager.set_extension_paths(&updated),
                ResourceType::Skills => self.settings_manager.set_skill_paths(&updated),
                ResourceType::Prompts => self.settings_manager.set_prompt_template_paths(&updated),
                ResourceType::Themes => self.settings_manager.set_theme_paths(&updated),
            }
        }
    }

    fn toggle_package_resource(&self, item: &ResourceItem, enabled: bool) {
        let scope = get_item_scope(item);
        let settings = if scope == SettingsScope::Project {
            self.settings_manager.get_project_settings()
        } else {
            self.settings_manager.get_global_settings()
        };
        let mut packages = settings.packages.unwrap_or_default();
        let Some(pkg_index) = packages.iter().position(|pkg| package_source_str(pkg) == item.metadata.source) else {
            return;
        };

        // Convert string to object form if needed
        let mut pkg = match packages[pkg_index].clone() {
            PackageSource::String(s) => object_package(s),
            PackageSource::PackageSourceVariant2(p) => p,
        };

        // Generate pattern relative to package root
        let pattern = self.get_package_resource_pattern(item);
        let mut updated: Vec<String> = package_list(&pkg, item.resource_type)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| get_pattern_entry_target(p) != pattern)
            .collect();
        updated.push(format!("{}{pattern}", if enabled { "+" } else { "-" }));
        set_package_list(&mut pkg, item.resource_type, if updated.is_empty() { None } else { Some(updated) });

        // Clean up empty filter object
        packages[pkg_index] = if has_package_filters(&pkg) {
            PackageSource::PackageSourceVariant2(pkg)
        } else {
            PackageSource::String(pkg.source)
        };

        if scope == SettingsScope::Project {
            let _ = self.settings_manager.set_project_packages(&packages);
        } else {
            self.settings_manager.set_packages(&packages);
        }
    }

    fn toggle_resource(&self, item: &ResourceItem, write_scope: ConfigWriteScope) -> Option<bool> {
        if write_scope == ConfigWriteScope::Project {
            let state = self.get_next_override_state(item, write_scope);
            if !self.set_project_resource_override(item, state) {
                return None;
            }
            return Some(match state {
                ProjectOverrideState::Inherit => self.get_inherited_enabled(item),
                ProjectOverrideState::Load => true,
                ProjectOverrideState::Unload => false,
            });
        }
        let enabled = !item.enabled;
        if item.metadata.origin == SourceOrigin::TopLevel {
            self.toggle_top_level_resource(item, enabled);
        } else {
            self.toggle_package_resource(item, enabled);
        }
        Some(enabled)
    }

    fn render_checkbox(&self, item: &ResourceItem, write_scope: ConfigWriteScope) -> String {
        let t = theme();
        if write_scope == ConfigWriteScope::Project {
            return match self.get_project_override_state(item, write_scope) {
                ProjectOverrideState::Load => t.fg(&ThemeColor::Success, "[+]"),
                ProjectOverrideState::Unload => t.fg(&ThemeColor::Warning, "[-]"),
                ProjectOverrideState::Inherit => t.fg(&ThemeColor::Dim, if item.enabled { "[x]" } else { "[ ]" }),
            };
        }
        if item.enabled { t.fg(&ThemeColor::Success, "[x]") } else { t.fg(&ThemeColor::Dim, "[ ]") }
    }

    fn get_item_suffix(&self, item: &ResourceItem, write_scope: ConfigWriteScope) -> String {
        if write_scope != ConfigWriteScope::Project {
            return String::new();
        }
        let t = theme();
        match self.get_project_override_state(item, write_scope) {
            ProjectOverrideState::Load => t.fg(&ThemeColor::Muted, "  project load"),
            ProjectOverrideState::Unload => t.fg(&ThemeColor::Muted, "  project unload"),
            ProjectOverrideState::Inherit => {
                if self.is_inherited_global_item(item) {
                    t.fg(&ThemeColor::Dim, "  inherited global")
                } else {
                    String::new()
                }
            }
        }
    }

    fn is_dimmed_item(&self, item: &ResourceItem, write_scope: ConfigWriteScope) -> bool {
        write_scope == ConfigWriteScope::Project
            && self.is_inherited_global_item(item)
            && self.get_project_override_state(item, write_scope) == ProjectOverrideState::Inherit
    }

    fn call(&self, f: impl FnOnce(&ResourceListCallbacks) -> Option<Arc<dyn Fn() + Send + Sync>>) {
        let cb = f(&self.callbacks.lock());
        if let Some(cb) = cb {
            cb();
        }
    }
}

impl Component for ResourceList {
    fn render(&self, width: usize) -> Vec<String> {
        let t = theme();
        let mut lines = Vec::new();

        // Search input
        lines.extend(self.search_input.render(width));
        lines.push(String::new());

        let s = self.state.lock();
        if s.filtered_items.is_empty() {
            lines.push(t.fg(&ThemeColor::Muted, "  No resources found"));
            return lines;
        }

        let len = s.filtered_items.len() as i64;
        let max_visible = self.max_visible as i64;
        let start_index = (s.selected_index as i64 - max_visible / 2).min(len - max_visible).max(0);
        let end_index = (start_index + max_visible).min(len);
        let write_scope = s.write_scope;
        let groups = s.groups();

        for i in start_index..end_index {
            let entry = s.filtered_items[i as usize];
            let is_selected = i as usize == s.selected_index;
            match entry {
                EntryRef::Group(g) => {
                    // Main group header (no cursor)
                    let group = &groups[g];
                    let inherited = write_scope == ConfigWriteScope::Project && group.scope == SourceScope::User;
                    let label =
                        t.bold(&format!("{}{}", group.label, if inherited { " · inherited global" } else { "" }));
                    let group_line =
                        t.fg(if inherited { &ThemeColor::Dim } else { &ThemeColor::Accent }, &label);
                    lines.push(truncate_to_width(&format!("  {group_line}"), width, "", false));
                }
                EntryRef::Subgroup(g, sg) => {
                    // Subgroup header (indented, no cursor)
                    let group = &groups[g];
                    let color = if write_scope == ConfigWriteScope::Project && group.scope == SourceScope::User {
                        ThemeColor::Dim
                    } else {
                        ThemeColor::Muted
                    };
                    let subgroup_line = t.fg(&color, &group.subgroups[sg].label);
                    lines.push(truncate_to_width(&format!("    {subgroup_line}"), width, "", false));
                }
                EntryRef::Item(..) => {
                    // Resource item (cursor only on items)
                    let Some(item) = s.item(entry) else { continue };
                    let cursor = if is_selected { "> " } else { "  " };
                    let dimmed = self.is_dimmed_item(item, write_scope);
                    let name_text =
                        if is_selected && !dimmed { t.bold(&item.display_name) } else { item.display_name.clone() };
                    let name = if dimmed { t.fg(&ThemeColor::Dim, &name_text) } else { name_text };
                    lines.push(truncate_to_width(
                        &format!(
                            "{cursor}    {} {name}{}",
                            self.render_checkbox(item, write_scope),
                            self.get_item_suffix(item, write_scope)
                        ),
                        width,
                        "...",
                        false,
                    ));
                }
            }
        }

        // Scroll indicator
        if start_index > 0 || end_index < len {
            let item_count = s.filtered_items.iter().filter(|e| matches!(e, EntryRef::Item(..))).count();
            let current_item_index =
                s.filtered_items[..s.selected_index].iter().filter(|e| matches!(e, EntryRef::Item(..))).count() + 1;
            lines.push(t.fg(&ThemeColor::Dim, &format!("  ({current_item_index}/{item_count})")));
        }

        lines
    }

    fn handle_input(&self, data: &str) {
        let kb = get_keybindings();

        if kb.matches(data, "tui.select.up") {
            let mut s = self.state.lock();
            s.selected_index = s.find_next_item(s.selected_index, -1);
            return;
        }
        if kb.matches(data, "tui.select.down") {
            let mut s = self.state.lock();
            s.selected_index = s.find_next_item(s.selected_index, 1);
            return;
        }
        if kb.matches(data, "tui.select.pageUp") {
            // Jump up by maxVisible, then find nearest item
            let mut s = self.state.lock();
            let mut target = s.selected_index.saturating_sub(self.max_visible);
            while target < s.filtered_items.len() && !matches!(s.filtered_items[target], EntryRef::Item(..)) {
                target += 1;
            }
            if target < s.filtered_items.len() {
                s.selected_index = target;
            }
            return;
        }
        if kb.matches(data, "tui.select.pageDown") {
            // Jump down by maxVisible, then find nearest item
            let mut s = self.state.lock();
            let mut target =
                ((s.filtered_items.len() as i64 - 1).min((s.selected_index + self.max_visible) as i64)) as i64;
            while target >= 0 && !matches!(s.filtered_items[target as usize], EntryRef::Item(..)) {
                target -= 1;
            }
            if target >= 0 {
                s.selected_index = target as usize;
            }
            return;
        }
        if kb.matches(data, "tui.select.cancel") {
            self.call(|c| c.on_cancel.clone());
            return;
        }
        if matches_key(data, "ctrl+c") {
            self.call(|c| c.on_exit.clone());
            return;
        }
        if kb.matches(data, "tui.input.tab") {
            self.call(|c| c.on_switch_mode.clone());
            return;
        }
        if data == " " || kb.matches(data, "tui.select.confirm") {
            let (item, write_scope) = {
                let s = self.state.lock();
                let entry = s.filtered_items.get(s.selected_index).copied();
                (entry.and_then(|e| s.item(e).cloned()), s.write_scope)
            };
            if let Some(item) = item {
                if write_scope == ConfigWriteScope::Project || get_item_scope(&item) == SettingsScope::User {
                    if let Some(new_enabled) = self.toggle_resource(&item, write_scope) {
                        self.update_item(&item, new_enabled);
                        let mut updated = item.clone();
                        updated.enabled = new_enabled;
                        let cb = self.callbacks.lock().on_toggle.clone();
                        if let Some(cb) = cb {
                            cb(updated, new_enabled);
                        }
                    }
                }
            }
            return;
        }

        // Pass to search input
        self.search_input.handle_input(data);
        let query = self.search_input.get_value();
        self.state.lock().filter_items(&query);
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {}

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.search_input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.state.lock().focused
    }
}

/// TS: `extends Container implements Focusable`
pub struct ConfigSelectorComponent {
    container: Container,
    header: Arc<ConfigSelectorHeader>,
    resource_list: Arc<ResourceList>,
    write_scope: Arc<Mutex<ConfigWriteScope>>,
    focused: Mutex<bool>,
}

impl ConfigSelectorComponent {
    #[allow(clippy::too_many_arguments)]
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
        let write_scope = write_scope.copied().unwrap_or(ConfigWriteScope::Global);
        let project_mode_available = project_mode_available.unwrap_or(true);
        let empty = ResolvedPaths::default();
        let mut groups_by_scope = IndexMap::new();
        groups_by_scope.insert(
            ConfigWriteScope::Global,
            build_groups(resolved_paths.get(&ConfigWriteScope::Global).unwrap_or(&empty), agent_dir),
        );
        groups_by_scope.insert(
            ConfigWriteScope::Project,
            build_groups(resolved_paths.get(&ConfigWriteScope::Project).unwrap_or(&empty), agent_dir),
        );

        let container = Container::new();
        // Add header
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));
        container.add_child(Arc::new(Spacer::new(1)));
        let header = Arc::new(ConfigSelectorHeader::new(&write_scope, project_mode_available));
        container.add_child(header.clone());
        container.add_child(Arc::new(Spacer::new(1)));

        // Resource list
        let resource_list = Arc::new(ResourceList::new(
            groups_by_scope,
            settings_manager,
            cwd,
            agent_dir,
            terminal_height,
            Some(&write_scope),
        ));
        resource_list.set_on_cancel(Some(on_close));
        resource_list.set_on_exit(Some(on_exit));
        {
            let request_render = request_render.clone();
            resource_list.set_on_toggle(Some(Arc::new(move |_item: ResourceItem, _enabled: bool| request_render())));
        }
        let write_scope = Arc::new(Mutex::new(write_scope));
        if project_mode_available {
            let header = header.clone();
            let list = Arc::downgrade(&resource_list);
            let scope = write_scope.clone();
            resource_list.set_on_switch_mode(Some(Arc::new(move || {
                let next = {
                    let mut s = scope.lock();
                    *s = if *s == ConfigWriteScope::Global { ConfigWriteScope::Project } else { ConfigWriteScope::Global };
                    *s
                };
                header.set_write_scope(&next);
                if let Some(list) = list.upgrade() {
                    list.set_write_scope(&next);
                }
                request_render();
            })));
        }
        container.add_child(resource_list.clone());

        // Bottom border
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(None)));

        Self { container, header, resource_list, write_scope, focused: Mutex::new(false) }
    }

    pub fn get_resource_list(&self) -> Arc<ResourceList> {
        self.resource_list.clone()
    }

    /// The current write scope.
    pub fn write_scope(&self) -> ConfigWriteScope {
        let _ = &self.header;
        *self.write_scope.lock()
    }
}

impl Component for ConfigSelectorComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn invalidate(&self) {
        self.container.invalidate();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        *self.focused.lock() = focused;
        self.resource_list.set_focused(focused);
    }

    fn focused(&self) -> bool {
        *self.focused.lock()
    }
}
