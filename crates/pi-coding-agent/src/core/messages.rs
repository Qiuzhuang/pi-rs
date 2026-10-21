//! Port of `packages/coding-agent/src/core/messages.ts`.
//!
//! Custom message types and transformers for the coding agent.
//!
//! The coding agent extends `AgentMessage` with four custom roles (`bashExecution`, `custom`,
//! `branchSummary`, `compactionSummary`). In Rust they travel as `AgentMessage::Custom`
//! (pi-agent's raw-JSON custom message); [`CodingAgentMessage`] converts between the raw form and
//! the typed structs below. Each struct keeps its `role` field so it serializes exactly like the
//! TS object on its own (e.g. inside extension events).

use pi_agent::types::AgentMessage;
use pi_ai::types::{Message, UserContent, UserContentBlock, UserMessage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const COMPACTION_SUMMARY_PREFIX: &str = "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";

pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";

pub const BRANCH_SUMMARY_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";

pub const BRANCH_SUMMARY_SUFFIX: &str = "</summary>";

pub const BASH_EXECUTION_ROLE: &str = "bashExecution";
pub const CUSTOM_ROLE: &str = "custom";
pub const BRANCH_SUMMARY_ROLE: &str = "branchSummary";
pub const COMPACTION_SUMMARY_ROLE: &str = "compactionSummary";

/// Message type for bash executions via the ! command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashExecutionMessage {
    /// Always "bashExecution".
    pub role: String,
    pub command: String,
    pub output: String,
    /// `number | undefined` in TS (`null` is tolerated when reading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    pub cancelled: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
    pub timestamp: i64,
    /// If true, this message is excluded from LLM context (!! prefix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_context: Option<bool>,
}

/// Message type for extension-injected messages via sendMessage().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomMessage {
    /// Always "custom".
    pub role: String,
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryMessage {
    /// Always "branchSummary".
    pub role: String,
    pub summary: String,
    /// `string | null`.
    pub from_id: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryMessage {
    /// Always "compactionSummary".
    pub role: String,
    pub summary: String,
    pub tokens_before: u64,
    pub timestamp: i64,
}

/// The coding-agent custom messages (TS declaration merging into `CustomAgentMessages`).
#[derive(Debug, Clone, PartialEq)]
pub enum CodingAgentMessage {
    BashExecution(BashExecutionMessage),
    Custom(CustomMessage),
    BranchSummary(BranchSummaryMessage),
    CompactionSummary(CompactionSummaryMessage),
}

impl CodingAgentMessage {
    /// Parse a coding-agent custom message out of an `AgentMessage`. `None` for LLM messages and
    /// unknown custom roles.
    pub fn from_agent_message(message: &AgentMessage) -> Option<Self> {
        let AgentMessage::Custom(custom) = message else {
            return None;
        };
        match custom.role.as_str() {
            BASH_EXECUTION_ROLE => custom.to_typed().ok().map(Self::BashExecution),
            CUSTOM_ROLE => custom.to_typed().ok().map(Self::Custom),
            BRANCH_SUMMARY_ROLE => custom.to_typed().ok().map(Self::BranchSummary),
            COMPACTION_SUMMARY_ROLE => custom.to_typed().ok().map(Self::CompactionSummary),
            _ => None,
        }
    }

    /// Convert into the raw `AgentMessage::Custom` form.
    pub fn to_agent_message(&self) -> AgentMessage {
        let result = match self {
            Self::BashExecution(m) => AgentMessage::custom(m),
            Self::Custom(m) => AgentMessage::custom(m),
            Self::BranchSummary(m) => AgentMessage::custom(m),
            Self::CompactionSummary(m) => AgentMessage::custom(m),
        };
        result.expect("coding-agent messages serialize to objects with a role")
    }
}

impl From<BashExecutionMessage> for AgentMessage {
    fn from(value: BashExecutionMessage) -> Self {
        CodingAgentMessage::BashExecution(value).to_agent_message()
    }
}

impl From<CustomMessage> for AgentMessage {
    fn from(value: CustomMessage) -> Self {
        CodingAgentMessage::Custom(value).to_agent_message()
    }
}

impl From<BranchSummaryMessage> for AgentMessage {
    fn from(value: BranchSummaryMessage) -> Self {
        CodingAgentMessage::BranchSummary(value).to_agent_message()
    }
}

impl From<CompactionSummaryMessage> for AgentMessage {
    fn from(value: CompactionSummaryMessage) -> Self {
        CodingAgentMessage::CompactionSummary(value).to_agent_message()
    }
}

/// `new Date(timestamp).getTime()` for ISO timestamps; 0 when unparseable.
pub fn parse_timestamp_ms(timestamp: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

/// Convert a BashExecutionMessage to user message text for LLM context.
pub fn bash_execution_to_text(msg: &BashExecutionMessage) -> String {
    let mut text = format!("Ran `{}`\n", msg.command);
    if !msg.output.is_empty() {
        text.push_str(&format!("```\n{}\n```", msg.output));
    } else {
        text.push_str("(no output)");
    }
    if msg.cancelled {
        text.push_str("\n\n(command cancelled)");
    } else if let Some(code) = msg.exit_code
        && code != 0
    {
        text.push_str(&format!("\n\nCommand exited with code {code}"));
    }
    if msg.truncated
        && let Some(path) = &msg.full_output_path
        && !path.is_empty()
    {
        text.push_str(&format!("\n\n[Output truncated. Full output: {path}]"));
    }
    text
}

pub fn create_branch_summary_message(
    summary: &str,
    from_id: &str,
    timestamp: &str,
) -> BranchSummaryMessage {
    BranchSummaryMessage {
        role: BRANCH_SUMMARY_ROLE.to_string(),
        summary: summary.to_string(),
        from_id: Some(from_id.to_string()),
        timestamp: parse_timestamp_ms(timestamp),
    }
}

pub fn create_compaction_summary_message(
    summary: &str,
    tokens_before: u64,
    timestamp: &str,
) -> CompactionSummaryMessage {
    CompactionSummaryMessage {
        role: COMPACTION_SUMMARY_ROLE.to_string(),
        summary: summary.to_string(),
        tokens_before,
        timestamp: parse_timestamp_ms(timestamp),
    }
}

/// Convert CustomMessageEntry to AgentMessage format.
pub fn create_custom_message(
    custom_type: &str,
    content: UserContent,
    display: bool,
    details: Option<Value>,
    timestamp: &str,
) -> CustomMessage {
    CustomMessage {
        role: CUSTOM_ROLE.to_string(),
        custom_type: custom_type.to_string(),
        content,
        display,
        details,
        timestamp: parse_timestamp_ms(timestamp),
    }
}

fn user_text(text: String, timestamp: i64) -> Message {
    Message::User(UserMessage::new(
        UserContent::Blocks(vec![UserContentBlock::text(text)]),
        timestamp,
    ))
}

/// Transform AgentMessages (including custom types) to LLM-compatible Messages.
///
/// Custom messages with roles other than the four coding-agent roles are dropped, like the
/// exhaustive switch in TS.
pub fn convert_to_llm(messages: &[AgentMessage]) -> Vec<Message> {
    messages
        .iter()
        .filter_map(|m| match m {
            AgentMessage::Custom(_) => match CodingAgentMessage::from_agent_message(m)? {
                CodingAgentMessage::BashExecution(msg) => {
                    if msg.exclude_from_context.unwrap_or(false) {
                        return None;
                    }
                    Some(user_text(bash_execution_to_text(&msg), msg.timestamp))
                }
                CodingAgentMessage::Custom(msg) => {
                    let content = match msg.content {
                        UserContent::Text(text) => {
                            UserContent::Blocks(vec![UserContentBlock::text(text)])
                        }
                        blocks => blocks,
                    };
                    Some(Message::User(UserMessage::new(content, msg.timestamp)))
                }
                CodingAgentMessage::BranchSummary(msg) => Some(user_text(
                    format!(
                        "{BRANCH_SUMMARY_PREFIX}{}{BRANCH_SUMMARY_SUFFIX}",
                        msg.summary
                    ),
                    msg.timestamp,
                )),
                CodingAgentMessage::CompactionSummary(msg) => Some(user_text(
                    format!(
                        "{COMPACTION_SUMMARY_PREFIX}{}{COMPACTION_SUMMARY_SUFFIX}",
                        msg.summary
                    ),
                    msg.timestamp,
                )),
            },
            other => other.to_message(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash(output: &str, exit_code: Option<i64>, cancelled: bool) -> BashExecutionMessage {
        BashExecutionMessage {
            role: BASH_EXECUTION_ROLE.into(),
            command: "ls".into(),
            output: output.into(),
            exit_code,
            cancelled,
            truncated: false,
            full_output_path: None,
            timestamp: 1,
            exclude_from_context: None,
        }
    }

    #[test]
    fn bash_text_formats() {
        assert_eq!(
            bash_execution_to_text(&bash("", Some(0), false)),
            "Ran `ls`\n(no output)"
        );
        assert_eq!(
            bash_execution_to_text(&bash("a", Some(2), false)),
            "Ran `ls`\n```\na\n```\n\nCommand exited with code 2"
        );
        assert_eq!(
            bash_execution_to_text(&bash("a", Some(2), true)),
            "Ran `ls`\n```\na\n```\n\n(command cancelled)"
        );
    }

    #[test]
    fn custom_messages_round_trip_through_agent_message() {
        let msg = create_custom_message(
            "x",
            UserContent::Text("hi".into()),
            true,
            None,
            "2024-01-01T00:00:00.000Z",
        );
        let agent: AgentMessage = msg.clone().into();
        assert_eq!(agent.role(), "custom");
        assert_eq!(
            CodingAgentMessage::from_agent_message(&agent),
            Some(CodingAgentMessage::Custom(msg))
        );
        let llm = convert_to_llm(&[agent]);
        assert_eq!(llm.len(), 1);
    }
}
