//! Port of `packages/coding-agent/src/modes/rpc/rpc-types.ts`.
//!
//! RPC protocol types for headless operation. Commands are sent as JSON lines on stdin;
//! responses and events are emitted as JSON lines on stdout.
//!
//! Field order of serialized objects matches the TS output: responses are
//! `{ id, type, command, success, data | error }`, UI requests `{ type, id, method, ... }`.

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use pi_agent::types::{AgentMessage, ThinkingLevel};
use pi_ai::types::{ImageContent, Model};

use crate::core::agent_session::{PromptDisposition, QueuedInputDisposition};
use crate::core::extensions::types::{NotifyType, StreamingBehavior, WidgetPlacement};
use crate::core::session_manager::{SessionEntry, SessionTreeNode};
use crate::core::slash_commands::SlashCommandSource;
use crate::core::source_info::SourceInfo;

/// `"all" | "one-at-a-time"` queue delivery mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RpcQueueMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "one-at-a-time")]
    OneAtATime,
}

// ============================================================================
// RPC Commands (stdin)
// ============================================================================

/// A command line read from stdin: optional `id` plus the command, tagged by `type`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcCommand {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub kind: RpcCommandKind,
}

/// The command variants of the TS `RpcCommand` union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum RpcCommandKind {
    // Prompting
    Prompt {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        streaming_behavior: Option<StreamingBehavior>,
    },
    Steer {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
    },
    FollowUp {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
    },
    Abort,
    ClearQueue,
    NewSession {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_session: Option<String>,
    },
    // State
    GetState,
    // Model
    SetModel {
        provider: String,
        model_id: String,
    },
    CycleModel,
    GetAvailableModels,
    // Thinking
    SetThinkingLevel {
        level: ThinkingLevel,
    },
    CycleThinkingLevel,
    GetAvailableThinkingLevels,
    // Queue modes
    SetSteeringMode {
        mode: RpcQueueMode,
    },
    SetFollowUpMode {
        mode: RpcQueueMode,
    },
    // Compaction
    Compact {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom_instructions: Option<String>,
    },
    SetAutoCompaction {
        enabled: bool,
    },
    // Retry
    SetAutoRetry {
        enabled: bool,
    },
    AbortRetry,
    // Bash
    Bash {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclude_from_context: Option<bool>,
    },
    AbortBash,
    // Session
    GetSessionStats,
    ExportHtml {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_path: Option<String>,
    },
    SwitchSession {
        session_path: String,
    },
    Fork {
        entry_id: String,
    },
    Clone,
    GetForkMessages,
    GetEntries {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<String>,
    },
    GetTree,
    GetLastAssistantText,
    SetSessionName {
        name: String,
    },
    // Messages
    GetMessages,
    // Commands (available for invocation via prompt)
    GetCommands,
}

impl RpcCommandKind {
    /// The `type` discriminant (TS `RpcCommandType`).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Prompt { .. } => "prompt",
            Self::Steer { .. } => "steer",
            Self::FollowUp { .. } => "follow_up",
            Self::Abort => "abort",
            Self::ClearQueue => "clear_queue",
            Self::NewSession { .. } => "new_session",
            Self::GetState => "get_state",
            Self::SetModel { .. } => "set_model",
            Self::CycleModel => "cycle_model",
            Self::GetAvailableModels => "get_available_models",
            Self::SetThinkingLevel { .. } => "set_thinking_level",
            Self::CycleThinkingLevel => "cycle_thinking_level",
            Self::GetAvailableThinkingLevels => "get_available_thinking_levels",
            Self::SetSteeringMode { .. } => "set_steering_mode",
            Self::SetFollowUpMode { .. } => "set_follow_up_mode",
            Self::Compact { .. } => "compact",
            Self::SetAutoCompaction { .. } => "set_auto_compaction",
            Self::SetAutoRetry { .. } => "set_auto_retry",
            Self::AbortRetry => "abort_retry",
            Self::Bash { .. } => "bash",
            Self::AbortBash => "abort_bash",
            Self::GetSessionStats => "get_session_stats",
            Self::ExportHtml { .. } => "export_html",
            Self::SwitchSession { .. } => "switch_session",
            Self::Fork { .. } => "fork",
            Self::Clone => "clone",
            Self::GetForkMessages => "get_fork_messages",
            Self::GetEntries { .. } => "get_entries",
            Self::GetTree => "get_tree",
            Self::GetLastAssistantText => "get_last_assistant_text",
            Self::SetSessionName { .. } => "set_session_name",
            Self::GetMessages => "get_messages",
            Self::GetCommands => "get_commands",
        }
    }
}

/// TS `RpcCommandType = RpcCommand["type"]`.
pub type RpcCommandType = String;

// ============================================================================
// RPC Slash Command (for get_commands response)
// ============================================================================

/// A command available for invocation via prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSlashCommand {
    /// Command name (without leading slash).
    pub name: String,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What kind of command this is.
    pub source: SlashCommandSource,
    /// Source metadata for the owning resource.
    pub source_info: SourceInfo,
}

// ============================================================================
// RPC State
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSessionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    pub thinking_level: ThinkingLevel,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub steering_mode: RpcQueueMode,
    pub follow_up_mode: RpcQueueMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    pub auto_compaction_enabled: bool,
    pub message_count: u64,
    pub pending_message_count: u64,
}

// ============================================================================
// RPC Responses (stdout)
// ============================================================================

/// A response line: `{ id?, type: "response", command, success, data? | error }`.
///
/// `data` is kept as JSON so every command's payload serializes exactly as in TS; the typed
/// payloads are the `*Data` structs below (`RpcResponse::success(id, cmd, Some(&data))`).
#[derive(Debug, Clone, PartialEq)]
pub struct RpcResponse {
    pub id: Option<String>,
    pub command: String,
    /// `Ok(data)` for `success: true` (`None` = no `data` field; `Some(Value::Null)` = `data: null`),
    /// `Err(message)` for `success: false`.
    pub result: Result<Option<Value>, String>,
}

impl RpcResponse {
    /// `success(id, command, data?)` of rpc-mode.ts.
    pub fn success<T: Serialize>(id: Option<String>, command: &str, data: Option<&T>) -> anyhow::Result<Self> {
        Ok(Self {
            id,
            command: command.to_string(),
            result: Ok(match data {
                Some(d) => Some(serde_json::to_value(d)?),
                None => None,
            }),
        })
    }

    /// `error(id, command, message)` of rpc-mode.ts.
    pub fn error(id: Option<String>, command: &str, message: impl Into<String>) -> Self {
        Self {
            id,
            command: command.to_string(),
            result: Err(message.into()),
        }
    }

    pub fn is_success(&self) -> bool {
        self.result.is_ok()
    }
}

impl Serialize for RpcResponse {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let Some(id) = &self.id {
            map.serialize_entry("id", id)?;
        }
        map.serialize_entry("type", "response")?;
        map.serialize_entry("command", &self.command)?;
        match &self.result {
            Ok(data) => {
                map.serialize_entry("success", &true)?;
                if let Some(data) = data {
                    map.serialize_entry("data", data)?;
                }
            }
            Err(error) => {
                map.serialize_entry("success", &false)?;
                map.serialize_entry("error", error)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for RpcResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut map = Map::<String, Value>::deserialize(deserializer)?;
        if map.get("type").and_then(Value::as_str) != Some("response") {
            return Err(D::Error::custom("expected type \"response\""));
        }
        let id = match map.shift_remove("id") {
            Some(Value::String(s)) => Some(s),
            _ => None,
        };
        let command = match map.shift_remove("command") {
            Some(Value::String(s)) => s,
            _ => return Err(D::Error::custom("missing command")),
        };
        let success = map.get("success").and_then(Value::as_bool).unwrap_or(false);
        let result = if success {
            Ok(map.shift_remove("data"))
        } else {
            Err(map.shift_remove("error").and_then(|e| e.as_str().map(str::to_string)).unwrap_or_default())
        };
        Ok(Self { id, command, result })
    }
}

/// `data` of `prompt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcPromptData {
    pub disposition: PromptDisposition,
}

/// `data` of `steer` / `follow_up`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcQueuedData {
    pub disposition: QueuedInputDisposition,
}

/// `data` of `clear_queue`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcClearQueueData {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

/// `data` of `new_session`, `switch_session`, `clone`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcCancelledData {
    pub cancelled: bool,
}

/// `data` of `cycle_model` (or `null`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcCycleModelData {
    pub model: Model,
    pub thinking_level: ThinkingLevel,
    pub is_scoped: bool,
}

/// `data` of `get_available_models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcAvailableModelsData {
    pub models: Vec<Model>,
}

/// `data` of `cycle_thinking_level` (or `null`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcThinkingLevelData {
    pub level: ThinkingLevel,
}

/// `data` of `get_available_thinking_levels`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcThinkingLevelsData {
    pub levels: Vec<ThinkingLevel>,
}

/// `data` of `export_html`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcExportHtmlData {
    pub path: String,
}

/// `data` of `fork`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcForkData {
    pub text: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcForkMessage {
    pub entry_id: String,
    pub text: String,
}

/// `data` of `get_fork_messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcForkMessagesData {
    pub messages: Vec<RpcForkMessage>,
}

/// `data` of `get_entries`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcEntriesData {
    pub entries: Vec<SessionEntry>,
    pub leaf_id: Option<String>,
}

/// `data` of `get_tree`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTreeData {
    pub tree: Vec<SessionTreeNode>,
    pub leaf_id: Option<String>,
}

/// `data` of `get_last_assistant_text`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcLastAssistantTextData {
    pub text: Option<String>,
}

/// `data` of `get_messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcMessagesData {
    pub messages: Vec<AgentMessage>,
}

/// `data` of `get_commands`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcCommandsData {
    pub commands: Vec<RpcSlashCommand>,
}

// Remaining payloads: `get_state` → RpcSessionState, `set_model` → Model, `compact` →
// CompactionResult, `bash` → BashResult, `get_session_stats` → SessionStats.

// ============================================================================
// Extension UI Events (stdout)
// ============================================================================

/// Always `"extension_ui_request"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionUIRequestTag {
    #[default]
    #[serde(rename = "extension_ui_request")]
    ExtensionUiRequest,
}

/// Emitted when an extension needs user input: `{ type, id, method, ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcExtensionUIRequest {
    #[serde(rename = "type")]
    pub tag: ExtensionUIRequestTag,
    pub id: String,
    #[serde(flatten)]
    pub request: RpcExtensionUIRequestMethod,
}

/// The `method`-tagged payload of an [`RpcExtensionUIRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all_fields = "camelCase")]
pub enum RpcExtensionUIRequestMethod {
    #[serde(rename = "select")]
    Select {
        title: String,
        options: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "confirm")]
    Confirm {
        title: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "input")]
    Input {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "editor")]
    Editor {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefill: Option<String>,
    },
    #[serde(rename = "notify")]
    Notify {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notify_type: Option<NotifyType>,
    },
    #[serde(rename = "setStatus")]
    SetStatus {
        status_key: String,
        /// `undefined` clears the status (field omitted).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status_text: Option<String>,
    },
    #[serde(rename = "setWidget")]
    SetWidget {
        widget_key: String,
        /// `undefined` removes the widget (field omitted).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        widget_lines: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        widget_placement: Option<WidgetPlacement>,
    },
    #[serde(rename = "setTitle")]
    SetTitle { title: String },
    #[serde(rename = "set_editor_text")]
    SetEditorText { text: String },
}

// ============================================================================
// Extension UI Commands (stdin)
// ============================================================================

/// Always `"extension_ui_response"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionUIResponseTag {
    #[default]
    #[serde(rename = "extension_ui_response")]
    ExtensionUiResponse,
}

/// Response to an extension UI request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcExtensionUIResponse {
    #[serde(rename = "type")]
    pub tag: ExtensionUIResponseTag,
    pub id: String,
    #[serde(flatten)]
    pub response: RpcExtensionUIResponseValue,
}

/// `{ value } | { confirmed } | { cancelled: true }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcExtensionUIResponseValue {
    Value { value: String },
    Confirmed { confirmed: bool },
    Cancelled { cancelled: bool },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_parses_with_id() {
        let cmd: RpcCommand =
            serde_json::from_value(json!({"id": "1", "type": "set_model", "provider": "p", "modelId": "m"})).unwrap();
        assert_eq!(cmd.id.as_deref(), Some("1"));
        assert_eq!(cmd.kind.type_name(), "set_model");
        assert!(matches!(cmd.kind, RpcCommandKind::SetModel { ref model_id, .. } if model_id == "m"));
        let cmd: RpcCommand = serde_json::from_value(json!({"type": "abort"})).unwrap();
        assert!(matches!(cmd.kind, RpcCommandKind::Abort));
    }

    #[test]
    fn response_field_order() {
        let r = RpcResponse::success(Some("1".into()), "export_html", Some(&RpcExportHtmlData { path: "a".into() })).unwrap();
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"id":"1","type":"response","command":"export_html","success":true,"data":{"path":"a"}}"#
        );
        let e = RpcResponse::error(None, "x", "boom");
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"type":"response","command":"x","success":false,"error":"boom"}"#);
        let back: RpcResponse = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn ui_request_shape() {
        let req = RpcExtensionUIRequest {
            tag: ExtensionUIRequestTag::ExtensionUiRequest,
            id: "u".into(),
            request: RpcExtensionUIRequestMethod::SetStatus { status_key: "k".into(), status_text: None },
        };
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"type":"extension_ui_request","id":"u","method":"setStatus","statusKey":"k"}"#
        );
    }
}
