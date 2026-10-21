//! Port of `packages/coding-agent/src/core/auth-guidance.ts`.

use std::path::Path;

use crate::config::get_docs_path;

const UNKNOWN_PROVIDER: &str = "unknown";

fn docs_file(name: &str) -> String {
    Path::new(&get_docs_path()).join(name).to_string_lossy().into_owned()
}

pub fn get_provider_login_help() -> String {
    [
        "Use /login to log into a provider via OAuth or API key. See:".to_string(),
        format!("  {}", docs_file("providers.md")),
        format!("  {}", docs_file("models.md")),
    ]
    .join("\n")
}

pub fn format_no_models_available_message() -> String {
    format!("No models available. {}", get_provider_login_help())
}

pub fn format_no_model_selected_message() -> String {
    format!(
        "No model selected.\n\n{}\n\nThen use /model to select a model.",
        get_provider_login_help()
    )
}

pub fn format_no_api_key_found_message(provider: &str) -> String {
    let provider_display = if provider == UNKNOWN_PROVIDER {
        "the selected model"
    } else {
        provider
    };
    format!(
        "No API key found for {provider_display}.\n\n{}",
        get_provider_login_help()
    )
}
