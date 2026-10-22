//! Port of `packages/coding-agent/src/modes/rpc/rpc-client.ts`.

use crate::core::agent_session::PromptDisposition;
use crate::core::agent_session::QueuedInputDisposition;
use crate::core::agent_session::SessionStats;
use crate::core::bash_executor::BashResult;
use crate::core::compaction::compaction::CompactionResult;
use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::SessionTreeNode;
use crate::modes::json_event::JsonAgentSessionEvent;
use crate::modes::rpc::rpc_types::RpcSessionState;
use crate::modes::rpc::rpc_types::RpcSlashCommand;
use indexmap::IndexMap;
use pi_agent::types::AgentMessage;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::ImageContent;
use crate::modes::rpc::jsonl::{JsonlLineReader, serialize_json_line};
use crate::modes::rpc::rpc_types::RpcResponse;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

/// Distributive Omit that works with union types (a command object without `id`).
pub type DistributiveOmit = Value;

/// RpcCommand without the id field (for internal send): a JSON object with `type`.
pub type RpcCommandBody = DistributiveOmit;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientOptions {
    /// Path to the CLI entry point. A `.js`/`.mjs`/`.cjs` path is run with `node` (as in TS);
    /// anything else is executed directly. Defaults to the `pi` executable on `PATH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_path: Option<String>,
    /// Working directory for the agent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Environment variables
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,
    /// Provider to use
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Model ID to use
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Additional CLI arguments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub id: String,
    pub context_window: u64,
    pub reasoning: bool,
}

pub type RpcEventListener = Arc<dyn Fn(JsonAgentSessionEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RpcClientPromptStreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl RpcClientPromptStreamingBehavior {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::FollowUp => "followUp",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientClearQueueResult {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientNewSessionResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientSetModelResult {
    pub provider: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientCycleModelResultModel {
    pub provider: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientCycleModelResult {
    pub model: RpcClientCycleModelResultModel,
    pub thinking_level: ThinkingLevel,
    pub is_scoped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientCycleThinkingLevelResult {
    pub level: ThinkingLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RpcClientSetSteeringModeMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

impl RpcClientSetSteeringModeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one-at-a-time",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientExportHtmlResult {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientSwitchSessionResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientForkResult {
    pub text: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientCloneResult {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientGetForkMessagesResult {
    pub entry_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientGetEntriesResult {
    pub entries: Vec<SessionEntry>,
    #[serde(default)]
    pub leaf_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientGetTreeResult {
    pub tree: Vec<SessionTreeNode>,
    #[serde(default)]
    pub leaf_id: Option<String>,
}

struct ChildState {
    child: tokio::process::Child,
    stdin: Option<tokio::process::ChildStdin>,
    /// Bumped on every start so stale reader tasks ignore later processes.
    generation: u64,
}

#[derive(Default)]
struct Inner {
    process: tokio::sync::Mutex<Option<ChildState>>,
    generation: AtomicU64,
    /// Set while a process is running; cleared on stop.
    running: Mutex<bool>,
    event_listeners: Mutex<Vec<(u64, RpcEventListener)>>,
    next_listener_id: AtomicU64,
    pending_requests: Mutex<HashMap<String, oneshot::Sender<Result<RpcResponse, String>>>>,
    request_id: AtomicU64,
    stderr: Mutex<String>,
    exit_error: Mutex<Option<String>>,
    exited: Mutex<Option<(Option<i32>, Option<String>)>>,
    exit_notify: tokio::sync::Notify,
}

impl Inner {
    fn create_process_exit_error(&self, code: Option<i32>, signal: Option<String>) -> String {
        format!(
            "Agent process exited (code={} signal={}). Stderr: {}",
            code.map(|c| c.to_string()).unwrap_or_else(|| "null".to_string()),
            signal.unwrap_or_else(|| "null".to_string()),
            self.stderr.lock()
        )
    }

    fn reject_pending_requests(&self, error: &str) {
        let pending: Vec<_> = self.pending_requests.lock().drain().collect();
        for (_, sender) in pending {
            let _ = sender.send(Err(error.to_string()));
        }
    }

    fn handle_line(&self, line: &str) {
        let Ok(data) = serde_json::from_str::<Value>(line) else {
            // Ignore non-JSON lines
            return;
        };

        // Check if it's a response to a pending request
        if data.get("type").and_then(Value::as_str) == Some("response")
            && let Some(id) = data.get("id").and_then(Value::as_str)
        {
            let sender = self.pending_requests.lock().remove(id);
            if let Some(sender) = sender {
                if let Ok(response) = serde_json::from_value::<RpcResponse>(data) {
                    let _ = sender.send(Ok(response));
                }
                return;
            }
        }

        // Otherwise it's an event. Iterate a snapshot so listeners that unsubscribe during dispatch
        // do not cause later listeners to miss this event.
        let event: JsonAgentSessionEvent =
            serde_json::from_value(data.clone()).unwrap_or(JsonAgentSessionEvent::Other(data));
        let listeners: Vec<RpcEventListener> = self.event_listeners.lock().iter().map(|(_, l)| l.clone()).collect();
        for listener in listeners {
            listener(event.clone());
        }
    }

    fn add_listener(&self, listener: RpcEventListener) -> u64 {
        let id = self.next_listener_id.fetch_add(1, Ordering::SeqCst);
        self.event_listeners.lock().push((id, listener));
        id
    }

    fn remove_listener(&self, id: u64) {
        self.event_listeners.lock().retain(|(listener_id, _)| *listener_id != id);
    }
}

#[cfg(unix)]
fn exit_signal_name(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    let signal = status.signal()?;
    Some(
        match signal {
            1 => "SIGHUP",
            2 => "SIGINT",
            3 => "SIGQUIT",
            6 => "SIGABRT",
            9 => "SIGKILL",
            13 => "SIGPIPE",
            14 => "SIGALRM",
            15 => "SIGTERM",
            _ => return Some(format!("SIG{signal}")),
        }
        .to_string(),
    )
}

#[cfg(not(unix))]
fn exit_signal_name(_status: &std::process::ExitStatus) -> Option<String> {
    None
}

/// RPC Client for programmatic access to the coding agent.
///
/// Spawns the agent in RPC mode and provides a typed API for all operations.
pub struct RpcClient {
    options: RpcClientOptions,
    inner: Arc<Inner>,
}

impl RpcClient {
    pub fn new(options: Option<&RpcClientOptions>) -> Self {
        Self { options: options.cloned().unwrap_or_default(), inner: Arc::new(Inner::default()) }
    }

    /// Start the RPC agent process.
    pub async fn start(&self) -> anyhow::Result<()> {
        let mut process = self.inner.process.lock().await;
        if process.is_some() {
            anyhow::bail!("Client already started");
        }
        *self.inner.exit_error.lock() = None;
        *self.inner.exited.lock() = None;

        let cli_path = self.options.cli_path.clone().unwrap_or_else(|| "pi".to_string());
        let mut args: Vec<String> = vec!["--mode".into(), "rpc".into()];
        if let Some(provider) = &self.options.provider {
            args.push("--provider".into());
            args.push(provider.clone());
        }
        if let Some(model) = &self.options.model {
            args.push("--model".into());
            args.push(model.clone());
        }
        if let Some(extra) = &self.options.args {
            args.extend(extra.iter().cloned());
        }

        let is_js = [".js", ".mjs", ".cjs"].iter().any(|ext| cli_path.ends_with(ext));
        let mut command = if is_js {
            let mut command = tokio::process::Command::new("node");
            command.arg(&cli_path);
            command
        } else {
            tokio::process::Command::new(&cli_path)
        };
        command.args(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(cwd) = &self.options.cwd {
            command.current_dir(cwd);
        }
        if let Some(env) = &self.options.env {
            command.envs(env.iter());
        }
        command.kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let message = format!("Agent process error: {error}. Stderr: {}", self.inner.stderr.lock());
                *self.inner.exit_error.lock() = Some(message.clone());
                anyhow::bail!(message);
            }
        };
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Collect stderr for debugging
        if let Some(mut stderr) = stderr {
            let inner = self.inner.clone();
            crate::utils::uncaught::spawn(async move {
                let mut buf = vec![0u8; 8192];
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                            inner.stderr.lock().push_str(&chunk);
                            use std::io::Write;
                            let _ = std::io::stderr().write_all(&buf[..n]);
                        }
                    }
                }
            });
        }

        // Set up strict JSONL reader for stdout.
        if let Some(stdout) = stdout {
            let inner = self.inner.clone();
            crate::utils::uncaught::spawn(async move {
                let mut reader = JsonlLineReader::new(stdout);
                while let Ok(Some(line)) = reader.next_line().await {
                    if inner.generation.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    inner.handle_line(&line);
                }
            });
        }

        *process = Some(ChildState { child, stdin, generation });
        *self.inner.running.lock() = true;
        drop(process);

        // Watch for process exit.
        {
            let inner = self.inner.clone();
            crate::utils::uncaught::spawn(async move {
                loop {
                    let status = {
                        let mut process = inner.process.lock().await;
                        match process.as_mut() {
                            Some(state) if state.generation == generation => state.child.try_wait(),
                            _ => return,
                        }
                    };
                    match status {
                        Ok(Some(status)) => {
                            let code = status.code();
                            let signal = exit_signal_name(&status);
                            *inner.exited.lock() = Some((code, signal.clone()));
                            let error = inner.create_process_exit_error(code, signal);
                            *inner.exit_error.lock() = Some(error.clone());
                            inner.reject_pending_requests(&error);
                            inner.exit_notify.notify_waiters();
                            return;
                        }
                        Ok(None) => tokio::time::sleep(Duration::from_millis(20)).await,
                        Err(_) => return,
                    }
                }
            });
        }

        // Wait a moment for process to initialize
        tokio::time::sleep(Duration::from_millis(100)).await;

        if let Some((code, signal)) = self.inner.exited.lock().clone() {
            let error =
                self.inner.exit_error.lock().clone().unwrap_or_else(|| self.inner.create_process_exit_error(code, signal));
            *self.inner.exit_error.lock() = Some(error.clone());
            anyhow::bail!(error);
        }
        Ok(())
    }

    /// Stop the RPC agent process.
    pub async fn stop(&self) -> anyhow::Result<()> {
        let mut process = self.inner.process.lock().await;
        let Some(mut state) = process.take() else {
            return Ok(());
        };
        // Stop reading stdout from this process.
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        drop(state.stdin.take());

        #[cfg(unix)]
        if let Some(pid) = state.child.id() {
            // SAFETY: sending SIGTERM to our own child process.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
        #[cfg(not(unix))]
        let _ = state.child.start_kill();

        // Wait for process to exit
        if tokio::time::timeout(Duration::from_millis(1000), state.child.wait()).await.is_err() {
            let _ = state.child.start_kill();
            let _ = state.child.wait().await;
        }

        *self.inner.running.lock() = false;
        self.inner.pending_requests.lock().clear();
        Ok(())
    }

    /// Subscribe to agent events.
    pub fn on_event(&self, listener: &RpcEventListener) -> Arc<dyn Fn() + Send + Sync> {
        let id = self.inner.add_listener(listener.clone());
        let inner = Arc::downgrade(&self.inner);
        Arc::new(move || {
            if let Some(inner) = inner.upgrade() {
                inner.remove_listener(id);
            }
        })
    }

    /// Get collected stderr output (useful for debugging).
    pub fn get_stderr(&self) -> String {
        self.inner.stderr.lock().clone()
    }

    // =========================================================================
    // Command Methods
    // =========================================================================

    /// Send a prompt to the agent.
    /// Returns the prompt's disposition after acceptance; use onEvent() to receive streaming events.
    /// If the disposition is "handled", no run started for this prompt, so don't wait for agent_settled.
    pub async fn prompt(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
        streaming_behavior: Option<RpcClientPromptStreamingBehavior>,
    ) -> anyhow::Result<PromptDisposition> {
        let mut command = json!({ "type": "prompt", "message": message });
        if let Some(images) = images {
            command["images"] = serde_json::to_value(images)?;
        }
        if let Some(behavior) = streaming_behavior {
            command["streamingBehavior"] = json!(behavior.as_str());
        }
        let response = self.send(command).await?;
        Ok(Self::get_data::<DispositionData<PromptDisposition>>(response)?.disposition)
    }

    /// Queue a steering message to interrupt the agent mid-run.
    pub async fn steer(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
    ) -> anyhow::Result<QueuedInputDisposition> {
        let mut command = json!({ "type": "steer", "message": message });
        if let Some(images) = images {
            command["images"] = serde_json::to_value(images)?;
        }
        let response = self.send(command).await?;
        Ok(Self::get_data::<DispositionData<QueuedInputDisposition>>(response)?.disposition)
    }

    /// Queue a follow-up message to be processed after the agent finishes.
    pub async fn follow_up(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
    ) -> anyhow::Result<QueuedInputDisposition> {
        let mut command = json!({ "type": "follow_up", "message": message });
        if let Some(images) = images {
            command["images"] = serde_json::to_value(images)?;
        }
        let response = self.send(command).await?;
        Ok(Self::get_data::<DispositionData<QueuedInputDisposition>>(response)?.disposition)
    }

    /// Abort current operation.
    pub async fn abort(&self) -> anyhow::Result<()> {
        self.send(json!({ "type": "abort" })).await?;
        Ok(())
    }

    /// Clear queued steering and follow-up messages, returning their text.
    pub async fn clear_queue(&self) -> anyhow::Result<RpcClientClearQueueResult> {
        let response = self.send(json!({ "type": "clear_queue" })).await?;
        Self::get_data(response)
    }

    /// Start a new session, optionally with parent tracking.
    /// @param parentSession - Optional parent session path for lineage tracking
    /// @returns Object with `cancelled: true` if an extension cancelled the new session
    pub async fn new_session(&self, parent_session: Option<&str>) -> anyhow::Result<RpcClientNewSessionResult> {
        let mut command = json!({ "type": "new_session" });
        if let Some(parent_session) = parent_session {
            command["parentSession"] = json!(parent_session);
        }
        let response = self.send(command).await?;
        Self::get_data(response)
    }

    /// Get current session state.
    pub async fn get_state(&self) -> anyhow::Result<RpcSessionState> {
        let response = self.send(json!({ "type": "get_state" })).await?;
        Self::get_data(response)
    }

    /// Set model by provider and ID.
    pub async fn set_model(&self, provider: &str, model_id: &str) -> anyhow::Result<RpcClientSetModelResult> {
        let response = self.send(json!({ "type": "set_model", "provider": provider, "modelId": model_id })).await?;
        Self::get_data(response)
    }

    /// Cycle to next model.
    pub async fn cycle_model(&self) -> anyhow::Result<Option<RpcClientCycleModelResult>> {
        let response = self.send(json!({ "type": "cycle_model" })).await?;
        Self::get_data(response)
    }

    /// Get list of available models.
    pub async fn get_available_models(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let response = self.send(json!({ "type": "get_available_models" })).await?;
        Ok(Self::get_data::<ModelsData>(response)?.models)
    }

    /// Set thinking level.
    pub async fn set_thinking_level(&self, level: ThinkingLevel) -> anyhow::Result<()> {
        self.send(json!({ "type": "set_thinking_level", "level": level })).await?;
        Ok(())
    }

    /// Cycle thinking level.
    pub async fn cycle_thinking_level(&self) -> anyhow::Result<Option<RpcClientCycleThinkingLevelResult>> {
        let response = self.send(json!({ "type": "cycle_thinking_level" })).await?;
        Self::get_data(response)
    }

    /// Get list of available thinking levels for the current model.
    pub async fn get_available_thinking_levels(&self) -> anyhow::Result<Vec<ThinkingLevel>> {
        let response = self.send(json!({ "type": "get_available_thinking_levels" })).await?;
        Ok(Self::get_data::<LevelsData>(response)?.levels)
    }

    /// Set steering mode.
    pub async fn set_steering_mode(&self, mode: RpcClientSetSteeringModeMode) -> anyhow::Result<()> {
        self.send(json!({ "type": "set_steering_mode", "mode": mode })).await?;
        Ok(())
    }

    /// Set follow-up mode.
    pub async fn set_follow_up_mode(&self, mode: RpcClientSetSteeringModeMode) -> anyhow::Result<()> {
        self.send(json!({ "type": "set_follow_up_mode", "mode": mode })).await?;
        Ok(())
    }

    /// Compact session context.
    pub async fn compact(&self, custom_instructions: Option<&str>) -> anyhow::Result<CompactionResult> {
        let mut command = json!({ "type": "compact" });
        if let Some(custom_instructions) = custom_instructions {
            command["customInstructions"] = json!(custom_instructions);
        }
        let response = self.send(command).await?;
        Self::get_data(response)
    }

    /// Set auto-compaction enabled/disabled.
    pub async fn set_auto_compaction(&self, enabled: bool) -> anyhow::Result<()> {
        self.send(json!({ "type": "set_auto_compaction", "enabled": enabled })).await?;
        Ok(())
    }

    /// Set auto-retry enabled/disabled.
    pub async fn set_auto_retry(&self, enabled: bool) -> anyhow::Result<()> {
        self.send(json!({ "type": "set_auto_retry", "enabled": enabled })).await?;
        Ok(())
    }

    /// Abort in-progress retry.
    pub async fn abort_retry(&self) -> anyhow::Result<()> {
        self.send(json!({ "type": "abort_retry" })).await?;
        Ok(())
    }

    /// Execute a bash command.
    pub async fn bash(&self, command: &str) -> anyhow::Result<BashResult> {
        let response = self.send(json!({ "type": "bash", "command": command })).await?;
        Self::get_data(response)
    }

    /// Abort running bash command.
    pub async fn abort_bash(&self) -> anyhow::Result<()> {
        self.send(json!({ "type": "abort_bash" })).await?;
        Ok(())
    }

    /// Get session statistics.
    pub async fn get_session_stats(&self) -> anyhow::Result<SessionStats> {
        let response = self.send(json!({ "type": "get_session_stats" })).await?;
        Self::get_data(response)
    }

    /// Export session to HTML.
    pub async fn export_html(&self, output_path: Option<&str>) -> anyhow::Result<RpcClientExportHtmlResult> {
        let mut command = json!({ "type": "export_html" });
        if let Some(output_path) = output_path {
            command["outputPath"] = json!(output_path);
        }
        let response = self.send(command).await?;
        Self::get_data(response)
    }

    /// Switch to a different session file.
    /// @returns Object with `cancelled: true` if an extension cancelled the switch
    pub async fn switch_session(&self, session_path: &str) -> anyhow::Result<RpcClientSwitchSessionResult> {
        let response = self.send(json!({ "type": "switch_session", "sessionPath": session_path })).await?;
        Self::get_data(response)
    }

    /// Fork from a specific message.
    /// @returns Object with `text` (the message text) and `cancelled` (if extension cancelled)
    pub async fn fork(&self, entry_id: &str) -> anyhow::Result<RpcClientForkResult> {
        let response = self.send(json!({ "type": "fork", "entryId": entry_id })).await?;
        Self::get_data(response)
    }

    /// Clone the current active branch into a new session.
    /// @returns Object with `cancelled: true` if an extension cancelled the clone
    pub async fn clone(&self) -> anyhow::Result<RpcClientCloneResult> {
        let response = self.send(json!({ "type": "clone" })).await?;
        Self::get_data(response)
    }

    /// Get messages available for forking.
    pub async fn get_fork_messages(&self) -> anyhow::Result<Vec<RpcClientGetForkMessagesResult>> {
        let response = self.send(json!({ "type": "get_fork_messages" })).await?;
        Ok(Self::get_data::<MessagesData<RpcClientGetForkMessagesResult>>(response)?.messages)
    }

    /// Get session entries in append order, optionally only those after the `since` entry id.
    pub async fn get_entries(&self, since: Option<&str>) -> anyhow::Result<RpcClientGetEntriesResult> {
        let mut command = json!({ "type": "get_entries" });
        if let Some(since) = since {
            command["since"] = json!(since);
        }
        let response = self.send(command).await?;
        Self::get_data(response)
    }

    /// Get the session entry tree.
    pub async fn get_tree(&self) -> anyhow::Result<RpcClientGetTreeResult> {
        let response = self.send(json!({ "type": "get_tree" })).await?;
        Self::get_data(response)
    }

    /// Get text of last assistant message.
    pub async fn get_last_assistant_text(&self) -> anyhow::Result<Option<String>> {
        let response = self.send(json!({ "type": "get_last_assistant_text" })).await?;
        Ok(Self::get_data::<TextData>(response)?.text)
    }

    /// Set the session display name.
    pub async fn set_session_name(&self, name: &str) -> anyhow::Result<()> {
        self.send(json!({ "type": "set_session_name", "name": name })).await?;
        Ok(())
    }

    /// Get all messages in the session.
    pub async fn get_messages(&self) -> anyhow::Result<Vec<AgentMessage>> {
        let response = self.send(json!({ "type": "get_messages" })).await?;
        Ok(Self::get_data::<MessagesData<AgentMessage>>(response)?.messages)
    }

    /// Get available commands (extension commands, prompt templates, skills).
    pub async fn get_commands(&self) -> anyhow::Result<Vec<RpcSlashCommand>> {
        let response = self.send(json!({ "type": "get_commands" })).await?;
        Ok(Self::get_data::<CommandsData>(response)?.commands)
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Register a listener now and return a future that collects events until `agent_settled`.
    fn start_collecting(
        &self,
        timeout: Option<i64>,
        timeout_message: &'static str,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<JsonAgentSessionEvent>>> + Send + 'static {
        let timeout = Duration::from_millis(timeout.unwrap_or(60000).max(0) as u64);
        let (tx, rx) = oneshot::channel::<Vec<JsonAgentSessionEvent>>();
        let events: Arc<Mutex<Vec<JsonAgentSessionEvent>>> = Arc::default();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let listener_id = {
            let events = events.clone();
            let tx = tx.clone();
            self.inner.add_listener(Arc::new(move |event: JsonAgentSessionEvent| {
                let settled = event.type_name() == "agent_settled";
                events.lock().push(event);
                if settled && let Some(tx) = tx.lock().take() {
                    let _ = tx.send(std::mem::take(&mut *events.lock()));
                }
            }))
        };
        let inner = self.inner.clone();
        async move {
            let result = tokio::time::timeout(timeout, rx).await;
            inner.remove_listener(listener_id);
            match result {
                Ok(Ok(events)) => Ok(events),
                _ => anyhow::bail!("{timeout_message}. Stderr: {}", inner.stderr.lock()),
            }
        }
    }

    /// Wait for agent to become idle (no streaming).
    /// Resolves when agent_settled event is received.
    pub async fn wait_for_idle(&self, timeout: Option<i64>) -> anyhow::Result<()> {
        self.start_collecting(timeout, "Timeout waiting for agent to become idle").await.map(|_| ())
    }

    /// Collect events until agent becomes idle.
    pub async fn collect_events(&self, timeout: Option<i64>) -> anyhow::Result<Vec<JsonAgentSessionEvent>> {
        self.start_collecting(timeout, "Timeout collecting events").await
    }

    /// Send prompt and wait for completion, returning all events.
    pub async fn prompt_and_wait(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
        timeout: Option<i64>,
    ) -> anyhow::Result<Vec<JsonAgentSessionEvent>> {
        let events = self.start_collecting(timeout, "Timeout collecting events");
        self.prompt(message, images, None).await?;
        events.await
    }

    // =========================================================================
    // Internal
    // =========================================================================

    async fn send(&self, mut command: RpcCommandBody) -> anyhow::Result<RpcResponse> {
        let command_type = command.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut process = self.inner.process.lock().await;
        let Some(state) = process.as_mut() else {
            anyhow::bail!("Client not started");
        };
        if let Some(error) = self.inner.exit_error.lock().clone() {
            anyhow::bail!(error);
        }
        if let Ok(Some(status)) = state.child.try_wait() {
            let error = self.inner.create_process_exit_error(status.code(), exit_signal_name(&status));
            *self.inner.exit_error.lock() = Some(error.clone());
            anyhow::bail!(error);
        }
        let Some(stdin) = state.stdin.as_mut() else {
            let error = format!("Agent process stdin is not writable. Stderr: {}", self.inner.stderr.lock());
            *self.inner.exit_error.lock() = Some(error.clone());
            anyhow::bail!(error);
        };

        let id = format!("req_{}", self.inner.request_id.fetch_add(1, Ordering::SeqCst) + 1);
        if let Some(object) = command.as_object_mut() {
            object.insert("id".to_string(), Value::String(id.clone()));
        }

        let (tx, rx) = oneshot::channel();
        self.inner.pending_requests.lock().insert(id.clone(), tx);

        let line = serialize_json_line(&command);
        let write_result = async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await
        }
        .await;
        drop(process);
        if let Err(error) = write_result {
            self.inner.pending_requests.lock().remove(&id);
            let stdin_error = self
                .inner
                .exit_error
                .lock()
                .clone()
                .unwrap_or_else(|| format!("Agent process stdin error: {error}. Stderr: {}", self.inner.stderr.lock()));
            *self.inner.exit_error.lock() = Some(stdin_error.clone());
            anyhow::bail!(stdin_error);
        }

        match tokio::time::timeout(Duration::from_millis(30000), rx).await {
            Ok(Ok(Ok(response))) => Ok(response),
            Ok(Ok(Err(error))) => anyhow::bail!(error),
            Ok(Err(_)) => {
                let error = self.inner.exit_error.lock().clone().unwrap_or_else(|| "Client stopped".to_string());
                anyhow::bail!(error)
            }
            Err(_) => {
                self.inner.pending_requests.lock().remove(&id);
                anyhow::bail!("Timeout waiting for response to {command_type}. Stderr: {}", self.inner.stderr.lock())
            }
        }
    }

    fn get_data<T: DeserializeOwned>(response: RpcResponse) -> anyhow::Result<T> {
        match response.result {
            Err(error) => anyhow::bail!(error),
            Ok(data) => Ok(serde_json::from_value(data.unwrap_or(Value::Null))?),
        }
    }
}

#[derive(Deserialize)]
struct DispositionData<T> {
    disposition: T,
}

#[derive(Deserialize)]
struct ModelsData {
    models: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct LevelsData {
    levels: Vec<ThinkingLevel>,
}

#[derive(Deserialize)]
struct MessagesData<T> {
    messages: Vec<T>,
}

#[derive(Deserialize)]
struct CommandsData {
    commands: Vec<RpcSlashCommand>,
}

#[derive(Deserialize)]
struct TextData {
    #[serde(default)]
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_child_script(contents: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("pi-rpc-client-exit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("child.sh");
        std::fs::write(&path, contents).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir, path.to_string_lossy().into_owned())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_an_in_flight_request_when_the_child_process_exits() {
        let (dir, path) = write_child_script("#!/bin/sh\nhead -c 1 >/dev/null\nexit 43\n");
        let client = RpcClient::new(Some(&RpcClientOptions { cli_path: Some(path), ..Default::default() }));
        client.start().await.unwrap();
        let err = client.get_commands().await.unwrap_err().to_string();
        assert!(err.contains("Agent process exited (code=43 signal=null)"), "{err}");
        client.stop().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sends_commands_and_routes_responses_and_events() {
        // A fake agent: answers clear_queue / clone, and emits an event first.
        let script = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"type":"clear_queue"'*)
      printf '{"type":"agent_settled","aborted":false}\n'
      printf '{"id":"%s","type":"response","command":"clear_queue","success":true,"data":{"steering":["Change direction"],"followUp":["Summarize when finished"]}}\n' "$id" ;;
    *'"type":"clone"'*)
      printf '{"id":"%s","type":"response","command":"clone","success":true,"data":{"cancelled":false}}\n' "$id" ;;
    *)
      printf '{"id":"%s","type":"response","command":"x","success":false,"error":"nope"}\n' "$id" ;;
  esac
done
"#;
        let (dir, path) = write_child_script(script);
        let client = RpcClient::new(Some(&RpcClientOptions { cli_path: Some(path), ..Default::default() }));
        client.start().await.unwrap();
        let events: Arc<Mutex<Vec<String>>> = Arc::default();
        let e = events.clone();
        let unsubscribe = client.on_event(&(Arc::new(move |event: JsonAgentSessionEvent| {
            e.lock().push(event.type_name());
        }) as RpcEventListener));
        let result = client.clear_queue().await.unwrap();
        assert_eq!(result.steering, vec!["Change direction".to_string()]);
        assert_eq!(result.follow_up, vec!["Summarize when finished".to_string()]);
        assert_eq!(*events.lock(), vec!["agent_settled".to_string()]);
        unsubscribe();
        assert!(!client.clone().await.unwrap().cancelled);
        // Like TS, `abort()` ignores the response payload; data accessors surface errors.
        client.abort().await.unwrap();
        assert_eq!(client.get_state().await.unwrap_err().to_string(), "nope");
        client.stop().await.unwrap();
        assert_eq!(client.get_state().await.unwrap_err().to_string(), "Client not started");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
