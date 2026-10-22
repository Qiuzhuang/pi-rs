//! Port of `packages/coding-agent/src/modes/rpc/rpc-mode.ts`.
//!
//! RPC mode: Headless operation with JSON stdin/stdout protocol.
//!
//! Used for embedding the agent in other applications.
//! Receives commands as JSON on stdin, outputs events and responses as JSON on stdout.
//!
//! Protocol:
//! - Commands: JSON objects with `type` field, optional `id` for correlation
//! - Responses: JSON objects with `type: "response"`, `command`, `success`, and optional `data`/`error`
//! - Events: AgentSessionEvent objects streamed as they occur
//! - Extension UI: Extension UI requests are emitted, client responds with extension_ui_response

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt;
use futures::future::LocalBoxFuture;
use futures::stream::FuturesUnordered;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::io::AsyncRead;
use tokio::sync::{Notify, oneshot};

use crate::core::agent_session::{
    AgentSessionEvent, AgentSessionEventListener, AgentSessionExecuteBashOptions, AgentSessionFollowUpOptions,
    AgentSessionRecordBashResultOptions, AgentSessionSteerOptions, AgentSessionSteeringMode, ExtensionBindings,
    PromptOptions, PromptOptionsStreamingBehavior,
};
use crate::core::agent_session_runtime::{
    AgentSessionRuntime, AgentSessionRuntimeForkOptions, AgentSessionRuntimeForkOptionsPosition,
    AgentSessionRuntimeNewSessionOptions,
};
use crate::core::extensions::types::*;
use crate::core::output_guard::{flush_raw_stdout, take_over_stdout, wait_for_raw_stdout_backpressure, write_raw_stdout};
use crate::core::slash_commands::SlashCommandSource;
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::json_event::to_json_event;
use crate::modes::print_mode::{RuntimeCommandContextActions, SessionSubscriptions, subscribe_backpressure};
use crate::modes::rpc::jsonl::{JsonlLineReader, serialize_json_line};
use crate::utils::shell::kill_tracked_detached_children;

pub use crate::modes::rpc::rpc_types::RpcCommand;
pub use crate::modes::rpc::rpc_types::RpcExtensionUIRequest;
pub use crate::modes::rpc::rpc_types::RpcExtensionUIResponse;
pub use crate::modes::rpc::rpc_types::RpcResponse;
pub use crate::modes::rpc::rpc_types::RpcSessionState;
use crate::modes::rpc::rpc_types::{
    ExtensionUIRequestTag, RpcCommandKind, RpcExtensionUIRequestMethod, RpcExtensionUIResponseValue, RpcQueueMode,
    RpcSlashCommand,
};

/// The `type` values of [`RpcCommandKind`].
const KNOWN_COMMAND_TYPES: [&str; 33] = [
    "prompt",
    "steer",
    "follow_up",
    "abort",
    "clear_queue",
    "new_session",
    "get_state",
    "set_model",
    "cycle_model",
    "get_available_models",
    "set_thinking_level",
    "cycle_thinking_level",
    "get_available_thinking_levels",
    "set_steering_mode",
    "set_follow_up_mode",
    "compact",
    "set_auto_compaction",
    "set_auto_retry",
    "abort_retry",
    "bash",
    "abort_bash",
    "get_session_stats",
    "export_html",
    "switch_session",
    "fork",
    "clone",
    "get_fork_messages",
    "get_entries",
    "get_tree",
    "get_last_assistant_text",
    "set_session_name",
    "get_messages",
    "get_commands",
];

/// Writes one output record (already serialized as a JSONL line).
pub type RpcOutput = Arc<dyn Fn(String) + Send + Sync>;

type PendingRequests = Arc<Mutex<HashMap<String, oneshot::Sender<RpcExtensionUIResponse>>>>;

fn output_value<T: Serialize + ?Sized>(output: &RpcOutput, value: &T) {
    output(serialize_json_line(value));
}

fn success(id: Option<String>, command: &str, data: Option<Value>) -> RpcResponse {
    RpcResponse { id, command: command.to_string(), result: Ok(data) }
}

fn success_data<T: Serialize>(id: Option<String>, command: &str, data: &T) -> RpcResponse {
    match serde_json::to_value(data) {
        Ok(value) => success(id, command, Some(value)),
        Err(error) => RpcResponse::error(id, command, error.to_string()),
    }
}

// ============================================================================
// Extension UI context
// ============================================================================

/// Extension UI context that uses the RPC protocol: dialogs are emitted as
/// `extension_ui_request` lines and resolved by `extension_ui_response` lines.
pub struct RpcExtensionUIContext {
    output: RpcOutput,
    pending: PendingRequests,
}

impl RpcExtensionUIContext {
    fn emit(&self, id: String, request: RpcExtensionUIRequestMethod) {
        output_value(
            &self.output,
            &RpcExtensionUIRequest { tag: ExtensionUIRequestTag::ExtensionUiRequest, id, request },
        );
    }

    fn new_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Helper for dialog methods with signal/timeout support.
    async fn dialog<T>(
        &self,
        opts: &ExtensionUIDialogOptions,
        default_value: T,
        request: RpcExtensionUIRequestMethod,
        parse_response: impl FnOnce(RpcExtensionUIResponse) -> T,
    ) -> T {
        if opts.signal.as_ref().is_some_and(|s| s.is_cancelled()) {
            return default_value;
        }
        let id = Self::new_id();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id.clone(), tx);
        self.emit(id.clone(), request);

        let signal = opts.signal.clone();
        let timeout = opts.timeout.filter(|t| *t > 0);
        let result = tokio::select! {
            response = rx => response.ok(),
            _ = async { match &signal { Some(s) => s.cancelled().await, None => std::future::pending().await } } => None,
            _ = async { match timeout { Some(ms) => tokio::time::sleep(Duration::from_millis(ms)).await, None => std::future::pending().await } } => None,
        };
        self.pending.lock().remove(&id);
        match result {
            Some(response) => parse_response(response),
            None => default_value,
        }
    }
}

fn response_value(response: &RpcExtensionUIResponse) -> Option<String> {
    match &response.response {
        RpcExtensionUIResponseValue::Cancelled { cancelled: true } => None,
        RpcExtensionUIResponseValue::Value { value } => Some(value.clone()),
        _ => None,
    }
}

#[async_trait::async_trait]
impl ExtensionUIContext for RpcExtensionUIContext {
    async fn select(&self, title: &str, options: Vec<String>, opts: ExtensionUIDialogOptions) -> Option<String> {
        let request = RpcExtensionUIRequestMethod::Select { title: title.to_string(), options, timeout: opts.timeout };
        self.dialog(&opts, None, request, |r| response_value(&r)).await
    }

    async fn confirm(&self, title: &str, message: &str, opts: ExtensionUIDialogOptions) -> bool {
        let request = RpcExtensionUIRequestMethod::Confirm {
            title: title.to_string(),
            message: message.to_string(),
            timeout: opts.timeout,
        };
        self.dialog(&opts, false, request, |r| match r.response {
            RpcExtensionUIResponseValue::Cancelled { cancelled: true } => false,
            RpcExtensionUIResponseValue::Confirmed { confirmed } => confirmed,
            _ => false,
        })
        .await
    }

    async fn input(&self, title: &str, placeholder: Option<&str>, opts: ExtensionUIDialogOptions) -> Option<String> {
        let request = RpcExtensionUIRequestMethod::Input {
            title: title.to_string(),
            placeholder: placeholder.map(str::to_string),
            timeout: opts.timeout,
        };
        self.dialog(&opts, None, request, |r| response_value(&r)).await
    }

    fn notify(&self, message: &str, kind: NotifyType) {
        // Fire and forget - no response needed
        self.emit(
            Self::new_id(),
            RpcExtensionUIRequestMethod::Notify { message: message.to_string(), notify_type: Some(kind) },
        );
    }

    fn on_terminal_input(&self, _handler: TerminalInputHandler) -> Unsubscribe {
        // Raw terminal input not supported in RPC mode
        Arc::new(|| {})
    }

    fn set_status(&self, key: &str, text: Option<&str>) {
        // Fire and forget - no response needed
        self.emit(
            Self::new_id(),
            RpcExtensionUIRequestMethod::SetStatus { status_key: key.to_string(), status_text: text.map(str::to_string) },
        );
    }

    fn set_working_message(&self, _message: Option<&str>) {
        // Working message not supported in RPC mode - requires TUI loader access
    }

    fn set_working_visible(&self, _visible: bool) {
        // Working visibility not supported in RPC mode - requires TUI loader access
    }

    fn set_working_indicator(&self, _options: Option<WorkingIndicatorOptions>) {
        // Working indicator customization not supported in RPC mode - requires TUI loader access
    }

    fn set_hidden_thinking_label(&self, _label: Option<&str>) {
        // Hidden thinking label not supported in RPC mode - requires TUI message rendering access
    }

    fn set_widget(&self, key: &str, content: Option<WidgetContent>, options: ExtensionWidgetOptions) {
        // Only support string arrays in RPC mode - factory functions are ignored
        let lines = match content {
            None => None,
            Some(WidgetContent::Lines(lines)) => Some(lines),
            // Component factories are not supported in RPC mode - would need TUI access
            Some(WidgetContent::Component(_)) => return,
        };
        self.emit(
            Self::new_id(),
            RpcExtensionUIRequestMethod::SetWidget {
                widget_key: key.to_string(),
                widget_lines: lines,
                widget_placement: options.placement,
            },
        );
    }

    fn set_footer(&self, _factory: Option<FooterFactory>) {
        // Custom footer not supported in RPC mode - requires TUI access
    }

    fn set_header(&self, _factory: Option<ComponentFactory>) {
        // Custom header not supported in RPC mode - requires TUI access
    }

    fn set_title(&self, title: &str) {
        // Fire and forget - host can implement terminal title control
        self.emit(Self::new_id(), RpcExtensionUIRequestMethod::SetTitle { title: title.to_string() });
    }

    async fn custom(&self, _factory: CustomComponentFactory, _options: CustomUiOptions) -> anyhow::Result<CustomUiResult> {
        // Custom UI not supported in RPC mode (TS resolves `undefined`)
        Ok(Box::new(()))
    }

    fn paste_to_editor(&self, text: &str) {
        // Paste handling not supported in RPC mode - falls back to setEditorText
        self.set_editor_text(text);
    }

    fn set_editor_text(&self, text: &str) {
        // Fire and forget - host can implement editor control
        self.emit(Self::new_id(), RpcExtensionUIRequestMethod::SetEditorText { text: text.to_string() });
    }

    fn get_editor_text(&self) -> String {
        // Synchronous method can't wait for RPC response
        // Host should track editor state locally if needed
        String::new()
    }

    async fn editor(&self, title: &str, prefill: Option<&str>) -> Option<String> {
        let id = Self::new_id();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id.clone(), tx);
        self.emit(
            id,
            RpcExtensionUIRequestMethod::Editor { title: title.to_string(), prefill: prefill.map(str::to_string) },
        );
        match rx.await {
            Ok(response) => response_value(&response),
            Err(_) => None,
        }
    }

    fn add_autocomplete_provider(&self, _factory: AutocompleteProviderFactory) {
        // Autocomplete provider composition is not supported in RPC mode
    }

    fn set_editor_component(&self, _factory: Option<EditorFactory>) {
        // Custom editor components not supported in RPC mode
    }

    fn get_editor_component(&self) -> Option<EditorFactory> {
        // Custom editor components not supported in RPC mode
        None
    }

    fn theme(&self) -> Arc<Theme> {
        crate::modes::interactive::theme::theme::theme()
    }

    fn get_all_themes(&self) -> Vec<ThemeInfo> {
        Vec::new()
    }

    fn get_theme(&self, _name: &str) -> Option<Arc<Theme>> {
        None
    }

    fn set_theme(&self, _theme: ThemeSelection) -> SetThemeResult {
        // Theme switching not supported in RPC mode
        SetThemeResult { success: false, error: Some("Theme switching not supported in RPC mode".to_string()) }
    }

    fn get_tools_expanded(&self) -> bool {
        // Tool expansion not supported in RPC mode - no TUI
        false
    }

    fn set_tools_expanded(&self, _expanded: bool) {
        // Tool expansion not supported in RPC mode - no TUI
    }
}

// ============================================================================
// Mode state
// ============================================================================

struct RpcModeState {
    runtime_host: Arc<AgentSessionRuntime>,
    output: RpcOutput,
    pending: PendingRequests,
    subscriptions: Mutex<SessionSubscriptions>,
    shutdown_requested: AtomicBool,
    /// Wakes the main loop to check `shutdown_requested` (after `agent_settled`).
    shutdown_check: Notify,
}

impl RpcModeState {
    fn output<T: Serialize + ?Sized>(&self, value: &T) {
        output_value(&self.output, value);
    }

    fn ui_context(&self) -> Arc<dyn ExtensionUIContext> {
        Arc::new(RpcExtensionUIContext { output: self.output.clone(), pending: self.pending.clone() })
    }

    async fn rebind_session(self: &Arc<Self>) -> anyhow::Result<()> {
        let session = self.runtime_host.session();
        let weak = Arc::downgrade(self);
        let output = self.output.clone();
        session
            .bind_extensions(&ExtensionBindings {
                ui_context: Some(self.ui_context()),
                mode: Some(ExtensionMode::Rpc),
                command_context_actions: Some(Arc::new(RuntimeCommandContextActions {
                    runtime_host: self.runtime_host.clone(),
                })),
                abort_handler: None,
                shutdown_handler: Some(Arc::new({
                    let weak = weak.clone();
                    move || {
                        if let Some(state) = weak.upgrade() {
                            state.shutdown_requested.store(true, Ordering::SeqCst);
                        }
                    }
                })),
                on_error: Some(Arc::new(move |err| {
                    output_value(
                        &output,
                        &json!({
                            "type": "extension_error",
                            "extensionPath": err.extension_path,
                            "event": err.event,
                            "error": err.error,
                        }),
                    );
                })),
            })
            .await?;

        let listener: AgentSessionEventListener = {
            let weak = weak.clone();
            Arc::new(move |event: AgentSessionEvent| {
                let Some(state) = weak.upgrade() else { return };
                if let Ok(wire) = to_json_event(&event) {
                    state.output(&wire);
                }
                if matches!(event, AgentSessionEvent::AgentSettled { .. }) {
                    state.shutdown_check.notify_one();
                }
            })
        };
        let unsubscribe = session.subscribe(&listener);
        let backpressure = subscribe_backpressure(&session);
        self.subscriptions.lock().set(unsubscribe, Some(backpressure));
        Ok(())
    }

    /// Handle a single command. `None` means the response was (or will be) emitted elsewhere.
    async fn handle_command(self: &Arc<Self>, id: Option<String>, command: RpcCommandKind) -> anyhow::Result<Option<RpcResponse>> {
        let session = self.runtime_host.session();
        let runtime_host = &self.runtime_host;
        let response = match command {
            // =================================================================
            // Prompting
            // =================================================================
            RpcCommandKind::Prompt { message, images, streaming_behavior } => {
                // Start prompt handling immediately, but emit the authoritative response only after
                // prompt preflight succeeds. Queued and immediately handled prompts also count as success.
                let preflight_succeeded = Arc::new(AtomicBool::new(false));
                let options = PromptOptions {
                    images,
                    streaming_behavior: streaming_behavior.map(|b| match b {
                        StreamingBehavior::Steer => PromptOptionsStreamingBehavior::Steer,
                        StreamingBehavior::FollowUp => PromptOptionsStreamingBehavior::FollowUp,
                    }),
                    source: Some(InputSource::Rpc),
                    preflight_result: Some(Arc::new({
                        let preflight_succeeded = preflight_succeeded.clone();
                        let output = self.output.clone();
                        let id = id.clone();
                        move |disposition| {
                            preflight_succeeded.store(true, Ordering::SeqCst);
                            output_value(&output, &success(id.clone(), "prompt", Some(json!({ "disposition": disposition }))));
                        }
                    })),
                    ..Default::default()
                };
                if let Err(error) = session.prompt(&message, Some(&options)).await
                    && !preflight_succeeded.load(Ordering::SeqCst)
                {
                    self.output(&RpcResponse::error(id, "prompt", error.to_string()));
                }
                return Ok(None);
            }

            RpcCommandKind::Steer { message, images } => {
                let disposition = session
                    .steer(&message, images.as_deref(), Some(&AgentSessionSteerOptions { source: Some(InputSource::Rpc) }))
                    .await?;
                success(id, "steer", Some(json!({ "disposition": disposition })))
            }

            RpcCommandKind::FollowUp { message, images } => {
                let disposition = session
                    .follow_up(
                        &message,
                        images.as_deref(),
                        Some(&AgentSessionFollowUpOptions { source: Some(InputSource::Rpc) }),
                    )
                    .await?;
                success(id, "follow_up", Some(json!({ "disposition": disposition })))
            }

            RpcCommandKind::Abort => {
                session.abort().await?;
                success(id, "abort", None)
            }

            RpcCommandKind::ClearQueue => success_data(id, "clear_queue", &session.clear_queue()),

            RpcCommandKind::NewSession { parent_session } => {
                let options = parent_session.map(|parent_session| AgentSessionRuntimeNewSessionOptions {
                    parent_session: Some(parent_session),
                    setup: None,
                    with_session: None,
                });
                let result = runtime_host.new_session(options.as_ref()).await?;
                if !result.cancelled {
                    self.rebind_session().await?;
                }
                success_data(id, "new_session", &result)
            }

            // =================================================================
            // State
            // =================================================================
            RpcCommandKind::GetState => {
                let queue_mode = |mode: AgentSessionSteeringMode| match mode {
                    AgentSessionSteeringMode::All => RpcQueueMode::All,
                    AgentSessionSteeringMode::OneAtATime => RpcQueueMode::OneAtATime,
                };
                let state = RpcSessionState {
                    model: session.model(),
                    thinking_level: session.thinking_level(),
                    is_streaming: session.is_streaming(),
                    is_compacting: session.is_compacting(),
                    steering_mode: queue_mode(session.steering_mode()),
                    follow_up_mode: queue_mode(session.follow_up_mode()),
                    session_file: session.session_file(),
                    session_id: session.session_id(),
                    session_name: session.session_name(),
                    auto_compaction_enabled: session.auto_compaction_enabled(),
                    message_count: session.messages().len() as u64,
                    pending_message_count: session.pending_message_count().max(0) as u64,
                };
                success_data(id, "get_state", &state)
            }

            // =================================================================
            // Model
            // =================================================================
            RpcCommandKind::SetModel { provider, model_id } => {
                let models = session.model_runtime().get_available_snapshot();
                let Some(model) = models.into_iter().find(|m| m.provider == provider && m.id == model_id) else {
                    return Ok(Some(RpcResponse::error(
                        id,
                        "set_model",
                        format!("Model not found: {provider}/{model_id}"),
                    )));
                };
                session.set_model(model.clone(), None).await?;
                success_data(id, "set_model", &model)
            }

            RpcCommandKind::CycleModel => match session.cycle_model(None, None).await? {
                Some(result) => success_data(id, "cycle_model", &result),
                None => success(id, "cycle_model", Some(Value::Null)),
            },

            RpcCommandKind::GetAvailableModels => {
                let models = session.model_runtime().get_available_snapshot();
                success(id, "get_available_models", Some(json!({ "models": models })))
            }

            // =================================================================
            // Thinking
            // =================================================================
            RpcCommandKind::SetThinkingLevel { level } => {
                session.set_thinking_level(level, None);
                success(id, "set_thinking_level", None)
            }

            RpcCommandKind::CycleThinkingLevel => match session.cycle_thinking_level(None) {
                Some(level) => success(id, "cycle_thinking_level", Some(json!({ "level": level }))),
                None => success(id, "cycle_thinking_level", Some(Value::Null)),
            },

            RpcCommandKind::GetAvailableThinkingLevels => {
                let levels = session.get_available_thinking_levels();
                success(id, "get_available_thinking_levels", Some(json!({ "levels": levels })))
            }

            // =================================================================
            // Queue Modes
            // =================================================================
            RpcCommandKind::SetSteeringMode { mode } => {
                session.set_steering_mode(match mode {
                    RpcQueueMode::All => AgentSessionSteeringMode::All,
                    RpcQueueMode::OneAtATime => AgentSessionSteeringMode::OneAtATime,
                });
                success(id, "set_steering_mode", None)
            }

            RpcCommandKind::SetFollowUpMode { mode } => {
                session.set_follow_up_mode(match mode {
                    RpcQueueMode::All => AgentSessionSteeringMode::All,
                    RpcQueueMode::OneAtATime => AgentSessionSteeringMode::OneAtATime,
                });
                success(id, "set_follow_up_mode", None)
            }

            // =================================================================
            // Compaction
            // =================================================================
            RpcCommandKind::Compact { custom_instructions } => {
                let result = session.compact(custom_instructions.as_deref()).await?;
                success_data(id, "compact", &result)
            }

            RpcCommandKind::SetAutoCompaction { enabled } => {
                session.set_auto_compaction_enabled(enabled);
                success(id, "set_auto_compaction", None)
            }

            // =================================================================
            // Retry
            // =================================================================
            RpcCommandKind::SetAutoRetry { enabled } => {
                session.set_auto_retry_enabled(enabled);
                success(id, "set_auto_retry", None)
            }

            RpcCommandKind::AbortRetry => {
                session.abort_retry();
                success(id, "abort_retry", None)
            }

            // =================================================================
            // Bash
            // =================================================================
            RpcCommandKind::Bash { command, exclude_from_context } => {
                let event_result = session
                    .extension_runner()
                    .emit_user_bash(&UserBashEvent {
                        command: command.clone(),
                        exclude_from_context: exclude_from_context.unwrap_or(false),
                        cwd: session.session_manager().get_cwd(),
                    })
                    .await?;

                let operations = match event_result {
                    Some(UserBashEventResult::Result(result)) => {
                        session.record_bash_result(
                            &command,
                            &result,
                            Some(&AgentSessionRecordBashResultOptions { exclude_from_context }),
                        );
                        return Ok(Some(success_data(id, "bash", &result)));
                    }
                    Some(UserBashEventResult::Operations(operations)) => Some(operations),
                    None => None,
                };

                let result = session
                    .execute_bash(
                        &command,
                        None,
                        Some(&AgentSessionExecuteBashOptions { exclude_from_context, id: id.clone(), operations }),
                    )
                    .await?;
                success_data(id, "bash", &result)
            }

            RpcCommandKind::AbortBash => {
                session.abort_bash();
                success(id, "abort_bash", None)
            }

            // =================================================================
            // Session
            // =================================================================
            RpcCommandKind::GetSessionStats => success_data(id, "get_session_stats", &session.get_session_stats()),

            RpcCommandKind::ExportHtml { output_path } => {
                let path = session.export_to_html(output_path.as_deref(), None).await?;
                success(id, "export_html", Some(json!({ "path": path })))
            }

            RpcCommandKind::SwitchSession { session_path } => {
                let result = runtime_host.switch_session(&session_path, None).await?;
                if !result.cancelled {
                    self.rebind_session().await?;
                }
                success_data(id, "switch_session", &result)
            }

            RpcCommandKind::Fork { entry_id } => {
                let result = runtime_host.fork(&entry_id, None).await?;
                if !result.cancelled {
                    self.rebind_session().await?;
                }
                let mut data = Map::new();
                if let Some(text) = result.selected_text {
                    data.insert("text".to_string(), Value::String(text));
                }
                data.insert("cancelled".to_string(), Value::Bool(result.cancelled));
                success(id, "fork", Some(Value::Object(data)))
            }

            RpcCommandKind::Clone => {
                let Some(leaf_id) = session.session_manager().get_leaf_id() else {
                    return Ok(Some(RpcResponse::error(id, "clone", "Cannot clone session: no current entry selected")));
                };
                let result = runtime_host
                    .fork(
                        &leaf_id,
                        Some(&AgentSessionRuntimeForkOptions {
                            position: Some(AgentSessionRuntimeForkOptionsPosition::At),
                            with_session: None,
                        }),
                    )
                    .await?;
                if !result.cancelled {
                    self.rebind_session().await?;
                }
                success(id, "clone", Some(json!({ "cancelled": result.cancelled })))
            }

            RpcCommandKind::GetForkMessages => {
                let messages = session.get_user_messages_for_forking();
                success(id, "get_fork_messages", Some(json!({ "messages": messages })))
            }

            RpcCommandKind::GetEntries { since } => {
                let session_manager = session.session_manager();
                let mut entries = session_manager.get_entries();
                if let Some(since) = since {
                    let Some(since_index) = entries.iter().position(|e| e.id() == since) else {
                        return Ok(Some(RpcResponse::error(id, "get_entries", format!("Entry not found: {since}"))));
                    };
                    entries = entries.split_off(since_index + 1);
                }
                success(id, "get_entries", Some(json!({ "entries": entries, "leafId": session_manager.get_leaf_id() })))
            }

            RpcCommandKind::GetTree => {
                let session_manager = session.session_manager();
                success(
                    id,
                    "get_tree",
                    Some(json!({ "tree": session_manager.get_tree(), "leafId": session_manager.get_leaf_id() })),
                )
            }

            RpcCommandKind::GetLastAssistantText => {
                let mut data = Map::new();
                if let Some(text) = session.get_last_assistant_text() {
                    data.insert("text".to_string(), Value::String(text));
                }
                success(id, "get_last_assistant_text", Some(Value::Object(data)))
            }

            RpcCommandKind::SetSessionName { name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Ok(Some(RpcResponse::error(id, "set_session_name", "Session name cannot be empty")));
                }
                session.set_session_name(name);
                success(id, "set_session_name", None)
            }

            // =================================================================
            // Messages
            // =================================================================
            RpcCommandKind::GetMessages => success(id, "get_messages", Some(json!({ "messages": session.messages() }))),

            // =================================================================
            // Commands (available for invocation via prompt)
            // =================================================================
            RpcCommandKind::GetCommands => {
                let mut commands: Vec<RpcSlashCommand> = Vec::new();
                for command in session.extension_runner().get_registered_commands() {
                    commands.push(RpcSlashCommand {
                        name: command.invocation_name.clone(),
                        description: command.command.description.clone(),
                        source: SlashCommandSource::Extension,
                        source_info: command.command.source_info.clone(),
                    });
                }
                for template in session.prompt_templates() {
                    commands.push(RpcSlashCommand {
                        name: template.name.clone(),
                        description: Some(template.description.clone()),
                        source: SlashCommandSource::Prompt,
                        source_info: template.source_info.clone(),
                    });
                }
                for skill in session.resource_loader().get_skills().skills {
                    commands.push(RpcSlashCommand {
                        name: format!("skill:{}", skill.name),
                        description: Some(skill.description.clone()),
                        source: SlashCommandSource::Skill,
                        source_info: skill.source_info.clone(),
                    });
                }
                success(id, "get_commands", Some(json!({ "commands": commands })))
            }
        };
        Ok(Some(response))
    }

    /// Handle one input line (a command or an `extension_ui_response`).
    async fn handle_input_line(self: &Arc<Self>, line: String) {
        let parsed: Value = match crate::utils::js_json::js_json_parse(&line) {
            Ok(value) => value,
            Err(parse_error) => {
                self.output(&RpcResponse::error(None, "parse", format!("Failed to parse command: {parse_error}")));
                let _ = wait_for_raw_stdout_backpressure().await;
                return;
            }
        };

        // Handle extension UI responses
        if parsed.get("type").and_then(Value::as_str) == Some("extension_ui_response") {
            if let Ok(response) = serde_json::from_value::<RpcExtensionUIResponse>(parsed.clone()) {
                if let Some(pending) = self.pending.lock().remove(&response.id) {
                    let _ = pending.send(response);
                }
            } else if let Some(id) = parsed.get("id").and_then(Value::as_str) {
                // A response without a recognizable value resolves the dialog with its default.
                if let Some(pending) = self.pending.lock().remove(id) {
                    drop(pending);
                }
            }
            return;
        }

        let id = parsed.get("id").and_then(Value::as_str).map(str::to_string);
        let command_type = match parsed.get("type") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => "undefined".to_string(),
            Some(other) => other.to_string(),
        };
        let command = match serde_json::from_value::<RpcCommand>(parsed) {
            Ok(command) => command,
            Err(error) => {
                let message = if KNOWN_COMMAND_TYPES.contains(&command_type.as_str()) {
                    format!("Invalid {command_type} command: {error}")
                } else {
                    format!("Unknown command: {command_type}")
                };
                self.output(&RpcResponse::error(id, &command_type, message));
                let _ = wait_for_raw_stdout_backpressure().await;
                return;
            }
        };

        match self.handle_command(command.id.clone(), command.kind).await {
            Ok(Some(response)) => {
                self.output(&response);
                let _ = wait_for_raw_stdout_backpressure().await;
            }
            Ok(None) => {}
            Err(command_error) => {
                self.output(&RpcResponse::error(command.id, &command_type, command_error.to_string()));
                let _ = wait_for_raw_stdout_backpressure().await;
            }
        }
        // `checkShutdownRequested` runs in the main loop.
        self.shutdown_check.notify_one();
    }
}

/// I/O of an RPC session.
pub struct RpcIo<R> {
    /// Command input (stdin).
    pub input: R,
    /// Output sink for serialized JSONL records (raw stdout).
    pub output: RpcOutput,
    /// Install SIGTERM/SIGHUP handlers that shut down with exit code 143/129.
    pub handle_signals: bool,
}

/// Why an RPC session ended, and the process exit code it requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcModeExit {
    pub exit_code: i32,
    /// The session ended because of SIGTERM (stdout is not flushed).
    pub sigterm: bool,
}

async fn next_signal(enabled: bool) -> (i32, bool) {
    if !enabled {
        return std::future::pending().await;
    }
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut hup)) = (signal(SignalKind::terminate()), signal(SignalKind::hangup())) else {
            return std::future::pending().await;
        };
        tokio::select! {
            _ = term.recv() => (143, true),
            _ = hup.recv() => (129, false),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        (143, true)
    }
}

/// Run the RPC protocol over the given I/O until input ends, an extension requests shutdown, or a
/// signal arrives. Disposes the runtime and returns the exit code (does not exit the process).
pub async fn run_rpc_session<R: AsyncRead + Unpin>(
    runtime_host: Arc<AgentSessionRuntime>,
    io: RpcIo<R>,
) -> anyhow::Result<RpcModeExit> {
    let state = Arc::new(RpcModeState {
        runtime_host: runtime_host.clone(),
        output: io.output.clone(),
        pending: Arc::default(),
        subscriptions: Mutex::new(SessionSubscriptions::default()),
        shutdown_requested: AtomicBool::new(false),
        shutdown_check: Notify::new(),
    });

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

    state.rebind_session().await?;

    let mut reader = JsonlLineReader::new(io.input);
    let mut in_flight: FuturesUnordered<LocalBoxFuture<'_, ()>> = FuturesUnordered::new();
    let signal = next_signal(io.handle_signals);
    tokio::pin!(signal);

    let mut input_ended = false;
    let exit = loop {
        tokio::select! {
            line = reader.next_line() => match line {
                Ok(Some(line)) => {
                    let state = state.clone();
                    in_flight.push(Box::pin(async move { state.handle_input_line(line).await }));
                }
                // Input ended (or failed): shut down.
                _ => {
                    input_ended = true;
                    break RpcModeExit { exit_code: 0, sigterm: false };
                }
            },
            Some(()) = in_flight.next(), if !in_flight.is_empty() => {}
            _ = state.shutdown_check.notified() => {
                if state.shutdown_requested.load(Ordering::SeqCst) {
                    break RpcModeExit { exit_code: 0, sigterm: false };
                }
            }
            (code, sigterm) = &mut signal => {
                kill_tracked_detached_children();
                break RpcModeExit { exit_code: code, sigterm };
            }
        }
    };

    state.subscriptions.lock().clear();
    runtime_host.set_rebind_session(None);
    if input_ended {
        // TS `shutdown()` on stdin end: commands already dispatched keep running (their responses
        // are still written) while the runtime is disposed; whatever is unfinished when disposal
        // completes ends with the process. TS starts each handler synchronously when its line
        // arrives, so first let every dispatched command run until it waits.
        while let Some(Some(())) = futures::FutureExt::now_or_never(in_flight.next()) {}
        let dispose = runtime_host.dispose();
        tokio::pin!(dispose);
        loop {
            tokio::select! {
                result = &mut dispose => {
                    result?;
                    break;
                }
                Some(()) = in_flight.next(), if !in_flight.is_empty() => {}
            }
        }
        drop(in_flight);
    } else {
        drop(in_flight);
        runtime_host.dispose().await?;
    }
    Ok(exit)
}

/// Run in RPC mode.
/// Listens for JSON commands on stdin, outputs events and responses on stdout.
/// Never returns: the process exits when the session ends.
pub async fn run_rpc_mode(runtime_host: Arc<AgentSessionRuntime>) -> anyhow::Result<()> {
    take_over_stdout();
    let output: RpcOutput = Arc::new(|line: String| write_raw_stdout(&line));
    let exit = run_rpc_session(
        runtime_host,
        RpcIo { input: tokio::io::stdin(), output, handle_signals: true },
    )
    .await;
    let exit = match exit {
        Ok(exit) => exit,
        Err(error) => {
            eprintln!("{error}");
            RpcModeExit { exit_code: 1, sigterm: false }
        }
    };
    if !exit.sigterm {
        let _ = flush_raw_stdout().await;
    }
    crate::utils::process_exit::exit(exit.exit_code)
}
