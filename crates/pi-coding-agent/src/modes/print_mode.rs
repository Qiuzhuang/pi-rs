//! Port of `packages/coding-agent/src/modes/print-mode.ts`.
//!
//! Print mode (single-shot): Send prompts, output result, exit.
//!
//! Used for:
//! - `pi -p "prompt"` - text output
//! - `pi --mode json "prompt"` - JSON event stream

use std::sync::Arc;

use parking_lot::Mutex;
use pi_agent::types::AgentMessage;
use pi_ai::types::{AssistantContent, ImageContent, StopReason};
use serde::{Deserialize, Serialize};

use crate::core::agent_session::{
    AgentSession, AgentSessionEventListener, AgentSessionNavigateTreeOptions, ExtensionBindings, PromptOptions,
};
use crate::core::agent_session_runtime::{
    AgentSessionRuntime, AgentSessionRuntimeForkOptions, AgentSessionRuntimeForkOptionsPosition,
    AgentSessionRuntimeNewSessionOptions, AgentSessionRuntimeSwitchSessionOptions,
};
use crate::core::extensions::types::{
    ExtensionCommandContextActions, ExtensionMode, ForkOptions, ForkPosition, NavigateTreeOptions,
    NewSessionOptions, SessionReplaceOutcome, SwitchSessionOptions,
};
use crate::core::output_guard::{flush_raw_stdout, wait_for_raw_stdout_backpressure, write_raw_stdout};
use crate::modes::json_event::to_json_event;
use crate::modes::rpc::jsonl::serialize_json_line;
use crate::utils::shell::kill_tracked_detached_children;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrintModeOptionsMode {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json")]
    Json,
}

impl PrintModeOptionsMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

/// Options for print mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintModeOptions {
    /// Output mode: "text" for final response only, "json" for all events
    pub mode: PrintModeOptionsMode,
    /// Array of additional prompts to send after initialMessage
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<String>>,
    /// First message to send (may contain @file content)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_message: Option<String>,
    /// Images to attach to the initial message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_images: Option<Vec<ImageContent>>,
}

/// Command-context actions for non-interactive modes (print, JSON, RPC): session replacement
/// goes through the runtime host, tree navigation and reload through the current session.
pub(crate) struct RuntimeCommandContextActions {
    pub runtime_host: Arc<AgentSessionRuntime>,
}

#[async_trait::async_trait]
impl ExtensionCommandContextActions for RuntimeCommandContextActions {
    async fn wait_for_idle(&self) -> anyhow::Result<()> {
        self.runtime_host.session().wait_for_idle().await
    }

    async fn new_session(&self, options: NewSessionOptions) -> anyhow::Result<SessionReplaceOutcome> {
        let result = self
            .runtime_host
            .new_session(Some(&AgentSessionRuntimeNewSessionOptions {
                parent_session: options.parent_session,
                setup: options.setup,
                with_session: options.with_session,
            }))
            .await?;
        Ok(SessionReplaceOutcome { cancelled: result.cancelled })
    }

    async fn fork(&self, entry_id: &str, options: ForkOptions) -> anyhow::Result<SessionReplaceOutcome> {
        let result = self
            .runtime_host
            .fork(
                entry_id,
                Some(&AgentSessionRuntimeForkOptions {
                    position: options.position.map(|position| match position {
                        ForkPosition::Before => AgentSessionRuntimeForkOptionsPosition::Before,
                        ForkPosition::At => AgentSessionRuntimeForkOptionsPosition::At,
                    }),
                    with_session: options.with_session,
                }),
            )
            .await?;
        Ok(SessionReplaceOutcome { cancelled: result.cancelled })
    }

    async fn navigate_tree(
        &self,
        target_id: &str,
        options: NavigateTreeOptions,
    ) -> anyhow::Result<SessionReplaceOutcome> {
        let result = self
            .runtime_host
            .session()
            .navigate_tree(
                target_id,
                Some(&AgentSessionNavigateTreeOptions {
                    summarize: options.summarize,
                    custom_instructions: options.custom_instructions,
                    replace_instructions: options.replace_instructions,
                    label: options.label,
                }),
            )
            .await?;
        Ok(SessionReplaceOutcome { cancelled: result.cancelled })
    }

    async fn switch_session(
        &self,
        session_path: &str,
        options: SwitchSessionOptions,
    ) -> anyhow::Result<SessionReplaceOutcome> {
        let result = self
            .runtime_host
            .switch_session(
                session_path,
                Some(&AgentSessionRuntimeSwitchSessionOptions {
                    cwd_override: None,
                    with_session: options.with_session,
                    project_trust_context_factory: None,
                }),
            )
            .await?;
        Ok(SessionReplaceOutcome { cancelled: result.cancelled })
    }

    async fn reload(&self) -> anyhow::Result<()> {
        self.runtime_host.session().reload(None).await
    }
}

/// Subscriptions owned by a non-interactive mode for the current session.
#[derive(Default)]
pub(crate) struct SessionSubscriptions {
    unsubscribe: Option<Arc<dyn Fn() + Send + Sync>>,
    backpressure: Option<pi_agent::agent::Subscription>,
}

impl SessionSubscriptions {
    pub(crate) fn clear(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe.take() {
            unsubscribe();
        }
        if let Some(subscription) = self.backpressure.take() {
            subscription.unsubscribe();
        }
    }

    pub(crate) fn set(
        &mut self,
        unsubscribe: Arc<dyn Fn() + Send + Sync>,
        backpressure: Option<pi_agent::agent::Subscription>,
    ) {
        self.clear();
        self.unsubscribe = Some(unsubscribe);
        self.backpressure = backpressure;
    }
}

/// An agent listener that waits for stdout to drain, so a slow reader slows the agent down
/// instead of buffering unbounded output.
pub(crate) fn subscribe_backpressure(session: &AgentSession) -> pi_agent::agent::Subscription {
    session.agent().subscribe(Arc::new(|_event, _signal| {
        Box::pin(async {
            let _ = wait_for_raw_stdout_backpressure().await;
        })
    }))
}

fn stop_reason_name(reason: StopReason) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

struct PrintModeState {
    runtime_host: Arc<AgentSessionRuntime>,
    mode: PrintModeOptionsMode,
    subscriptions: Mutex<SessionSubscriptions>,
    disposed: Mutex<bool>,
}

impl PrintModeState {
    async fn dispose_runtime(&self) {
        {
            let mut disposed = self.disposed.lock();
            if *disposed {
                return;
            }
            *disposed = true;
        }
        self.subscriptions.lock().clear();
        let _ = self.runtime_host.dispose().await;
    }

    async fn rebind_session(self: &Arc<Self>) -> anyhow::Result<()> {
        let session = self.runtime_host.session();
        session
            .bind_extensions(&ExtensionBindings {
                ui_context: None,
                mode: Some(if self.mode == PrintModeOptionsMode::Json {
                    ExtensionMode::Json
                } else {
                    ExtensionMode::Print
                }),
                command_context_actions: Some(Arc::new(RuntimeCommandContextActions {
                    runtime_host: self.runtime_host.clone(),
                })),
                abort_handler: None,
                shutdown_handler: None,
                on_error: Some(Arc::new(|err| {
                    eprintln!("Extension error ({}): {}", err.extension_path, err.error);
                })),
            })
            .await?;

        let json = self.mode == PrintModeOptionsMode::Json;
        let listener: AgentSessionEventListener = Arc::new(move |event| {
            if json && let Ok(wire) = to_json_event(&event) {
                write_raw_stdout(&serialize_json_line(&wire));
            }
        });
        let unsubscribe = session.subscribe(&listener);
        let backpressure = json.then(|| subscribe_backpressure(&session));
        self.subscriptions.lock().set(unsubscribe, backpressure);
        Ok(())
    }
}

/// Register SIGTERM (and SIGHUP on Unix) handlers that kill detached children, dispose the
/// runtime and exit with 143 / 129. Returns the handler task (abort it to unregister).
fn register_signal_handlers(state: Arc<PrintModeState>) -> tokio::task::JoinHandle<()> {
    crate::utils::uncaught::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut hup)) = (signal(SignalKind::terminate()), signal(SignalKind::hangup())) else {
                return;
            };
            let code = tokio::select! {
                _ = term.recv() => 143,
                _ = hup.recv() => 129,
            };
            kill_tracked_detached_children();
            state.dispose_runtime().await;
            crate::utils::process_exit::exit(code);
        }
        #[cfg(not(unix))]
        {
            if tokio::signal::ctrl_c().await.is_ok() {
                kill_tracked_detached_children();
                state.dispose_runtime().await;
                crate::utils::process_exit::exit(143);
            }
        }
    })
}

/// Run in print (single-shot) mode.
/// Sends prompts to the agent and outputs the result. Returns the process exit code.
pub async fn run_print_mode(runtime_host: Arc<AgentSessionRuntime>, options: &PrintModeOptions) -> anyhow::Result<i32> {
    let mode = options.mode;
    let messages = options.messages.clone().unwrap_or_default();
    let state = Arc::new(PrintModeState {
        runtime_host: runtime_host.clone(),
        mode,
        subscriptions: Mutex::new(SessionSubscriptions::default()),
        disposed: Mutex::new(false),
    });

    let signal_task = register_signal_handlers(state.clone());

    {
        let weak = Arc::downgrade(&state);
        runtime_host.set_rebind_session(Some(Arc::new(move |_session| {
            let weak = weak.clone();
            Box::pin(async move {
                match weak.upgrade() {
                    Some(state) => state.rebind_session().await,
                    None => Ok(()),
                }
            })
        })));
    }

    let result: anyhow::Result<i32> = async {
        let mut exit_code = 0;
        if mode == PrintModeOptionsMode::Json
            && let Some(header) = runtime_host.session().session_manager().get_header()
        {
            // The header as stored in the session file: `{ type: "session", ... }`.
            let mut object = serde_json::Map::new();
            object.insert("type".to_string(), serde_json::Value::String("session".to_string()));
            if let serde_json::Value::Object(fields) = serde_json::to_value(&header)? {
                object.extend(fields);
            }
            write_raw_stdout(&serialize_json_line(&object));
        }

        state.rebind_session().await?;

        if let Some(initial_message) = options.initial_message.as_deref().filter(|m| !m.is_empty()) {
            runtime_host
                .session()
                .prompt(
                    initial_message,
                    Some(&PromptOptions { images: options.initial_images.clone(), ..Default::default() }),
                )
                .await?;
        }

        for message in &messages {
            runtime_host.session().prompt(message, None).await?;
        }

        if mode == PrintModeOptionsMode::Text {
            let session_state = runtime_host.session().state();
            if let Some(AgentMessage::Assistant(assistant)) = session_state.messages.last() {
                if matches!(assistant.stop_reason, StopReason::Error | StopReason::Aborted) {
                    let message = assistant
                        .error_message
                        .clone()
                        .filter(|m| !m.is_empty())
                        .unwrap_or_else(|| format!("Request {}", stop_reason_name(assistant.stop_reason)));
                    eprintln!("{message}");
                    exit_code = 1;
                } else {
                    for content in &assistant.content {
                        if let AssistantContent::Text(text) = content {
                            write_raw_stdout(&format!("{}\n", text.text));
                        }
                    }
                    if assistant.stop_reason == StopReason::Length {
                        eprintln!("Response was truncated: the model reached its output token limit.");
                        exit_code = 1;
                    }
                }
            }
        }
        Ok(exit_code)
    }
    .await;

    let exit_code = match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };
    signal_task.abort();
    state.dispose_runtime().await;
    let _ = flush_raw_stdout().await;
    Ok(exit_code)
}
