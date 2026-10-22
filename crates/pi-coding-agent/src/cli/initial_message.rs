//! Port of `packages/coding-agent/src/cli/initial-message.ts`.

use crate::cli::args::Args;
use pi_ai::types::ImageContent;
use serde::{Deserialize, Serialize};

/// Input of [`build_initial_message`]. `parsed.messages` is consumed (its first message is
/// moved into the initial prompt), as in TS.
pub struct InitialMessageInput<'a> {
    pub parsed: &'a mut Args,
    pub file_text: Option<String>,
    pub file_images: Option<Vec<ImageContent>>,
    pub stdin_content: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialMessageResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_images: Option<Vec<ImageContent>>,
}

/// Combine stdin content, @file text, and the first CLI message into a single
/// initial prompt for non-interactive mode.
pub fn build_initial_message(input: InitialMessageInput<'_>) -> InitialMessageResult {
    let mut parts: Vec<String> = Vec::new();
    if let Some(stdin_content) = input.stdin_content {
        parts.push(stdin_content);
    }
    if let Some(file_text) = input.file_text.filter(|t| !t.is_empty()) {
        parts.push(file_text);
    }
    if !input.parsed.messages.is_empty() {
        parts.push(input.parsed.messages.remove(0));
    }
    InitialMessageResult {
        initial_message: if parts.is_empty() { None } else { Some(parts.concat()) },
        initial_images: input.file_images.filter(|images| !images.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_args(messages: &[&str]) -> Args {
        Args { messages: messages.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn merges_piped_stdin_with_the_first_cli_message_into_one_prompt() {
        let mut parsed = create_args(&["Summarize the text given"]);
        let result = build_initial_message(InitialMessageInput {
            parsed: &mut parsed,
            file_text: None,
            file_images: None,
            stdin_content: Some("README contents\n".into()),
        });
        assert_eq!(result.initial_message.as_deref(), Some("README contents\nSummarize the text given"));
        assert!(parsed.messages.is_empty());
    }

    #[test]
    fn uses_stdin_as_the_initial_prompt_when_no_cli_message_is_present() {
        let mut parsed = create_args(&[]);
        let result = build_initial_message(InitialMessageInput {
            parsed: &mut parsed,
            file_text: None,
            file_images: None,
            stdin_content: Some("README contents".into()),
        });
        assert_eq!(result.initial_message.as_deref(), Some("README contents"));
        assert!(parsed.messages.is_empty());
    }

    #[test]
    fn combines_stdin_file_text_and_first_cli_message_in_one_prompt() {
        let mut parsed = create_args(&["Explain it", "Second message"]);
        let result = build_initial_message(InitialMessageInput {
            parsed: &mut parsed,
            file_text: Some("file\n".into()),
            file_images: None,
            stdin_content: Some("stdin\n".into()),
        });
        assert_eq!(result.initial_message.as_deref(), Some("stdin\nfile\nExplain it"));
        assert_eq!(parsed.messages, vec!["Second message".to_string()]);
    }

    #[test]
    fn no_parts_means_no_initial_message() {
        let mut parsed = create_args(&[]);
        let result = build_initial_message(InitialMessageInput {
            parsed: &mut parsed,
            file_text: Some(String::new()),
            file_images: Some(vec![]),
            stdin_content: None,
        });
        assert_eq!(result, InitialMessageResult::default());
    }
}
