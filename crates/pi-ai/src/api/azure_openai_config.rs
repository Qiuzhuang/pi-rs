//! Azure OpenAI endpoint configuration (port of `api/azure-openai-config.ts`).
//!
//! Options read from `StreamOptions.extra`: `azureApiVersion`, `azureResourceName`,
//! `azureBaseUrl`, `azureDeploymentName`.

use std::collections::HashMap;

use url::Url;

use crate::types::{Model, StreamOptions};
use crate::utils::provider_env::get_provider_env_value;

const DEFAULT_AZURE_API_VERSION: &str = "v1";

fn parse_deployment_name_map(value: Option<&str>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(value) = value else { return map };
    for entry in value.split(',') {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        // JS `split("=", 2)`: the first two pieces; anything after a second `=` is dropped.
        let mut parts = trimmed.split('=');
        let (Some(model_id), Some(deployment)) = (parts.next(), parts.next()) else { continue };
        if model_id.is_empty() || deployment.is_empty() {
            continue;
        }
        map.insert(model_id.trim().to_string(), deployment.trim().to_string());
    }
    map
}

/// The deployment name to send as the request model.
pub fn resolve_deployment_name(model: &Model, options: Option<&StreamOptions>) -> String {
    if let Some(name) = options.and_then(|o| o.extra_str("azureDeploymentName")).filter(|n| !n.is_empty()) {
        return name.to_string();
    }
    let map_value = get_provider_env_value("AZURE_OPENAI_DEPLOYMENT_NAME_MAP", options.and_then(|o| o.env.as_ref()));
    parse_deployment_name_map(map_value.as_deref())
        .get(&model.id)
        .filter(|d| !d.is_empty())
        .cloned()
        .unwrap_or_else(|| model.id.clone())
}

fn normalize_azure_base_url(base_url: &str) -> anyhow::Result<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let mut url = Url::parse(trimmed).map_err(|_| anyhow::anyhow!("Invalid Azure OpenAI base URL: {base_url}"))?;
    let host = url.host_str().unwrap_or_default().to_string();
    let is_azure_host = host.ends_with(".openai.azure.com")
        || host.ends_with(".cognitiveservices.azure.com")
        || host.ends_with(".ai.azure.com");
    let normalized_path = url.path().trim_end_matches('/').to_string();
    if is_azure_host && matches!(normalized_path.as_str(), "" | "/" | "/openai" | "/openai/v1/responses") {
        url.set_path("/openai/v1");
        url.set_query(None);
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}

/// Resolve the Azure base URL from options, env, or the model.
pub fn resolve_azure_base_url(model: &Model, options: Option<&StreamOptions>) -> anyhow::Result<String> {
    let env = options.and_then(|o| o.env.as_ref());
    let base_url = options
        .and_then(|o| o.extra_str("azureBaseUrl"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| get_provider_env_value("AZURE_OPENAI_BASE_URL", env).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    let resource_name = options
        .and_then(|o| o.extra_str("azureResourceName"))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| get_provider_env_value("AZURE_OPENAI_RESOURCE_NAME", env));
    let resolved = base_url
        .or_else(|| resource_name.map(|r| format!("https://{r}.openai.azure.com/openai/v1")))
        .or_else(|| Some(model.base_url.clone()).filter(|b| !b.is_empty()));
    let Some(resolved) = resolved else {
        anyhow::bail!(
            "Azure OpenAI base URL is required. Set AZURE_OPENAI_BASE_URL or AZURE_OPENAI_RESOURCE_NAME, or pass azureBaseUrl, azureResourceName, or model.baseUrl."
        );
    };
    normalize_azure_base_url(&resolved)
}

/// Base URL and API version.
pub fn resolve_azure_config(model: &Model, options: Option<&StreamOptions>) -> anyhow::Result<(String, String)> {
    let api_version = options
        .and_then(|o| o.extra_str("azureApiVersion"))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| get_provider_env_value("AZURE_OPENAI_API_VERSION", options.and_then(|o| o.env.as_ref())))
        .unwrap_or_else(|| DEFAULT_AZURE_API_VERSION.to_string());
    Ok((resolve_azure_base_url(model, options)?, api_version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_azure_hosts() {
        assert_eq!(normalize_azure_base_url("https://x.openai.azure.com").unwrap(), "https://x.openai.azure.com/openai/v1");
        assert_eq!(
            normalize_azure_base_url("https://x.cognitiveservices.azure.com/openai/").unwrap(),
            "https://x.cognitiveservices.azure.com/openai/v1"
        );
        assert_eq!(normalize_azure_base_url("https://proxy.example.com/v1/").unwrap(), "https://proxy.example.com/v1");
    }

    #[test]
    fn deployment_map() {
        let map = parse_deployment_name_map(Some("gpt-4o=prod-4o, bad, x="));
        assert_eq!(map.get("gpt-4o").map(String::as_str), Some("prod-4o"));
        assert_eq!(map.len(), 1);
        // JS `split("=", 2)` drops the text after a second `=`.
        let map = parse_deployment_name_map(Some("a=b=c"));
        assert_eq!(map.get("a").map(String::as_str), Some("b"));
    }
}
