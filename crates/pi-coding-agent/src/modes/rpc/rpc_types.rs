//! Port of `packages/coding-agent/src/modes/rpc/rpc-types.ts`.

use crate::core::agent_session::PromptDisposition;
use crate::core::agent_session::QueuedInputDisposition;
use crate::core::agent_session::SessionStats;
use crate::core::bash_executor::BashResult;
use crate::core::compaction::compaction::CompactionResult;
use crate::core::extensions::types::WidgetPlacement;
use crate::core::session_manager::SessionEntry;
use crate::core::session_manager::SessionTreeNode;
use crate::core::slash_commands::SlashCommandSource;
use crate::core::source_info::SourceInfo;
use pi_agent::types::AgentMessage;
use pi_agent::types::ThinkingLevel;
use pi_ai::types::ImageContent;
use pi_ai::types::Model;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RpcCommandPromptStreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

impl RpcCommandPromptStreamingBehavior {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::FollowUp => "followUp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RpcCommandSetSteeringModeMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

impl RpcCommandSetSteeringModeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one-at-a-time",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum RpcCommand {
    #[serde(rename = "prompt")]
    Prompt {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        streaming_behavior: Option<RpcCommandPromptStreamingBehavior>,
    },
    #[serde(rename = "steer")]
    Steer {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
    },
    #[serde(rename = "follow_up")]
    FollowUp {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
    },
    #[serde(rename = "abort")]
    Abort {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "clear_queue")]
    ClearQueue {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "new_session")]
    NewSession {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_session: Option<String>,
    },
    #[serde(rename = "get_state")]
    GetState {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "set_model")]
    SetModel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        provider: String,
        model_id: String,
    },
    #[serde(rename = "cycle_model")]
    CycleModel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "get_available_models")]
    GetAvailableModels {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "set_thinking_level")]
    SetThinkingLevel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        level: ThinkingLevel,
    },
    #[serde(rename = "cycle_thinking_level")]
    CycleThinkingLevel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "get_available_thinking_levels")]
    GetAvailableThinkingLevels {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "set_steering_mode")]
    SetSteeringMode {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        mode: RpcCommandSetSteeringModeMode,
    },
    #[serde(rename = "set_follow_up_mode")]
    SetFollowUpMode {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        mode: RpcCommandSetSteeringModeMode,
    },
    #[serde(rename = "compact")]
    Compact {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom_instructions: Option<String>,
    },
    #[serde(rename = "set_auto_compaction")]
    SetAutoCompaction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        enabled: bool,
    },
    #[serde(rename = "set_auto_retry")]
    SetAutoRetry {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        enabled: bool,
    },
    #[serde(rename = "abort_retry")]
    AbortRetry {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "bash")]
    Bash {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclude_from_context: Option<bool>,
    },
    #[serde(rename = "abort_bash")]
    AbortBash {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "get_session_stats")]
    GetSessionStats {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "export_html")]
    ExportHtml {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_path: Option<String>,
    },
    #[serde(rename = "switch_session")]
    SwitchSession {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        session_path: String,
    },
    #[serde(rename = "fork")]
    Fork {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        entry_id: String,
    },
    #[serde(rename = "clone")]
    Clone {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "get_fork_messages")]
    GetForkMessages {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "get_entries")]
    GetEntries {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<String>,
    },
    #[serde(rename = "get_tree")]
    GetTree {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "get_last_assistant_text")]
    GetLastAssistantText {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "set_session_name")]
    SetSessionName {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
    },
    #[serde(rename = "get_messages")]
    GetMessages {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "get_commands")]
    GetCommands {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
}

/// A command available for invocation via prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSlashCommand {
    /// Command name (without leading slash)
    pub name: String,
    /// Human-readable description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What kind of command this is
    pub source: SlashCommandSource,
    /// Source metadata for the owning resource
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSessionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    pub thinking_level: ThinkingLevel,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub steering_mode: RpcCommandSetSteeringModeMode,
    pub follow_up_mode: RpcCommandSetSteeringModeMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    pub auto_compaction_enabled: bool,
    pub message_count: i64,
    pub pending_message_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant1Data {
    pub disposition: PromptDisposition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "prompt".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant1Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant2Data {
    pub disposition: QueuedInputDisposition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant2 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "steer".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant2Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant3Data {
    pub disposition: QueuedInputDisposition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant3 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "follow_up".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant3Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant4 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "abort".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant5Data {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant5 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "clear_queue".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant5Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant6Data {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant6 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "new_session".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant6Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant7 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_state".
    pub command: String,
    pub success: bool,
    pub data: RpcSessionState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant8 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "set_model".
    pub command: String,
    pub success: bool,
    pub data: Model,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant9Data {
    pub model: Model,
    pub thinking_level: ThinkingLevel,
    pub is_scoped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant9 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "cycle_model".
    pub command: String,
    pub success: bool,
    #[serde(default)]
    pub data: Option<RpcResponseVariant9Data>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant10Data {
    pub models: Vec<Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant10 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_available_models".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant10Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant11 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "set_thinking_level".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant12Data {
    pub level: ThinkingLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant12 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "cycle_thinking_level".
    pub command: String,
    pub success: bool,
    #[serde(default)]
    pub data: Option<RpcResponseVariant12Data>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant13Data {
    pub levels: Vec<ThinkingLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant13 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_available_thinking_levels".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant13Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant14 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "set_steering_mode".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant15 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "set_follow_up_mode".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant16 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "compact".
    pub command: String,
    pub success: bool,
    pub data: CompactionResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant17 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "set_auto_compaction".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant18 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "set_auto_retry".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant19 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "abort_retry".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant20 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "bash".
    pub command: String,
    pub success: bool,
    pub data: BashResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant21 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "abort_bash".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant22 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_session_stats".
    pub command: String,
    pub success: bool,
    pub data: SessionStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant23Data {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant23 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "export_html".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant23Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant24Data {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant24 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "switch_session".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant24Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant25Data {
    pub text: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant25 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "fork".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant25Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant26Data {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant26 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "clone".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant26Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant27DataMessages {
    pub entry_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant27Data {
    pub messages: Vec<RpcResponseVariant27DataMessages>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant27 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_fork_messages".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant27Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant28Data {
    pub entries: Vec<SessionEntry>,
    #[serde(default)]
    pub leaf_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant28 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_entries".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant28Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant29Data {
    pub tree: Vec<SessionTreeNode>,
    #[serde(default)]
    pub leaf_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant29 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_tree".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant29Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant30Data {
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant30 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_last_assistant_text".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant30Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant31 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "set_session_name".
    pub command: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant32Data {
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant32 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_messages".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant32Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant33Data {
    pub commands: Vec<RpcSlashCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant33 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    /// Always "get_commands".
    pub command: String,
    pub success: bool,
    pub data: RpcResponseVariant33Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponseVariant34 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always "response".
    pub r#type: String,
    pub command: String,
    pub success: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcResponse {
    RpcResponseVariant1(RpcResponseVariant1),
    RpcResponseVariant2(RpcResponseVariant2),
    RpcResponseVariant3(RpcResponseVariant3),
    RpcResponseVariant4(RpcResponseVariant4),
    RpcResponseVariant5(RpcResponseVariant5),
    RpcResponseVariant6(RpcResponseVariant6),
    RpcResponseVariant7(RpcResponseVariant7),
    RpcResponseVariant8(RpcResponseVariant8),
    RpcResponseVariant9(RpcResponseVariant9),
    RpcResponseVariant10(RpcResponseVariant10),
    RpcResponseVariant11(RpcResponseVariant11),
    RpcResponseVariant12(RpcResponseVariant12),
    RpcResponseVariant13(RpcResponseVariant13),
    RpcResponseVariant14(RpcResponseVariant14),
    RpcResponseVariant15(RpcResponseVariant15),
    RpcResponseVariant16(RpcResponseVariant16),
    RpcResponseVariant17(RpcResponseVariant17),
    RpcResponseVariant18(RpcResponseVariant18),
    RpcResponseVariant19(RpcResponseVariant19),
    RpcResponseVariant20(RpcResponseVariant20),
    RpcResponseVariant21(RpcResponseVariant21),
    RpcResponseVariant22(RpcResponseVariant22),
    RpcResponseVariant23(RpcResponseVariant23),
    RpcResponseVariant24(RpcResponseVariant24),
    RpcResponseVariant25(RpcResponseVariant25),
    RpcResponseVariant26(RpcResponseVariant26),
    RpcResponseVariant27(RpcResponseVariant27),
    RpcResponseVariant28(RpcResponseVariant28),
    RpcResponseVariant29(RpcResponseVariant29),
    RpcResponseVariant30(RpcResponseVariant30),
    RpcResponseVariant31(RpcResponseVariant31),
    RpcResponseVariant32(RpcResponseVariant32),
    RpcResponseVariant33(RpcResponseVariant33),
    RpcResponseVariant34(RpcResponseVariant34),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RpcExtensionUIRequestNotifyNotifyType {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

impl RpcExtensionUIRequestNotifyNotifyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Emitted when an extension needs user input
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all_fields = "camelCase")]
pub enum RpcExtensionUIRequest {
    #[serde(rename = "select")]
    Select {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        title: String,
        options: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<i64>,
    },
    #[serde(rename = "confirm")]
    Confirm {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        title: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<i64>,
    },
    #[serde(rename = "input")]
    Input {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<i64>,
    },
    #[serde(rename = "editor")]
    Editor {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefill: Option<String>,
    },
    #[serde(rename = "notify")]
    Notify {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notify_type: Option<RpcExtensionUIRequestNotifyNotifyType>,
    },
    #[serde(rename = "setStatus")]
    SetStatus {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        status_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status_text: Option<String>,
    },
    #[serde(rename = "setWidget")]
    SetWidget {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        widget_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        widget_lines: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        widget_placement: Option<WidgetPlacement>,
    },
    #[serde(rename = "setTitle")]
    SetTitle {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        title: String,
    },
    #[serde(rename = "set_editor_text")]
    SetEditorText {
        /// Always "extension_ui_request".
        r#type: String,
        id: String,
        text: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcExtensionUIResponseVariant1 {
    /// Always "extension_ui_response".
    pub r#type: String,
    pub id: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcExtensionUIResponseVariant2 {
    /// Always "extension_ui_response".
    pub r#type: String,
    pub id: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcExtensionUIResponseVariant3 {
    /// Always "extension_ui_response".
    pub r#type: String,
    pub id: String,
    pub cancelled: bool,
}

/// Response to an extension UI request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcExtensionUIResponse {
    RpcExtensionUIResponseVariant1(RpcExtensionUIResponseVariant1),
    RpcExtensionUIResponseVariant2(RpcExtensionUIResponseVariant2),
    RpcExtensionUIResponseVariant3(RpcExtensionUIResponseVariant3),
}

pub type RpcCommandType = Value;
