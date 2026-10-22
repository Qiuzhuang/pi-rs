//! Ports of provider catalog/request tests that the TS map filed as E2E because they read
//! provider env vars: fireworks-models, together-models, baseten-models.

use pi_ai::env_api_keys::{find_env_keys, get_env_api_key};
use pi_ai::providers::all::get_builtin_model;
use pi_ai::*;
use serde_json::{Value, json};

use crate::mock_http::{MockServer, Request, Response};
use crate::support::*;

fn builtin(provider: &str, id: &str) -> Model {
    get_builtin_model(provider, id).unwrap_or_else(|| panic!("{provider}/{id}"))
}

fn as_json(model: &Model) -> Value {
    serde_json::to_value(model).unwrap()
}

fn levels(model: &Model) -> Vec<String> {
    get_supported_thinking_levels(model).into_iter().map(|l| serde_json::to_value(l).unwrap().as_str().unwrap().to_string()).collect()
}

fn test_context() -> Context {
    ctx(json!({"messages": [{"role": "user", "content": "test", "timestamp": 0}]}))
}

async fn payload(model: &Model, options: StreamOptions) -> Value {
    simple_payload(model, &test_context(), options).await
}

fn with_reasoning(key: &str, level: Option<ThinkingLevel>) -> StreamOptions {
    StreamOptions { api_key: Some(key.into()), reasoning: level, ..Default::default() }
}

fn level(name: &str) -> ThinkingLevel {
    serde_json::from_value(json!(name)).unwrap()
}

// ---------------------------------------------------------------------------
// fireworks-models.test.ts
// ---------------------------------------------------------------------------

#[test]
fn fireworks_registers_messages_models() {
    let model = builtin("fireworks", "accounts/fireworks/models/deepseek-v4p1-flash");
    assert_eq!(model.api, "anthropic-messages");
    assert_eq!(model.provider, "fireworks");
    assert_eq!(model.base_url, "https://api.fireworks.ai/inference");
    assert!(model.reasoning);
    assert_eq!(as_json(&model)["input"], json!(["text", "image"]));
}

#[test]
fn fireworks_glm_fast_aligns_with_glm() {
    let base = as_json(&builtin("fireworks", "accounts/fireworks/models/glm-5p3"));
    let fast = as_json(&builtin("fireworks", "accounts/fireworks/routers/glm-5p3-fast"));
    for key in ["api", "baseUrl", "compat", "thinkingLevelMap"] {
        assert_eq!(fast[key], base[key], "{key}");
    }
}

#[tokio::test]
async fn fireworks_glm_omits_long_cache_retention() {
    for id in ["accounts/fireworks/models/glm-5p3", "accounts/fireworks/routers/glm-5p3-fast"] {
        let options = StreamOptions {
            api_key: Some("test-fireworks-key".into()),
            cache_retention: Some(CacheRetention::Long),
            session_id: Some("test-fireworks-session".into()),
            ..Default::default()
        };
        let body = payload(&builtin("fireworks", id), options).await;
        assert!(body.get("prompt_cache_retention").is_none(), "{id}");
    }
}

#[tokio::test]
async fn fireworks_kimi_k3_uses_completions_with_native_effort() {
    let compat = json!({"supportsStore": false, "supportsDeveloperRole": false, "supportsStrictMode": true,
        "requiresReasoningContentOnAssistantMessages": true, "thinkingFormat": "openai", "supportsMidConvoSystemMessages": true,
        "supportsMidConvoToolAdditions": true, "sendSessionAffinityHeaders": true, "supportsLongCacheRetention": false});
    let map = json!({"off": null, "minimal": null, "low": "low", "medium": null, "high": "high", "xhigh": null, "max": "max"});
    let base = builtin("fireworks", "accounts/fireworks/models/kimi-k3");
    for model in [&base, &builtin("fireworks", "accounts/fireworks/routers/kimi-k3-fast")] {
        let value = as_json(model);
        assert_eq!(value["api"], "openai-completions");
        assert_eq!(value["baseUrl"], "https://api.fireworks.ai/inference/v1");
        assert_eq!(value["compat"], compat);
        assert_eq!(value["thinkingLevelMap"], map);
    }
    let body = payload(&base, with_reasoning("test-fireworks-key", Some(level("max")))).await;
    assert_eq!(body["reasoning_effort"], "max");
}

// Regression for #9323: native effort must reach Messages without budget-based fallback.
#[tokio::test]
async fn fireworks_messages_models_send_native_effort() {
    for (id, expected) in [
        ("accounts/fireworks/models/deepseek-v4p1-flash", ["off", "low", "high", "max"]),
        ("accounts/fireworks/models/qwen3p8-max", ["off", "low", "medium", "xhigh"]),
        ("accounts/fireworks/models/qwen3p8-2p4t-a95b", ["off", "low", "medium", "xhigh"]),
    ] {
        let model = builtin("fireworks", id);
        assert_eq!(model.api, "anthropic-messages");
        assert_eq!(as_json(&model)["compat"]["forceAdaptiveThinking"], true, "{id}");
        assert_eq!(levels(&model), expected, "{id}");
        for name in expected {
            let reasoning = (name != "off").then(|| level(name));
            let body = payload(&model, with_reasoning("test-fireworks-key", reasoning)).await;
            if name == "off" {
                assert_eq!(body["thinking"], json!({"type": "disabled"}), "{id} {name}");
                assert!(body.get("output_config").is_none(), "{id} {name}");
            } else {
                assert_eq!(body["thinking"], json!({"type": "adaptive", "display": "summarized"}), "{id} {name}");
                assert_eq!(body["output_config"], json!({"effort": name}), "{id} {name}");
            }
        }
    }
}

// Regression for #9323: accepted aliases are not distinct native effort levels.
#[test]
fn fireworks_exposes_distinct_native_effort_levels() {
    for id in [
        "accounts/fireworks/models/glm-5p3",
        "accounts/fireworks/routers/glm-5p3-fast",
        "accounts/fireworks/models/kimi-k3",
        "accounts/fireworks/routers/kimi-k3-fast",
    ] {
        assert_eq!(levels(&builtin("fireworks", id)), ["low", "high", "max"], "{id}");
    }
}

#[tokio::test]
async fn fireworks_toggle_only_models_keep_budget_thinking() {
    let model = builtin("fireworks", "accounts/fireworks/models/nemotron-3-ultra-nvfp4");
    assert!(as_json(&model)["compat"].get("forceAdaptiveThinking").is_none());
    let body = payload(&model, with_reasoning("test-fireworks-key", Some(level("high")))).await;
    assert_eq!(body["thinking"], json!({"type": "enabled", "budget_tokens": 16384, "display": "summarized"}));
    assert!(body.get("output_config").is_none());
}

#[tokio::test]
async fn provider_env_keys_resolve() {
    let _lock = ENV_LOCK.lock().await;
    for (provider, name, value) in [
        ("fireworks", "FIREWORKS_API_KEY", "test-fireworks-key"),
        ("together", "TOGETHER_API_KEY", "test-together-key"),
        ("baseten", "BASETEN_API_KEY", "test-baseten-key"),
    ] {
        let _env = EnvGuard::set(&[(name, Some(value))]);
        assert_eq!(find_env_keys(provider, None), Some(vec![name.to_string()]));
        assert_eq!(get_env_api_key(provider, None).as_deref(), Some(value));
    }
}

#[test]
fn fireworks_compat_for_session_affinity_and_tool_fields() {
    let compat = &as_json(&builtin("fireworks", "accounts/fireworks/models/nemotron-3-ultra-nvfp4"))["compat"];
    assert_eq!(compat["sendSessionAffinityHeaders"], true);
    assert_eq!(compat["supportsEagerToolInputStreaming"], false);
    assert_eq!(compat["supportsCacheControlOnTools"], false);
    assert_eq!(compat["supportsLongCacheRetention"], false);
    assert_eq!(compat["allowEmptySignature"], true);
}

fn fireworks_messages_model(compat: Value) -> Value {
    json!({"id": "accounts/fireworks/models/kimi-k2p6", "name": "Kimi K2.6", "api": "anthropic-messages", "provider": "fireworks",
        "baseUrl": "http://127.0.0.1:0", "reasoning": true, "input": ["text", "image"],
        "cost": {"input": 0.95, "output": 4, "cacheRead": 0.16, "cacheWrite": 0}, "contextWindow": 262000, "maxTokens": 262000, "compat": compat})
}

fn fireworks_compat() -> Value {
    json!({"allowEmptySignature": true, "sendSessionAffinityHeaders": true, "supportsEagerToolInputStreaming": false,
        "supportsCacheControlOnTools": false, "supportsLongCacheRetention": false})
}

fn native_anthropic_model() -> Value {
    json!({"id": "claude-opus-4-8", "name": "Claude Opus 4.8", "api": "anthropic-messages", "provider": "anthropic",
        "baseUrl": "http://127.0.0.1:0", "reasoning": true, "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": 200000, "maxTokens": 32000})
}

fn openrouter_messages_model() -> Value {
    let mut model = native_anthropic_model();
    model["id"] = json!("anthropic/claude-opus-4.8");
    model["provider"] = json!("openrouter");
    model
}

/// `captureAnthropicRequest`: the Messages adapter against a local server answering an empty SSE body.
async fn capture_anthropic_request(model: Value, session_id: Option<&str>, cache_retention: CacheRetention) -> Request {
    let server = MockServer::start(|_, _| Response::sse("")).await;
    let mut model = crate::support::model(model);
    model.base_url = server.url.clone();
    let context = pi_ai::utils::transcript::normalize_context(&ctx(json!({
        "messages": [{"role": "user", "content": "Use the tool", "timestamp": 1}],
        "tools": [{"name": "lookup", "description": "Look up a value", "parameters": {"type": "object", "properties": {"value": {"type": "string"}}, "required": ["value"]}}]
    })));
    let options = StreamOptions {
        api_key: Some("test-key".into()),
        cache_retention: Some(cache_retention),
        session_id: session_id.map(str::to_string),
        ..Default::default()
    };
    pi_ai::api::anthropic_messages::stream(&model, &context, options).result().await;
    server.requests().into_iter().next().expect("Anthropic request was not captured")
}

#[tokio::test]
async fn anthropic_compatible_session_affinity_and_tool_compat() {
    let request = capture_anthropic_request(fireworks_messages_model(fireworks_compat()), Some("fireworks-session-1"), CacheRetention::Short).await;
    assert_eq!(request.header("x-session-affinity"), Some("fireworks-session-1"));

    let request = capture_anthropic_request(native_anthropic_model(), Some("anthropic-session-1"), CacheRetention::Short).await;
    assert_eq!(request.header("x-session-affinity"), None);

    let request = capture_anthropic_request(fireworks_messages_model(fireworks_compat()), Some("fireworks-session-2"), CacheRetention::None).await;
    assert_eq!(request.header("x-session-affinity"), None);

    // Regression test for #9102.
    let request = capture_anthropic_request(openrouter_messages_model(), Some("openrouter-session-1"), CacheRetention::Short).await;
    assert_eq!(request.header("x-session-id"), Some("openrouter-session-1"));
    assert_eq!(request.header("x-session-affinity"), None);

    let request = capture_anthropic_request(openrouter_messages_model(), Some("openrouter-session-2"), CacheRetention::None).await;
    assert_eq!(request.header("x-session-id"), None);
    assert_eq!(request.header("x-session-affinity"), None);

    let mut disabled = openrouter_messages_model();
    disabled["compat"] = json!({"sendSessionAffinityHeaders": false});
    let request = capture_anthropic_request(disabled, Some("openrouter-session-3"), CacheRetention::Short).await;
    assert_eq!(request.header("x-session-id"), None);

    let fireworks = capture_anthropic_request(fireworks_messages_model(fireworks_compat()), None, CacheRetention::Short).await.json();
    let tools = fireworks["tools"].as_array().unwrap();
    assert!(tools.last().unwrap().get("cache_control").is_none());
    assert!(tools.iter().all(|t| t.get("eager_input_streaming").is_none()));

    let native = capture_anthropic_request(native_anthropic_model(), None, CacheRetention::Short).await.json();
    let tools = native["tools"].as_array().unwrap();
    assert_eq!(tools.last().unwrap()["cache_control"]["type"], "ephemeral");
    assert_eq!(tools[0]["eager_input_streaming"], true);
}

// ---------------------------------------------------------------------------
// together-models.test.ts
// ---------------------------------------------------------------------------

#[test]
fn together_kimi_k3_uses_chat_completions() {
    let model = as_json(&builtin("together", "moonshotai/Kimi-K3"));
    assert_eq!(model["api"], "openai-completions");
    assert_eq!(model["provider"], "together");
    assert_eq!(model["baseUrl"], "https://api.together.ai/v1");
    assert_eq!(model["reasoning"], true);
    assert_eq!(model["thinkingLevelMap"], json!({"minimal": null, "low": null, "medium": null}));
    assert_eq!(model["input"], json!(["text", "image"]));
    assert_eq!(model["contextWindow"], 1048576);
    assert_eq!(model["maxTokens"], 131072);
    assert_json_matches(&model["cost"], json!({"input": 3, "output": 15, "cacheRead": 0.3, "cacheWrite": 0}));
    assert_eq!(
        model["compat"],
        json!({"supportsStore": false, "supportsDeveloperRole": false, "supportsReasoningEffort": false, "maxTokensField": "max_tokens",
            "thinkingFormat": "together", "supportsStrictMode": false, "supportsLongCacheRetention": false})
    );
}

#[test]
fn together_reasoning_controls_follow_the_api_surface() {
    let gpt_oss = as_json(&builtin("together", "openai/gpt-oss-120b"));
    assert_eq!(
        gpt_oss["thinkingLevelMap"],
        json!({"off": null, "minimal": null, "low": "low", "medium": "medium", "high": "high", "max": null, "xhigh": null})
    );
    assert_json_matches(&gpt_oss["compat"]["supportsReasoningEffort"], json!(true));
    assert_eq!(gpt_oss["compat"]["thinkingFormat"], "openai");
    let deepseek = as_json(&builtin("together", "deepseek-ai/DeepSeek-V4-Pro-0813"));
    assert_eq!(deepseek["thinkingLevelMap"], json!({"minimal": null, "low": null, "medium": null, "high": "high", "xhigh": null}));
    assert_eq!(deepseek["compat"]["supportsReasoningEffort"], true);
    assert_eq!(deepseek["compat"]["thinkingFormat"], "together");
}

// ---------------------------------------------------------------------------
// baseten-models.test.ts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn baseten_glm_models_and_reasoning_payloads() {
    assert_eq!(as_json(&builtin("baseten", "zai-org/GLM-5.2"))["input"], json!(["text"]));
    assert_eq!(as_json(&builtin("baseten", "zai-org/GLM-5.2-Fast"))["input"], json!(["text"]));
    let model = builtin("baseten", "zai-org/GLM-5.2");
    let body = payload(&model, with_reasoning("test-baseten-key", Some(level("high")))).await;
    assert_eq!(body["chat_template_args"], json!({"enable_thinking": true}));
    assert_eq!(body["reasoning_effort"], "high");
    let body = payload(&model, with_reasoning("test-baseten-key", None)).await;
    assert_eq!(body["chat_template_args"], json!({"enable_thinking": false}));
    assert_eq!(body["reasoning_effort"], "none");
}
