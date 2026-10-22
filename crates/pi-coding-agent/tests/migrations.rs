//! Ports of `test/keybindings-migration.test.ts`, `test/config-value-migration.test.ts` and
//! `test/suite/regressions/5661-uppercase-header-values.test.ts`.
//!
//! `runMigrations` reads the agent directory from `PI_CODING_AGENT_DIR`, so the tests hold a lock
//! while they set it. TS also asserts that migrations log nothing (`console.log` spy); the Rust
//! migrations write to stdout, which an in-process test cannot capture, so that check is left out.

use std::sync::Arc;

use pi_coding_agent::config::ENV_AGENT_DIR;
use pi_coding_agent::core::auth_storage::AuthStorage;
use pi_coding_agent::core::model_registry::{ModelRegistry, ResolvedRequestAuth};
use pi_coding_agent::core::model_runtime::{CreateModelRuntimeOptions, ModelRuntime};
use pi_coding_agent::core::models_store::InMemoryCodingAgentModelsStore;
use pi_coding_agent::migrations::run_migrations;
use serde_json::{Value, json};

static ENV_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

fn with_env<R>(vars: &[(&str, Option<&str>)], f: impl FnOnce() -> R) -> R {
    let saved: Vec<(String, Option<String>)> = vars.iter().map(|(k, _)| (k.to_string(), std::env::var(k).ok())).collect();
    for (key, value) in vars {
        match value {
            // SAFETY: tests that touch the environment hold ENV_LOCK.
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
    let result = f();
    for (key, value) in saved {
        match value {
            Some(value) => unsafe { std::env::set_var(&key, value) },
            None => unsafe { std::env::remove_var(&key) },
        }
    }
    result
}

fn migrate(agent_dir: &std::path::Path) {
    let dir = agent_dir.to_string_lossy().into_owned();
    with_env(&[(ENV_AGENT_DIR, Some(&dir))], || {
        run_migrations(&dir);
    });
}

fn write_json(path: &std::path::Path, value: &Value) {
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(value).unwrap())).unwrap();
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

async fn create_model_registry(auth_path: &std::path::Path, models_path: &std::path::Path) -> ModelRegistry {
    let runtime = ModelRuntime::create(Some(&CreateModelRuntimeOptions {
        credentials: Some(AuthStorage::create(Some(&auth_path.to_string_lossy()))),
        models_path: Some(models_path.to_string_lossy().into_owned()),
        models_store: Some(Arc::new(InMemoryCodingAgentModelsStore::new())),
        allow_model_network: Some(false),
        ..Default::default()
    }))
    .await
    .unwrap();
    ModelRegistry::new(runtime)
}

// ---------------------------------------------------------------------------
// keybindings-migration.test.ts
// ---------------------------------------------------------------------------

#[test]
fn rewrites_old_key_names_to_namespaced_ids() {
    let _guard = ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keybindings.json");
    write_json(&path, &json!({ "cursorUp": ["up", "ctrl+p"], "expandTools": "ctrl+x" }));
    migrate(dir.path());
    assert_eq!(read_json(&path), json!({ "tui.editor.cursorUp": ["up", "ctrl+p"], "app.tools.expand": "ctrl+x" }));
}

#[test]
fn keeps_the_namespaced_value_when_old_and_new_names_both_exist() {
    let _guard = ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keybindings.json");
    write_json(&path, &json!({ "expandTools": "ctrl+x", "app.tools.expand": "ctrl+y" }));
    migrate(dir.path());
    assert_eq!(read_json(&path), json!({ "app.tools.expand": "ctrl+y" }));
}

// "loads old key names in memory before the file is rewritten" is
// `core::keybindings::tests::loads_old_key_names_in_memory_and_ignores_schema_and_invalid_values`.

// ---------------------------------------------------------------------------
// config-value-migration.test.ts
// ---------------------------------------------------------------------------

#[test]
fn leaves_uppercase_auth_json_api_key_values_unchanged() {
    let _guard = ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    write_json(
        &path,
        &json!({
            "anthropic": { "type": "api_key", "key": "ANTHROPIC_API_KEY" },
            "openai": { "type": "api_key", "key": "$OPENAI_API_KEY" },
            "opencode": { "type": "api_key", "key": "public" },
            "github": { "type": "oauth", "access": "ACCESS_TOKEN", "refresh": "REFRESH_TOKEN", "expires": 1 },
        }),
    );
    migrate(dir.path());
    let migrated = read_json(&path);
    assert_eq!(migrated["anthropic"]["key"], "ANTHROPIC_API_KEY");
    assert_eq!(migrated["openai"]["key"], "$OPENAI_API_KEY");
    assert_eq!(migrated["opencode"]["key"], "public");
    assert_eq!(migrated["github"]["access"], "ACCESS_TOKEN");
}

#[tokio::test]
async fn does_not_throw_on_malformed_or_blank_models_json_during_migrations() {
    for content in ["{\n  \"providers\": {\n", ""] {
        let dir = tempfile::tempdir().unwrap();
        let models_path = dir.path().join("models.json");
        std::fs::write(&models_path, content).unwrap();
        {
            let _guard = ENV_LOCK.lock();
            migrate(dir.path());
        }
        assert_eq!(std::fs::read_to_string(&models_path).unwrap(), content);
        let registry = create_model_registry(&dir.path().join("auth.json"), &models_path).await;
        let error = registry.get_error().unwrap_or_default();
        assert!(error.contains("Failed to parse models.json"), "{error}");
        assert!(error.contains(&format!("File: {}", models_path.display())), "{error}");
    }
}

#[tokio::test]
async fn leaves_uppercase_models_json_api_key_and_header_values_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let models_path = dir.path().join("models.json");
    write_json(
        &models_path,
        &json!({
            "providers": {
                "custom-provider": {
                    "baseUrl": "https://example.com/v1",
                    "apiKey": "CUSTOM_API_KEY",
                    "api": "openai-completions",
                    "headers": { "x-api-key": "HEADER_API_KEY", "x-literal": "literal" },
                    "models": [{ "id": "model-a", "headers": { "x-model-key": "MODEL_API_KEY" } }],
                    "modelOverrides": { "model-b": { "headers": { "x-override-key": "OVERRIDE_API_KEY" } } }
                }
            }
        }),
    );
    let env_keys = ["CUSTOM_API_KEY", "HEADER_API_KEY", "MODEL_API_KEY", "OVERRIDE_API_KEY"];
    let values: Vec<String> = env_keys.iter().map(|k| format!("env-{k}")).collect();
    let vars: Vec<(&str, Option<&str>)> = env_keys.iter().zip(&values).map(|(k, v)| (*k, Some(v.as_str()))).collect();
    let _guard = ENV_LOCK.lock();
    with_env(&vars, || migrate(dir.path()));

    let provider = &read_json(&models_path)["providers"]["custom-provider"];
    assert_eq!(provider["apiKey"], "CUSTOM_API_KEY");
    assert_eq!(provider["headers"]["x-api-key"], "HEADER_API_KEY");
    assert_eq!(provider["headers"]["x-literal"], "literal");
    assert_eq!(provider["models"][0]["headers"]["x-model-key"], "MODEL_API_KEY");
    assert_eq!(provider["modelOverrides"]["model-b"]["headers"]["x-override-key"], "OVERRIDE_API_KEY");

    // The registry resolves these values while the env vars are set, and keeps them literal.
    let (key, auth) = {
        let saved: Vec<(String, Option<String>)> = env_keys.iter().map(|k| (k.to_string(), std::env::var(k).ok())).collect();
        for (k, v) in &vars {
            unsafe { std::env::set_var(k, v.unwrap()) };
        }
        let registry = create_model_registry(&dir.path().join("auth.json"), &models_path).await;
        let model = registry.find("custom-provider", "model-a").expect("model-a");
        let key = registry.get_api_key_for_provider("custom-provider").await;
        let auth = registry.get_api_key_and_headers(&model).await;
        for (k, v) in saved {
            match v {
                Some(v) => unsafe { std::env::set_var(&k, v) },
                None => unsafe { std::env::remove_var(&k) },
            }
        }
        (key, auth)
    };
    drop(_guard);
    assert_eq!(key.as_deref(), Some("CUSTOM_API_KEY"));
    let ResolvedRequestAuth::Ok(auth) = auth else { panic!("auth should resolve") };
    assert_eq!(auth.api_key.as_deref(), Some("CUSTOM_API_KEY"));
    let headers = auth.headers.unwrap();
    assert_eq!(headers.get("x-api-key"), Some(&Some("HEADER_API_KEY".to_string())));
    assert_eq!(headers.get("x-literal"), Some(&Some("literal".to_string())));
    assert_eq!(headers.get("x-model-key"), Some(&Some("MODEL_API_KEY".to_string())));
}

// ---------------------------------------------------------------------------
// suite/regressions/5661-uppercase-header-values.test.ts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn keeps_uppercase_header_strings_as_literals_during_startup_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let models_path = dir.path().join("models.json");
    write_json(
        &models_path,
        &json!({
            "providers": {
                "my-provider": {
                    "baseUrl": "https://example.com/v1",
                    "apiKey": "CUSTOM_API_KEY",
                    "api": "openai-completions",
                    "headers": { "Authorization": "BEARER" },
                    "models": [{ "id": "my-model" }]
                }
            }
        }),
    );
    let _guard = ENV_LOCK.lock();
    let vars = [("CUSTOM_API_KEY", Some("env-CUSTOM_API_KEY")), ("BEARER", Some("env-BEARER"))];
    with_env(&vars, || migrate(dir.path()));
    let provider = &read_json(&models_path)["providers"]["my-provider"];
    assert_eq!(provider["apiKey"], "CUSTOM_API_KEY");
    assert_eq!(provider["headers"]["Authorization"], "BEARER");

    for (k, v) in vars {
        unsafe { std::env::set_var(k, v.unwrap()) };
    }
    let registry = create_model_registry(&dir.path().join("auth.json"), &models_path).await;
    let model = registry.find("my-provider", "my-model").expect("my-model");
    let auth = registry.get_api_key_and_headers(&model).await;
    for (k, _) in vars {
        unsafe { std::env::remove_var(k) };
    }
    drop(_guard);
    let ResolvedRequestAuth::Ok(auth) = auth else { panic!("auth should resolve") };
    assert_eq!(auth.api_key.as_deref(), Some("CUSTOM_API_KEY"));
    assert_eq!(auth.headers.unwrap().get("Authorization"), Some(&Some("BEARER".to_string())));
}
