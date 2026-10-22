//! Port of cache-retention.test.ts. The TS cases gated on `ANTHROPIC_API_KEY`/`OPENAI_API_KEY`
//! only need a key to get past auth before the payload hook throws; they run offline here with a
//! placeholder key. `PI_CACHE_RETENTION` is passed through the scoped provider env.

use pi_ai::api::{anthropic_messages, openai_completions, openai_responses};
use pi_ai::providers::all::get_builtin_model;
use pi_ai::utils::transcript::normalize_context;
use pi_ai::*;
use serde_json::{Value, json};

use crate::support::*;

fn builtin(provider: &str, id: &str) -> Model {
    get_builtin_model(provider, id).unwrap_or_else(|| panic!("{provider}/{id}"))
}

fn context() -> Context {
    ctx(json!({"systemPrompt": "You are a helpful assistant.", "messages": [{"role": "user", "content": "Hello", "timestamp": 1}]}))
}

fn long_env() -> Option<ProviderEnv> {
    let mut env = ProviderEnv::new();
    env.insert("PI_CACHE_RETENTION".into(), "long".into());
    Some(env)
}

fn options(retention: Option<CacheRetention>, session: Option<&str>, env: Option<ProviderEnv>) -> StreamOptions {
    StreamOptions {
        api_key: Some("fake-key".into()),
        cache_retention: retention,
        session_id: session.map(str::to_string),
        env,
        ..Default::default()
    }
}

async fn anthropic_payload(model: &Model, options: StreamOptions) -> Value {
    let transcript = normalize_context(&context());
    capture_payload_offline(|o| anthropic_messages::stream(model, &transcript, o), options).await
}

async fn responses_payload(model: &Model, options: StreamOptions) -> Value {
    // OpenAI keys start with `sk-`; anything else reads as a ChatGPT sign-in token.
    let options = StreamOptions { api_key: Some("sk-fake-key".into()), ..options };
    let transcript = normalize_context(&context());
    // Keep the host (proxy detection reads it); the request is cancelled from the payload hook.
    capture_payload_offline(|o| openai_responses::stream(model, &transcript, o), options).await
}

async fn completions_payload(model: &Model, context: &Context, options: StreamOptions) -> Value {
    let transcript = normalize_context(context);
    capture_payload_offline(|o| openai_completions::stream(model, &transcript, o), options).await
}

#[tokio::test]
async fn anthropic_cache_retention() {
    let haiku = builtin("anthropic", "claude-haiku-4-5");
    let system_cache = |payload: &Value| payload["system"][0].get("cache_control").cloned();

    // Defaults: an ephemeral breakpoint without ttl; PI_CACHE_RETENTION=long adds the 1h ttl.
    let payload = capture_payload_offline(|o| pi_ai::compat::stream(&haiku, &context(), o), options(None, None, None)).await;
    assert_eq!(system_cache(&payload), Some(json!({"type": "ephemeral"})));
    let payload = capture_payload_offline(|o| pi_ai::compat::stream(&haiku, &context(), o), options(None, None, long_env())).await;
    assert_eq!(system_cache(&payload), Some(json!({"type": "ephemeral", "ttl": "1h"})));

    // A proxy base URL still gets the ttl by default.
    let mut proxy = haiku.clone();
    proxy.base_url = "https://my-proxy.example.com/v1".into();
    let payload = anthropic_payload(&proxy, options(None, None, long_env())).await;
    assert_eq!(system_cache(&payload), Some(json!({"type": "ephemeral", "ttl": "1h"})));

    // supportsLongCacheRetention: false drops it.
    let mut no_long = proxy.clone();
    no_long.compat = Some(serde_json::from_value(json!({"supportsLongCacheRetention": false})).unwrap());
    let payload = anthropic_payload(&no_long, options(Some(CacheRetention::Long), None, None)).await;
    assert_eq!(system_cache(&payload), Some(json!({"type": "ephemeral"})));

    let payload = anthropic_payload(&haiku, options(Some(CacheRetention::None), None, None)).await;
    assert_eq!(system_cache(&payload), None);

    // String user messages become blocks carrying the breakpoint.
    let payload = anthropic_payload(&haiku, options(None, None, None)).await;
    let last = payload["messages"].as_array().unwrap().last().unwrap().clone();
    let blocks = last["content"].as_array().expect("content blocks");
    assert_eq!(blocks.last().unwrap()["cache_control"], json!({"type": "ephemeral"}));

    let payload = anthropic_payload(&haiku, options(Some(CacheRetention::Long), None, None)).await;
    assert_eq!(system_cache(&payload), Some(json!({"type": "ephemeral", "ttl": "1h"})));
}

#[tokio::test]
async fn openai_responses_cache_retention() {
    for id in ["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-6-astra", "gpt-6-luna", "gpt-6-sol"] {
        assert!(serde_json::to_value(builtin("openai", id)).unwrap().get("promptCache").is_none(), "{id}");
    }
    let mini = builtin("openai", "gpt-4o-mini");
    let payload = responses_payload_compat(&mini, options(None, None, None)).await;
    assert!(payload.get("prompt_cache_retention").is_none());
    let payload = responses_payload_compat(&mini, options(None, None, long_env())).await;
    assert_eq!(payload["prompt_cache_retention"], "24h");

    let mut proxy = mini.clone();
    proxy.base_url = "https://my-proxy.example.com/v1".into();
    let payload = responses_payload(&proxy, options(None, None, long_env())).await;
    assert_eq!(payload["prompt_cache_retention"], "24h");

    let mut no_long = mini.clone();
    no_long.compat = Some(serde_json::from_value(json!({"supportsLongCacheRetention": false})).unwrap());
    let payload = responses_payload(&no_long, options(Some(CacheRetention::Long), Some("session-compat-false"), None)).await;
    assert!(payload.get("prompt_cache_retention").is_none());

    let payload = responses_payload(&builtin("openai", "gpt-5.6-sol"), options(Some(CacheRetention::None), Some("session-1"), None)).await;
    assert!(payload.get("prompt_cache_key").is_none());
    assert!(payload.get("prompt_cache_retention").is_none());
    assert_eq!(payload["prompt_cache_options"], json!({"mode": "explicit"}));

    let payload = responses_payload(&mini, options(Some(CacheRetention::None), Some("session-1"), None)).await;
    assert!(payload.get("prompt_cache_key").is_none());
    assert!(payload.get("prompt_cache_options").is_none());

    for (id, retention, cache_options) in [
        ("gpt-4o-mini", Some("24h"), None),
        ("gpt-6-astra", None, Some(json!({"ttl": "30m"}))),
        ("gpt-6-sol", None, Some(json!({"ttl": "30m"}))),
        ("gpt-6-luna", None, Some(json!({"ttl": "30m"}))),
    ] {
        let payload = responses_payload(&builtin("openai", id), options(Some(CacheRetention::Long), Some("session-2"), None)).await;
        assert_eq!(payload["prompt_cache_key"], "session-2", "{id}");
        assert_eq!(payload.get("prompt_cache_retention").and_then(Value::as_str), retention, "{id}");
        assert_eq!(payload.get("prompt_cache_options").cloned(), cache_options, "{id}");
    }
}

async fn responses_payload_compat(model: &Model, options: StreamOptions) -> Value {
    let options = StreamOptions { api_key: Some("sk-fake-key".into()), ..options };
    capture_payload_offline(|o| pi_ai::compat::stream(model, &context(), o), options).await
}

fn completions_test_model(compat: Option<Value>) -> Model {
    let mut value = json!({"id": "test-model", "name": "Test Model", "api": "openai-completions", "provider": "test-openai-completions",
        "baseUrl": "https://my-proxy.example.com/v1", "reasoning": false, "input": ["text"], "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": 128000, "maxTokens": 4096});
    if let Some(compat) = compat {
        value["compat"] = compat;
    }
    model(value)
}

#[tokio::test]
async fn openai_completions_cache_retention() {
    let payload = completions_payload(&completions_test_model(None), &context(), options(Some(CacheRetention::Long), Some("session-completions"), None)).await;
    assert_eq!(payload["prompt_cache_key"], "session-completions");
    assert_eq!(payload["prompt_cache_retention"], "24h");

    let no_long = completions_test_model(Some(json!({"supportsLongCacheRetention": false})));
    let payload = completions_payload(&no_long, &context(), options(Some(CacheRetention::Long), Some("session-completions-false"), None)).await;
    assert!(payload.get("prompt_cache_key").is_none());
    assert!(payload.get("prompt_cache_retention").is_none());

    for id in ["deepseek-v4-flash", "deepseek-v4-pro", "kimi-k2.5", "kimi-k2.6", "minimax-m2.7"] {
        let model = builtin("opencode", id);
        assert_eq!(serde_json::to_value(&model).unwrap()["compat"]["supportsLongCacheRetention"], false, "{id}");
        let payload =
            completions_payload(&model, &context(), options(Some(CacheRetention::Long), Some("session-opencode-long-cache-unsupported"), None)).await;
        assert!(payload.get("prompt_cache_key").is_none(), "{id}");
        assert!(payload.get("prompt_cache_retention").is_none(), "{id}");
    }

    let tools_context = ctx(json!({"messages": [
        {"role": "system", "content": "test", "toolsAdded": [
            {"name": "t1", "description": "strict tool", "parameters": {"type": "object", "properties": {"x": {"type": "string"}}, "required": ["x"]},
             "constrainedSampling": {"type": "json_schema", "strict": "prefer"}},
            {"name": "t2", "description": "non-strict tool", "parameters": {"type": "object", "properties": {"y": {"type": "string"}}, "required": ["y"]}}
        ], "timestamp": 0},
        {"role": "user", "content": "hello", "timestamp": 1}
    ]}));
    for id in ["gpt-oss-120b", "qwen-3.8-27b"] {
        let model = builtin("cerebras", id);
        assert!(serde_json::to_value(&model).unwrap()["compat"].get("supportsStrictMode").is_none(), "{id}");
        let payload = completions_payload(&model, &tools_context, options(None, Some("test"), None)).await;
        let tools = payload["tools"].as_array().expect("tools");
        assert!(tools.iter().all(|t| t["function"].get("strict").is_none()), "{id}: {tools:?}");
    }
}
