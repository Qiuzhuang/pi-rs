//! Built-in provider definitions with standard env-var API key auth
//! (port of the simple `providers/<id>.ts` factories).

use std::sync::Arc;

use indexmap::IndexMap;

use crate::api::registry::{
    anthropic_messages_api, builtin_classifier_api, google_generative_ai_api, mistral_conversations_api,
    openai_codex_responses_api, openai_completions_api, openai_responses_api,
};
use crate::auth::helpers::env_api_key_auth;
use crate::auth::oauth_load::{OAuthFlowArgs, lazy_builtin_oauth};
use crate::auth::types::{Credential, ProviderAuth};
use crate::models::{CreateProviderOptions, Provider, ProviderApi, create_provider};
use crate::models_generated::CATALOG;
use crate::types::{AnyModel, Model, ModelType};

/// Static chat models of a built-in provider.
pub fn catalog_chat_models(provider: &str) -> Vec<AnyModel> {
    CATALOG.chat.get(provider).map(|m| m.values().cloned().map(AnyModel::Chat).collect()).unwrap_or_default()
}

/// Static image models of a built-in provider.
pub fn catalog_image_models(provider: &str) -> Vec<AnyModel> {
    CATALOG.image.get(provider).map(|m| m.values().cloned().map(AnyModel::Image).collect()).unwrap_or_default()
}

/// Static classifier models of a built-in provider.
pub fn catalog_classifier_models(provider: &str) -> Vec<AnyModel> {
    CATALOG
        .classifier
        .get(provider)
        .map(|m| m.values().cloned().map(AnyModel::Classifier).collect())
        .unwrap_or_default()
}

fn build(options: CreateProviderOptions) -> Arc<dyn Provider> {
    create_provider(options).expect("built-in provider definition is valid")
}

fn simple(
    id: &str,
    name: &str,
    base_url: &str,
    key_name: &str,
    env_vars: &[&str],
    api: Arc<dyn crate::models::ProviderStreams>,
) -> Arc<dyn Provider> {
    build(CreateProviderOptions {
        id: id.to_string(),
        name: Some(name.to_string()),
        base_url: Some(base_url.to_string()),
        auth: ProviderAuth { api_key: Some(env_api_key_auth(key_name, env_vars)), oauth: None },
        models: catalog_chat_models(id),
        api: Some(ProviderApi::Single(api)),
        ..Default::default()
    })
}

pub fn ant_ling_provider() -> Arc<dyn Provider> {
    simple("ant-ling", "Ant Ling", "https://api.ant-ling.com/v1", "Ant Ling API key", &["ANT_LING_API_KEY"], openai_completions_api())
}

pub fn baseten_provider() -> Arc<dyn Provider> {
    simple("baseten", "Baseten", "https://inference.baseten.co/v1", "Baseten API key", &["BASETEN_API_KEY"], openai_completions_api())
}

pub fn cerebras_provider() -> Arc<dyn Provider> {
    simple("cerebras", "Cerebras", "https://api.cerebras.ai/v1", "Cerebras API key", &["CEREBRAS_API_KEY"], openai_completions_api())
}

pub fn deepseek_provider() -> Arc<dyn Provider> {
    simple("deepseek", "DeepSeek", "https://api.deepseek.com", "DeepSeek API key", &["DEEPSEEK_API_KEY"], openai_completions_api())
}

pub fn google_provider() -> Arc<dyn Provider> {
    simple(
        "google",
        "Google",
        "https://generativelanguage.googleapis.com/v1beta",
        "Gemini API key",
        &["GEMINI_API_KEY"],
        google_generative_ai_api(),
    )
}

pub fn groq_provider() -> Arc<dyn Provider> {
    simple("groq", "Groq", "https://api.groq.com/openai/v1", "Groq API key", &["GROQ_API_KEY"], openai_completions_api())
}

pub fn huggingface_provider() -> Arc<dyn Provider> {
    simple("huggingface", "Hugging Face", "https://router.huggingface.co/v1", "Hugging Face token", &["HF_TOKEN"], openai_completions_api())
}

pub fn minimax_provider() -> Arc<dyn Provider> {
    simple("minimax", "MiniMax", "https://api.minimax.io/anthropic", "MiniMax API key", &["MINIMAX_API_KEY"], anthropic_messages_api())
}

pub fn minimax_cn_provider() -> Arc<dyn Provider> {
    simple(
        "minimax-cn",
        "MiniMax CN",
        "https://api.minimaxi.com/anthropic",
        "MiniMax CN API key",
        &["MINIMAX_CN_API_KEY"],
        anthropic_messages_api(),
    )
}

pub fn mistral_provider() -> Arc<dyn Provider> {
    simple("mistral", "Mistral", "https://api.mistral.ai", "Mistral API key", &["MISTRAL_API_KEY"], mistral_conversations_api())
}

pub fn moonshotai_provider() -> Arc<dyn Provider> {
    simple("moonshotai", "Moonshot AI", "https://api.moonshot.ai/v1", "Moonshot AI API key", &["MOONSHOT_API_KEY"], openai_completions_api())
}

pub fn moonshotai_cn_provider() -> Arc<dyn Provider> {
    simple(
        "moonshotai-cn",
        "Moonshot AI CN",
        "https://api.moonshot.cn/v1",
        "Moonshot AI API key",
        &["MOONSHOT_API_KEY"],
        openai_completions_api(),
    )
}

pub fn nvidia_provider() -> Arc<dyn Provider> {
    simple("nvidia", "NVIDIA", "https://integrate.api.nvidia.com/v1", "NVIDIA API key", &["NVIDIA_API_KEY"], openai_completions_api())
}

pub fn qwen_token_plan_provider() -> Arc<dyn Provider> {
    simple(
        "qwen-token-plan",
        "Qwen Token Plan",
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
        "Qwen Token Plan API key",
        &["QWEN_TOKEN_PLAN_API_KEY"],
        openai_completions_api(),
    )
}

pub fn qwen_token_plan_cn_provider() -> Arc<dyn Provider> {
    simple(
        "qwen-token-plan-cn",
        "Qwen Token Plan CN",
        "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
        "Qwen Token Plan CN API key",
        &["QWEN_TOKEN_PLAN_CN_API_KEY"],
        openai_completions_api(),
    )
}

pub fn qwen_token_plan_individual_provider() -> Arc<dyn Provider> {
    simple(
        "qwen-token-plan-individual",
        "Qwen Token Plan Individual",
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
        "Qwen Token Plan Individual API key",
        &["QWEN_TOKEN_PLAN_API_KEY"],
        openai_completions_api(),
    )
}

pub fn together_provider() -> Arc<dyn Provider> {
    simple("together", "Together", "https://api.together.ai/v1", "Together API key", &["TOGETHER_API_KEY"], openai_completions_api())
}

pub fn xiaomi_provider() -> Arc<dyn Provider> {
    simple("xiaomi", "Xiaomi", "https://api.xiaomimimo.com/v1", "Xiaomi API key", &["XIAOMI_API_KEY"], openai_completions_api())
}

pub fn xiaomi_token_plan_ams_provider() -> Arc<dyn Provider> {
    simple(
        "xiaomi-token-plan-ams",
        "Xiaomi Token Plan AMS",
        "https://token-plan-ams.xiaomimimo.com/v1",
        "Xiaomi Token Plan AMS API key",
        &["XIAOMI_TOKEN_PLAN_AMS_API_KEY"],
        openai_completions_api(),
    )
}

pub fn xiaomi_token_plan_cn_provider() -> Arc<dyn Provider> {
    simple(
        "xiaomi-token-plan-cn",
        "Xiaomi Token Plan CN",
        "https://token-plan-cn.xiaomimimo.com/v1",
        "Xiaomi Token Plan CN API key",
        &["XIAOMI_TOKEN_PLAN_CN_API_KEY"],
        openai_completions_api(),
    )
}

pub fn xiaomi_token_plan_sgp_provider() -> Arc<dyn Provider> {
    simple(
        "xiaomi-token-plan-sgp",
        "Xiaomi Token Plan SGP",
        "https://token-plan-sgp.xiaomimimo.com/v1",
        "Xiaomi Token Plan SGP API key",
        &["XIAOMI_TOKEN_PLAN_SGP_API_KEY"],
        openai_completions_api(),
    )
}

pub fn zai_provider() -> Arc<dyn Provider> {
    simple("zai", "Z.AI", "https://api.z.ai/api/coding/paas/v4", "Z.AI API key", &["ZAI_API_KEY"], openai_completions_api())
}

pub fn zai_coding_cn_provider() -> Arc<dyn Provider> {
    simple(
        "zai-coding-cn",
        "Z.AI Coding CN",
        "https://open.bigmodel.cn/api/coding/paas/v4",
        "Z.AI Coding CN API key",
        &["ZAI_CODING_CN_API_KEY"],
        openai_completions_api(),
    )
}

pub fn typesafe_provider() -> Arc<dyn Provider> {
    let mut classifiers = IndexMap::new();
    classifiers.insert("typesafe-system-one".to_string(), builtin_classifier_api("typesafe-system-one"));
    build(CreateProviderOptions {
        id: "typesafe".into(),
        name: Some("TypeSafe".into()),
        auth: ProviderAuth { api_key: Some(env_api_key_auth("TypeSafe API key", &["TYPESAFE_API_KEY"])), oauth: None },
        models: catalog_classifier_models("typesafe"),
        classifiers,
        ..Default::default()
    })
}

pub fn vercel_ai_gateway_provider() -> Arc<dyn Provider> {
    let mut classifiers = IndexMap::new();
    classifiers.insert("typesafe-system-one".to_string(), builtin_classifier_api("typesafe-system-one"));
    let mut models = catalog_chat_models("vercel-ai-gateway");
    models.extend(catalog_classifier_models("vercel-ai-gateway"));
    build(CreateProviderOptions {
        id: "vercel-ai-gateway".into(),
        name: Some("Vercel AI Gateway".into()),
        base_url: Some("https://ai-gateway.vercel.sh".into()),
        auth: ProviderAuth { api_key: Some(env_api_key_auth("Vercel AI Gateway API key", &["AI_GATEWAY_API_KEY"])), oauth: None },
        models,
        api: Some(ProviderApi::Single(anthropic_messages_api())),
        classifiers,
        ..Default::default()
    })
}

pub fn fireworks_provider() -> Arc<dyn Provider> {
    let mut apis = IndexMap::new();
    apis.insert("anthropic-messages".to_string(), anthropic_messages_api());
    apis.insert("openai-completions".to_string(), openai_completions_api());
    build(CreateProviderOptions {
        id: "fireworks".into(),
        name: Some("Fireworks".into()),
        base_url: Some("https://api.fireworks.ai/inference".into()),
        auth: ProviderAuth { api_key: Some(env_api_key_auth("Fireworks API key", &["FIREWORKS_API_KEY"])), oauth: None },
        models: catalog_chat_models("fireworks"),
        api: Some(ProviderApi::ByApi(apis)),
        ..Default::default()
    })
}

pub fn openai_codex_provider() -> Arc<dyn Provider> {
    build(CreateProviderOptions {
        id: "openai-codex".into(),
        name: Some("OpenAI Codex (legacy)".into()),
        base_url: Some("https://chatgpt.com/backend-api".into()),
        auth: ProviderAuth {
            api_key: None,
            oauth: Some(lazy_builtin_oauth("OpenAI (ChatGPT Plus/Pro)", Some(true), None, "openaiCodex", OAuthFlowArgs::default())),
        },
        models: catalog_chat_models("openai-codex"),
        api: Some(ProviderApi::Single(openai_codex_responses_api())),
        ..Default::default()
    })
}

fn env_and_oauth(
    key_name: &str,
    env_vars: &[&str],
    oauth_name: &str,
    login_label: Option<&str>,
    flow: &str,
) -> ProviderAuth {
    ProviderAuth {
        api_key: Some(env_api_key_auth(key_name, env_vars)),
        oauth: Some(lazy_builtin_oauth(oauth_name, Some(true), login_label, flow, OAuthFlowArgs::default())),
    }
}

pub fn meta_provider() -> Arc<dyn Provider> {
    build(CreateProviderOptions {
        id: "meta".into(),
        name: Some("Meta".into()),
        base_url: Some("https://api.meta.ai/v1".into()),
        auth: env_and_oauth("Meta Model API key", &["META_API_KEY"], "Meta (Muse subscription)", Some("Sign in with Meta"), "meta"),
        models: catalog_chat_models("meta"),
        api: Some(ProviderApi::Single(openai_responses_api())),
        ..Default::default()
    })
}

pub fn xai_provider() -> Arc<dyn Provider> {
    build(CreateProviderOptions {
        id: "xai".into(),
        name: Some("xAI".into()),
        base_url: Some("https://api.x.ai/v1".into()),
        auth: env_and_oauth(
            "xAI API key",
            &["XAI_API_KEY"],
            "xAI (Grok/X subscription)",
            Some("Sign in with SuperGrok or X Premium"),
            "xai",
        ),
        models: catalog_chat_models("xai"),
        api: Some(ProviderApi::Single(openai_responses_api())),
        ..Default::default()
    })
}

pub fn kimi_coding_provider() -> Arc<dyn Provider> {
    build(CreateProviderOptions {
        id: "kimi-coding".into(),
        name: Some("Kimi For Coding".into()),
        base_url: Some("https://api.kimi.com/coding/v1".into()),
        auth: env_and_oauth("Kimi API key", &["KIMI_API_KEY"], "Kimi Code (subscription)", Some("Sign in with Kimi Code"), "kimiCoding"),
        models: catalog_chat_models("kimi-coding"),
        api: Some(ProviderApi::Single(openai_completions_api())),
        ..Default::default()
    })
}

pub fn openai_provider() -> Arc<dyn Provider> {
    let mut classifiers = IndexMap::new();
    classifiers.insert("openai-decisions".to_string(), builtin_classifier_api("openai-decisions"));
    let mut models = catalog_chat_models("openai");
    models.extend(catalog_classifier_models("openai"));
    build(CreateProviderOptions {
        id: "openai".into(),
        name: Some("OpenAI".into()),
        base_url: Some("https://api.openai.com/v1".into()),
        auth: env_and_oauth(
            "OpenAI API key",
            &["OPENAI_API_KEY"],
            "OpenAI (ChatGPT subscription)",
            Some("Sign in with ChatGPT"),
            "openaiChatGPT",
        ),
        models,
        // Sign in with ChatGPT tokens only reach the Responses API; the Decisions API rejects them.
        filter_all_models: Some(Arc::new(|models: &[AnyModel], credential: Option<&Credential>| {
            if matches!(credential, Some(Credential::OAuth(_))) {
                models.iter().filter(|m| m.model_type() != ModelType::Classifier).cloned().collect()
            } else {
                models.to_vec()
            }
        })),
        api: Some(ProviderApi::Single(openai_responses_api())),
        classifiers,
        ..Default::default()
    })
}

pub fn github_copilot_provider() -> Arc<dyn Provider> {
    let mut apis = IndexMap::new();
    apis.insert("anthropic-messages".to_string(), anthropic_messages_api());
    apis.insert("openai-completions".to_string(), openai_completions_api());
    apis.insert("openai-responses".to_string(), openai_responses_api());
    build(CreateProviderOptions {
        id: "github-copilot".into(),
        name: Some("GitHub Copilot".into()),
        base_url: Some("https://api.individual.githubcopilot.com".into()),
        auth: ProviderAuth {
            api_key: Some(env_api_key_auth("GitHub Copilot token", &["COPILOT_GITHUB_TOKEN"])),
            oauth: Some(lazy_builtin_oauth("GitHub Copilot", Some(true), None, "githubCopilot", OAuthFlowArgs::default())),
        },
        models: catalog_chat_models("github-copilot"),
        filter_models: Some(Arc::new(|models: &[Model], credential: Option<&Credential>| {
            let Some(Credential::OAuth(oauth)) = credential else { return models.to_vec() };
            let Some(serde_json::Value::Array(ids)) = oauth.extra.get("availableModelIds") else {
                return models.to_vec();
            };
            if !ids.iter().all(serde_json::Value::is_string) {
                return models.to_vec();
            }
            let available: std::collections::HashSet<&str> = ids.iter().filter_map(serde_json::Value::as_str).collect();
            models.iter().filter(|m| available.contains(m.id.as_str())).cloned().collect()
        })),
        api: Some(ProviderApi::ByApi(apis)),
        ..Default::default()
    })
}
