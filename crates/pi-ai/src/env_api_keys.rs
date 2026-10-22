//! Environment API key discovery (port of `env-api-keys.ts`).

use std::path::Path;

use crate::types::ProviderEnv;
use crate::utils::provider_env::get_provider_env_value;

pub const ANTHROPIC_AUTH_TOKEN_ENV: &str = "ANTHROPIC_AUTH_TOKEN";
pub const ANTHROPIC_OAUTH_TOKEN_ENV: &str = "ANTHROPIC_OAUTH_TOKEN";
pub const ANTHROPIC_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
pub const ANTHROPIC_FEDERATION_RULE_ID_ENV: &str = "ANTHROPIC_FEDERATION_RULE_ID";
pub const ANTHROPIC_ORGANIZATION_ID_ENV: &str = "ANTHROPIC_ORGANIZATION_ID";
pub const ANTHROPIC_SERVICE_ACCOUNT_ID_ENV: &str = "ANTHROPIC_SERVICE_ACCOUNT_ID";
pub const ANTHROPIC_IDENTITY_TOKEN_FILE_ENV: &str = "ANTHROPIC_IDENTITY_TOKEN_FILE";
pub const ANTHROPIC_WORKSPACE_ID_ENV: &str = "ANTHROPIC_WORKSPACE_ID";

/// Marker returned for ambient (non-key) credentials.
pub const AUTHENTICATED_MARKER: &str = "<authenticated>";

fn has_vertex_adc_credentials(env: Option<&ProviderEnv>) -> bool {
    if let Some(explicit) = env.and_then(|e| e.get("GOOGLE_APPLICATION_CREDENTIALS")).filter(|v| !v.is_empty()) {
        return Path::new(explicit).exists();
    }
    // The first answer without an explicit path is cached for the process, like TS
    // (`cachedVertexAdcCredentialsExists`).
    static CACHED: once_cell::sync::OnceCell<bool> = once_cell::sync::OnceCell::new();
    *CACHED.get_or_init(|| {
        if let Some(path) = get_provider_env_value("GOOGLE_APPLICATION_CREDENTIALS", env) {
            return Path::new(&path).exists();
        }
        dirs::home_dir()
            .map(|home| home.join(".config").join("gcloud").join("application_default_credentials.json").exists())
            .unwrap_or(false)
    })
}

fn api_key_env_vars(provider: &str) -> Option<Vec<&'static str>> {
    if provider == "github-copilot" {
        return Some(vec!["COPILOT_GITHUB_TOKEN"]);
    }
    if provider == "anthropic" {
        return Some(vec![ANTHROPIC_AUTH_TOKEN_ENV, ANTHROPIC_OAUTH_TOKEN_ENV, ANTHROPIC_API_KEY_ENV]);
    }
    let var = match provider {
        "ant-ling" => "ANT_LING_API_KEY",
        "qwen-token-plan" => "QWEN_TOKEN_PLAN_API_KEY",
        "qwen-token-plan-cn" => "QWEN_TOKEN_PLAN_CN_API_KEY",
        "qwen-token-plan-individual" => "QWEN_TOKEN_PLAN_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "azure" => "AZURE_OPENAI_API_KEY",
        "nvidia" => "NVIDIA_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "google" => "GEMINI_API_KEY",
        "google-vertex" => "GOOGLE_CLOUD_API_KEY",
        "groq" => "GROQ_API_KEY",
        "cerebras" => "CEREBRAS_API_KEY",
        "xai" => "XAI_API_KEY",
        "typesafe" => "TYPESAFE_API_KEY",
        "radius" => "RADIUS_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "vercel-ai-gateway" => "AI_GATEWAY_API_KEY",
        "zai" => "ZAI_API_KEY",
        "zai-coding-cn" => "ZAI_CODING_CN_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "minimax-cn" => "MINIMAX_CN_API_KEY",
        "moonshotai" => "MOONSHOT_API_KEY",
        "moonshotai-cn" => "MOONSHOT_API_KEY",
        "huggingface" => "HF_TOKEN",
        "fireworks" => "FIREWORKS_API_KEY",
        "together" => "TOGETHER_API_KEY",
        "baseten" => "BASETEN_API_KEY",
        "opencode" => "OPENCODE_API_KEY",
        "opencode-go" => "OPENCODE_API_KEY",
        "kimi-coding" => "KIMI_API_KEY",
        "meta" => "META_API_KEY",
        "cloudflare-workers-ai" => "CLOUDFLARE_API_KEY",
        "cloudflare-ai-gateway" => "CLOUDFLARE_API_KEY",
        "xiaomi" => "XIAOMI_API_KEY",
        "xiaomi-token-plan-cn" => "XIAOMI_TOKEN_PLAN_CN_API_KEY",
        "xiaomi-token-plan-ams" => "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
        "xiaomi-token-plan-sgp" => "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
        _ => return None,
    };
    Some(vec![var])
}

/// Configured environment variables that can provide an API key for a provider.
pub fn find_env_keys(provider: &str, env: Option<&ProviderEnv>) -> Option<Vec<String>> {
    let vars = api_key_env_vars(provider)?;
    let found: Vec<String> =
        vars.into_iter().filter(|v| get_provider_env_value(v, env).is_some()).map(str::to_string).collect();
    if found.is_empty() { None } else { Some(found) }
}

/// API key for a provider from known environment variables. Ambient credentials yield
/// [`AUTHENTICATED_MARKER`].
pub fn get_env_api_key(provider: &str, env: Option<&ProviderEnv>) -> Option<String> {
    if let Some(keys) = find_env_keys(provider, env) {
        let key = if provider == "anthropic" {
            keys.iter().find(|k| k.as_str() != ANTHROPIC_AUTH_TOKEN_ENV).cloned()
        } else {
            keys.first().cloned()
        };
        if let Some(key) = key {
            return get_provider_env_value(&key, env);
        }
    }
    let has = |name: &str| get_provider_env_value(name, env).is_some();
    if provider == "google-vertex" {
        let has_credentials = has_vertex_adc_credentials(env);
        let has_project = has("GOOGLE_CLOUD_PROJECT") || has("GCLOUD_PROJECT");
        let has_location = has("GOOGLE_CLOUD_LOCATION");
        if has_credentials && has_project && has_location {
            return Some(AUTHENTICATED_MARKER.to_string());
        }
    }
    if provider == "amazon-bedrock"
        && (has("AWS_PROFILE")
            || (has("AWS_ACCESS_KEY_ID") && has("AWS_SECRET_ACCESS_KEY"))
            || has("AWS_BEARER_TOKEN_BEDROCK")
            || has("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI")
            || has("AWS_CONTAINER_CREDENTIALS_FULL_URI")
            || has("AWS_WEB_IDENTITY_TOKEN_FILE"))
    {
        return Some(AUTHENTICATED_MARKER.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_env_wins_and_anthropic_skips_auth_token() {
        let mut env = ProviderEnv::new();
        env.insert("OPENAI_API_KEY".into(), "sk-scoped".into());
        assert_eq!(get_env_api_key("openai", Some(&env)).as_deref(), Some("sk-scoped"));
        let mut env = ProviderEnv::new();
        env.insert(ANTHROPIC_AUTH_TOKEN_ENV.into(), "tok".into());
        env.insert(ANTHROPIC_API_KEY_ENV.into(), "key".into());
        assert_eq!(get_env_api_key("anthropic", Some(&env)).as_deref(), Some("key"));
        let mut env = ProviderEnv::new();
        env.insert("AWS_BEARER_TOKEN_BEDROCK".into(), "x".into());
        assert_eq!(get_env_api_key("amazon-bedrock", Some(&env)).as_deref(), Some(AUTHENTICATED_MARKER));
        assert_eq!(get_env_api_key("unknown-provider-xyz", None), None);
    }
}
