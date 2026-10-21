//! Port of catalog parts of `test/providers.test.ts`.

use pi_ai::providers::all::*;
use pi_ai::*;
use serde_json::json;

#[test]
fn builtin_models_registers_every_provider() {
    let models = builtin_models(CreateModelsOptions::default());
    let providers = models.get_providers();
    assert_eq!(providers.len(), builtin_providers().len());
    assert!(providers.iter().any(|p| p.id() == "anthropic"));
    let haiku = models.get_model("anthropic", "claude-haiku-4-5").unwrap();
    assert_eq!(haiku.api, "anthropic-messages");
    assert!(models.get_models(None).len() > 500);
    for provider in &providers {
        let list = models.get_all_models(Some(provider.id()));
        assert!(!list.is_empty(), "{} has models", provider.id());
        assert!(list.iter().all(|m| m.provider() == provider.id()));
    }
    let balanced = get_builtin_model("radius", "balanced").unwrap();
    assert_eq!(balanced.api, "pi-messages");
    assert_eq!(balanced.provider, "radius");
}

#[test]
fn unknown_provider_ids_return_empty() {
    assert!(get_builtin_model("not-a-provider", "x").is_none());
    assert!(get_builtin_image_model("not-a-provider", "x").is_none());
    assert!(get_builtin_classifier_model("not-a-provider", "x").is_none());
    assert!(get_builtin_models("not-a-provider").is_empty());
    assert!(get_all_builtin_models("not-a-provider").is_empty());
}

#[test]
fn native_constrained_sampling_metadata() {
    let gpt4o = get_builtin_model("openai", "gpt-4o").unwrap();
    assert_eq!(gpt4o.compat.as_ref().unwrap().supports_strict_mode, Some(true));
    assert_eq!(gpt4o.compat.as_ref().unwrap().supports_openai_grammar_tools, None);
    let gpt54 = get_builtin_model("openai", "gpt-5.4").unwrap().compat.unwrap();
    assert_eq!(gpt54.supports_strict_mode, Some(true));
    assert_eq!(gpt54.supports_openai_grammar_tools, Some(true));
    assert_eq!(get_builtin_model("anthropic", "claude-haiku-4-5").unwrap().compat.unwrap().supports_strict_tools, Some(true));
}

#[test]
fn vision_models_keep_resize_profile_and_limits() {
    let resize = ModelImageResizeOptions {
        max_width: Some(2000),
        max_height: Some(2000),
        max_bytes: Some((4.5 * 1024.0 * 1024.0) as u64),
        jpeg_quality: Some(80),
    };
    let vision: Vec<Model> = get_builtin_providers()
        .iter()
        .flat_map(|p| get_builtin_models(p))
        .filter(|m| m.input.contains(&ModelInputModality::Image))
        .collect();
    assert!(!vision.is_empty());
    for model in &vision {
        assert_eq!(model.input_limits.as_ref().and_then(|l| l.images.as_ref()).and_then(|i| i.resize.as_ref()), Some(&resize), "{}", model.id);
    }
    let haiku = get_builtin_model("anthropic", "claude-haiku-4-5").unwrap().input_limits.unwrap();
    assert_eq!(haiku.max_request_bytes, Some(32 * 1024 * 1024));
    assert_eq!(haiku.images.unwrap().max_per_request, Some(100));
}

#[test]
fn model_json_roundtrips() {
    let model = get_builtin_model("anthropic", "claude-haiku-4-5").unwrap();
    let value = serde_json::to_value(&model).unwrap();
    assert_eq!(value["type"], json!("chat"));
    let back: Model = serde_json::from_value(value).unwrap();
    assert_eq!(back, model);
}

#[test]
fn thinking_levels() {
    let model = get_builtin_model("anthropic", "claude-fable-5").unwrap();
    let levels = get_supported_thinking_levels(&model);
    assert!(!levels.contains(&ModelThinkingLevel::Off));
    assert!(levels.contains(&ModelThinkingLevel::Max));
    assert_eq!(clamp_thinking_level(&model, ModelThinkingLevel::Off), ModelThinkingLevel::Minimal);
    let mut plain = model.clone();
    plain.reasoning = false;
    assert_eq!(get_supported_thinking_levels(&plain), vec![ModelThinkingLevel::Off]);
    assert_eq!(clamp_thinking_level(&plain, ModelThinkingLevel::High), ModelThinkingLevel::Off);
}

#[test]
fn calculate_cost_with_tiers_and_1h_writes() {
    let cost = ModelCost {
        input: 3.0,
        output: 15.0,
        cache_read: 0.3,
        cache_write: 3.75,
        tiers: Some(vec![ModelCostTier { input_tokens_above: 200000.0, input: 6.0, output: 22.5, cache_read: 0.6, cache_write: 7.5 }]),
    };
    let mut usage = Usage { input: 1000, output: 1000, cache_write: 1000, cache_write_1h: Some(400), ..Default::default() };
    let c = calculate_cost(&cost, &mut usage);
    assert!((c.input - 0.003).abs() < 1e-12);
    assert!((c.cache_write - (3.75 * 600.0 + 3.0 * 2.0 * 400.0) / 1e6).abs() < 1e-12);
    let mut big = Usage { input: 300000, ..Default::default() };
    assert!((calculate_cost(&cost, &mut big).input - 1.8).abs() < 1e-9);
}
