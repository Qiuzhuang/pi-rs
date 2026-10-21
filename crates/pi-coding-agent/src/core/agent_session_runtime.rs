//! Port of `packages/coding-agent/src/core/agent-session-runtime.ts`.

use crate::core::agent_session::AgentSession;
use crate::core::extensions::types::LoadExtensionsResult;
use crate::core::extensions::types::ProjectTrustContext;
use crate::core::extensions::types::ReplacedSessionContext;
use crate::core::extensions::types::SessionStartEvent;
use crate::core::session_manager::SessionManager;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use crate::core::agent_session_services::AgentSessionRuntimeDiagnostic;
pub use crate::core::agent_session_services::AgentSessionServices;
pub use crate::core::agent_session_services::CreateAgentSessionFromServicesOptions;
pub use crate::core::agent_session_services::CreateAgentSessionServicesOptions;
pub use crate::core::agent_session_services::create_agent_session_from_services;
pub use crate::core::agent_session_services::create_agent_session_services;

/// Result returned by runtime creation.
///
/// The caller gets the created session, its cwd-bound services, and all
/// diagnostics collected during setup.
// TS: extends CreateAgentSessionResult (inherited fields inlined)
#[derive(Clone)]
pub struct CreateAgentSessionRuntimeResult {
    /// The created session
    pub session: Arc<AgentSession>,
    /// Extensions result (for UI context setup in interactive mode)
    pub extensions_result: LoadExtensionsResult,
    /// Warning if session was restored with a different model than saved
    pub model_fallback_message: Option<String>,
    pub services: AgentSessionServices,
    pub diagnostics: Vec<AgentSessionRuntimeDiagnostic>,
}

#[derive(Clone)]
pub struct CreateAgentSessionRuntimeFactoryOptions {
    pub cwd: String,
    pub agent_dir: String,
    pub session_manager: Arc<SessionManager>,
    pub session_start_event: Option<SessionStartEvent>,
    pub project_trust_context: Option<ProjectTrustContext>,
}

/// Creates a full runtime for a target cwd and session manager.
///
/// The factory closes over process-global fixed inputs, recreates cwd-bound
/// services for the effective cwd, resolves session options against those
/// services, and finally creates the AgentSession.
pub type CreateAgentSessionRuntimeFactory = Arc<
    dyn Fn(
            CreateAgentSessionRuntimeFactoryOptions,
        ) -> BoxFuture<'static, anyhow::Result<CreateAgentSessionRuntimeResult>>
        + Send
        + Sync,
>;

/// Thrown when /import references a JSONL file path that does not exist.
/// TS: `extends Error`
#[derive(Debug, Clone)]
pub struct SessionImportFileNotFoundError {
    pub message: String,
}

impl std::fmt::Display for SessionImportFileNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SessionImportFileNotFoundError {}

impl SessionImportFileNotFoundError {
    pub fn new(file_path: &str) -> Self {
        todo!("port core/agent-session-runtime.ts:SessionImportFileNotFoundError.constructor")
    }

    pub fn file_path(&self) -> String {
        todo!("port core/agent-session-runtime.ts:SessionImportFileNotFoundError.filePath")
    }
}

#[derive(Clone)]
pub struct AgentSessionRuntimeOptions {
    pub cwd_override: Option<String>,
    pub with_session: Option<
        Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
    pub project_trust_context_factory:
        Option<Arc<dyn Fn(String) -> ProjectTrustContext + Send + Sync>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRuntimeSwitchSessionResult {
    pub cancelled: bool,
}

#[derive(Clone)]
pub struct AgentSessionRuntimeOptions2 {
    pub parent_session: Option<String>,
    pub setup: Option<
        Arc<dyn Fn(Arc<SessionManager>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
    pub with_session: Option<
        Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRuntimeNewSessionResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentSessionRuntimeOptions3Position {
    #[serde(rename = "before")]
    Before,
    #[serde(rename = "at")]
    At,
}

impl AgentSessionRuntimeOptions3Position {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::At => "at",
        }
    }
}

#[derive(Clone)]
pub struct AgentSessionRuntimeOptions3 {
    pub position: Option<AgentSessionRuntimeOptions3Position>,
    pub with_session: Option<
        Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
    >,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRuntimeForkResult {
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRuntimeImportFromJsonlResult {
    pub cancelled: bool,
}

/// Owns the current AgentSession plus its cwd-bound services.
///
/// Session replacement methods tear down the current runtime first, then create
/// and apply the next runtime. If creation fails, the error is propagated to the
/// caller. The caller is responsible for user-facing error handling.
pub struct AgentSessionRuntime {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl AgentSessionRuntime {
    pub fn new(
        session: Arc<AgentSession>,
        services: &AgentSessionServices,
        create_runtime: &CreateAgentSessionRuntimeFactory,
        diagnostics: Option<&[AgentSessionRuntimeDiagnostic]>,
        model_fallback_message: Option<&str>,
    ) -> Self {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.constructor")
    }

    pub fn services(&self) -> AgentSessionServices {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.services")
    }

    pub fn session(&self) -> Arc<AgentSession> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.session")
    }

    pub fn cwd(&self) -> String {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.cwd")
    }

    pub fn diagnostics(&self) -> Vec<AgentSessionRuntimeDiagnostic> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.diagnostics")
    }

    pub fn model_fallback_message(&self) -> Option<String> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.modelFallbackMessage")
    }

    pub fn set_rebind_session(
        &self,
        rebind_session: Option<
            Arc<dyn Fn(Arc<AgentSession>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
        >,
    ) {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.setRebindSession")
    }

    /// Set a synchronous callback that runs after `session_shutdown` handlers finish
    /// but before the current session is invalidated.
    ///
    /// This is for host-owned UI teardown that must not yield to the event loop,
    /// such as detaching extension-provided TUI components before the old extension
    /// context becomes stale.
    pub fn set_before_session_invalidate(
        &self,
        before_session_invalidate: Option<Arc<dyn Fn() + Send + Sync>>,
    ) {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.setBeforeSessionInvalidate")
    }

    pub async fn switch_session(
        &self,
        session_path: &str,
        options: Option<&AgentSessionRuntimeOptions>,
    ) -> anyhow::Result<AgentSessionRuntimeSwitchSessionResult> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.switchSession")
    }

    pub async fn new_session(
        &self,
        options: Option<&AgentSessionRuntimeOptions2>,
    ) -> anyhow::Result<AgentSessionRuntimeNewSessionResult> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.newSession")
    }

    pub async fn fork(
        &self,
        entry_id: &str,
        options: Option<&AgentSessionRuntimeOptions3>,
    ) -> anyhow::Result<AgentSessionRuntimeForkResult> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.fork")
    }

    /// Import a session JSONL file and switch runtime state to the imported session.
    ///
    /// @returns `{ cancelled: true }` when cancelled by `session_before_switch`, otherwise `{ cancelled: false }`.
    /// @throws {SessionImportFileNotFoundError} When the input path does not exist.
    /// @throws {MissingSessionCwdError} When the imported session cwd cannot be resolved and no override is provided.
    pub async fn import_from_jsonl(
        &self,
        input_path: &str,
        cwd_override: Option<&str>,
    ) -> anyhow::Result<AgentSessionRuntimeImportFromJsonlResult> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.importFromJsonl")
    }

    pub async fn dispose(&self) -> anyhow::Result<()> {
        todo!("port core/agent-session-runtime.ts:AgentSessionRuntime.dispose")
    }
}

#[derive(Clone)]
pub struct CreateAgentSessionRuntimeOptions {
    pub cwd: String,
    pub agent_dir: String,
    pub session_manager: Arc<SessionManager>,
    pub session_start_event: Option<SessionStartEvent>,
}

/// Create the initial runtime from a runtime factory and initial session target.
///
/// The same factory is stored on the returned AgentSessionRuntime and reused for
/// later /new, /resume, /fork, and import flows.
pub async fn create_agent_session_runtime(
    create_runtime: &CreateAgentSessionRuntimeFactory,
    options: &CreateAgentSessionRuntimeOptions,
) -> anyhow::Result<Arc<AgentSessionRuntime>> {
    todo!("port core/agent-session-runtime.ts:createAgentSessionRuntime")
}
