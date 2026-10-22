//! Port of transcript-tool-changes.test.ts: system-message tool changes per transport.

use pi_ai::providers::all::get_builtin_model;
use pi_ai::utils::transcript::{TranscriptToolSupport, resolve_transcript_tools};
use pi_ai::*;
use serde_json::{Value, json};

use crate::support::*;

fn tool_value(name: &str) -> Value {
    json!({"name": name, "description": format!("{name} tool"), "parameters": {"type": "object", "properties": {}}})
}

fn with_description(name: &str, description: &str) -> Value {
    let mut value = tool_value(name);
    value["description"] = json!(description);
    value
}

fn test_model(extra: Value) -> Model {
    let mut value = json!({"baseUrl": "http://127.0.0.1:9", "reasoning": true, "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}, "contextWindow": 100000, "maxTokens": 1000});
    value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    model(value)
}

async fn capture(model: &Model, messages: Value) -> Value {
    simple_payload(model, &ctx(json!({ "messages": messages })), StreamOptions { api_key: Some("test-key".into()), ..Default::default() }).await
}

fn context_messages() -> Value {
    json!([
        {"role": "system", "content": "base prompt",
         "sections": {"rules": "<rules>\nold rules\n</rules>", "docs": "<docs>\nread docs\n</docs>"},
         "toolsAdded": [tool_value("base_tool")], "timestamp": 0},
        {"role": "user", "content": "before", "timestamp": 1},
        {"role": "system", "content": "updated guidance",
         "sections": {"rules": "<rules>\nnew rules\n</rules>", "docs": null},
         "toolsRemoved": [{"name": "base_tool"}], "toolsAdded": [tool_value("late_tool")], "timestamp": 2}
    ])
}

fn addition_messages() -> Value {
    json!([
        {"role": "system", "content": "base prompt", "toolsAdded": [tool_value("base_tool")], "timestamp": 0},
        {"role": "user", "content": "before", "timestamp": 1},
        {"role": "system", "content": "updated guidance", "toolsAdded": [tool_value("late_tool")], "timestamp": 2}
    ])
}

fn redefinition_messages() -> Value {
    json!([
        {"role": "system", "content": "base prompt", "toolsAdded": [tool_value("base_tool")], "timestamp": 0},
        {"role": "user", "content": "before", "timestamp": 1},
        {"role": "system", "content": "", "toolsRemoved": [{"name": "base_tool"}],
         "toolsAdded": [with_description("base_tool", "changed")], "timestamp": 2}
    ])
}

fn anthropic_native() -> Model {
    test_model(json!({"id": "claude-opus-5", "name": "Claude Opus 5", "api": "anthropic-messages", "provider": "anthropic",
        "compat": {"supportsMidConvoSystemMessages": true, "supportsMidConvoToolChanges": true}}))
}

fn additional_tools_model() -> Model {
    test_model(json!({"id": "gpt-5.4", "name": "GPT-5.4", "api": "openai-responses", "provider": "openai",
        "compat": {"supportsMidConvoSystemMessages": true, "supportsAdditionalTools": true}}))
}

fn tool_search_model() -> Model {
    test_model(json!({"id": "gpt-5.4", "name": "GPT-5.4", "api": "openai-responses", "provider": "openai",
        "compat": {"supportsMidConvoSystemMessages": true, "supportsToolSearch": true}}))
}

/// One request after each step: initial tools, removal of drop_tool, then two loads.
async fn capture_tool_history(model: &Model) -> Vec<Value> {
    let steps = [
        json!([{"role": "user", "content": "first", "timestamp": 1}]),
        json!([{"role": "system", "content": "", "toolsRemoved": [{"name": "drop_tool"}], "timestamp": 2},
               {"role": "user", "content": "after removal", "timestamp": 3}]),
        json!([{"role": "system", "content": "", "toolsAdded": [tool_value("late_tool")], "timestamp": 4},
               {"role": "user", "content": "after first load", "timestamp": 5}]),
        json!([{"role": "system", "content": "", "toolsAdded": [tool_value("later_tool")], "timestamp": 6},
               {"role": "user", "content": "after second load", "timestamp": 7}]),
    ];
    let mut messages =
        vec![json!({"role": "system", "content": "base prompt", "toolsAdded": [tool_value("base_tool"), tool_value("drop_tool")], "timestamp": 0})];
    let mut payloads = Vec::new();
    for step in steps {
        messages.extend(step.as_array().unwrap().iter().cloned());
        payloads.push(capture(model, Value::Array(messages.clone())).await);
    }
    payloads
}

#[track_caller]
fn expect_growing_prefix(inputs: &[&Vec<Value>]) {
    for pair in inputs.windows(2) {
        assert_eq!(&pair[1][..pair[0].len()], &pair[0][..]);
    }
}

fn names(list: &Value) -> Vec<String> {
    list.as_array().into_iter().flatten().map(|t| t["name"].as_str().or(t["function"]["name"].as_str()).unwrap_or_default().to_string()).collect()
}

fn items<'a>(payload: &'a Value, key: &str) -> &'a Vec<Value> {
    payload[key].as_array().unwrap()
}

fn content_str(item: &Value) -> &str {
    item["content"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn anthropic_native_updates_and_tool_changes() {
    let payload = capture(&anthropic_native(), context_messages()).await;
    let betas = payload["betas"].as_array().unwrap();
    assert!(betas.contains(&json!("inline-tools-2026-09-15")));
    assert!(!betas.contains(&json!("mid-conversation-tool-changes-2026-07-01")));
    let system: Vec<_> = items(&payload, "system").iter().map(|b| b["text"].as_str().unwrap()).collect();
    assert_eq!(system, ["base prompt\n\n<rules>\nold rules\n</rules>\n\n<docs>\nread docs\n</docs>"]);
    assert_json_matches(
        &payload["tools"],
        json!([{"name": "base_tool", "cache_control": {"type": "ephemeral"}}, {"name": "__pi_deferred_placeholder__", "defer_loading": true}]),
    );
    assert!(payload["tools"][0].get("defer_loading").is_none());
    assert!(payload["tools"][1].get("cache_control").is_none());
    let update = items(&payload, "messages").last().unwrap();
    assert_json_matches(
        update,
        json!({"role": "system", "content": [
            {"type": "text"},
            {"type": "tool_removal", "tool": {"type": "tool_reference", "name": "base_tool"}},
            {"type": "tool_addition", "tool": {"type": "tool_definition", "definition": {"name": "late_tool", "description": "late_tool tool"}},
             "cache_control": {"type": "ephemeral"}}
        ]}),
    );
    let definition = &update["content"][2]["tool"]["definition"];
    assert!(definition.get("cache_control").is_none() && definition.get("defer_loading").is_none());
    let text = update["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("updated guidance"));
    assert!(text.contains("<rules>\nnew rules\n</rules>"));
    assert!(text.contains("Removed system prompt section \"docs\""));

    let initial = capture(&anthropic_native(), Value::Array(context_messages().as_array().unwrap()[..2].to_vec())).await;
    assert_eq!(names(&initial["tools"]), ["base_tool", "__pi_deferred_placeholder__"]);
}

#[tokio::test]
async fn anthropic_redefines_tool_by_value() {
    let payload = capture(&anthropic_native(), redefinition_messages()).await;
    assert!(payload["betas"].as_array().unwrap().contains(&json!("inline-tools-2026-09-15")));
    assert_json_matches(&payload["tools"], json!([{"name": "base_tool", "description": "base_tool tool"}, {"name": "__pi_deferred_placeholder__"}]));
    let last = items(&payload, "messages").last().unwrap();
    assert_json_matches(
        last,
        json!({"role": "system", "content": [
            {"type": "tool_addition", "tool": {"type": "tool_definition", "definition": {"name": "base_tool", "description": "changed"}}}
        ]}),
    );
}

#[tokio::test]
async fn anthropic_without_initial_tool_sends_current_list() {
    let payload = capture(
        &anthropic_native(),
        json!([
            {"role": "system", "content": "base prompt", "timestamp": 0},
            {"role": "user", "content": "before", "timestamp": 1},
            {"role": "system", "content": "updated guidance", "toolsAdded": [tool_value("late_tool")], "timestamp": 2}
        ]),
    )
    .await;
    assert!(!payload["betas"].as_array().is_some_and(|b| b.contains(&json!("inline-tools-2026-09-15"))));
    assert_json_matches(&payload["tools"], json!([{"name": "late_tool", "cache_control": {"type": "ephemeral"}}]));
    assert!(payload["tools"][0].get("defer_loading").is_none());
    let last = items(&payload, "messages").last().unwrap();
    let types: Vec<_> = last["content"].as_array().unwrap().iter().map(|b| b["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["text"]);
}

fn roles(payload: &Value) -> Vec<&str> {
    items(payload, "messages").iter().map(|m| m["role"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn anthropic_folds_updates_without_native_support() {
    let model = test_model(json!({"id": "claude-sonnet-4-5", "name": "Claude Sonnet 4.5", "api": "anthropic-messages", "provider": "anthropic"}));
    let payload = capture(&model, context_messages()).await;
    assert!(!payload["betas"].as_array().is_some_and(|b| b.contains(&json!("inline-tools-2026-09-15"))));
    let system: Vec<_> = items(&payload, "system").iter().map(|b| b["text"].as_str().unwrap()).collect();
    assert_eq!(system, ["base prompt\n\nupdated guidance\n\n<rules>\nnew rules\n</rules>"]);
    assert_eq!(names(&payload["tools"]), ["late_tool"]);
    assert_eq!(roles(&payload), ["user"]);
}

#[tokio::test]
async fn anthropic_native_tool_changes_need_both_capabilities() {
    let model = test_model(json!({"id": "claude-opus-5", "name": "Claude Opus 5", "api": "anthropic-messages", "provider": "anthropic",
        "compat": {"supportsMidConvoToolChanges": true}}));
    let payload = capture(&model, context_messages()).await;
    assert!(!payload["betas"].as_array().is_some_and(|b| b.contains(&json!("inline-tools-2026-09-15"))));
    assert_eq!(names(&payload["tools"]), ["late_tool"]);
    assert_eq!(roles(&payload), ["user"]);
}

fn additional_tool_names(payload: &Value) -> Vec<Vec<String>> {
    items(payload, "input").iter().filter(|i| i["type"] == "additional_tools").map(|i| names(&i["tools"])).collect()
}

#[tokio::test]
async fn openai_declares_every_tool_in_additional_tools() {
    let payload = capture(&additional_tools_model(), addition_messages()).await;
    assert!(payload.get("tools").is_none());
    assert_eq!(additional_tool_names(&payload), [vec!["base_tool"], vec!["late_tool"]]);
    let input = items(&payload, "input");
    assert_eq!(input.len(), 6);
    assert_eq!(input[0]["type"], "additional_tools");
    assert_eq!(input[1]["content"], "base prompt");
    assert!(input[2].get("type").is_none() && input[2]["content"].is_array());
    assert!(content_str(&input[3]).contains("incremental tools update"));
    assert_eq!(input[4]["type"], "additional_tools");
    assert_eq!(input[5]["content"], "updated guidance");
}

#[tokio::test]
async fn openai_maps_additions_into_synthetic_tool_search() {
    let payload = capture(&tool_search_model(), addition_messages()).await;
    assert_eq!(names(&payload["tools"]), ["base_tool"]);
    let input = items(&payload, "input");
    assert!(input.iter().any(|i| i["type"] == "tool_search_call"));
    let output = input.iter().find(|i| i["type"] == "tool_search_output").unwrap();
    assert_eq!(names(&output["tools"]), ["late_tool"]);
}

#[tokio::test]
async fn openai_folds_updates_without_native_support() {
    let model = test_model(json!({"id": "gpt-4.1", "name": "GPT-4.1", "api": "openai-responses", "provider": "openai",
        "compat": {"supportsAdditionalTools": true}}));
    let payload = capture(&model, context_messages()).await;
    assert_eq!(names(&payload["tools"]), ["late_tool"]);
    let input = items(&payload, "input");
    let kinds: Vec<_> = input.iter().map(|i| i["type"].as_str().or(i["role"].as_str()).unwrap()).collect();
    assert_eq!(kinds, ["developer", "user"]);
    assert_eq!(input[0]["content"], "base prompt\n\nupdated guidance\n\n<rules>\nnew rules\n</rules>");
}

#[tokio::test]
async fn openai_announces_removals_as_text() {
    let payload = capture(&additional_tools_model(), context_messages()).await;
    assert!(payload.get("tools").is_none());
    assert_eq!(additional_tool_names(&payload), [vec!["base_tool"], vec!["late_tool"]]);
    let update = content_str(items(&payload, "input").last().unwrap());
    assert!(update.contains("updated guidance"));
    assert!(update.contains("The following tools are no longer available. Do not call them:\n- base_tool"));
}

#[tokio::test]
async fn openai_redefines_tool_with_additional_tools() {
    let payload = capture(&additional_tools_model(), redefinition_messages()).await;
    assert!(payload.get("tools").is_none());
    let input = &payload["input"];
    assert_eq!(input.as_array().unwrap().len(), 5);
    let rest = json!([
        {"type": "additional_tools", "tools": [{"name": "base_tool", "description": "base_tool tool"}]},
        {"role": "developer", "content": "base prompt"},
        {"role": "user"},
    ]);
    for (index, expected) in rest.as_array().unwrap().iter().enumerate() {
        assert_json_matches(&input[index], expected.clone());
    }
    assert_eq!(input[3]["role"], "developer");
    assert!(content_str(&input[3]).contains("latest definition replaces the earlier one"));
    assert_json_matches(&input[4], json!({"type": "additional_tools", "tools": [{"name": "base_tool", "description": "changed"}]}));
}

// #10940: deferred loads after a removal must not rewrite the cached prefix.
#[tokio::test]
async fn openai_prefix_survives_removal_and_loads() {
    let payloads = capture_tool_history(&additional_tools_model()).await;
    for payload in &payloads {
        assert!(payload.get("tools").is_none());
    }
    expect_growing_prefix(&payloads.iter().map(|p| items(p, "input")).collect::<Vec<_>>());
    let last = payloads.last().unwrap();
    assert_eq!(additional_tool_names(last), [vec!["base_tool", "drop_tool"], vec!["late_tool"], vec!["later_tool"]]);
    let input = items(last, "input");
    assert!(input.iter().any(|i| content_str(i).contains("no longer available. Do not call them:\n- drop_tool")));
    let notices: Vec<_> = input.iter().map(content_str).filter(|c| c.contains("incremental tools update")).collect();
    assert_eq!(notices.len(), 2);
    assert!(notices.iter().all(|n| n.contains("\nStill unavailable: drop_tool.")), "{notices:?}");
}

#[tokio::test]
async fn openai_stops_listing_redeclared_tool_as_unavailable() {
    let payload = capture(
        &additional_tools_model(),
        json!([
            {"role": "system", "content": "base prompt", "toolsAdded": [tool_value("base_tool"), tool_value("drop_tool")], "timestamp": 0},
            {"role": "system", "content": "", "toolsRemoved": [{"name": "drop_tool"}], "timestamp": 1},
            {"role": "system", "content": "", "toolsAdded": [tool_value("drop_tool")], "timestamp": 2},
            {"role": "user", "content": "go", "timestamp": 3}
        ]),
    )
    .await;
    let input = items(&payload, "input");
    let notice = input.iter().map(content_str).find(|c| c.contains("incremental tools update")).expect("notice");
    assert!(!notice.contains("Still unavailable"));
    assert_json_matches(&input[input.len() - 2], json!({"type": "additional_tools", "tools": [{"name": "drop_tool"}]}));
}

#[tokio::test]
async fn openai_unannounced_removal_is_not_still_unavailable() {
    let payload = capture(
        &additional_tools_model(),
        json!([
            {"role": "system", "content": "base prompt", "toolsAdded": [tool_value("base_tool"), tool_value("drop_tool")], "timestamp": 0},
            {"role": "system", "content": "", "toolsAdded": [tool_value("late_tool")], "toolsRemoved": [{"name": "drop_tool"}], "timestamp": 1},
            {"role": "user", "content": "go", "timestamp": 2}
        ]),
    )
    .await;
    let input = items(&payload, "input");
    let tail = &input[input.len() - 4..];
    assert!(content_str(&tail[0]).contains("incremental tools update"));
    assert!(!content_str(&tail[0]).contains("Still unavailable"));
    assert_json_matches(&tail[1], json!({"type": "additional_tools", "tools": [{"name": "late_tool"}]}));
    assert!(content_str(&tail[2]).contains("no longer available. Do not call them:\n- drop_tool"));
    assert_eq!(tail[3]["role"], "user");
}

// #10940: synthetic tool search cannot remove tools, so a removal rewrites the prefix once.
#[tokio::test]
async fn openai_tool_search_prefix_survives_loads_after_removal() {
    let payloads = capture_tool_history(&tool_search_model()).await;
    let (before, after) = payloads.split_first().unwrap();
    assert_eq!(names(&before["tools"]), ["base_tool", "drop_tool"]);
    for payload in after {
        assert_eq!(names(&payload["tools"]), ["base_tool"]);
    }
    expect_growing_prefix(&after.iter().map(|p| items(p, "input")).collect::<Vec<_>>());
    let outputs: Vec<_> =
        items(after.last().unwrap(), "input").iter().filter(|i| i["type"] == "tool_search_output").map(|i| names(&i["tools"])[0].clone()).collect();
    assert_eq!(outputs, ["late_tool", "later_tool"]);
}

// #10940: removals and redeclarations move the top-level boundary once; plain additions stay anchored.
#[test]
fn resolves_top_level_tool_boundary_per_transport() {
    let messages = ctx(json!({"messages": [
        {"role": "system", "content": "base prompt", "toolsAdded": [tool_value("base_tool"), tool_value("drop_tool")], "timestamp": 0},
        {"role": "system", "content": "", "toolsRemoved": [{"name": "drop_tool"}], "timestamp": 1},
        {"role": "system", "content": "", "toolsAdded": [tool_value("drop_tool")], "timestamp": 2},
        {"role": "system", "content": "", "toolsAdded": [with_description("late_tool", "x")], "timestamp": 3},
        {"role": "system", "content": "", "toolsAdded": [with_description("late_tool", "y")], "timestamp": 4}
    ]}))
    .messages;
    let tools = |values: &[Value]| values.iter().map(|v| tool(v.clone())).collect::<Vec<_>>();
    let resolved = resolve_transcript_tools(&messages[..2], TranscriptToolSupport::Additions);
    assert_eq!((resolved.request_tools, resolved.folded_system_messages), (tools(&[tool_value("base_tool")]), 2));
    // Kimi K3 ignores a removed tool that a later tool message declares again, so the re-added
    // drop_tool moves the boundary; the new late_tool after it stays anchored.
    let resolved = resolve_transcript_tools(&messages[..4], TranscriptToolSupport::Additions);
    assert_eq!((resolved.request_tools, resolved.folded_system_messages), (tools(&[tool_value("base_tool"), tool_value("drop_tool")]), 3));
    let all = tools(&[tool_value("base_tool"), tool_value("drop_tool"), with_description("late_tool", "y")]);
    let resolved = resolve_transcript_tools(&messages, TranscriptToolSupport::Additions);
    assert_eq!((resolved.request_tools, resolved.folded_system_messages), (all.clone(), 5));
    let resolved = resolve_transcript_tools(&messages, TranscriptToolSupport::Changes);
    assert_eq!((resolved.request_tools, resolved.folded_system_messages), (vec![], 0));
    let resolved = resolve_transcript_tools(&messages, TranscriptToolSupport::None);
    assert_eq!((resolved.request_tools, resolved.folded_system_messages), (all, 5));
}

fn tool_bearing(payload: &Value) -> Vec<Vec<String>> {
    items(payload, "messages").iter().filter(|m| m.get("tools").is_some()).map(|m| names(&m["tools"])).collect()
}

fn system_contents(payload: &Value) -> Vec<Value> {
    items(payload, "messages").iter().filter(|m| m["role"] == "system").map(|m| m.get("content").cloned().unwrap_or(Value::Null)).collect()
}

#[tokio::test]
async fn kimi_anchors_additions_in_tool_bearing_system_messages() {
    let model = test_model(json!({"id": "kimi-k3", "name": "Kimi K3", "api": "openai-completions", "provider": "moonshotai",
        "compat": {"supportsMidConvoSystemMessages": true, "supportsMidConvoToolAdditions": true}}));
    let payload = capture(&model, addition_messages()).await;
    assert_eq!(names(&payload["tools"]), ["base_tool"]);
    assert_eq!(tool_bearing(&payload).first().unwrap(), &["late_tool"]);
    assert_eq!(system_contents(&payload), [json!("base prompt"), Value::Null, json!("updated guidance")]);
}

// #10940: Kimi tool messages cannot remove tools, so a removal rewrites the prefix once.
#[tokio::test]
async fn kimi_prefix_survives_loads_after_removal() {
    let payloads = capture_tool_history(&get_builtin_model("kimi-coding", "k3").unwrap()).await;
    let (before, after) = payloads.split_first().unwrap();
    assert_eq!(names(&before["tools"]), ["base_tool", "drop_tool"]);
    for payload in after {
        assert_eq!(names(&payload["tools"]), ["base_tool"]);
    }
    expect_growing_prefix(&after.iter().map(|p| items(p, "messages")).collect::<Vec<_>>());
    let anchored: Vec<_> = tool_bearing(after.last().unwrap()).into_iter().map(|t| t[0].clone()).collect();
    assert_eq!(anchored, ["late_tool", "later_tool"]);
}

// #10720: Kimi Coding uses Chat Completions so K3-style models keep added tools out of the top-level list.
#[tokio::test]
async fn kimi_coding_models_anchor_additions() {
    for id in ["k3", "k3-256k", "kimi-for-coding"] {
        let payload = capture(&get_builtin_model("kimi-coding", id).unwrap(), addition_messages()).await;
        assert_eq!(names(&payload["tools"]), ["base_tool"], "{id}");
        assert_eq!(tool_bearing(&payload).first().unwrap(), &["late_tool"], "{id}");
    }
}

// #10720: Kimi Code Highspeed rejects tool-bearing system messages.
#[tokio::test]
async fn kimi_coding_highspeed_keeps_additions_top_level() {
    let payload = capture(&get_builtin_model("kimi-coding", "kimi-for-coding-highspeed").unwrap(), addition_messages()).await;
    assert_eq!(names(&payload["tools"]), ["base_tool", "late_tool"]);
    assert!(tool_bearing(&payload).is_empty());
}

#[tokio::test]
async fn kimi_k2_keeps_system_text_inline() {
    let model = test_model(json!({"id": "kimi-k2.7-code", "name": "Kimi K2.7 Code", "api": "openai-completions", "provider": "moonshotai",
        "compat": {"supportsMidConvoSystemMessages": true}}));
    let payload = capture(&model, addition_messages()).await;
    assert_eq!(names(&payload["tools"]), ["base_tool", "late_tool"]);
    assert!(tool_bearing(&payload).is_empty());
    assert_eq!(system_contents(&payload), [json!("base prompt"), json!("updated guidance")]);
}

#[tokio::test]
async fn openai_compatible_folds_updates_without_native_support() {
    let model = test_model(json!({"id": "custom-model", "name": "Custom model", "api": "openai-completions", "provider": "custom-provider", "reasoning": false}));
    let payload = capture(&model, context_messages()).await;
    assert_eq!(names(&payload["tools"]), ["late_tool"]);
    assert_eq!(roles(&payload), ["system", "user"]);
    assert_eq!(payload["messages"][0]["content"], "base prompt\n\nupdated guidance\n\n<rules>\nnew rules\n</rules>");
}
