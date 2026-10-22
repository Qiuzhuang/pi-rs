//! `test/agent-session-tree-navigation.test.ts` (an API-key e2e suite in TS) run on the faux
//! provider, plus regression #6324 (`test/suite/regressions/6324-branch-summary-ambient-auth.test.ts`).
//!
//! The TS e2e suite predates persisted system messages: its `tree[0]` is the first user message.
//! The port navigates to the first user entry explicitly. The custom-instructions case checks that
//! the instructions reach the summary request instead of asking a real model to follow them.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_coding_agent::core::agent_session::AgentSessionNavigateTreeOptions;
use pi_coding_agent::core::session_manager::SessionEntry;
use serde_json::json;

use crate::common::*;
use crate::support::*;

fn summarize(summarize: bool) -> AgentSessionNavigateTreeOptions {
    AgentSessionNavigateTreeOptions {
        summarize: Some(summarize),
        ..Default::default()
    }
}

fn message_entries(harness: &Harness, role: &str) -> Vec<SessionEntry> {
    harness
        .session_manager
        .get_entries()
        .into_iter()
        .filter(|e| matches!(e, SessionEntry::Message(m) if m.message.role() == role))
        .collect()
}

async fn create() -> Harness {
    create_harness(HarnessOptions {
        settings: Some(json!({"compaction": {"keepRecentTokens": 1}})),
        ..Default::default()
    })
    .await
    .unwrap()
}

async fn prompts(harness: &Harness, texts: &[&str]) {
    for text in texts {
        harness.append_responses(vec![crate::common::text(&format!("reply to {text}"))]);
        harness.session.prompt(text, None).await.unwrap();
    }
}

#[tokio::test]
async fn navigates_to_a_user_message_and_puts_its_text_in_the_editor() {
    let harness = create().await;
    prompts(&harness, &["First message", "Second message"]).await;
    let first_user = message_entries(&harness, "user")[0].clone();

    let result = harness
        .session
        .navigate_tree(first_user.id(), Some(&summarize(false)))
        .await
        .unwrap();

    assert!(!result.cancelled);
    assert_eq!(result.editor_text.as_deref(), Some("First message"));
    // The leaf moves to the user message's parent.
    assert_eq!(
        harness.session_manager.get_leaf_id(),
        first_user.parent_id().map(str::to_string)
    );
    harness.cleanup();
}

#[tokio::test]
async fn navigates_to_a_non_user_message_without_editor_text() {
    let harness = create().await;
    prompts(&harness, &["Hello"]).await;
    let assistant = message_entries(&harness, "assistant")[0].clone();

    let result = harness
        .session
        .navigate_tree(assistant.id(), Some(&summarize(false)))
        .await
        .unwrap();

    assert!(!result.cancelled);
    assert!(result.editor_text.is_none());
    assert_eq!(harness.session_manager.get_leaf_id().as_deref(), Some(assistant.id()));
    harness.cleanup();
}

#[tokio::test]
async fn creates_a_branch_summary_when_navigating_with_summarize() {
    let harness = create().await;
    prompts(&harness, &["What is 2+2?", "What is 3+3?"]).await;
    let first_user = message_entries(&harness, "user")[0].clone();
    harness.append_responses(vec![text("summary of the abandoned branch")]);

    let result = harness
        .session
        .navigate_tree(first_user.id(), Some(&summarize(true)))
        .await
        .unwrap();

    assert!(!result.cancelled);
    assert_eq!(result.editor_text.as_deref(), Some("What is 2+2?"));
    let summary = result.summary_entry.expect("summary entry");
    assert!(summary.summary.contains("summary of the abandoned branch"));
    // The summary hangs where the user message hung.
    assert_eq!(summary.parent_id.as_deref(), first_user.parent_id());
    assert_eq!(harness.session_manager.get_leaf_id().as_deref(), Some(summary.id.as_str()));
    harness.cleanup();
}

#[tokio::test]
async fn attaches_the_summary_to_the_correct_parent_when_navigating_to_a_nested_user_message() {
    let harness = create().await;
    prompts(&harness, &["Message one", "Message two", "Message three"]).await;
    let users = message_entries(&harness, "user");
    assert_eq!(users.len(), 3);
    let u2 = users[1].clone();
    let a1_id = u2.parent_id().unwrap().to_string();
    harness.append_responses(vec![text("summary")]);

    let result = harness.session.navigate_tree(u2.id(), Some(&summarize(true))).await.unwrap();

    assert!(!result.cancelled);
    assert_eq!(result.editor_text.as_deref(), Some("Message two"));
    let summary = result.summary_entry.expect("summary entry");
    assert_eq!(summary.parent_id.as_deref(), Some(a1_id.as_str()));
    let mut child_types: Vec<String> = harness
        .session_manager
        .get_children(&a1_id)
        .iter()
        .map(|c| serde_json::to_value(c).unwrap()["type"].as_str().unwrap().to_string())
        .collect();
    child_types.sort();
    assert_eq!(child_types, vec!["branch_summary", "message"]);
    harness.cleanup();
}

#[tokio::test]
async fn attaches_the_summary_to_the_selected_node_when_navigating_to_an_assistant_message() {
    let harness = create().await;
    prompts(&harness, &["Hello", "Goodbye"]).await;
    let a1 = message_entries(&harness, "assistant")[0].clone();
    harness.append_responses(vec![text("summary")]);

    let result = harness.session.navigate_tree(a1.id(), Some(&summarize(true))).await.unwrap();

    assert!(!result.cancelled);
    assert!(result.editor_text.is_none());
    let summary = result.summary_entry.expect("summary entry");
    assert_eq!(summary.parent_id.as_deref(), Some(a1.id()));
    assert_eq!(harness.session_manager.get_leaf_id().as_deref(), Some(summary.id.as_str()));
    harness.cleanup();
}

#[tokio::test]
async fn handles_abort_during_summarization() {
    let harness = Arc::new(create().await);
    prompts(&harness, &["Tell me about something", "Continue"]).await;
    let entries_before = harness.session_manager.get_entries().len();
    let leaf_before = harness.session_manager.get_leaf_id();
    let first_user = message_entries(&harness, "user")[0].clone();
    // The summary request never answers until it is aborted.
    harness.session.agent().set_stream_fn(Some(Arc::new(|model, _context, options| {
        let (sender, stream) = pi_ai::AssistantMessageEventStream::new();
        let signal = options.signal.clone();
        let mut aborted = faux_text_message("");
        aborted.provider = model.provider.clone();
        aborted.stop_reason = pi_ai::types::StopReason::Aborted;
        tokio::spawn(async move {
            if let Some(signal) = signal {
                signal.cancelled().await;
            }
            sender.push(pi_ai::AssistantMessageEvent::Error {
                reason: pi_ai::types::StopReason::Aborted,
                error: aborted,
            });
        });
        Box::pin(async move { Ok(stream) })
    })));

    let session = harness.session.clone();
    let id = first_user.id().to_string();
    let navigation = tokio::spawn(async move { session.navigate_tree(&id, Some(&summarize(true))).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // Branch summarization counts as compacting.
    assert!(harness.session.is_compacting());
    harness.session.abort_branch_summary();

    let result = navigation.await.unwrap().unwrap();
    assert!(result.cancelled);
    assert_eq!(result.aborted, Some(true));
    assert!(result.summary_entry.is_none());
    assert_eq!(harness.session_manager.get_entries().len(), entries_before);
    assert_eq!(harness.session_manager.get_leaf_id(), leaf_before);
    harness.cleanup();
}

#[tokio::test]
async fn does_not_create_a_summary_when_navigating_without_summarize() {
    let harness = create().await;
    prompts(&harness, &["First", "Second"]).await;
    let entries_before = harness.session_manager.get_entries().len();
    let first_user = message_entries(&harness, "user")[0].clone();

    harness
        .session
        .navigate_tree(first_user.id(), Some(&summarize(false)))
        .await
        .unwrap();

    assert_eq!(harness.session_manager.get_entries().len(), entries_before);
    assert!(
        !harness
            .session_manager
            .get_entries()
            .iter()
            .any(|e| matches!(e, SessionEntry::BranchSummary(_)))
    );
    harness.cleanup();
}

#[tokio::test]
async fn handles_navigation_to_the_same_position_as_a_no_op() {
    let harness = create().await;
    prompts(&harness, &["Hello"]).await;
    let leaf = harness.session_manager.get_leaf_id().expect("leaf");
    let entries_before = harness.session_manager.get_entries().len();

    let result = harness.session.navigate_tree(&leaf, Some(&summarize(false))).await.unwrap();

    assert!(!result.cancelled);
    assert_eq!(harness.session_manager.get_leaf_id().as_deref(), Some(leaf.as_str()));
    assert_eq!(harness.session_manager.get_entries().len(), entries_before);
    harness.cleanup();
}

#[tokio::test]
async fn passes_custom_summarization_instructions_to_the_summary_request() {
    let harness = create().await;
    prompts(&harness, &["What is TypeScript?"]).await;
    let first_user = message_entries(&harness, "user")[0].clone();
    let request = Arc::new(Mutex::new(String::new()));
    let r = request.clone();
    harness.append_responses(vec![respond_with(move |context| {
        *r.lock() = serde_json::to_string(&context.messages).unwrap();
        faux_text_message("summary MONKEY MONKEY MONKEY")
    })]);

    let result = harness
        .session
        .navigate_tree(
            first_user.id(),
            Some(&AgentSessionNavigateTreeOptions {
                summarize: Some(true),
                custom_instructions: Some("After the summary, you MUST end with exactly: MONKEY MONKEY MONKEY.".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    assert!(result.summary_entry.unwrap().summary.contains("MONKEY MONKEY MONKEY"));
    assert!(request.lock().contains("you MUST end with exactly: MONKEY MONKEY MONKEY."));
    harness.cleanup();
}

#[tokio::test]
async fn navigates_between_branches() {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    prompts(&harness, &["Main branch start", "Main branch continue"]).await;
    let a1 = message_entries(&harness, "assistant")[0].clone();
    let u2 = message_entries(&harness, "user")[1].clone();

    harness.session_manager.branch(a1.id()).unwrap();
    prompts(&harness, &["Branch path"]).await;
    harness.append_responses(vec![text("summary of the branch path")]);

    let result = harness.session.navigate_tree(u2.id(), Some(&summarize(true))).await.unwrap();

    assert!(!result.cancelled);
    assert_eq!(result.editor_text.as_deref(), Some("Main branch continue"));
    assert!(!result.summary_entry.expect("summary entry").summary.is_empty());
    harness.cleanup();
}

// Regression test for #6324.
#[tokio::test]
async fn summarizes_tree_branches_when_request_auth_has_no_api_key() {
    let harness = create_harness(HarnessOptions {
        with_configured_auth: Some(false),
        ..Default::default()
    })
    .await
    .unwrap();
    let calls = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let c = calls.clone();
    harness.session.agent().set_stream_fn(Some(stream_fn(move |model, _context, options| {
        c.lock().push(options.api_key.clone());
        let mut message = faux_text_message("branch summary text");
        message.api = model.api.clone();
        message.provider = model.provider.clone();
        message.model = model.id.clone();
        message.usage = serde_json::from_value(json!({
            "input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 2,
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0.25}
        }))
        .unwrap();
        message
    })));
    let sm = &harness.session_manager;
    let target = sm.append_message(user_msg("first branch")).unwrap();
    sm.append_message(assistant_msg("first reply")).unwrap();
    sm.append_message(user_msg("abandoned branch work")).unwrap();
    sm.append_message(assistant_msg("abandoned reply")).unwrap();

    let result = harness.session.navigate_tree(&target, Some(&summarize(true))).await.unwrap();

    assert!(!result.cancelled);
    assert_eq!(*calls.lock(), vec![None]);
    let summary = result.summary_entry.expect("branch summary");
    assert!(summary.summary.contains("branch summary text"));
    assert_eq!(summary.usage.unwrap().cost.total, 0.25);
    harness.cleanup();
}
