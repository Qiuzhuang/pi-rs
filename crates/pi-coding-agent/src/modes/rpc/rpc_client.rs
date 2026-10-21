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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Distributive Omit that works with union types
pub type DistributiveOmit = Value;

/// RpcCommand without the id field (for internal send)
pub type RpcCommandBody = DistributiveOmit;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClientOptions {
    /// Path to the CLI entry point (default: searches for dist/cli.js)
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
pub enum RpcClientStreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl RpcClientStreamingBehavior {
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
pub enum RpcClientMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

impl RpcClientMode {
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

pub struct RpcClient {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl RpcClient {
    pub fn new(options: Option<&RpcClientOptions>) -> Self {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.constructor")
    }

    /// Start the RPC agent process.
    pub async fn start(&self) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.start")
    }

    /// Stop the RPC agent process.
    pub async fn stop(&self) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.stop")
    }

    /// Subscribe to agent events.
    pub fn on_event(&self, listener: &RpcEventListener) -> Arc<dyn Fn() + Send + Sync> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.onEvent")
    }

    /// Get collected stderr output (useful for debugging).
    pub fn get_stderr(&self) -> String {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getStderr")
    }

    /// Send a prompt to the agent.
    /// Returns the prompt's disposition after acceptance; use onEvent() to receive streaming events.
    /// If the disposition is "handled", no run started for this prompt, so don't wait for agent_settled.
    pub async fn prompt(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
        streaming_behavior: Option<RpcClientStreamingBehavior>,
    ) -> anyhow::Result<PromptDisposition> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.prompt")
    }

    /// Queue a steering message to interrupt the agent mid-run.
    pub async fn steer(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
    ) -> anyhow::Result<QueuedInputDisposition> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.steer")
    }

    /// Queue a follow-up message to be processed after the agent finishes.
    pub async fn follow_up(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
    ) -> anyhow::Result<QueuedInputDisposition> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.followUp")
    }

    /// Abort current operation.
    pub async fn abort(&self) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.abort")
    }

    /// Clear queued steering and follow-up messages, returning their text.
    pub async fn clear_queue(&self) -> anyhow::Result<RpcClientClearQueueResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.clearQueue")
    }

    /// Start a new session, optionally with parent tracking.
    /// @param parentSession - Optional parent session path for lineage tracking
    /// @returns Object with `cancelled: true` if an extension cancelled the new session
    pub async fn new_session(
        &self,
        parent_session: Option<&str>,
    ) -> anyhow::Result<RpcClientNewSessionResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.newSession")
    }

    /// Get current session state.
    pub async fn get_state(&self) -> anyhow::Result<RpcSessionState> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getState")
    }

    /// Set model by provider and ID.
    pub async fn set_model(
        &self,
        provider: &str,
        model_id: &str,
    ) -> anyhow::Result<RpcClientSetModelResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.setModel")
    }

    /// Cycle to next model.
    pub async fn cycle_model(&self) -> anyhow::Result<Option<RpcClientCycleModelResult>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.cycleModel")
    }

    /// Get list of available models.
    pub async fn get_available_models(&self) -> anyhow::Result<Vec<ModelInfo>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getAvailableModels")
    }

    /// Set thinking level.
    pub async fn set_thinking_level(&self, level: ThinkingLevel) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.setThinkingLevel")
    }

    /// Cycle thinking level.
    pub async fn cycle_thinking_level(
        &self,
    ) -> anyhow::Result<Option<RpcClientCycleThinkingLevelResult>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.cycleThinkingLevel")
    }

    /// Get list of available thinking levels for the current model.
    pub async fn get_available_thinking_levels(&self) -> anyhow::Result<Vec<ThinkingLevel>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getAvailableThinkingLevels")
    }

    /// Set steering mode.
    pub async fn set_steering_mode(&self, mode: RpcClientMode) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.setSteeringMode")
    }

    /// Set follow-up mode.
    pub async fn set_follow_up_mode(&self, mode: RpcClientMode) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.setFollowUpMode")
    }

    /// Compact session context.
    pub async fn compact(
        &self,
        custom_instructions: Option<&str>,
    ) -> anyhow::Result<CompactionResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.compact")
    }

    /// Set auto-compaction enabled/disabled.
    pub async fn set_auto_compaction(&self, enabled: bool) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.setAutoCompaction")
    }

    /// Set auto-retry enabled/disabled.
    pub async fn set_auto_retry(&self, enabled: bool) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.setAutoRetry")
    }

    /// Abort in-progress retry.
    pub async fn abort_retry(&self) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.abortRetry")
    }

    /// Execute a bash command.
    pub async fn bash(&self, command: &str) -> anyhow::Result<BashResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.bash")
    }

    /// Abort running bash command.
    pub async fn abort_bash(&self) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.abortBash")
    }

    /// Get session statistics.
    pub async fn get_session_stats(&self) -> anyhow::Result<SessionStats> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getSessionStats")
    }

    /// Export session to HTML.
    pub async fn export_html(
        &self,
        output_path: Option<&str>,
    ) -> anyhow::Result<RpcClientExportHtmlResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.exportHtml")
    }

    /// Switch to a different session file.
    /// @returns Object with `cancelled: true` if an extension cancelled the switch
    pub async fn switch_session(
        &self,
        session_path: &str,
    ) -> anyhow::Result<RpcClientSwitchSessionResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.switchSession")
    }

    /// Fork from a specific message.
    /// @returns Object with `text` (the message text) and `cancelled` (if extension cancelled)
    pub async fn fork(&self, entry_id: &str) -> anyhow::Result<RpcClientForkResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.fork")
    }

    /// Clone the current active branch into a new session.
    /// @returns Object with `cancelled: true` if an extension cancelled the clone
    pub async fn clone(&self) -> anyhow::Result<RpcClientCloneResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.clone")
    }

    /// Get messages available for forking.
    pub async fn get_fork_messages(&self) -> anyhow::Result<Vec<RpcClientGetForkMessagesResult>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getForkMessages")
    }

    /// Get session entries in append order, optionally only those after the `since` entry id.
    pub async fn get_entries(
        &self,
        since: Option<&str>,
    ) -> anyhow::Result<RpcClientGetEntriesResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getEntries")
    }

    /// Get the session entry tree.
    pub async fn get_tree(&self) -> anyhow::Result<RpcClientGetTreeResult> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getTree")
    }

    /// Get text of last assistant message.
    pub async fn get_last_assistant_text(&self) -> anyhow::Result<Option<String>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getLastAssistantText")
    }

    /// Set the session display name.
    pub async fn set_session_name(&self, name: &str) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.setSessionName")
    }

    /// Get all messages in the session.
    pub async fn get_messages(&self) -> anyhow::Result<Vec<AgentMessage>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getMessages")
    }

    /// Get available commands (extension commands, prompt templates, skills).
    pub async fn get_commands(&self) -> anyhow::Result<Vec<RpcSlashCommand>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.getCommands")
    }

    /// Wait for agent to become idle (no streaming).
    /// Resolves when agent_settled event is received.
    pub async fn wait_for_idle(&self, timeout: Option<i64>) -> anyhow::Result<()> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.waitForIdle")
    }

    /// Collect events until agent becomes idle.
    pub async fn collect_events(
        &self,
        timeout: Option<i64>,
    ) -> anyhow::Result<Vec<JsonAgentSessionEvent>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.collectEvents")
    }

    /// Send prompt and wait for completion, returning all events.
    pub async fn prompt_and_wait(
        &self,
        message: &str,
        images: Option<&[ImageContent]>,
        timeout: Option<i64>,
    ) -> anyhow::Result<Vec<JsonAgentSessionEvent>> {
        todo!("port modes/rpc/rpc-client.ts:RpcClient.promptAndWait")
    }
}
