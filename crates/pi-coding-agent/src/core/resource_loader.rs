//! Port of `packages/coding-agent/src/core/resource-loader.ts`.

use crate::core::event_bus::EventBus;
use crate::core::extensions::types::InlineExtension;
use crate::core::extensions::types::LoadExtensionsResult;
use crate::core::package_manager::PathMetadata;
use crate::core::prompt_templates::PromptTemplate;
use crate::core::settings_manager::SettingsManager;
use crate::core::skills::Skill;
use crate::modes::interactive::theme::theme::Theme;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use crate::core::diagnostics::ResourceCollision;
pub use crate::core::diagnostics::ResourceDiagnostic;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceExtensionPathsSkillPaths {
    pub path: String,
    pub metadata: PathMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceExtensionPathsPromptPaths {
    pub path: String,
    pub metadata: PathMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceExtensionPathsThemePaths {
    pub path: String,
    pub metadata: PathMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceExtensionPaths {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_paths: Option<Vec<ResourceExtensionPathsSkillPaths>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_paths: Option<Vec<ResourceExtensionPathsPromptPaths>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_paths: Option<Vec<ResourceExtensionPathsThemePaths>>,
}

#[derive(Clone)]
pub struct ResourceLoaderReloadOptionsInput {
    pub extensions_result: LoadExtensionsResult,
}

#[derive(Clone)]
pub struct ResourceLoaderReloadOptions {
    pub resolve_project_trust: Option<
        Arc<
            dyn Fn(ResourceLoaderReloadOptionsInput) -> BoxFuture<'static, anyhow::Result<bool>>
                + Send
                + Sync,
        >,
    >,
}

pub type BuiltinExtension = InlineExtension;

/// Built-in extensions supply the code of `builtin:<name>` extension paths (see `InlineExtension`).
pub fn is_builtin_extension(input: &InlineExtension) -> bool {
    todo!("port core/resource-loader.ts:isBuiltinExtension")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLoaderGetSkillsResult {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLoaderGetPromptsResult {
    pub prompts: Vec<PromptTemplate>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Clone)]
pub struct ResourceLoaderGetThemesResult {
    pub themes: Vec<Arc<Theme>>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLoaderGetAgentsFilesResultAgentsFiles {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLoaderGetAgentsFilesResult {
    pub agents_files: Vec<ResourceLoaderGetAgentsFilesResultAgentsFiles>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLoaderGetSystemPromptSourceResult {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLoaderGetAppendSystemPromptSourcesResult {
    pub path: String,
}

#[async_trait::async_trait]
pub trait ResourceLoader: Send + Sync {
    fn get_extensions(&self) -> LoadExtensionsResult;
    fn get_skills(&self) -> ResourceLoaderGetSkillsResult;
    fn get_prompts(&self) -> ResourceLoaderGetPromptsResult;
    fn get_themes(&self) -> ResourceLoaderGetThemesResult;
    fn get_agents_files(&self) -> ResourceLoaderGetAgentsFilesResult;
    fn get_system_prompt(&self) -> Option<String>;
    fn get_system_prompt_source(&self) -> Option<ResourceLoaderGetSystemPromptSourceResult>;
    fn get_append_system_prompt(&self) -> Vec<String>;
    fn get_append_system_prompt_sources(
        &self,
    ) -> Vec<ResourceLoaderGetAppendSystemPromptSourcesResult>;
    fn extend_resources(&self, paths: &ResourceExtensionPaths);
    async fn reload(&self, options: Option<&ResourceLoaderReloadOptions>) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadProjectContextFilesOptions {
    pub cwd: String,
    pub agent_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadProjectContextFilesResult {
    pub path: String,
    pub content: String,
}

pub fn load_project_context_files(
    options: &LoadProjectContextFilesOptions,
) -> Vec<LoadProjectContextFilesResult> {
    todo!("port core/resource-loader.ts:loadProjectContextFiles")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsBase {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsResult {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsBase2 {
    pub prompts: Vec<PromptTemplate>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsResult2 {
    pub prompts: Vec<PromptTemplate>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Clone)]
pub struct DefaultResourceLoaderOptionsBase3 {
    pub themes: Vec<Arc<Theme>>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Clone)]
pub struct DefaultResourceLoaderOptionsResult3 {
    pub themes: Vec<Arc<Theme>>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsBase4AgentsFiles {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsBase4 {
    pub agents_files: Vec<DefaultResourceLoaderOptionsBase4AgentsFiles>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsResult4AgentsFiles {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderOptionsResult4 {
    pub agents_files: Vec<DefaultResourceLoaderOptionsResult4AgentsFiles>,
}

#[derive(Clone)]
pub struct DefaultResourceLoaderOptions {
    pub cwd: String,
    pub agent_dir: String,
    pub settings_manager: Option<Arc<SettingsManager>>,
    pub event_bus: Option<Arc<dyn EventBus>>,
    pub additional_extension_paths: Option<Vec<String>>,
    pub additional_skill_paths: Option<Vec<String>>,
    pub additional_prompt_template_paths: Option<Vec<String>>,
    pub additional_theme_paths: Option<Vec<String>>,
    pub extension_factories: Option<Vec<InlineExtension>>,
    pub no_extensions: Option<bool>,
    /// Built-in extensions not to load, by name (such as `mcp`), even when settings or `-e` enable them.
    pub disabled_builtin_extensions: Option<Vec<String>>,
    pub no_skills: Option<bool>,
    pub no_prompt_templates: Option<bool>,
    pub no_themes: Option<bool>,
    pub no_context_files: Option<bool>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<Vec<String>>,
    pub extensions_override:
        Option<Arc<dyn Fn(LoadExtensionsResult) -> LoadExtensionsResult + Send + Sync>>,
    pub skills_override: Option<
        Arc<
            dyn Fn(DefaultResourceLoaderOptionsBase) -> DefaultResourceLoaderOptionsResult
                + Send
                + Sync,
        >,
    >,
    pub prompts_override: Option<
        Arc<
            dyn Fn(DefaultResourceLoaderOptionsBase2) -> DefaultResourceLoaderOptionsResult2
                + Send
                + Sync,
        >,
    >,
    pub themes_override: Option<
        Arc<
            dyn Fn(DefaultResourceLoaderOptionsBase3) -> DefaultResourceLoaderOptionsResult3
                + Send
                + Sync,
        >,
    >,
    pub agents_files_override: Option<
        Arc<
            dyn Fn(DefaultResourceLoaderOptionsBase4) -> DefaultResourceLoaderOptionsResult4
                + Send
                + Sync,
        >,
    >,
    pub system_prompt_override: Option<Arc<dyn Fn(Option<String>) -> Option<String> + Send + Sync>>,
    pub append_system_prompt_override:
        Option<Arc<dyn Fn(Vec<String>) -> Vec<String> + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderGetSkillsResult {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderGetPromptsResult {
    pub prompts: Vec<PromptTemplate>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Clone)]
pub struct DefaultResourceLoaderGetThemesResult {
    pub themes: Vec<Arc<Theme>>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderGetAgentsFilesResultAgentsFiles {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderGetAgentsFilesResult {
    pub agents_files: Vec<DefaultResourceLoaderGetAgentsFilesResultAgentsFiles>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderGetSystemPromptSourceResult {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultResourceLoaderGetAppendSystemPromptSourcesResult {
    pub path: String,
}

/// TS: `implements ResourceLoader`
pub struct DefaultResourceLoader {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl DefaultResourceLoader {
    pub fn new(options: &DefaultResourceLoaderOptions) -> Self {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.constructor")
    }

    pub async fn load_project_trust_extensions(&self) -> anyhow::Result<LoadExtensionsResult> {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.loadProjectTrustExtensions")
    }
}

#[async_trait::async_trait]
impl ResourceLoader for DefaultResourceLoader {
    fn get_extensions(&self) -> LoadExtensionsResult {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_extensions")
    }
    fn get_skills(&self) -> ResourceLoaderGetSkillsResult {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_skills")
    }
    fn get_prompts(&self) -> ResourceLoaderGetPromptsResult {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_prompts")
    }
    fn get_themes(&self) -> ResourceLoaderGetThemesResult {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_themes")
    }
    fn get_agents_files(&self) -> ResourceLoaderGetAgentsFilesResult {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_agents_files")
    }
    fn get_system_prompt(&self) -> Option<String> {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_system_prompt")
    }
    fn get_system_prompt_source(&self) -> Option<ResourceLoaderGetSystemPromptSourceResult> {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_system_prompt_source")
    }
    fn get_append_system_prompt(&self) -> Vec<String> {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_append_system_prompt")
    }
    fn get_append_system_prompt_sources(
        &self,
    ) -> Vec<ResourceLoaderGetAppendSystemPromptSourcesResult> {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.get_append_system_prompt_sources")
    }
    fn extend_resources(&self, paths: &ResourceExtensionPaths) {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.extend_resources")
    }
    async fn reload(&self, options: Option<&ResourceLoaderReloadOptions>) -> anyhow::Result<()> {
        todo!("port core/resource-loader.ts:DefaultResourceLoader.reload")
    }
}
