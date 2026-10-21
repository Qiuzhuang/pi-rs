//! Port of `packages/coding-agent/src/core/agent-session-services.ts`.

use crate::core::diagnostics::ResourceDiagnostic;
use crate::core::event_bus::EventBus;
use crate::core::extensions::types::InlineExtension;
use crate::core::extensions::types::LoadExtensionsResult;
use crate::core::extensions::types::SessionStartEvent;
use crate::core::extensions::types::ToolDefinition;
use crate::core::model_runtime::ModelRuntime;
use crate::core::prompt_templates::PromptTemplate;
use crate::core::resource_loader::ResourceLoader;
use crate::core::resource_loader::ResourceLoaderReloadOptions;
use crate::core::sdk::CreateAgentSessionResult;
use crate::core::session_manager::SessionManager;
use crate::core::settings_manager::SettingsManager;
use crate::core::skills::Skill;
use crate::modes::interactive::theme::theme::Theme;
use indexmap::IndexMap;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::Model;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentSessionRuntimeDiagnosticType {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

impl AgentSessionRuntimeDiagnosticType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Non-fatal issues collected while creating services or sessions.
///
/// Runtime creation returns diagnostics to the caller instead of printing or
/// exiting. The app layer decides whether warnings should be shown and whether
/// errors should abort startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRuntimeDiagnostic {
    pub r#type: AgentSessionRuntimeDiagnosticType,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CreateAgentSessionServicesOptionsExtensionFlagValues {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsBase {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsResult {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsBase2 {
    pub prompts: Vec<PromptTemplate>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsResult2 {
    pub prompts: Vec<PromptTemplate>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Clone)]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsBase3 {
    pub themes: Vec<Arc<Theme>>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Clone)]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsResult3 {
    pub themes: Vec<Arc<Theme>>,
    pub diagnostics: Vec<ResourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsBase4AgentsFiles {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsBase4 {
    pub agents_files: Vec<CreateAgentSessionServicesOptionsResourceLoaderOptionsBase4AgentsFiles>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsResult4AgentsFiles {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptionsResult4 {
    pub agents_files: Vec<CreateAgentSessionServicesOptionsResourceLoaderOptionsResult4AgentsFiles>,
}

#[derive(Clone)]
pub struct CreateAgentSessionServicesOptionsResourceLoaderOptions {
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
            dyn Fn(
                    CreateAgentSessionServicesOptionsResourceLoaderOptionsBase,
                ) -> CreateAgentSessionServicesOptionsResourceLoaderOptionsResult
                + Send
                + Sync,
        >,
    >,
    pub prompts_override: Option<
        Arc<
            dyn Fn(
                    CreateAgentSessionServicesOptionsResourceLoaderOptionsBase2,
                ) -> CreateAgentSessionServicesOptionsResourceLoaderOptionsResult2
                + Send
                + Sync,
        >,
    >,
    pub themes_override: Option<
        Arc<
            dyn Fn(
                    CreateAgentSessionServicesOptionsResourceLoaderOptionsBase3,
                ) -> CreateAgentSessionServicesOptionsResourceLoaderOptionsResult3
                + Send
                + Sync,
        >,
    >,
    pub agents_files_override: Option<
        Arc<
            dyn Fn(
                    CreateAgentSessionServicesOptionsResourceLoaderOptionsBase4,
                ) -> CreateAgentSessionServicesOptionsResourceLoaderOptionsResult4
                + Send
                + Sync,
        >,
    >,
    pub system_prompt_override: Option<Arc<dyn Fn(Option<String>) -> Option<String> + Send + Sync>>,
    pub append_system_prompt_override:
        Option<Arc<dyn Fn(Vec<String>) -> Vec<String> + Send + Sync>>,
}

/// Inputs for creating cwd-bound runtime services.
///
/// These services are recreated whenever the effective session cwd changes.
/// CLI-provided resource paths should be resolved to absolute paths before they
/// reach this function, so later cwd switches do not reinterpret them.
#[derive(Clone)]
pub struct CreateAgentSessionServicesOptions {
    pub cwd: String,
    pub agent_dir: Option<String>,
    pub settings_manager: Option<Arc<SettingsManager>>,
    pub model_runtime: Option<Arc<ModelRuntime>>,
    pub model_runtime_signal: Option<CancellationToken>,
    pub extension_flag_values:
        Option<IndexMap<String, CreateAgentSessionServicesOptionsExtensionFlagValues>>,
    pub resource_loader_options: Option<CreateAgentSessionServicesOptionsResourceLoaderOptions>,
    pub resource_loader_reload_options: Option<ResourceLoaderReloadOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionFromServicesOptionsScopedModels {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CreateAgentSessionFromServicesOptionsNoTools {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "builtin")]
    Builtin,
}

impl CreateAgentSessionFromServicesOptionsNoTools {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Builtin => "builtin",
        }
    }
}

/// Inputs for creating an AgentSession from already-created services.
///
/// Use this after services exist and any cwd-bound model/tool/session options
/// have been resolved against those services.
#[derive(Clone)]
pub struct CreateAgentSessionFromServicesOptions {
    pub services: AgentSessionServices,
    pub session_manager: Arc<SessionManager>,
    pub session_start_event: Option<SessionStartEvent>,
    pub model: Option<Model>,
    pub thinking_level: Option<ThinkingLevel>,
    pub scoped_models: Option<Vec<CreateAgentSessionFromServicesOptionsScopedModels>>,
    pub tools: Option<Vec<String>>,
    pub exclude_tools: Option<Vec<String>>,
    pub no_tools: Option<CreateAgentSessionFromServicesOptionsNoTools>,
    pub custom_tools: Option<Vec<Arc<dyn ToolDefinition>>>,
}

/// Coherent cwd-bound runtime services for one effective session cwd.
///
/// This is infrastructure only. The AgentSession itself is created separately so
/// session options can be resolved against these services first.
#[derive(Clone)]
pub struct AgentSessionServices {
    pub cwd: String,
    pub agent_dir: String,
    pub model_runtime: Arc<ModelRuntime>,
    pub settings_manager: Arc<SettingsManager>,
    pub resource_loader: Arc<dyn ResourceLoader>,
    pub diagnostics: Vec<AgentSessionRuntimeDiagnostic>,
}

/// Create cwd-bound runtime services.
///
/// Returns services plus diagnostics. It does not create an AgentSession.
pub async fn create_agent_session_services(
    options: &CreateAgentSessionServicesOptions,
) -> anyhow::Result<AgentSessionServices> {
    todo!("port core/agent-session-services.ts:createAgentSessionServices")
}

/// Create an AgentSession from previously created services.
///
/// This keeps session creation separate from service creation so callers can
/// resolve model, thinking, tools, and other session inputs against the target
/// cwd before constructing the session.
pub async fn create_agent_session_from_services(
    options: &CreateAgentSessionFromServicesOptions,
) -> anyhow::Result<CreateAgentSessionResult> {
    todo!("port core/agent-session-services.ts:createAgentSessionFromServices")
}
