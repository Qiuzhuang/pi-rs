//! Built-in providers and catalog reads (port of `providers/all.ts`).

use std::sync::Arc;

use super::amazon_bedrock::amazon_bedrock_provider;
use super::anthropic::anthropic_provider;
use super::azure::azure_provider;
use super::builtin::*;
use super::cloudflare::{cloudflare_ai_gateway_provider, cloudflare_workers_ai_provider};
use super::google_vertex::google_vertex_provider;
use super::opencode::{opencode_go_provider, opencode_provider};
use super::openrouter::openrouter_provider;
pub use super::radius::radius_provider;
use super::radius::RadiusProviderOptions;
use crate::models::{CreateModelsOptions, Models, Provider, create_models};
use crate::models_generated::{CATALOG, catalog_providers, manifest_generated_at};
use crate::types::{AnyModel, ClassifierModel, ImageModel, Model};

/// Typed read of one generated built-in chat model.
pub fn get_builtin_model(provider: &str, model_id: &str) -> Option<Model> {
    CATALOG.chat.get(provider)?.get(model_id).cloned()
}

/// Typed read of one generated built-in image model.
pub fn get_builtin_image_model(provider: &str, model_id: &str) -> Option<ImageModel> {
    CATALOG.image.get(provider)?.get(model_id).cloned()
}

/// Typed read of one generated built-in classifier model.
pub fn get_builtin_classifier_model(provider: &str, model_id: &str) -> Option<ClassifierModel> {
    CATALOG.classifier.get(provider)?.get(model_id).cloned()
}

/// Providers present in the generated catalog.
pub fn get_builtin_providers() -> Vec<String> {
    catalog_providers().into_iter().map(str::to_string).collect()
}

/// Generation timestamp (Unix ms) shared by all built-in provider catalogs.
pub fn get_builtin_model_data_generated_at() -> Option<i64> {
    let text = manifest_generated_at()?;
    parse_iso_timestamp(&text)
}

fn parse_iso_timestamp(text: &str) -> Option<i64> {
    // Format: 2026-10-15T19:00:55.287Z
    let (date, time) = text.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let (hms, millis) = match time.split_once('.') {
        Some((hms, frac)) => (hms, format!("{frac:0<3}")[..3].parse::<i64>().ok()?),
        None => (time, 0),
    };
    let mut t = hms.split(':').map(|p| p.parse::<i64>());
    let (h, mi, s) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 86400 + h * 3600 + mi * 60 + s) * 1000) + millis)
}

pub fn get_builtin_models(provider: &str) -> Vec<Model> {
    CATALOG.chat.get(provider).map(|m| m.values().cloned().collect()).unwrap_or_default()
}

pub fn get_builtin_image_models(provider: &str) -> Vec<ImageModel> {
    CATALOG.image.get(provider).map(|m| m.values().cloned().collect()).unwrap_or_default()
}

pub fn get_builtin_classifier_models(provider: &str) -> Vec<ClassifierModel> {
    CATALOG.classifier.get(provider).map(|m| m.values().cloned().collect()).unwrap_or_default()
}

pub fn get_all_builtin_models(provider: &str) -> Vec<AnyModel> {
    let mut all: Vec<AnyModel> = get_builtin_models(provider).into_iter().map(AnyModel::Chat).collect();
    all.extend(get_builtin_image_models(provider).into_iter().map(AnyModel::Image));
    all.extend(get_builtin_classifier_models(provider).into_iter().map(AnyModel::Classifier));
    all
}

/// All built-in providers, freshly constructed.
pub fn builtin_providers() -> Vec<Arc<dyn Provider>> {
    vec![
        amazon_bedrock_provider(),
        ant_ling_provider(),
        anthropic_provider(),
        azure_provider(),
        baseten_provider(),
        cerebras_provider(),
        cloudflare_ai_gateway_provider(),
        cloudflare_workers_ai_provider(),
        deepseek_provider(),
        fireworks_provider(),
        github_copilot_provider(),
        google_provider(),
        google_vertex_provider(),
        groq_provider(),
        huggingface_provider(),
        kimi_coding_provider(),
        meta_provider(),
        minimax_provider(),
        minimax_cn_provider(),
        mistral_provider(),
        moonshotai_provider(),
        moonshotai_cn_provider(),
        nvidia_provider(),
        openai_provider(),
        openai_codex_provider(),
        opencode_provider(),
        opencode_go_provider(),
        openrouter_provider(),
        qwen_token_plan_provider(),
        qwen_token_plan_cn_provider(),
        qwen_token_plan_individual_provider(),
        radius_provider(RadiusProviderOptions::default()),
        together_provider(),
        typesafe_provider(),
        vercel_ai_gateway_provider(),
        xai_provider(),
        xiaomi_provider(),
        xiaomi_token_plan_ams_provider(),
        xiaomi_token_plan_cn_provider(),
        xiaomi_token_plan_sgp_provider(),
        zai_provider(),
        zai_coding_cn_provider(),
    ]
}

/// A `Models` collection with every built-in provider registered.
pub fn builtin_models(options: CreateModelsOptions) -> Models {
    let models = create_models(options);
    for provider in builtin_providers() {
        models.set_provider(provider);
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_timestamp() {
        assert_eq!(parse_iso_timestamp("1970-01-01T00:00:01.5Z"), Some(1500));
        assert!(get_builtin_model_data_generated_at().is_some());
    }
}
