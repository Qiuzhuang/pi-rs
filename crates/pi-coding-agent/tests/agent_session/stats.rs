//! Port of `test/agent-session-stats.test.ts` (on the harness model instead of the catalog's
//! claude-sonnet-4-5; the assertions use the harness model's window).

use pi_agent::types::{AgentMessage, ThinkingLevel};
use pi_ai::types::Usage;
use pi_coding_agent::core::session_manager::SessionManager;
use pi_coding_agent::core::usage_totals::get_usage_cost_breakdown;
use serde_json::{Value, json};

use crate::common::*;
use crate::support::*;

fn usage_with(total_tokens: u64, cost: Value) -> Usage {
    serde_json::from_value(json!({
        "input": total_tokens, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": total_tokens,
        "cost": cost
    }))
    .unwrap()
}

fn full_usage() -> Usage {
    serde_json::from_value(json!({
        "input": 10, "output": 20, "cacheRead": 30, "cacheWrite": 40, "totalTokens": 100,
        "cost": {"input": 0.1, "output": 0.2, "cacheRead": 0.3, "cacheWrite": 0.4, "total": 1}
    }))
    .unwrap()
}

fn cost(total: f64) -> Value {
    json!({"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": total})
}

fn assistant(model: &pi_ai::types::Model, text: &str, total_tokens: u64, timestamp: i64) -> AgentMessage {
    agent_message(json!({
        "role": "assistant",
        "content": [{"type": "text", "text": text}],
        "api": model.api, "provider": model.provider, "model": model.id,
        "usage": usage_with(total_tokens, cost(0.0)),
        "stopReason": "stop",
        "timestamp": timestamp
    }))
}

fn user(text: &str, timestamp: i64) -> AgentMessage {
    agent_message(json!({"role": "user", "content": text, "timestamp": timestamp}))
}

fn tool_result(usage: Usage) -> AgentMessage {
    agent_message(json!({
        "role": "toolResult", "toolCallId": "tool-call-1", "toolName": "test_tool",
        "content": [{"type": "text", "text": "tool result"}],
        "usage": usage, "isError": false, "timestamp": 1
    }))
}

async fn create() -> Harness {
    let harness = create_harness(HarnessOptions::default()).await.unwrap();
    harness.session.set_thinking_level(ThinkingLevel::High, None);
    harness
}

fn sync(harness: &Harness) {
    harness
        .session
        .agent()
        .set_messages(harness.session_manager.build_session_context().messages);
}

fn tokens(harness: &Harness) -> Value {
    serde_json::to_value(harness.session.get_session_stats().tokens).unwrap()
}

/// first/response1/second/response2/compaction/third.
fn seed_compacted(harness: &Harness) -> String {
    let model = harness.get_model();
    let sm = &harness.session_manager;
    sm.append_message(user("first", 1)).unwrap();
    sm.append_message(assistant(&model, "response1", 180_000, 2)).unwrap();
    let kept = sm.append_message(user("second", 3)).unwrap();
    sm.append_message(assistant(&model, "response2", 195_000, 4)).unwrap();
    sm.append_compaction("summary", Some(&kept), 195_000, None, None, None).unwrap();
    sm.append_message(user("third", 5)).unwrap();
    kept
}

#[tokio::test]
async fn exposes_the_current_context_usage_alongside_token_totals() {
    let harness = create().await;
    let model = harness.get_model();
    harness.session_manager.append_message(user("hello", 1)).unwrap();
    harness.session_manager.append_message(assistant(&model, "hi", 200, 2)).unwrap();
    sync(&harness);

    let stats = harness.session.get_session_stats();
    let usage = stats.context_usage.clone().unwrap();
    assert_eq!(
        serde_json::to_value(&stats.context_usage).unwrap(),
        serde_json::to_value(harness.session.get_context_usage()).unwrap()
    );
    assert_eq!(usage.tokens, Some(200));
    assert_eq!(usage.context_window, model.context_window);
    assert_eq!(usage.percent, Some(200.0 / model.context_window as f64 * 100.0));
    harness.cleanup();
}

#[tokio::test]
async fn reports_unknown_current_context_usage_immediately_after_compaction() {
    let harness = create().await;
    seed_compacted(&harness);
    sync(&harness);

    let stats = harness.session.get_session_stats();
    // Totals cover ALL entries, including history compacted away (180k + 195k).
    assert_eq!(stats.tokens.input, 375_000.0);
    let usage = stats.context_usage.unwrap();
    assert_eq!(usage.tokens, None);
    assert_eq!(usage.percent, None);
    harness.cleanup();
}

#[tokio::test]
async fn uses_post_compaction_usage_for_current_context_instead_of_stale_kept_usage() {
    let harness = create().await;
    let model = harness.get_model();
    seed_compacted(&harness);
    harness
        .session_manager
        .append_message(assistant(&model, "response3", 25_000, 6))
        .unwrap();
    sync(&harness);

    let stats = harness.session.get_session_stats();
    assert_eq!(stats.tokens.input, 400_000.0);
    let usage = stats.context_usage.unwrap();
    assert_eq!(usage.tokens, Some(25_000));
    assert_eq!(usage.percent, Some(25_000.0 / model.context_window as f64 * 100.0));
    harness.cleanup();
}

#[tokio::test]
async fn includes_branch_summary_usage_in_session_totals() {
    let harness = create().await;
    harness
        .session_manager
        .branch_with_summary(None, "summary", None, Some(false), Some(full_usage()))
        .unwrap();
    sync(&harness);

    assert_eq!(
        tokens(&harness),
        json!({"input": 10, "output": 20, "cacheRead": 30, "cacheWrite": 40, "total": 100})
    );
    assert_eq!(harness.session.get_session_stats().cost, 1.0);
    harness.cleanup();
}

#[tokio::test]
async fn includes_compaction_usage_in_session_totals() {
    let harness = create().await;
    let first = harness.session_manager.append_message(user("hello", 1)).unwrap();
    harness
        .session_manager
        .append_compaction("summary", Some(&first), 100, None, Some(false), Some(full_usage()))
        .unwrap();
    sync(&harness);

    assert_eq!(
        tokens(&harness),
        json!({"input": 10, "output": 20, "cacheRead": 30, "cacheWrite": 40, "total": 100})
    );
    assert_eq!(harness.session.get_session_stats().cost, 1.0);
    harness.cleanup();
}

#[tokio::test]
async fn includes_cache_warming_usage_exactly_once_without_adding_messages() {
    let harness = create().await;
    let model = harness.get_model();
    let usage: Usage = serde_json::from_value(json!({
        "input": 2, "output": 1, "cacheRead": 97, "cacheWrite": 0, "totalTokens": 100,
        "cost": {"input": 0.001, "output": 0.002, "cacheRead": 0.007, "cacheWrite": 0, "total": 0.01}
    }))
    .unwrap();
    harness
        .session_manager
        .append_usage("cache_warm", &model.provider, &model.id, usage, Some("extension override"))
        .unwrap();

    let entry = serde_json::to_value(&harness.session_manager.get_entries()[0]).unwrap();
    assert_eq!(entry["type"], json!("usage"));
    assert_eq!(entry["kind"], json!("cache_warm"));
    assert_eq!(entry["note"], json!("extension override"));
    let stats = harness.session.get_session_stats();
    assert_eq!(
        serde_json::to_value(&stats.tokens).unwrap(),
        json!({"input": 2, "output": 1, "cacheRead": 97, "cacheWrite": 0, "total": 100})
    );
    assert_eq!(stats.total_messages, 0);
    assert!(harness.session_manager.build_session_context().messages.is_empty());
    assert_eq!(
        serde_json::to_value(get_usage_cost_breakdown(&harness.session_manager.get_entries())).unwrap(),
        json!([{"key": format!("{}/{}", model.provider, model.id), "cost": 0.01, "tokens": 100}])
    );
    harness.cleanup();
}

#[tokio::test]
async fn includes_tool_result_usage_in_session_totals() {
    let harness = create().await;
    harness.session_manager.append_message(tool_result(full_usage())).unwrap();
    sync(&harness);

    assert_eq!(
        tokens(&harness),
        json!({"input": 10, "output": 20, "cacheRead": 30, "cacheWrite": 40, "total": 100})
    );
    assert_eq!(harness.session.get_session_stats().cost, 1.0);
    harness.cleanup();
}

#[test]
fn groups_tool_and_summary_usage_separately_from_model_attributed_usage() {
    let model = pi_ai::compat::get_model("anthropic", "claude-sonnet-4-5").unwrap();
    let sm = SessionManager::in_memory(None, None, None).unwrap();
    let root = sm.append_message(user("hello", 1)).unwrap();
    let mut response = assistant(&model, "response", 100, 2);
    if let AgentMessage::Assistant(a) = &mut response {
        a.usage = usage_with(100, cost(0.5));
    }
    sm.append_message(response).unwrap();
    sm.append_message(tool_result(usage_with(100, cost(1.0)))).unwrap();
    sm.append_compaction("summary", Some(&root), 100, None, Some(false), Some(usage_with(100, cost(2.0))))
        .unwrap();
    sm.branch_with_summary(None, "branch summary", None, Some(false), Some(usage_with(100, cost(3.0))))
        .unwrap();

    assert_eq!(
        serde_json::to_value(get_usage_cost_breakdown(&sm.get_entries())).unwrap(),
        json!([
            {"key": "Tools/summaries", "cost": 6, "tokens": 300},
            {"key": format!("{}/{}", model.provider, model.id), "cost": 0.5, "tokens": 100},
        ])
    );
}

#[tokio::test]
async fn ignores_zero_usage_messages_when_checking_for_post_compaction_context_usage() {
    let harness = create().await;
    let model = harness.get_model();
    seed_compacted(&harness);
    let sm = &harness.session_manager;
    sm.append_message(assistant(&model, "response3", 25_000, 6)).unwrap();
    sm.append_message(user("continue", 7)).unwrap();
    sm.append_message(assistant(&model, "partial", 0, 8)).unwrap();
    sync(&harness);

    let usage = harness.session.get_session_stats().context_usage.unwrap();
    assert!(usage.tokens.unwrap() > 25_000);
    harness.cleanup();
}
