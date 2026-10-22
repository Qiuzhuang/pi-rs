//! Port of `test/model-registry.test.ts` (models.json composition, overrides, dynamic providers and
//! API key resolution through `ModelRegistry`/`ModelRuntime`).

use std::sync::Arc;

use futures::FutureExt;
use pi_ai::auth::types::{ApiKeyCredential, Credential, CredentialStore, OAuthCredentials};
use pi_ai::models::get_supported_thinking_levels;
use pi_ai::providers::model_schema::{ModelInputModality, ModelThinkingLevel};
use pi_ai::providers::model_schema::ModelCost;
use pi_ai::types::Model;
use pi_coding_agent::core::auth_storage::AuthStorage;
use pi_coding_agent::core::extensions::types::{
    ProviderChatModelConfig, ProviderConfig, ProviderModelConfig, ProviderOAuthConfig,
};
use pi_coding_agent::core::model_registry::{
    ModelRegistry, ModelRegistryRegisterProviderProviderOrName, ResolvedRequestAuth, ResolvedRequestAuthOk,
};
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use pi_coding_agent::core::models_store::InMemoryCodingAgentModelsStore;
use pi_coding_agent::core::provider_composer::{AuthStatus, AuthStatusSource};
use serde_json::{Value, json};

struct Fixture {
    dir: tempfile::TempDir,
    auth: Arc<AuthStorage>,
}

impl Fixture {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap(), auth: AuthStorage::in_memory(None) }
    }
    fn models_path(&self) -> String {
        self.dir.path().join("models.json").to_string_lossy().into_owned()
    }
    fn write_models(&self, providers: Value) {
        std::fs::write(self.models_path(), json!({ "providers": providers }).to_string()).unwrap();
    }
    fn file(&self, name: &str) -> String {
        self.dir.path().join(name).to_string_lossy().into_owned()
    }
    async fn registry(&self) -> ModelRegistry {
        create_model_registry(self.auth.clone(), Some(self.models_path())).await
    }
}

async fn create_model_registry(credentials: Arc<dyn CredentialStore>, models_path: Option<String>) -> ModelRegistry {
    let runtime = ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(credentials),
        models_path,
        models_store: Some(Arc::new(InMemoryCodingAgentModelsStore::new())),
        allow_model_network: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap();
    ModelRegistry::new(runtime)
}

fn provider_models(registry: &ModelRegistry, provider: &str) -> Vec<Model> {
    registry.get_all().into_iter().filter(|m| m.provider == provider).collect()
}

fn chat_config(id: &str) -> ProviderModelConfig {
    ProviderModelConfig::Chat(ProviderChatModelConfig {
        id: id.to_string(),
        name: id.to_string(),
        api: None,
        base_url: None,
        input: vec![ModelInputModality::Text],
        input_limits: None,
        cost: ModelCost::default(),
        headers: None,
        reasoning: false,
        thinking_level_map: None,
        prompt_cache: None,
        context_window: 100000,
        max_tokens: 8000,
        sampling_params: None,
        compat: None,
    })
}

fn provider_config(base_url: &str, ids: &[&str], api: &str) -> ProviderConfig {
    ProviderConfig {
        base_url: Some(base_url.to_string()),
        api_key: Some("test-key".to_string()),
        api: Some(api.to_string()),
        models: Some(ids.iter().map(|id| chat_config(id)).collect()),
        ..Default::default()
    }
}

fn provider_config_json(base_url: &str, ids: &[&str], api: &str) -> Value {
    json!({
        "baseUrl": base_url,
        "apiKey": "test-key",
        "api": api,
        "models": ids.iter().map(|id| json!({
            "id": id,
            "name": id,
            "reasoning": false,
            "input": ["text"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
            "contextWindow": 100000,
            "maxTokens": 8000,
        })).collect::<Vec<_>>(),
    })
}

fn register(registry: &ModelRegistry, name: &str, config: ProviderConfig) -> anyhow::Result<()> {
    registry.register_provider(&ModelRegistryRegisterProviderProviderOrName::String(name.to_string()), Some(&config))
}

fn ok_auth(auth: ResolvedRequestAuth) -> ResolvedRequestAuthOk {
    match auth {
        ResolvedRequestAuth::Ok(auth) => auth,
        ResolvedRequestAuth::Err { error } => panic!("expected ok auth, got error {error}"),
    }
}

fn header(auth: &ResolvedRequestAuthOk, name: &str) -> Option<String> {
    auth.headers.as_ref().and_then(|h| h.get(name)).cloned().flatten()
}

fn compat_value(model: &Model) -> Value {
    serde_json::to_value(model.compat.clone().unwrap_or_default()).unwrap()
}

// ---------------------------------------------------------------------------
// baseUrl override (no custom models)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn overriding_base_url_keeps_and_rewrites_all_built_in_models() {
    let fx = Fixture::new();
    fx.write_models(json!({ "anthropic": { "baseUrl": "https://my-proxy.example.com/v1" } }));
    let registry = fx.registry().await;
    let models = provider_models(&registry, "anthropic");
    assert!(models.len() > 1);
    assert!(models.iter().any(|m| m.id.contains("claude")));
    assert!(models.iter().all(|m| m.base_url == "https://my-proxy.example.com/v1"));
    let google = provider_models(&registry, "google");
    assert!(!google.is_empty());
    assert_ne!(google[0].base_url, "https://my-proxy.example.com/v1");
}

#[tokio::test]
async fn overriding_headers_resolves_at_request_time() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "anthropic": { "baseUrl": "https://my-proxy.example.com/v1", "headers": { "X-Custom-Header": "custom-value" } }
    }));
    let registry = fx.registry().await;
    for model in provider_models(&registry, "anthropic").iter().take(3) {
        let auth = ok_auth(registry.get_api_key_and_headers(model).await);
        assert_eq!(header(&auth, "X-Custom-Header").as_deref(), Some("custom-value"));
    }
}

#[tokio::test]
async fn headers_only_override_resolves_at_request_time() {
    let fx = Fixture::new();
    fx.write_models(json!({ "anthropic": { "headers": { "X-Custom-Header": "custom-value" } } }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_error(), None);
    let model = provider_models(&registry, "anthropic")[0].clone();
    let auth = ok_auth(registry.get_api_key_and_headers(&model).await);
    assert_eq!(header(&auth, "X-Custom-Header").as_deref(), Some("custom-value"));
}

#[tokio::test]
async fn unconfigured_compatibility_auth_includes_static_model_headers() {
    let fx = Fixture::new();
    let registry = fx.registry().await;
    let mut model = registry.get_all()[0].clone();
    model.provider = "missing-provider".into();
    model.headers = Some([("X-Static-Model".to_string(), "static-value".to_string())].into_iter().collect());
    let auth = registry.get_api_key_and_headers(&model).await;
    assert_eq!(serde_json::to_value(&auth).unwrap(), json!({ "ok": true, "headers": { "X-Static-Model": "static-value" } }));
}

#[tokio::test]
async fn can_mix_base_url_override_and_models_merge() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "anthropic": { "baseUrl": "https://anthropic-proxy.example.com/v1" },
        "google": provider_config_json("https://google-proxy.example.com/v1", &["gemini-custom"], "google-generative-ai"),
    }));
    let registry = fx.registry().await;
    let anthropic = provider_models(&registry, "anthropic");
    assert!(anthropic.len() > 1);
    assert_eq!(anthropic[0].base_url, "https://anthropic-proxy.example.com/v1");
    let google = provider_models(&registry, "google");
    assert!(google.len() > 1);
    assert!(google.iter().any(|m| m.id == "gemini-custom"));
}

#[tokio::test]
async fn refresh_picks_up_base_url_override_changes() {
    let fx = Fixture::new();
    fx.write_models(json!({ "anthropic": { "baseUrl": "https://first-proxy.example.com/v1" } }));
    let registry = fx.registry().await;
    assert_eq!(provider_models(&registry, "anthropic")[0].base_url, "https://first-proxy.example.com/v1");
    fx.write_models(json!({ "anthropic": { "baseUrl": "https://second-proxy.example.com/v1" } }));
    registry.refresh(None).await.unwrap();
    assert_eq!(provider_models(&registry, "anthropic")[0].base_url, "https://second-proxy.example.com/v1");
}

// ---------------------------------------------------------------------------
// custom models merge behavior
// ---------------------------------------------------------------------------

#[tokio::test]
async fn built_in_provider_custom_models_inherit_api_and_base_url() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "openrouter": { "models": [{ "id": "fake-provider/fake-model", "name": "Fake model", "reasoning": true, "input": ["text"] }] }
    }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_error(), None);
    let model = registry.find("openrouter", "fake-provider/fake-model").unwrap();
    assert_eq!(model.api, "openai-completions");
    assert_eq!(model.base_url, "https://openrouter.ai/api/v1");
}

#[tokio::test]
async fn non_built_in_provider_custom_models_still_require_base_url() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "my-custom-provider": { "apiKey": "test-key", "models": [{ "id": "my-model", "api": "openai-completions" }] }
    }));
    let registry = fx.registry().await;
    assert!(registry.get_error().unwrap().contains("baseUrl"));
}

#[tokio::test]
async fn reports_every_provider_composition_error() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "broken-one": { "api": "openai-completions", "models": [{ "id": "one" }] },
        "broken-two": { "api": "openai-completions", "models": [{ "id": "two" }] },
    }));
    let registry = fx.registry().await;
    let error = registry.get_error().unwrap();
    assert!(error.contains("Provider \"broken-one\""));
    assert!(error.contains("Provider \"broken-two\""));
}

#[tokio::test]
async fn custom_provider_with_built_in_name_merges_and_replaces_by_id() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "anthropic": provider_config_json("https://my-proxy.example.com/v1", &["claude-custom"], "anthropic-messages"),
        "openrouter": provider_config_json("https://my-proxy.example.com/v1", &["anthropic/claude-sonnet-4.5"], "openai-completions"),
    }));
    let registry = fx.registry().await;
    let anthropic = provider_models(&registry, "anthropic");
    assert!(anthropic.len() > 1);
    assert!(anthropic.iter().any(|m| m.id == "claude-custom"));
    assert!(anthropic.iter().all(|m| m.base_url == "https://my-proxy.example.com/v1"));
    let sonnet: Vec<Model> = provider_models(&registry, "openrouter")
        .into_iter()
        .filter(|m| m.id == "anthropic/claude-sonnet-4.5")
        .collect();
    assert_eq!(sonnet.len(), 1);
    assert_eq!(sonnet[0].base_url, "https://my-proxy.example.com/v1");
    assert!(!provider_models(&registry, "google").is_empty());
    assert!(!provider_models(&registry, "openai").is_empty());
}

#[tokio::test]
async fn provider_and_model_level_compat_for_custom_models() {
    let fx = Fixture::new();
    let model = |compat: Option<Value>| {
        let mut value = json!({
            "id": "demo-model", "reasoning": false, "input": ["text"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
            "contextWindow": 1000, "maxTokens": 100,
        });
        if let Some(compat) = compat {
            value["compat"] = compat;
        }
        value
    };
    fx.write_models(json!({
        "demo": {
            "baseUrl": "https://example.com/v1", "apiKey": "DEMO_KEY", "api": "openai-completions",
            "compat": { "supportsUsageInStreaming": false, "maxTokensField": "max_tokens" },
            "models": [model(None)],
        }
    }));
    let registry = fx.registry().await;
    let compat = compat_value(&registry.find("demo", "demo-model").unwrap());
    assert_eq!(compat["supportsUsageInStreaming"], json!(false));
    assert_eq!(compat["maxTokensField"], json!("max_tokens"));

    fx.write_models(json!({
        "demo": {
            "baseUrl": "https://example.com/v1", "apiKey": "DEMO_KEY", "api": "openai-completions",
            "compat": { "supportsUsageInStreaming": false, "maxTokensField": "max_tokens" },
            "models": [model(Some(json!({ "supportsUsageInStreaming": true, "maxTokensField": "max_completion_tokens" })))],
        }
    }));
    registry.refresh(None).await.unwrap();
    let compat = compat_value(&registry.find("demo", "demo-model").unwrap());
    assert_eq!(compat["supportsUsageInStreaming"], json!(true));
    assert_eq!(compat["maxTokensField"], json!("max_completion_tokens"));
}

#[tokio::test]
async fn provider_level_compat_applies_to_built_in_models() {
    let fx = Fixture::new();
    fx.write_models(json!({ "openrouter": { "compat": { "supportsUsageInStreaming": false, "supportsStrictMode": false } } }));
    let registry = fx.registry().await;
    let models = provider_models(&registry, "openrouter");
    assert!(!models.is_empty());
    for model in models {
        let compat = compat_value(&model);
        assert_eq!(compat["supportsUsageInStreaming"], json!(false));
        assert_eq!(compat["supportsStrictMode"], json!(false));
    }
}

#[tokio::test]
async fn schema_accepts_thinking_level_map_and_chat_template_compat() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "demo": {
            "baseUrl": "https://example.com/v1", "apiKey": "DEMO_KEY", "api": "openai-completions",
            "models": [
                {
                    "id": "demo-model", "reasoning": true, "input": ["text"],
                    "thinkingLevelMap": { "minimal": null, "high": "max" },
                    "compat": { "supportsStrictMode": false, "cacheControlFormat": "anthropic" },
                },
                {
                    "id": "kwargs-model", "reasoning": true,
                    "compat": {
                        "thinkingFormat": "chat-template",
                        "chatTemplateKwargs": { "preserve_thinking": true, "thinking": { "$var": "thinking.enabled" } },
                    },
                },
            ],
        }
    }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_error(), None);
    let model = registry.find("demo", "demo-model").unwrap();
    assert_eq!(serde_json::to_value(&model.thinking_level_map).unwrap(), json!({ "minimal": null, "high": "max" }));
    let compat = compat_value(&model);
    assert_eq!(compat["supportsStrictMode"], json!(false));
    assert_eq!(compat["cacheControlFormat"], json!("anthropic"));
    let compat = compat_value(&registry.find("demo", "kwargs-model").unwrap());
    assert_eq!(compat["thinkingFormat"], json!("chat-template"));
    assert_eq!(
        compat["chatTemplateKwargs"],
        json!({ "preserve_thinking": true, "thinking": { "$var": "thinking.enabled" } })
    );
}

#[tokio::test]
async fn model_level_base_url_overrides_provider_level_base_url() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "opencode-go": {
            "baseUrl": "https://opencode.ai/zen/go/v1",
            "apiKey": "TEST_KEY",
            "models": [
                { "id": "minimax-m2.5", "api": "anthropic-messages", "baseUrl": "https://opencode.ai/zen/go" },
                { "id": "glm-5", "api": "openai-completions" },
            ],
        }
    }));
    let registry = fx.registry().await;
    assert_eq!(registry.find("opencode-go", "minimax-m2.5").unwrap().base_url, "https://opencode.ai/zen/go");
    assert_eq!(registry.find("opencode-go", "glm-5").unwrap().base_url, "https://opencode.ai/zen/go/v1");
}

#[tokio::test]
async fn refresh_reloads_and_removes_custom_models() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "anthropic": provider_config_json("https://first-proxy.example.com/v1", &["claude-custom"], "anthropic-messages"),
    }));
    let registry = fx.registry().await;
    assert!(provider_models(&registry, "anthropic").iter().any(|m| m.id == "claude-custom"));
    fx.write_models(json!({
        "anthropic": provider_config_json("https://second-proxy.example.com/v1", &["claude-custom-2"], "anthropic-messages"),
    }));
    registry.refresh(None).await.unwrap();
    let models = provider_models(&registry, "anthropic");
    assert!(!models.iter().any(|m| m.id == "claude-custom"));
    assert!(models.iter().any(|m| m.id == "claude-custom-2"));
    fx.write_models(json!({}));
    registry.refresh(None).await.unwrap();
    let models = provider_models(&registry, "anthropic");
    assert!(!models.iter().any(|m| m.id == "claude-custom-2"));
    assert!(models.iter().any(|m| m.id.contains("claude")));
}

// ---------------------------------------------------------------------------
// modelOverrides
// ---------------------------------------------------------------------------

#[tokio::test]
async fn model_overrides_apply_to_single_built_in_models() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "openrouter": {
            "baseUrl": "https://my-proxy.example.com/v1",
            "modelOverrides": {
                "anthropic/claude-sonnet-4.5": { "name": "Custom Sonnet Name", "cost": { "input": 99 } },
                "nonexistent/model-id": { "name": "This should not appear" },
            },
        }
    }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_error(), None);
    let models = provider_models(&registry, "openrouter");
    let sonnet = models.iter().find(|m| m.id == "anthropic/claude-sonnet-4.5").unwrap();
    assert_eq!(sonnet.name, "Custom Sonnet Name");
    assert_eq!(sonnet.cost.input, 99.0);
    assert!(sonnet.cost.output > 0.0);
    assert_eq!(sonnet.base_url, "https://my-proxy.example.com/v1");
    let opus = models.iter().find(|m| m.id == "anthropic/claude-opus-4.1").unwrap();
    assert_ne!(opus.name, "Custom Sonnet Name");
    assert_eq!(opus.base_url, "https://my-proxy.example.com/v1");
    assert!(!models.iter().any(|m| m.id == "nonexistent/model-id"));

    fx.write_models(json!({}));
    registry.refresh(None).await.unwrap();
    let restored = registry.find("openrouter", "anthropic/claude-sonnet-4.5").unwrap();
    assert_ne!(restored.name, "Custom Sonnet Name");
}

#[tokio::test]
async fn anthropic_model_override_replaces_allowed_fallback_metadata() {
    let fx = Fixture::new();
    let fallbacks = json!([
        { "provider": "anthropic", "model": "claude-opus-5", "cost": { "input": 5, "output": 25, "cacheRead": 0.5, "cacheWrite": 6.25 } },
        { "provider": "anthropic", "model": "claude-opus-4-8", "cost": { "input": 4, "output": 20, "cacheRead": 0.4, "cacheWrite": 5 } },
    ]);
    fx.write_models(json!({
        "anthropic": { "modelOverrides": { "claude-fable-5": { "compat": { "allowedFallbackModels": fallbacks } } } }
    }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_error(), None);
    let compat = compat_value(&registry.find("anthropic", "claude-fable-5").unwrap());
    assert_eq!(compat["allowedFallbackModels"], fallbacks);

    fx.write_models(json!({
        "anthropic": { "modelOverrides": { "claude-fable-5": { "compat": { "allowedFallbackModels": [] } } } }
    }));
    registry.refresh(None).await.unwrap();
    let compat = compat_value(&registry.find("anthropic", "claude-fable-5").unwrap());
    assert_eq!(compat["allowedFallbackModels"], json!([]));
}

#[tokio::test]
async fn custom_models_and_model_overrides_carry_sampling_params_and_prompt_cache() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "openrouter": {
            "baseUrl": "https://my-proxy.example.com/v1",
            "api": "openai-completions",
            "models": [{
                "id": "custom/sampling-model",
                "samplingParams": { "temperature": 1, "top_p": 0.95, "top_k": 0 },
                "samplingParamsByThinkingLevel": { "low": { "temperature": 0.6, "top_p": 0.95 }, "high": { "temperature": 0.8 } },
                "promptCache": { "short": 120 },
            }],
            "modelOverrides": {
                "custom/sampling-model": {
                    "samplingParamsByThinkingLevel": { "low": { "temperature": 0.5, "top_k": 20 }, "max": { "temperature": 1 } },
                },
                "anthropic/claude-sonnet-4.5": {
                    "samplingParams": { "top_p": 0.9 },
                    "samplingParamsByThinkingLevel": { "high": { "temperature": 0.8 } },
                    "promptCache": { "short": 300 },
                },
            },
        },
        "anthropic": { "modelOverrides": { "claude-sonnet-4-6": { "promptCache": { "long": 1800 } } } },
    }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_error(), None);
    let custom = registry.find("openrouter", "custom/sampling-model").unwrap();
    assert_eq!(serde_json::to_value(&custom.sampling_params).unwrap(), json!({ "temperature": 1, "top_p": 0.95, "top_k": 0 }));
    assert_eq!(
        serde_json::to_value(&custom.sampling_params_by_thinking_level).unwrap(),
        json!({
            "low": { "temperature": 0.5, "top_p": 0.95, "top_k": 20 },
            "high": { "temperature": 0.8 },
            "max": { "temperature": 1 },
        })
    );
    assert_eq!(serde_json::to_value(&custom.prompt_cache).unwrap(), json!({ "short": 120 }));
    let sonnet = registry.find("openrouter", "anthropic/claude-sonnet-4.5").unwrap();
    assert_eq!(serde_json::to_value(&sonnet.sampling_params).unwrap(), json!({ "top_p": 0.9 }));
    assert_eq!(serde_json::to_value(&sonnet.prompt_cache).unwrap(), json!({ "short": 300 }));
    let opus = registry.find("openrouter", "anthropic/claude-opus-4.1").unwrap();
    assert!(opus.sampling_params.is_none());
    assert!(opus.sampling_params_by_thinking_level.is_none());
    // Overrides merge per tier with the built-in catalog.
    assert_eq!(
        serde_json::to_value(&registry.find("anthropic", "claude-sonnet-4-6").unwrap().prompt_cache).unwrap(),
        json!({ "short": 300, "long": 1800 })
    );
}

#[tokio::test]
async fn model_override_deep_merges_image_resize_limits() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "test": {
            "baseUrl": "https://example.com", "apiKey": "test-key", "api": "openai-completions",
            "models": [{
                "id": "vision-model",
                "input": ["text", "image"],
                "inputLimits": {
                    "maxRequestBytes": 33554432,
                    "images": { "maxPerRequest": 100, "resize": { "maxWidth": 2000, "maxHeight": 2000, "maxBytes": 4718592, "jpegQuality": 80 } },
                },
            }],
            "modelOverrides": {
                "vision-model": { "inputLimits": { "images": { "resize": { "maxWidth": 1568, "maxBytes": 524288, "jpegQuality": 75 } } } },
            },
        }
    }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_error(), None);
    let model = registry.find("test", "vision-model").unwrap();
    assert_eq!(
        serde_json::to_value(&model.input_limits).unwrap(),
        json!({
            "maxRequestBytes": 33554432,
            "images": { "resize": { "maxWidth": 1568, "maxHeight": 2000, "maxBytes": 524288, "jpegQuality": 75 }, "maxPerRequest": 100 },
        })
    );
}

#[tokio::test]
async fn model_overrides_merge_compat_routing_per_model() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "openrouter": {
            "compat": { "supportsFinishReason": true },
            "modelOverrides": {
                "anthropic/claude-sonnet-4.5": { "compat": { "openRouterRouting": { "only": ["amazon-bedrock"] }, "supportsFinishReason": false } },
                "anthropic/claude-opus-4.1": { "compat": { "openRouterRouting": { "only": ["anthropic"] } } },
            },
        }
    }));
    let registry = fx.registry().await;
    let sonnet = compat_value(&registry.find("openrouter", "anthropic/claude-sonnet-4.5").unwrap());
    let opus = compat_value(&registry.find("openrouter", "anthropic/claude-opus-4.1").unwrap());
    assert_eq!(sonnet["openRouterRouting"], json!({ "only": ["amazon-bedrock"] }));
    assert_eq!(opus["openRouterRouting"], json!({ "only": ["anthropic"] }));
    assert_eq!(sonnet["supportsFinishReason"], json!(false));
    assert_eq!(opus["supportsFinishReason"], json!(true));
}

#[tokio::test]
async fn model_override_can_add_headers_at_request_time() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "openrouter": { "modelOverrides": { "anthropic/claude-sonnet-4.5": { "headers": { "X-Custom-Model-Header": "value" } } } }
    }));
    let registry = fx.registry().await;
    let sonnet = registry.find("openrouter", "anthropic/claude-sonnet-4.5").unwrap();
    let auth = ok_auth(registry.get_api_key_and_headers(&sonnet).await);
    assert_eq!(header(&auth, "X-Custom-Model-Header").as_deref(), Some("value"));
}

// ---------------------------------------------------------------------------
// dynamic provider lifecycle
// ---------------------------------------------------------------------------

fn oauth_config(name: &str) -> ProviderOAuthConfig {
    ProviderOAuthConfig {
        name: name.to_string(),
        is_subscription: None,
        uses_callback_server: None,
        login: Arc::new(|_| {
            async {
                Ok(OAuthCredentials {
                    refresh: "refresh".into(),
                    access: "access".into(),
                    expires: pi_ai::utils::now_ms() + 60_000,
                    extra: Default::default(),
                })
            }
            .boxed()
        }),
        refresh_token: Arc::new(|credentials, _| async move { Ok(credentials) }.boxed()),
        get_api_key: Arc::new(|credentials| credentials.access.clone()),
        modify_models: None,
    }
}

#[tokio::test]
async fn provider_display_names() {
    let fx = Fixture::new();
    let registry = fx.registry().await;
    assert_eq!(registry.get_provider_display_name("openai"), "OpenAI");
    assert_eq!(registry.get_provider_display_name("github-copilot"), "GitHub Copilot");
    assert_eq!(registry.get_provider_display_name("unknown-provider"), "unknown-provider");

    let mut named = provider_config("https://provider.test/v1", &["demo-model"], "openai-completions");
    named.name = Some("Named Provider".into());
    register(&registry, "named-provider", named).unwrap();
    assert_eq!(registry.get_provider_display_name("named-provider"), "Named Provider");

    let mut oauth = provider_config("https://provider.test/v1", &["demo-model"], "openai-completions");
    oauth.api_key = None;
    oauth.oauth = Some(oauth_config("OAuth Provider"));
    register(&registry, "oauth-provider", oauth).unwrap();
    assert_eq!(registry.get_provider_display_name("oauth-provider"), "OAuth Provider");
}

#[tokio::test]
async fn model_overrides_apply_to_dynamically_registered_provider_models() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "extension-provider": {
            "modelOverrides": {
                "extension-model": {
                    "name": "Overridden Extension Model",
                    "thinkingLevelMap": { "off": null, "minimal": null, "low": null, "medium": null, "xhigh": "max" },
                    "headers": { "x-model-override": "enabled" },
                }
            }
        }
    }));
    let registry = fx.registry().await;
    let mut config = provider_config("https://provider.test/v1", &["extension-model"], "openai-completions");
    if let Some(ProviderModelConfig::Chat(model)) = config.models.as_mut().and_then(|m| m.first_mut()) {
        model.name = "Extension Model".into();
        model.reasoning = true;
    }
    register(&registry, "extension-provider", config).unwrap();
    let model = registry.find("extension-provider", "extension-model").unwrap();
    assert_eq!(model.name, "Overridden Extension Model");
    assert_eq!(
        get_supported_thinking_levels(&model),
        vec![ModelThinkingLevel::High, ModelThinkingLevel::Xhigh]
    );
    let auth = ok_auth(registry.get_api_key_and_headers(&model).await);
    assert_eq!(header(&auth, "x-model-override").as_deref(), Some("enabled"));
}

#[tokio::test]
async fn register_provider_treats_uppercase_api_key_and_headers_as_literals() {
    // SAFETY: unique variable names.
    unsafe {
        std::env::set_var("CUSTOM_NAME_RS", "env-CUSTOM_NAME");
        std::env::set_var("BEARER_RS", "env-BEARER");
    }
    let fx = Fixture::new();
    let registry = fx.registry().await;
    let mut config = provider_config("https://provider.test/v1", &["demo-model"], "openai-completions");
    config.api_key = Some("CUSTOM_NAME_RS".into());
    config.headers = Some([("Authorization".to_string(), "BEARER_RS".to_string())].into_iter().collect());
    register(&registry, "literal-provider", config).unwrap();
    assert_eq!(registry.get_api_key_for_provider("literal-provider").await.as_deref(), Some("CUSTOM_NAME_RS"));
    let model = registry.find("literal-provider", "demo-model").unwrap();
    let auth = ok_auth(registry.get_api_key_and_headers(&model).await);
    assert_eq!(auth.api_key.as_deref(), Some("CUSTOM_NAME_RS"));
    assert_eq!(header(&auth, "Authorization").as_deref(), Some("BEARER_RS"));
}

#[tokio::test]
async fn failed_register_provider_does_not_persist_invalid_config() {
    let fx = Fixture::new();
    let registry = fx.registry().await;
    let config = ProviderConfig {
        stream_simple: Some(Arc::new(|model, _, _| {
            pi_ai::api::lazy::error_stream(&model, anyhow::anyhow!("should not run"))
        })),
        ..Default::default()
    };
    let error = register(&registry, "broken-provider", config).unwrap_err();
    assert_eq!(error.to_string(), "Provider broken-provider: \"api\" is required when registering streamSimple.");
    assert!(!registry.refresh(None).await.unwrap().aborted);

    register(&registry, "demo-provider", provider_config("https://provider.test/v1", &["demo-model"], "openai-completions")).unwrap();
    assert!(registry.find("demo-provider", "demo-model").is_some());
    let broken = ProviderConfig {
        base_url: Some("https://provider.test/v2".into()),
        api_key: Some("test-key".into()),
        models: Some(vec![chat_config("broken-model")]),
        ..Default::default()
    };
    let error = register(&registry, "demo-provider", broken).unwrap_err();
    assert!(error.to_string().starts_with("Provider demo-provider, model broken-model: no \"api\" specified."));
    assert!(registry.find("demo-provider", "demo-model").is_some());
    registry.refresh(None).await.unwrap();
    assert!(registry.find("demo-provider", "demo-model").is_some());
}

#[tokio::test]
async fn unregister_provider_removes_the_runtime_oauth_overlay() {
    let fx = Fixture::new();
    let registry = fx.registry().await;
    register(&registry, "anthropic", ProviderConfig { oauth: Some(oauth_config("Custom Anthropic OAuth")), ..Default::default() })
        .unwrap();
    assert_eq!(
        registry.get_registered_provider_config("anthropic").and_then(|c| c.oauth).map(|o| o.name).as_deref(),
        Some("Custom Anthropic OAuth")
    );
    registry.unregister_provider("anthropic");
    assert!(registry.get_registered_provider_config("anthropic").is_none());
}

#[tokio::test]
async fn dynamic_provider_overrides_persist_across_refresh() {
    let fx = Fixture::new();
    let registry = fx.registry().await;
    register(&registry, "anthropic", ProviderConfig { base_url: Some("https://proxy.test/anthropic".into()), ..Default::default() })
        .unwrap();
    registry.refresh(None).await.unwrap();
    let models = provider_models(&registry, "anthropic");
    assert!(models.len() > 1);
    assert!(models.iter().all(|m| m.base_url == "https://proxy.test/anthropic"));

    register(&registry, "anthropic", provider_config("https://custom.test/anthropic", &["custom-claude"], "anthropic-messages"))
        .unwrap();
    registry.refresh(None).await.unwrap();
    assert_eq!(provider_models(&registry, "anthropic").iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["custom-claude"]);
    register(&registry, "anthropic", ProviderConfig { base_url: Some("https://proxy.test/anthropic".into()), ..Default::default() })
        .unwrap();
    registry.refresh(None).await.unwrap();
    assert_eq!(registry.find("anthropic", "custom-claude").unwrap().base_url, "https://proxy.test/anthropic");

    register(&registry, "custom-provider", provider_config("https://custom.test/v1", &["custom-a", "custom-b"], "openai-completions"))
        .unwrap();
    register(
        &registry,
        "custom-provider",
        ProviderConfig { headers: Some([("x-proxy".to_string(), "enabled".to_string())].into_iter().collect()), ..Default::default() },
    )
    .unwrap();
    registry.refresh(None).await.unwrap();
    let models = provider_models(&registry, "custom-provider");
    assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["custom-a", "custom-b"]);
    assert!(models.iter().all(|m| m.base_url == "https://custom.test/v1"));
    let auth = ok_auth(registry.get_api_key_and_headers(&models[0]).await);
    assert_eq!(header(&auth, "x-proxy").as_deref(), Some("enabled"));
}

// ---------------------------------------------------------------------------
// API key resolution
// ---------------------------------------------------------------------------

fn provider_with_api_key(api_key: &str) -> Value {
    json!({
        "baseUrl": "https://example.com/v1",
        "apiKey": api_key,
        "api": "anthropic-messages",
        "models": [{
            "id": "test-model", "name": "Test Model", "reasoning": false, "input": ["text"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
            "contextWindow": 100000, "maxTokens": 8000,
        }],
    })
}

async fn api_key_for(api_key: &str) -> Option<String> {
    let fx = Fixture::new();
    fx.write_models(json!({ "custom-provider": provider_with_api_key(api_key) }));
    fx.registry().await.get_api_key_for_provider("custom-provider").await
}

#[cfg(unix)]
#[tokio::test]
async fn api_key_commands() {
    assert_eq!(api_key_for("!echo test-api-key-from-command").await.as_deref(), Some("test-api-key-from-command"));
    assert_eq!(api_key_for("!echo '  spaced-key  '").await.as_deref(), Some("spaced-key"));
    assert_eq!(api_key_for("!printf 'line1\\nline2'").await.as_deref(), Some("line1\nline2"));
    assert_eq!(api_key_for("!exit 1").await, None);
    assert_eq!(api_key_for("!nonexistent-command-12345").await, None);
    assert_eq!(api_key_for("!printf ''").await, None);
    assert_eq!(api_key_for("!echo 'hello world' | tr ' ' '-'").await.as_deref(), Some("hello-world"));
}

#[tokio::test]
async fn api_key_environment_interpolation() {
    // SAFETY: unique variable names.
    unsafe {
        std::env::set_var("TEST_API_KEY_RS_12345", "env-api-key-value");
        std::env::set_var("TEST_BRACED_API_KEY_RS_12345", "braced-env-api-key-value");
        std::env::set_var("TEST_INTERPOLATED_PART_A_RS", "left");
        std::env::set_var("TEST_INTERPOLATED_PART_B_RS", "right");
    }
    assert_eq!(api_key_for("$TEST_API_KEY_RS_12345").await.as_deref(), Some("env-api-key-value"));
    assert_eq!(api_key_for("${TEST_BRACED_API_KEY_RS_12345}").await.as_deref(), Some("braced-env-api-key-value"));
    assert_eq!(
        api_key_for("${TEST_INTERPOLATED_PART_A_RS}_${TEST_INTERPOLATED_PART_B_RS}").await.as_deref(),
        Some("left_right")
    );
    assert_eq!(api_key_for("$$TEST_API_KEY_RS_12345").await.as_deref(), Some("$TEST_API_KEY_RS_12345"));
    assert_eq!(
        api_key_for("$!literal-$TEST_API_KEY_RS_12345").await.as_deref(),
        Some("!literal-env-api-key-value")
    );
    assert_eq!(api_key_for("TEST_API_KEY_RS_12345").await.as_deref(), Some("TEST_API_KEY_RS_12345"));
    assert_eq!(api_key_for("literal_api_key_value").await.as_deref(), Some("literal_api_key_value"));
}

fn sh_counter_command(counter: &str, tail: &str) -> String {
    format!("!sh -c 'count=$(cat \"{counter}\"); echo $((count + 1)) > \"{counter}\"; {tail}'")
}

#[cfg(unix)]
#[tokio::test]
async fn commands_are_executed_on_every_lookup_and_failures_are_retried() {
    let fx = Fixture::new();
    let counter = fx.file("counter");
    std::fs::write(&counter, "0").unwrap();
    fx.write_models(json!({ "custom-provider": provider_with_api_key(&sh_counter_command(&counter, "echo \"key-value\"")) }));
    let registry = fx.registry().await;
    for _ in 0..3 {
        assert_eq!(registry.get_api_key_for_provider("custom-provider").await.as_deref(), Some("key-value"));
    }
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "3");
    let second = fx.registry().await;
    second.get_api_key_for_provider("custom-provider").await;
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "4");

    let failing = Fixture::new();
    let counter = failing.file("counter");
    std::fs::write(&counter, "0").unwrap();
    failing.write_models(json!({ "custom-provider": provider_with_api_key(&sh_counter_command(&counter, "exit 1")) }));
    let registry = failing.registry().await;
    assert_eq!(registry.get_api_key_for_provider("custom-provider").await, None);
    assert_eq!(registry.get_api_key_for_provider("custom-provider").await, None);
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "2");
}

#[tokio::test]
async fn provider_auth_status_reports_configured_sources() {
    // SAFETY: unique variable names.
    unsafe {
        std::env::set_var("TEST_API_KEY_STATUS_RS_98765", "status-test-key");
        std::env::set_var("TEST_STATUS_PART_A_RS", "left");
        std::env::set_var("TEST_STATUS_PART_B_RS", "right");
        std::env::remove_var("TEST_API_KEY_MISSING_RS_98765");
    }
    let status = |api_key: &'static str| async move {
        let fx = Fixture::new();
        fx.write_models(json!({ "custom-provider": provider_with_api_key(api_key) }));
        let registry = fx.registry().await;
        let status = registry.get_provider_auth_status("custom-provider");
        let available = registry.get_available().iter().any(|m| m.provider == "custom-provider");
        (status, available)
    };
    assert_eq!(
        status("$TEST_API_KEY_STATUS_RS_98765").await.0,
        AuthStatus::configured(AuthStatusSource::Environment, Some("TEST_API_KEY_STATUS_RS_98765".into()))
    );
    assert_eq!(
        status("${TEST_STATUS_PART_A_RS}_${TEST_STATUS_PART_B_RS}").await.0,
        AuthStatus::configured(AuthStatusSource::Environment, Some("TEST_STATUS_PART_A_RS, TEST_STATUS_PART_B_RS".into()))
    );
    assert_eq!(status("literal_api_key_value").await.0, AuthStatus::configured(AuthStatusSource::ModelsJsonKey, None));
    let (missing, available) = status("$TEST_API_KEY_MISSING_RS_98765").await;
    assert_eq!(missing, AuthStatus::unconfigured());
    assert!(!available);
}

#[cfg(unix)]
#[tokio::test]
async fn command_api_keys_are_not_executed_for_status_or_availability() {
    let fx = Fixture::new();
    let counter = fx.file("counter");
    std::fs::write(&counter, "0").unwrap();
    fx.write_models(json!({ "custom-provider": provider_with_api_key(&sh_counter_command(&counter, "echo \"key-value\"")) }));
    let registry = fx.registry().await;
    assert_eq!(
        registry.get_provider_auth_status("custom-provider"),
        AuthStatus::configured(AuthStatusSource::ModelsJsonCommand, None)
    );
    assert!(registry.get_available().iter().any(|m| m.provider == "custom-provider"));
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "0");
}

#[tokio::test]
async fn environment_variables_are_not_cached() {
    // SAFETY: unique variable name.
    unsafe { std::env::set_var("TEST_API_KEY_CACHE_RS_98765", "first-value") };
    let fx = Fixture::new();
    fx.write_models(json!({ "custom-provider": provider_with_api_key("$TEST_API_KEY_CACHE_RS_98765") }));
    let registry = fx.registry().await;
    assert_eq!(registry.get_api_key_for_provider("custom-provider").await.as_deref(), Some("first-value"));
    unsafe { std::env::set_var("TEST_API_KEY_CACHE_RS_98765", "second-value") };
    assert_eq!(registry.get_api_key_for_provider("custom-provider").await.as_deref(), Some("second-value"));
}

#[cfg(unix)]
#[tokio::test]
async fn get_api_key_and_headers_resolves_auth_header_on_every_request() {
    let fx = Fixture::new();
    let token = fx.file("token");
    std::fs::write(&token, "token-1").unwrap();
    let mut provider = provider_with_api_key(&format!("!sh -c 'cat \"{token}\"'"));
    provider["authHeader"] = json!(true);
    fx.write_models(json!({ "custom-provider": provider }));
    let registry = fx.registry().await;
    let model = registry.find("custom-provider", "test-model").unwrap();
    let auth = registry.get_api_key_and_headers(&model).await;
    assert_eq!(
        serde_json::to_value(&auth).unwrap(),
        json!({ "ok": true, "apiKey": "token-1", "headers": { "Authorization": "Bearer token-1" } })
    );
    std::fs::write(&token, "token-2").unwrap();
    let auth = registry.get_api_key_and_headers(&model).await;
    assert_eq!(
        serde_json::to_value(&auth).unwrap(),
        json!({ "ok": true, "apiKey": "token-2", "headers": { "Authorization": "Bearer token-2" } })
    );
}

#[cfg(unix)]
#[tokio::test]
async fn stored_credentials_bypass_configured_auth_commands() {
    let fx = Fixture::new();
    let counter = fx.file("fallback-counter");
    std::fs::write(&counter, "0").unwrap();
    fx.write_models(json!({
        "custom-provider": provider_with_api_key(&format!("!sh -c 'echo 1 > \"{counter}\"; echo fallback-key'"))
    }));
    fx.auth
        .modify(
            "custom-provider",
            Box::new(|_| async { Ok(Some(Credential::ApiKey(ApiKeyCredential { key: Some("stored-key".into()), env: None }))) }.boxed()),
            None,
        )
        .await
        .unwrap();
    let registry = fx.registry().await;
    let auth = ok_auth(registry.get_api_key_and_headers(&registry.find("custom-provider", "test-model").unwrap()).await);
    assert_eq!(auth.api_key.as_deref(), Some("stored-key"));
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "0");
}

#[tokio::test]
async fn get_api_key_and_headers_auth_header_errors() {
    let fx = Fixture::new();
    fx.write_models(json!({
        "custom-provider": { "baseUrl": "https://example.test/v1", "api": "openai-completions", "authHeader": true, "models": [{ "id": "test-model" }] }
    }));
    let registry = fx.registry().await;
    let auth = registry.get_api_key_and_headers(&registry.find("custom-provider", "test-model").unwrap()).await;
    assert_eq!(auth, ResolvedRequestAuth::Err { error: "No API key found for \"custom-provider\"".into() });
}

#[cfg(unix)]
#[tokio::test]
async fn get_api_key_and_headers_reports_failed_auth_header_resolution() {
    let fx = Fixture::new();
    let mut provider = provider_with_api_key("!exit 1");
    provider["authHeader"] = json!(true);
    fx.write_models(json!({ "custom-provider": provider }));
    let registry = fx.registry().await;
    match registry.get_api_key_and_headers(&registry.find("custom-provider", "test-model").unwrap()).await {
        ResolvedRequestAuth::Err { error } => {
            assert!(error.contains("Failed to resolve API key for provider \"custom-provider\""), "{error}")
        }
        other => panic!("expected an error, got {other:?}"),
    }
}

#[tokio::test]
async fn stored_api_key_env_propagates_to_request_auth_and_resolves_headers() {
    let fx = Fixture::new();
    fx.auth
        .modify(
            "cloudflare-ai-gateway",
            Box::new(|_| {
                async {
                    Ok(Some(serde_json::from_value::<Credential>(json!({
                        "type": "api_key",
                        "key": "$CLOUDFLARE_API_KEY",
                        "env": {
                            "CLOUDFLARE_API_KEY": "stored-cf-token",
                            "CLOUDFLARE_ACCOUNT_ID": "stored-account",
                            "CLOUDFLARE_GATEWAY_ID": "stored-gateway",
                        },
                    }))?))
                }
                .boxed()
            }),
            None,
        )
        .await
        .unwrap();
    fx.write_models(json!({ "cloudflare-ai-gateway": { "headers": { "x-account": "$CLOUDFLARE_ACCOUNT_ID" } } }));
    let registry = fx.registry().await;
    let model = registry.get_all().into_iter().find(|m| m.provider == "cloudflare-ai-gateway").unwrap();
    let auth = ok_auth(registry.get_api_key_and_headers(&model).await);
    assert_eq!(auth.api_key, None);
    assert_eq!(header(&auth, "cf-aig-authorization").as_deref(), Some("Bearer stored-cf-token"));
    assert_eq!(header(&auth, "x-account").as_deref(), Some("stored-account"));
    let env = auth.env.unwrap();
    assert_eq!(env.get("CLOUDFLARE_ACCOUNT_ID").map(String::as_str), Some("stored-account"));
    assert_eq!(env.get("CLOUDFLARE_GATEWAY_ID").map(String::as_str), Some("stored-gateway"));
}
