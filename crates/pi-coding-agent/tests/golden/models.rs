//! Builders for `models.json` (scripts/golden/gen-models.mjs): model, scoped-models and config
//! selectors.

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use indexmap::IndexMap;
use pi_ai::providers::faux::FauxModelDefinition;
use pi_coding_agent::core::package_manager::ResolvedPaths;
use pi_coding_agent::core::settings_manager::{
    InMemorySettingsStorage, SettingsManager, SettingsManagerCreateOptions, SettingsScope, SettingsStorage,
};
use pi_coding_agent::modes::interactive::components::config_selector::{ConfigSelectorComponent, ConfigWriteScope};
use pi_coding_agent::modes::interactive::components::model_selector::{
    DefaultModelReference, ModelSelectorComponent, ScopedModelItem,
};
use pi_coding_agent::modes::interactive::components::scoped_models_selector::{
    ModelsCallbacks, ModelsConfig, ScopedModelsSelectorComponent,
};
use pi_tui::tui::Component;
use serde_json::Value;

use super::harness::*;
use super::selectors::feed;
use super::support::{HarnessOptions, create_harness};

fn build_model_selector(input: &Value) -> Built {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let models: Vec<FauxModelDefinition> = input["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| FauxModelDefinition {
            id: s(m, "id").into(),
            name: opt_s(m, "name").map(str::to_string),
            reasoning: m.get("reasoning").and_then(Value::as_bool),
            input: opt_from(m, "input"),
            context_window: m.get("contextWindow").and_then(Value::as_u64),
            ..Default::default()
        })
        .collect();
    let (selector, harness) = runtime.block_on(async {
        let harness = create_harness(HarnessOptions { models, ..Default::default() }).await.unwrap();
        let current = opt_s(input, "current").and_then(|id| harness.get_model_by_id(id));
        let scoped: Vec<ScopedModelItem> = input
            .get("scoped")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|id| ScopedModelItem {
                model: harness.get_model_by_id(id.as_str().unwrap()).unwrap(),
                thinking_level: opt_s(input, "scopedThinking").map(str::to_string),
            })
            .collect();
        let default_model = opt_s(input, "defaultModel").map(|id| DefaultModelReference {
            provider: current.as_ref().map(|m| m.provider.clone()).unwrap_or_else(|| "faux".into()),
            id: id.into(),
        });
        let selector = ModelSelectorComponent::new(
            test_tui(),
            current,
            harness.model_runtime.clone(),
            &scoped,
            Arc::new(|_| {}),
            Arc::new(|| {}),
            opt_s(input, "search"),
            b(input, "withDefault").then(|| Arc::new(|_| {}) as Arc<dyn Fn(_) + Send + Sync>),
            default_model.as_ref(),
        );
        if b(input, "focused") {
            selector.set_focused(true);
        }
        // The background catalog refresh finishes within the TS generator's settle time; the debug
        // build may need longer, so wait for it to finish instead.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while selector.render(120).join("\n").contains("Refreshing model catalogs") && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        feed(&selector, input);
        (selector, harness)
    });
    Built::Render(Box::new(move |w| {
        // The runtime and harness stay alive while the case renders.
        let _keep = (&runtime, &harness);
        selector.render(w)
    }))
}

fn build_scoped(input: &Value) -> Built {
    let config = ModelsConfig {
        all_models: from(input, "allModels"),
        enabled_model_ids: opt_from(input, "enabled"),
        refresh_status: opt_s(input, "refreshStatus").map(str::to_string),
    };
    let c = ScopedModelsSelectorComponent::new(
        &config,
        &ModelsCallbacks {
            on_change: Arc::new(|_| async { Ok(()) }.boxed()),
            on_persist: Arc::new(|_| async { Ok(()) }.boxed()),
            on_cancel: Arc::new(|| {}),
        },
    );
    if b(input, "focused") {
        c.set_focused(true);
    }
    feed(&c, input);
    component(c)
}

fn build_config(input: &Value) -> Built {
    let storage = Arc::new(InMemorySettingsStorage::new());
    let global = input.get("globalSettings").cloned().unwrap_or_else(|| serde_json::json!({})).to_string();
    storage.with_lock(SettingsScope::Global, &mut |_| Ok(Some(global.clone()))).unwrap();
    let settings_manager = SettingsManager::from_storage(
        storage as Arc<dyn SettingsStorage>,
        Some(&SettingsManagerCreateOptions { project_trusted: Some(true) }),
    );
    let mut paths: IndexMap<ConfigWriteScope, ResolvedPaths> = IndexMap::new();
    paths.insert(ConfigWriteScope::Global, from(input, "global"));
    paths.insert(ConfigWriteScope::Project, from(input, "project"));
    let scope: ConfigWriteScope = from(input, "scope");
    let c = ConfigSelectorComponent::new(
        &paths,
        settings_manager,
        "/home/golden/project",
        "/home/golden/.pi/agent",
        Arc::new(|| {}),
        Arc::new(|| {}),
        Arc::new(|| {}),
        Some(opt_i(input, "height").unwrap_or(24)),
        Some(&scope),
        Some(input.get("projectMode").and_then(Value::as_bool).unwrap_or(true)),
    );
    if b(input, "focused") {
        c.set_focused(true);
    }
    feed(c.get_resource_list().as_ref(), input);
    component(c)
}

pub fn build(kind: &str, input: &Value) -> Option<Built> {
    Some(match kind {
        "model_selector" => build_model_selector(input),
        "scoped_models_selector" => build_scoped(input),
        "config_selector" => build_config(input),
        _ => return None,
    })
}
