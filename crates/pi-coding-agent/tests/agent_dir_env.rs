//! Tests that read the agent directory from `PI_CODING_AGENT_DIR` (they hold a lock while they
//! set it): ports of `test/keybindings-migration.test.ts`, `test/config-value-migration.test.ts`,
//! `test/suite/regressions/5661-uppercase-header-values.test.ts` and
//! `test/suite/regressions/7497-session-discovery-symlink.test.ts`.
//! TS also asserts that migrations log nothing (`console.log` spy); the Rust
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

// ---------------------------------------------------------------------------
// suite/regressions/7497-session-discovery-symlink.test.ts
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod session_discovery_symlink {
    use super::*;
    use pi_coding_agent::core::session_manager::SessionManager;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    fn write_session(root: &Path, dir: &Path, id: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let header = json!({
            "type": "session", "version": 3, "id": id, "timestamp": "2026-08-03T00:00:00.000Z",
            "cwd": root.join("project").to_string_lossy(),
        });
        std::fs::write(dir.join(format!("{id}.jsonl")), format!("{header}\n")).unwrap();
    }

    async fn list_all(agent_dir: &Path) -> Vec<(String, String)> {
        let dir = agent_dir.to_string_lossy().into_owned();
        let _guard = ENV_LOCK.lock();
        let saved = std::env::var(ENV_AGENT_DIR).ok();
        unsafe { std::env::set_var(ENV_AGENT_DIR, &dir) };
        let sessions = SessionManager::list_all(None, None, None).await;
        match saved {
            Some(saved) => unsafe { std::env::set_var(ENV_AGENT_DIR, saved) },
            None => unsafe { std::env::remove_var(ENV_AGENT_DIR) },
        }
        sessions.unwrap().into_iter().map(|s| (s.id, s.path)).collect()
    }

    fn setup() -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let sessions_dir = temp.path().join("agent").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        (temp, sessions_dir)
    }

    #[tokio::test]
    async fn discovers_a_session_through_a_directory_link_and_preserves_the_alias_path() {
        let (temp, sessions_dir) = setup();
        let target = temp.path().join("linked-sessions");
        write_session(temp.path(), &target, "linked");
        let alias = sessions_dir.join("--linked--");
        symlink(&target, &alias).unwrap();
        let sessions = list_all(&temp.path().join("agent")).await;
        assert_eq!(sessions.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), vec!["linked"]);
        assert_eq!(sessions[0].1, alias.join("linked.jsonl").to_string_lossy());
    }

    #[tokio::test]
    async fn ignores_a_broken_directory_link_without_hiding_valid_sessions() {
        let (temp, sessions_dir) = setup();
        write_session(temp.path(), &sessions_dir.join("--regular--"), "regular");
        let target = temp.path().join("removed-sessions");
        std::fs::create_dir_all(&target).unwrap();
        symlink(&target, sessions_dir.join("--broken--")).unwrap();
        std::fs::remove_dir_all(&target).unwrap();
        let sessions = list_all(&temp.path().join("agent")).await;
        assert_eq!(sessions.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), vec!["regular"]);
    }

    #[tokio::test]
    async fn ignores_links_to_files() {
        let (temp, sessions_dir) = setup();
        write_session(temp.path(), &sessions_dir.join("--regular--"), "regular");
        let target = temp.path().join("not-a-directory");
        std::fs::write(&target, "").unwrap();
        symlink(&target, sessions_dir.join("--file--")).unwrap();
        let sessions = list_all(&temp.path().join("agent")).await;
        assert_eq!(sessions.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), vec!["regular"]);
    }
}

// ---------------------------------------------------------------------------
// resource-loader-theme.test.ts: theme color mode follows the settings (#9973) and themes are
// validated strictly on load and reload. These set `PI_TRUE_COLOR` and the process-wide terminal
// capability overrides, so they hold ENV_LOCK.
// ---------------------------------------------------------------------------

mod resource_loader_theme {
    use super::*;
    use pi_coding_agent::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
    use pi_coding_agent::core::settings_manager::SettingsManager;
    use pi_coding_agent::modes::interactive::theme::theme::set_theme_json_validator;
    use pi_coding_agent::modes::interactive::theme::theme_schema::validate_theme_json;
    use pi_coding_agent::modes::interactive::theme::theme_tokens::ThemeBg;
    use pi_tui::terminal_image::{CapabilityOverrides, reset_capabilities_cache, set_capability_overrides};
    use std::path::{Path, PathBuf};

    const DARK_THEME: &str = include_str!("../src/modes/interactive/theme/dark.json");

    struct Fixture {
        _temp: tempfile::TempDir,
        agent_dir: PathBuf,
        cwd: PathBuf,
        theme_path: PathBuf,
        theme_json: Value,
    }

    fn fixture() -> Fixture {
        let validator: pi_coding_agent::modes::interactive::theme::theme::ThemeJsonValidator =
            Arc::new(|label, json| validate_theme_json(&label, json));
        set_theme_json_validator(&validator);
        let temp = tempfile::tempdir().unwrap();
        let agent_dir = temp.path().join("agent");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        let mut theme_json: Value = serde_json::from_str(DARK_THEME).unwrap();
        theme_json["name"] = json!("capability-test");
        theme_json["colors"]["userMessageBg"] = json!("#3c3544");
        let theme_path = temp.path().join("capability-test.json");
        std::fs::write(&theme_path, theme_json.to_string()).unwrap();
        Fixture { _temp: temp, agent_dir, cwd, theme_path, theme_json }
    }

    fn loader(fixture: &Fixture, settings_manager: Arc<SettingsManager>) -> DefaultResourceLoader {
        DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
            cwd: fixture.cwd.to_string_lossy().into_owned(),
            agent_dir: fixture.agent_dir.to_string_lossy().into_owned(),
            settings_manager: Some(settings_manager),
            additional_theme_paths: Some(vec![fixture.theme_path.to_string_lossy().into_owned()]),
            no_extensions: Some(true),
            no_skills: Some(true),
            no_prompt_templates: Some(true),
            no_context_files: Some(true),
            ..Default::default()
        })
    }

    fn user_message_bg(loader: &DefaultResourceLoader) -> Option<String> {
        loader
            .get_themes()
            .themes
            .iter()
            .find(|theme| theme.name().as_deref() == Some("capability-test"))
            .map(|theme| theme.bg(&ThemeBg::UserMessageBg, "x"))
    }

    fn reset_capabilities(true_color: Option<&str>) {
        match true_color {
            Some(value) => unsafe { std::env::set_var("PI_TRUE_COLOR", value) },
            None => unsafe { std::env::remove_var("PI_TRUE_COLOR") },
        }
        set_capability_overrides(CapabilityOverrides::default());
        reset_capabilities_cache();
    }

    // Regression test for #9973.
    #[tokio::test]
    async fn uses_the_true_color_setting_over_the_environment() {
        let _guard = ENV_LOCK.lock();
        let saved = std::env::var("PI_TRUE_COLOR").ok();
        for (environment_override, setting, expected) in
            [("0", true, "\x1b[48;2;60;53;68mx\x1b[49m"), ("1", false, "\x1b[48;5;59mx\x1b[49m")]
        {
            let fixture = fixture();
            reset_capabilities(Some(environment_override));
            let settings = SettingsManager::in_memory_json(
                json!({ "terminal": { "trueColor": setting } }).as_object().unwrap().clone(),
                None,
            );
            let loader = loader(&fixture, settings);
            loader.reload(None).await.unwrap();
            assert_eq!(user_message_bg(&loader).as_deref(), Some(expected), "trueColor {setting}");
        }
        reset_capabilities(saved.as_deref());
    }

    #[tokio::test]
    async fn returns_to_automatic_detection_after_an_explicit_setting_is_removed() {
        let _guard = ENV_LOCK.lock();
        let saved = std::env::var("PI_TRUE_COLOR").ok();
        reset_capabilities(Some("1"));
        let fixture = fixture();
        let settings_path = fixture.agent_dir.join("settings.json");
        std::fs::write(&settings_path, json!({ "terminal": { "trueColor": false } }).to_string()).unwrap();
        let settings = SettingsManager::create(
            &fixture.cwd.to_string_lossy(),
            Some(&fixture.agent_dir.to_string_lossy()),
            None,
        );
        let loader = loader(&fixture, settings.clone());
        loader.reload(None).await.unwrap();
        set_capability_overrides(settings.get_terminal_capability_overrides());

        std::fs::write(&settings_path, "{}").unwrap();
        loader.reload(None).await.unwrap();
        let bg = user_message_bg(&loader);
        reset_capabilities(saved.as_deref());
        assert_eq!(bg.as_deref(), Some("\x1b[48;2;60;53;68mx\x1b[49m"));
    }

    #[tokio::test]
    async fn applies_strict_validation_on_initial_load_and_reload() {
        let _guard = ENV_LOCK.lock();
        let mut fixture = fixture();
        let loader = loader(&fixture, SettingsManager::in_memory(None, None));
        let write = |path: &Path, value: &Value| std::fs::write(path, value.to_string()).unwrap();

        fixture.theme_json["unsupported"] = json!(true);
        write(&fixture.theme_path, &fixture.theme_json);
        loader.reload(None).await.unwrap();
        let initial = loader.get_themes().diagnostics;
        assert_eq!(initial.len(), 1, "{initial:?}");
        let diagnostic = serde_json::to_value(&initial[0]).unwrap();
        assert_eq!(diagnostic["type"], "warning");
        assert_eq!(diagnostic["path"], json!(fixture.theme_path.to_string_lossy()));
        assert!(diagnostic["message"].as_str().unwrap().contains("/unsupported"), "{diagnostic}");

        fixture.theme_json.as_object_mut().unwrap().remove("unsupported");
        write(&fixture.theme_path, &fixture.theme_json);
        loader.reload(None).await.unwrap();
        assert!(loader.get_themes().diagnostics.is_empty());
        assert!(loader.get_themes().themes.iter().any(|t| t.name().as_deref() == Some("capability-test")));

        fixture.theme_json["unsupported"] = json!(true);
        write(&fixture.theme_path, &fixture.theme_json);
        loader.reload(None).await.unwrap();
        assert_eq!(
            serde_json::to_value(loader.get_themes().diagnostics).unwrap(),
            serde_json::to_value(&initial).unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// clipboard-image-bmp-conversion.test.ts (linux case): TS mocks `runClipboardCommand`; here a
// fake `wl-paste` on PATH serves a BMP. The win32 case reads pi-tui's native clipboard, which
// cannot be faked, and is not ported.
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn linux_converts_command_bmp_clipboard_images_to_png() {
    use pi_coding_agent::utils::clipboard_image::{ReadClipboardImageOptions, read_clipboard_image};
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let mut bmp = vec![0u8; 58];
    bmp[0..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&58u32.to_le_bytes());
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&1i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    bmp[34..38].copy_from_slice(&4u32.to_le_bytes());
    bmp[56] = 0xff;
    let bmp_path = dir.path().join("clip.bmp");
    std::fs::write(&bmp_path, &bmp).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n  *--list-types*) echo image/bmp ;;\n  *image/bmp*) cat '{}' ;;\nesac\n",
        bmp_path.display()
    );
    let wl_paste = dir.path().join("wl-paste");
    std::fs::write(&wl_paste, script).unwrap();
    std::fs::set_permissions(&wl_paste, std::fs::Permissions::from_mode(0o755)).unwrap();

    let _guard = ENV_LOCK.lock();
    let path = format!("{}:{}", dir.path().display(), std::env::var("PATH").unwrap_or_default());
    let saved = std::env::var("PATH").ok();
    unsafe { std::env::set_var("PATH", &path) };
    let options = ReadClipboardImageOptions {
        env: Some([("WAYLAND_DISPLAY".to_string(), "wayland-0".to_string())].into_iter().collect()),
        platform: Some("linux".into()),
    };
    let image = read_clipboard_image(Some(&options)).await;
    if let Some(saved) = saved {
        unsafe { std::env::set_var("PATH", saved) };
    }
    let image = image.unwrap().expect("clipboard image");
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(&image.bytes[..4], &[0x89, 0x50, 0x4e, 0x47]);
}

// ---------------------------------------------------------------------------
// tools-manager.test.ts "ensureTool": an empty agent dir and an empty PATH stand in for the
// mocked `existsSync`/`spawnSync`, so fd is never found.
// ---------------------------------------------------------------------------

mod ensure_tool {
    use super::ENV_LOCK;
    use pi_coding_agent::config::ENV_AGENT_DIR;
    use pi_coding_agent::utils::tools_manager::{GetToolPathTool, ToolStatus, ensure_tool};
    use std::sync::Arc;

    const PROXY_VARS: &[&str] = &[
        "HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy",
    ];

    async fn run_with_env(vars: &[(&str, Option<&str>)]) -> (Option<String>, Vec<(String, String)>) {
        let saved: Vec<(String, Option<String>)> =
            vars.iter().map(|(k, _)| (k.to_string(), std::env::var(k).ok())).collect();
        for (key, value) in vars {
            match value {
                // SAFETY: the caller holds ENV_LOCK.
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
        let statuses: Arc<parking_lot::Mutex<Vec<(String, String)>>> = Arc::default();
        let sink = statuses.clone();
        let result = ensure_tool(
            GetToolPathTool::Fd,
            Some(Arc::new(move |status: ToolStatus| {
                sink.lock().push((status.r#type.as_str().to_string(), status.message));
            })),
        )
        .await;
        for (key, value) in saved {
            match value {
                Some(value) => unsafe { std::env::set_var(&key, value) },
                None => unsafe { std::env::remove_var(&key) },
            }
        }
        let statuses = statuses.lock().clone();
        (result.unwrap(), statuses)
    }

    #[tokio::test]
    async fn reports_status_through_a_callback_without_writing_to_the_console() {
        let agent_dir = tempfile::tempdir().unwrap();
        let empty_path = tempfile::tempdir().unwrap();
        let _guard = ENV_LOCK.lock();
        let (result, statuses) = run_with_env(&[
            (ENV_AGENT_DIR, Some(&agent_dir.path().to_string_lossy())),
            ("PATH", Some(&empty_path.path().to_string_lossy())),
            ("PI_OFFLINE", Some("1")),
        ])
        .await;
        assert_eq!(result, None);
        assert_eq!(
            statuses,
            vec![("warning".to_string(), "fd not found. Offline mode enabled, skipping download.".to_string())]
        );
    }

    #[tokio::test]
    async fn surfaces_the_error_cause_chain_when_a_download_fails() {
        let agent_dir = tempfile::tempdir().unwrap();
        let empty_path = tempfile::tempdir().unwrap();
        // A proxy that refuses connections makes the release lookup fail at the transport level.
        let mut vars: Vec<(&str, Option<&str>)> = PROXY_VARS.iter().map(|k| (*k, None)).collect();
        vars.extend([("HTTPS_PROXY", Some("http://127.0.0.1:1")), ("https_proxy", Some("http://127.0.0.1:1"))]);
        let agent_dir_str = agent_dir.path().to_string_lossy().into_owned();
        let path_str = empty_path.path().to_string_lossy().into_owned();
        vars.extend([(ENV_AGENT_DIR, Some(agent_dir_str.as_str())), ("PATH", Some(path_str.as_str())), ("PI_OFFLINE", None)]);
        let _guard = ENV_LOCK.lock();
        let (result, statuses) = run_with_env(&vars).await;
        assert_eq!(result, None);
        assert_eq!(statuses.len(), 2, "{statuses:?}");
        assert_eq!(statuses[0], ("info".to_string(), "fd not found. Downloading...".to_string()));
        assert_eq!(statuses[1].0, "warning");
        // "fetch failed" followed by its causes, like the TS `fetch failed: connect ETIMEDOUT ...`.
        let message = &statuses[1].1;
        assert!(message.starts_with("Failed to download fd: fetch failed: "), "{message}");
        assert!(message.matches(": ").count() >= 3, "{message}");
    }
}

// ---------------------------------------------------------------------------
// http-dispatcher.test.ts "http proxy settings". The dispatcher cases (CONNECT tunneling of
// proxied HTTP origins, Node's happy-eyeballs attempt timeout) test undici/Node internals; reqwest
// forwards proxied plain-HTTP requests instead of tunneling them.
// ---------------------------------------------------------------------------

mod http_proxy_settings {
    use super::{ENV_LOCK, with_env};
    use pi_coding_agent::core::http_dispatcher::apply_http_proxy_settings;

    fn proxy_vars() -> (Option<String>, Option<String>) {
        (std::env::var("HTTP_PROXY").ok(), std::env::var("HTTPS_PROXY").ok())
    }

    #[test]
    fn applies_http_proxy_to_http_proxy_and_https_proxy() {
        let _guard = ENV_LOCK.lock();
        let vars = with_env(&[("HTTP_PROXY", None), ("HTTPS_PROXY", None)], || {
            apply_http_proxy_settings(Some("http://127.0.0.1:7890"));
            proxy_vars()
        });
        assert_eq!(vars, (Some("http://127.0.0.1:7890".into()), Some("http://127.0.0.1:7890".into())));
    }

    #[test]
    fn does_not_override_existing_proxy_env_vars() {
        let _guard = ENV_LOCK.lock();
        let vars = with_env(
            &[("HTTP_PROXY", Some("http://env-http:8080")), ("HTTPS_PROXY", Some("http://env-https:8080"))],
            || {
                apply_http_proxy_settings(Some("http://settings:7890"));
                proxy_vars()
            },
        );
        assert_eq!(vars, (Some("http://env-http:8080".into()), Some("http://env-https:8080".into())));
    }

    #[test]
    fn ignores_empty_values() {
        let _guard = ENV_LOCK.lock();
        let vars = with_env(&[("HTTP_PROXY", None), ("HTTPS_PROXY", None)], || {
            apply_http_proxy_settings(Some("   "));
            proxy_vars()
        });
        assert_eq!(vars, (None, None));
    }
}
