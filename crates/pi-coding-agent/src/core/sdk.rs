//! Port of `packages/coding-agent/src/core/sdk.ts`.

use crate::core::agent_session::AgentSession;
use crate::core::extensions::types::LoadExtensionsResult;
use crate::core::extensions::types::SessionStartEvent;
use crate::core::model_runtime::ModelRuntime;
use crate::core::resource_loader::ResourceLoader;
use crate::core::session_manager::SessionManager;
use crate::core::settings_manager::SettingsManager;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::Model;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use crate::core::agent_session_runtime::*;
pub use crate::core::extensions::types::ExtensionAPI;
pub use crate::core::extensions::types::ExtensionCommandContext;
pub use crate::core::extensions::types::ExtensionContext;
pub use crate::core::extensions::types::ExtensionFactory;
pub use crate::core::extensions::types::InlineExtension;
pub use crate::core::extensions::types::ToolDefinition;
pub use crate::core::prompt_templates::PromptTemplate;
pub use crate::core::skills::Skill;
pub use crate::core::slash_commands::SlashCommandInfo;
pub use crate::core::slash_commands::SlashCommandSource;
pub use crate::core::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionOptionsScopedModels {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CreateAgentSessionOptionsNoTools {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "builtin")]
    Builtin,
}

impl CreateAgentSessionOptionsNoTools {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Builtin => "builtin",
        }
    }
}

#[derive(Clone)]
pub struct CreateAgentSessionOptions {
    /// Working directory for project-local discovery. Default: process.cwd()
    pub cwd: Option<String>,
    /// Global config directory. Default: ~/.pi/agent
    pub agent_dir: Option<String>,
    /// Canonical model/auth runtime. Defaults to a runtime using agentDir/auth.json and models.json.
    pub model_runtime: Option<Arc<ModelRuntime>>,
    /// Model to use. Default: from settings, else first available
    pub model: Option<Model>,
    /// Thinking level. Default: from settings, else 'medium' (clamped to model capabilities)
    pub thinking_level: Option<ThinkingLevel>,
    /// Models available for cycling (Ctrl+P in interactive mode)
    pub scoped_models: Option<Vec<CreateAgentSessionOptionsScopedModels>>,
    /// Optional default tool suppression mode when no explicit allowlist is provided.
    ///
    /// - "all": start with no tools enabled
    /// - "builtin": disable the default built-in tools (read, bash, edit, write)
    /// but keep extension/custom tools enabled
    pub no_tools: Option<CreateAgentSessionOptionsNoTools>,
    /// Optional allowlist of tool names or patterns, where `*` matches any characters.
    ///
    /// When omitted, pi uses the resolved `defaultTools` setting for the initial
    /// selection when configured. Otherwise it enables the default built-in tools
    /// (read, bash, edit, write). Extension/custom tools remain enabled unless
    /// `noTools` changes that default. When provided, only matching tools are
    /// enabled. MCP tools stay registered for codemode and tool search unless an
    /// entry starts with `mcp__`; then only matching MCP tools are kept. An empty
    /// list, like `noTools: "all"`, disables MCP tools too.
    ///
    /// A list of only `+name` and `-name` entries is not an allowlist: it adds tools
    /// to or removes them from the default selection, like the `defaultTools` setting.
    /// These entries take exact names. Mixing them with plain entries throws.
    pub tools: Option<Vec<String>>,
    /// Optional denylist of tool names or patterns to disable. Applies after `tools` when both are
    /// provided, MCP tools included.
    pub exclude_tools: Option<Vec<String>>,
    /// Custom tools to register (in addition to built-in tools).
    pub custom_tools: Option<Vec<ToolDefinition>>,
    /// Resource loader. When omitted, DefaultResourceLoader is used.
    pub resource_loader: Option<Arc<dyn ResourceLoader>>,
    /// Session manager. Default: SessionManager.create(cwd)
    pub session_manager: Option<Arc<SessionManager>>,
    /// Settings manager. Default: SettingsManager.create(cwd, agentDir)
    pub settings_manager: Option<Arc<SettingsManager>>,
    /// Session start event metadata for extension runtime startup.
    pub session_start_event: Option<SessionStartEvent>,
}

/// Result from createAgentSession
#[derive(Clone)]
pub struct CreateAgentSessionResult {
    /// The created session
    pub session: Arc<AgentSession>,
    /// Extensions result (for UI context setup in interactive mode)
    pub extensions_result: LoadExtensionsResult,
    /// Warning if session was restored with a different model than saved
    pub model_fallback_message: Option<String>,
}

/// Create an AgentSession with the specified options.
///
/// @example
/// ```text
/// // Minimal - uses defaults
/// const { session } = await createAgentSession();
///
/// // With explicit model
/// import { getModel } from '@earendil-works/pi-ai';
/// const { session } = await createAgentSession({
///   model: getModel('anthropic', 'claude-opus-4-5'),
///   thinkingLevel: 'high',
/// });
///
/// // Continue previous session
/// const { session, modelFallbackMessage } = await createAgentSession({
///   continueSession: true,
/// });
///
/// // Full control
/// const loader = new DefaultResourceLoader({
///   cwd: process.cwd(),
///   agentDir: getAgentDir(),
///   settingsManager: SettingsManager.create(),
/// });
/// await loader.reload();
/// const { session } = await createAgentSession({
///   model: myModel,
///   tools: ["read", "bash"],
///   resourceLoader: loader,
///   sessionManager: SessionManager.inMemory(),
/// });
/// ```
pub async fn create_agent_session(
    options: Option<&CreateAgentSessionOptions>,
) -> anyhow::Result<CreateAgentSessionResult> {
    todo!("port core/sdk.ts:createAgentSession")
}
