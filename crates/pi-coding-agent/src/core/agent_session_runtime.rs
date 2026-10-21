//! Port of `packages/coding-agent/src/core/agent-session-runtime.ts`.

use crate::core::agent_session::AgentSession;
use crate::core::extensions::runner::emit_session_shutdown_event;
use crate::core::extensions::types::{
    ExtensionEvent, ForkPosition, SessionBeforeForkEvent, SessionBeforeSwitchEvent,
    SessionShutdownEvent, SessionShutdownReason, SessionStartReason, SessionSwitchReason,
};
use crate::core::session_cwd::{SessionCwdSource, assert_session_cwd_exists};
use crate::core::session_manager::{NewSessionOptions, SessionEntry};
use crate::utils::paths::resolve_path;
use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::types::UserContent;
use std::path::Path;
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
    file_path: String,
}

impl std::fmt::Display for SessionImportFileNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SessionImportFileNotFoundError {}

impl SessionImportFileNotFoundError {
    pub fn new(file_path: &str) -> Self {
        Self {
            message: format!("File not found: {file_path}"),
            file_path: file_path.to_string(),
        }
    }

    pub fn file_path(&self) -> String {
        self.file_path.clone()
    }
}

#[derive(Clone)]
pub struct AgentSessionRuntimeSwitchSessionOptions {
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
pub struct AgentSessionRuntimeNewSessionOptions {
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
pub enum AgentSessionRuntimeForkOptionsPosition {
    #[serde(rename = "before")]
    Before,
    #[serde(rename = "at")]
    At,
}

impl AgentSessionRuntimeForkOptionsPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::At => "at",
        }
    }
}

#[derive(Clone)]
pub struct AgentSessionRuntimeForkOptions {
    pub position: Option<AgentSessionRuntimeForkOptionsPosition>,
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

type RebindSessionFn =
    Arc<dyn Fn(Arc<AgentSession>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;
type WithSessionFn =
    Arc<dyn Fn(ReplacedSessionContext) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

struct RuntimeState {
    session: Arc<AgentSession>,
    services: AgentSessionServices,
    diagnostics: Vec<AgentSessionRuntimeDiagnostic>,
    model_fallback_message: Option<String>,
    rebind_session: Option<RebindSessionFn>,
    before_session_invalidate: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// [`SessionCwdSource`] view of a session manager.
struct ManagerCwdSource<'a>(&'a SessionManager);

impl SessionCwdSource for ManagerCwdSource<'_> {
    fn get_cwd(&self) -> String {
        self.0.get_cwd()
    }
    fn get_session_file(&self) -> Option<String> {
        self.0.get_session_file()
    }
}

fn extract_user_message_text(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| b.as_text().map(|t| t.text.as_str()))
            .collect::<Vec<_>>()
            .join(""),
    }
}

/// Owns the current AgentSession plus its cwd-bound services.
///
/// Session replacement methods tear down the current runtime first, then create
/// and apply the next runtime. If creation fails, the error is propagated to the
/// caller. The caller is responsible for user-facing error handling.
pub struct AgentSessionRuntime {
    create_runtime: CreateAgentSessionRuntimeFactory,
    state: Mutex<RuntimeState>,
}

impl AgentSessionRuntime {
    pub fn new(
        session: Arc<AgentSession>,
        services: &AgentSessionServices,
        create_runtime: &CreateAgentSessionRuntimeFactory,
        diagnostics: Option<&[AgentSessionRuntimeDiagnostic]>,
        model_fallback_message: Option<&str>,
    ) -> Self {
        Self {
            create_runtime: create_runtime.clone(),
            state: Mutex::new(RuntimeState {
                session,
                services: services.clone(),
                diagnostics: diagnostics.map(<[_]>::to_vec).unwrap_or_default(),
                model_fallback_message: model_fallback_message.map(str::to_string),
                rebind_session: None,
                before_session_invalidate: None,
            }),
        }
    }

    pub fn services(&self) -> AgentSessionServices {
        self.state.lock().services.clone()
    }

    pub fn session(&self) -> Arc<AgentSession> {
        self.state.lock().session.clone()
    }

    pub fn cwd(&self) -> String {
        self.state.lock().services.cwd.clone()
    }

    pub fn diagnostics(&self) -> Vec<AgentSessionRuntimeDiagnostic> {
        self.state.lock().diagnostics.clone()
    }

    pub fn model_fallback_message(&self) -> Option<String> {
        self.state.lock().model_fallback_message.clone()
    }

    pub fn set_rebind_session(&self, rebind_session: Option<RebindSessionFn>) {
        self.state.lock().rebind_session = rebind_session;
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
        self.state.lock().before_session_invalidate = before_session_invalidate;
    }

    async fn emit_before_switch(
        &self,
        reason: SessionSwitchReason,
        target_session_file: Option<String>,
    ) -> anyhow::Result<bool> {
        let runner = self.session().extension_runner();
        if !runner.has_handlers("session_before_switch") {
            return Ok(false);
        }
        let result = runner
            .emit(ExtensionEvent::SessionBeforeSwitch(SessionBeforeSwitchEvent {
                reason,
                target_session_file,
            }))
            .await?;
        Ok(result.is_some_and(|r| r.cancel()))
    }

    async fn emit_before_fork(&self, entry_id: &str, position: ForkPosition) -> anyhow::Result<bool> {
        let runner = self.session().extension_runner();
        if !runner.has_handlers("session_before_fork") {
            return Ok(false);
        }
        let result = runner
            .emit(ExtensionEvent::SessionBeforeFork(SessionBeforeForkEvent {
                entry_id: entry_id.to_string(),
                position,
            }))
            .await?;
        Ok(result.is_some_and(|r| r.cancel()))
    }

    async fn teardown_current(
        &self,
        reason: SessionShutdownReason,
        target_session_file: Option<String>,
    ) -> anyhow::Result<()> {
        let session = self.session();
        // Settle any active response first so the aborted turn (including tool
        // results) is persisted to the outgoing session before it is replaced.
        session.abort().await?;
        emit_session_shutdown_event(
            session.extension_runner(),
            &SessionShutdownEvent {
                reason,
                target_session_file,
            },
        )
        .await?;
        let before = self.state.lock().before_session_invalidate.clone();
        if let Some(before) = before {
            before();
        }
        session.dispose();
        Ok(())
    }

    fn apply(&self, result: CreateAgentSessionRuntimeResult) {
        let mut state = self.state.lock();
        state.session = result.session;
        state.services = result.services;
        state.diagnostics = result.diagnostics;
        state.model_fallback_message = result.model_fallback_message;
    }

    async fn finish_session_replacement(
        &self,
        with_session: Option<WithSessionFn>,
    ) -> anyhow::Result<()> {
        let rebind = self.state.lock().rebind_session.clone();
        if let Some(rebind) = rebind {
            rebind(self.session()).await?;
        }
        if let Some(with_session) = with_session {
            with_session(self.session().create_replaced_session_context()).await?;
        }
        Ok(())
    }

    async fn create(
        &self,
        cwd: String,
        session_manager: Arc<SessionManager>,
        reason: SessionStartReason,
        previous_session_file: Option<String>,
        project_trust_context: Option<ProjectTrustContext>,
    ) -> anyhow::Result<CreateAgentSessionRuntimeResult> {
        (self.create_runtime)(CreateAgentSessionRuntimeFactoryOptions {
            cwd,
            agent_dir: self.services().agent_dir,
            session_manager,
            session_start_event: Some(SessionStartEvent {
                reason,
                previous_session_file,
            }),
            project_trust_context,
        })
        .await
    }

    pub async fn switch_session(
        &self,
        session_path: &str,
        options: Option<&AgentSessionRuntimeSwitchSessionOptions>,
    ) -> anyhow::Result<AgentSessionRuntimeSwitchSessionResult> {
        if self
            .emit_before_switch(SessionSwitchReason::Resume, Some(session_path.to_string()))
            .await?
        {
            return Ok(AgentSessionRuntimeSwitchSessionResult { cancelled: true });
        }
        let previous_session_file = self.session().session_file();
        let session_manager = SessionManager::open(
            session_path,
            None,
            options.and_then(|o| o.cwd_override.as_deref()),
        )?;
        assert_session_cwd_exists(&ManagerCwdSource(&session_manager), &self.cwd())?;
        self.teardown_current(SessionShutdownReason::Resume, session_manager.get_session_file())
            .await?;
        let cwd = session_manager.get_cwd();
        let project_trust_context = options
            .and_then(|o| o.project_trust_context_factory.as_ref())
            .map(|factory| factory(cwd.clone()));
        let result = self
            .create(
                cwd,
                session_manager,
                SessionStartReason::Resume,
                previous_session_file,
                project_trust_context,
            )
            .await?;
        self.apply(result);
        self.finish_session_replacement(options.and_then(|o| o.with_session.clone()))
            .await?;
        Ok(AgentSessionRuntimeSwitchSessionResult { cancelled: false })
    }

    pub async fn new_session(
        &self,
        options: Option<&AgentSessionRuntimeNewSessionOptions>,
    ) -> anyhow::Result<AgentSessionRuntimeNewSessionResult> {
        if self
            .emit_before_switch(SessionSwitchReason::New, None)
            .await?
        {
            return Ok(AgentSessionRuntimeNewSessionResult { cancelled: true });
        }
        let session = self.session();
        let previous_session_file = session.session_file();
        let current_manager = session.session_manager();
        let session_dir = current_manager.get_session_dir();
        let cwd = self.cwd();
        let session_manager = if current_manager.is_persisted() {
            SessionManager::create(&cwd, Some(&session_dir), None)?
        } else {
            SessionManager::in_memory(Some(&cwd), None, None)?
        };
        if let Some(parent) = options.and_then(|o| o.parent_session.clone()) {
            session_manager.new_session(Some(&NewSessionOptions {
                id: None,
                parent_session: Some(parent),
            }))?;
        }
        self.teardown_current(SessionShutdownReason::New, session_manager.get_session_file())
            .await?;
        let result = self
            .create(
                cwd,
                session_manager,
                SessionStartReason::New,
                previous_session_file,
                None,
            )
            .await?;
        self.apply(result);
        if let Some(setup) = options.and_then(|o| o.setup.clone()) {
            let session = self.session();
            setup(session.session_manager()).await?;
            session.refresh_context();
        }
        self.finish_session_replacement(options.and_then(|o| o.with_session.clone()))
            .await?;
        Ok(AgentSessionRuntimeNewSessionResult { cancelled: false })
    }

    pub async fn fork(
        &self,
        entry_id: &str,
        options: Option<&AgentSessionRuntimeForkOptions>,
    ) -> anyhow::Result<AgentSessionRuntimeForkResult> {
        let position = options
            .and_then(|o| o.position)
            .unwrap_or(AgentSessionRuntimeForkOptionsPosition::Before);
        let fork_position = match position {
            AgentSessionRuntimeForkOptionsPosition::Before => ForkPosition::Before,
            AgentSessionRuntimeForkOptionsPosition::At => ForkPosition::At,
        };
        if self.emit_before_fork(entry_id, fork_position).await? {
            return Ok(AgentSessionRuntimeForkResult {
                cancelled: true,
                selected_text: None,
            });
        }
        let with_session = options.and_then(|o| o.with_session.clone());
        let session = self.session();
        let current_manager = session.session_manager();
        let Some(selected_entry) = current_manager.get_entry(entry_id) else {
            anyhow::bail!("Invalid entry ID for forking");
        };

        let (target_leaf_id, selected_text) = match position {
            AgentSessionRuntimeForkOptionsPosition::At => (Some(selected_entry.id().to_string()), None),
            AgentSessionRuntimeForkOptionsPosition::Before => match &selected_entry {
                SessionEntry::Message(e) => match &e.message {
                    AgentMessage::User(user) => (
                        e.parent_id.clone(),
                        Some(extract_user_message_text(&user.content)),
                    ),
                    _ => anyhow::bail!("Invalid entry ID for forking"),
                },
                _ => anyhow::bail!("Invalid entry ID for forking"),
            },
        };

        let previous_session_file = session.session_file();
        let cwd = self.cwd();
        if current_manager.is_persisted() {
            let Some(current_session_file) = session.session_file() else {
                anyhow::bail!("Persisted session is missing a session file");
            };
            let session_dir = current_manager.get_session_dir();
            let Some(target_leaf_id) = target_leaf_id else {
                let session_manager = SessionManager::create(&cwd, Some(&session_dir), None)?;
                session_manager.new_session(Some(&NewSessionOptions {
                    id: None,
                    parent_session: Some(current_session_file),
                }))?;
                self.teardown_current(SessionShutdownReason::Fork, session_manager.get_session_file())
                    .await?;
                let result = self
                    .create(
                        cwd,
                        session_manager,
                        SessionStartReason::Fork,
                        previous_session_file,
                        None,
                    )
                    .await?;
                self.apply(result);
                self.finish_session_replacement(with_session).await?;
                return Ok(AgentSessionRuntimeForkResult {
                    cancelled: false,
                    selected_text,
                });
            };

            if !Path::new(&current_session_file).exists() {
                anyhow::bail!(
                    "This session has not been saved yet. Send a message before cloning or forking it."
                );
            }
            let session_manager =
                SessionManager::open(&current_session_file, Some(&session_dir), None)?;
            if session_manager
                .create_branched_session(&target_leaf_id)?
                .is_none()
            {
                anyhow::bail!("Failed to create forked session");
            }
            self.teardown_current(SessionShutdownReason::Fork, session_manager.get_session_file())
                .await?;
            let result = self
                .create(
                    session_manager.get_cwd(),
                    session_manager.clone(),
                    SessionStartReason::Fork,
                    previous_session_file,
                    None,
                )
                .await?;
            self.apply(result);
            self.finish_session_replacement(with_session).await?;
            return Ok(AgentSessionRuntimeForkResult {
                cancelled: false,
                selected_text,
            });
        }

        let session_manager = current_manager;
        self.teardown_current(SessionShutdownReason::Fork, session_manager.get_session_file())
            .await?;
        match &target_leaf_id {
            None => {
                session_manager.new_session(Some(&NewSessionOptions {
                    id: None,
                    parent_session: previous_session_file.clone(),
                }))?;
            }
            Some(leaf) => {
                session_manager.create_branched_session(leaf)?;
            }
        }
        let result = self
            .create(
                cwd,
                session_manager,
                SessionStartReason::Fork,
                previous_session_file,
                None,
            )
            .await?;
        self.apply(result);
        self.finish_session_replacement(with_session).await?;
        Ok(AgentSessionRuntimeForkResult {
            cancelled: false,
            selected_text,
        })
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
        let resolved_path = resolve_path(input_path, None, None);
        if !Path::new(&resolved_path).exists() {
            return Err(SessionImportFileNotFoundError::new(&resolved_path).into());
        }
        let session = self.session();
        let session_dir = session.session_manager().get_session_dir();
        if !Path::new(&session_dir).exists() {
            std::fs::create_dir_all(&session_dir)?;
        }
        let file_name = Path::new(&resolved_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut destination = Path::new(&session_dir).join(&file_name);
        let source_already_stored = std::path::absolute(&destination)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
            == resolved_path;
        if !source_already_stored {
            let stem = destination
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = destination
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let mut suffix = 1;
            while destination.exists() {
                destination = Path::new(&session_dir).join(format!("{stem}-{suffix}{ext}"));
                suffix += 1;
            }
        }
        let destination_path = destination.to_string_lossy().into_owned();
        if self
            .emit_before_switch(SessionSwitchReason::Resume, Some(destination_path.clone()))
            .await?
        {
            return Ok(AgentSessionRuntimeImportFromJsonlResult { cancelled: true });
        }
        let previous_session_file = session.session_file();
        if !source_already_stored {
            // COPYFILE_EXCL: never overwrite an existing file.
            let mut source = std::fs::File::open(&resolved_path)?;
            let mut target = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            std::io::copy(&mut source, &mut target)?;
        }
        let session_manager =
            SessionManager::open(&destination_path, Some(&session_dir), cwd_override)?;
        assert_session_cwd_exists(&ManagerCwdSource(&session_manager), &self.cwd())?;
        self.teardown_current(SessionShutdownReason::Resume, session_manager.get_session_file())
            .await?;
        let result = self
            .create(
                session_manager.get_cwd(),
                session_manager,
                SessionStartReason::Resume,
                previous_session_file,
                None,
            )
            .await?;
        self.apply(result);
        self.finish_session_replacement(None).await?;
        Ok(AgentSessionRuntimeImportFromJsonlResult { cancelled: false })
    }

    pub async fn dispose(&self) -> anyhow::Result<()> {
        let session = self.session();
        emit_session_shutdown_event(
            session.extension_runner(),
            &SessionShutdownEvent {
                reason: SessionShutdownReason::Quit,
                target_session_file: None,
            },
        )
        .await?;
        let before = self.state.lock().before_session_invalidate.clone();
        if let Some(before) = before {
            before();
        }
        session.dispose();
        Ok(())
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
    assert_session_cwd_exists(&ManagerCwdSource(&options.session_manager), &options.cwd)?;
    let result = create_runtime(CreateAgentSessionRuntimeFactoryOptions {
        cwd: options.cwd.clone(),
        agent_dir: options.agent_dir.clone(),
        session_manager: options.session_manager.clone(),
        session_start_event: options.session_start_event.clone(),
        project_trust_context: None,
    })
    .await?;
    Ok(Arc::new(AgentSessionRuntime::new(
        result.session,
        &result.services,
        create_runtime,
        Some(&result.diagnostics),
        result.model_fallback_message.as_deref(),
    )))
}
