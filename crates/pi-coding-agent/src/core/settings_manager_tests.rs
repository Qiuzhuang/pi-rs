//! Ports of `test/settings-manager.test.ts`, `test/settings-manager-bug.test.ts` and
//! `test/settings-manager-compaction.test.ts`.

use super::*;
use serde_json::json;
use std::path::{Path, PathBuf};

struct Dirs {
    _temp: tempfile::TempDir,
    agent_dir: PathBuf,
    project_dir: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let agent_dir = temp.path().join("agent");
        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::create_dir_all(project_dir.join(".pi")).unwrap();
        Self {
            _temp: temp,
            agent_dir,
            project_dir,
        }
    }

    fn global_path(&self) -> PathBuf {
        self.agent_dir.join("settings.json")
    }

    fn project_path(&self) -> PathBuf {
        self.project_dir.join(".pi").join("settings.json")
    }

    fn manager(&self) -> Arc<SettingsManager> {
        SettingsManager::create(
            self.project_dir.to_str().unwrap(),
            Some(self.agent_dir.to_str().unwrap()),
            None,
        )
    }

    fn manager_with(&self, trusted: bool) -> Arc<SettingsManager> {
        SettingsManager::create(
            self.project_dir.to_str().unwrap(),
            Some(self.agent_dir.to_str().unwrap()),
            Some(&SettingsManagerCreateOptions {
                project_trusted: Some(trusted),
            }),
        )
    }
}

fn write_json(path: &Path, value: Value) {
    std::fs::write(path, serde_json::to_string(&value).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn in_memory(value: Value) -> Arc<SettingsManager> {
    SettingsManager::in_memory_json(value.as_object().unwrap().clone(), None)
}

fn storage_with(global: Option<Value>, project: Option<Value>) -> Arc<InMemorySettingsStorage> {
    let storage = Arc::new(InMemorySettingsStorage::new());
    if let Some(global) = global {
        let content = global.to_string();
        storage
            .with_lock(SettingsScope::Global, &mut |_| Ok(Some(content.clone())))
            .unwrap();
    }
    if let Some(project) = project {
        let content = project.to_string();
        storage
            .with_lock(SettingsScope::Project, &mut |_| Ok(Some(content.clone())))
            .unwrap();
    }
    storage
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------------------------------------------------------------------------
// settings-manager.test.ts
// ---------------------------------------------------------------------------

#[test]
fn preserves_enabled_models_when_changing_thinking_level() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"theme": "dark", "defaultModel": "claude-sonnet"}),
    );
    let manager = dirs.manager();

    let mut current = read_json(&dirs.global_path());
    current["enabledModels"] = json!(["claude-opus-4-5", "gpt-5.2-codex"]);
    std::fs::write(
        dirs.global_path(),
        serde_json::to_string_pretty(&current).unwrap(),
    )
    .unwrap();

    manager.set_default_thinking_level(ThinkingLevel::High);

    let saved = read_json(&dirs.global_path());
    assert_eq!(saved["enabledModels"], json!(["claude-opus-4-5", "gpt-5.2-codex"]));
    assert_eq!(saved["defaultThinkingLevel"], "high");
    assert_eq!(saved["theme"], "dark");
    assert_eq!(saved["defaultModel"], "claude-sonnet");
}

#[test]
fn preserves_custom_settings_when_changing_theme() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"defaultModel": "claude-sonnet"}));
    let manager = dirs.manager();
    let mut current = read_json(&dirs.global_path());
    current["shellPath"] = json!("/bin/zsh");
    current["extensions"] = json!(["/path/to/extension.ts"]);
    write_json(&dirs.global_path(), current);

    manager.set_theme("light");

    let saved = read_json(&dirs.global_path());
    assert_eq!(saved["shellPath"], "/bin/zsh");
    assert_eq!(saved["extensions"], json!(["/path/to/extension.ts"]));
    assert_eq!(saved["theme"], "light");
}

#[test]
fn in_memory_changes_override_file_changes_for_same_key() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    let manager = dirs.manager();
    let mut current = read_json(&dirs.global_path());
    current["defaultThinkingLevel"] = json!("low");
    write_json(&dirs.global_path(), current);

    manager.set_default_thinking_level(ThinkingLevel::High);
    assert_eq!(read_json(&dirs.global_path())["defaultThinkingLevel"], "high");
}

#[test]
fn creates_one_global_device_id_and_reuses_it() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    write_json(&dirs.project_path(), json!({"deviceId": "project-device"}));
    let first = dirs.manager();
    let device_id = first.get_or_create_device_id();
    let re = regex::Regex::new(
        r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
    )
    .unwrap();
    assert!(re.is_match(&device_id));
    assert_eq!(first.get_or_create_device_id(), device_id);
    assert_eq!(dirs.manager().get_or_create_device_id(), device_id);
    assert_eq!(
        read_json(&dirs.global_path()),
        json!({"theme": "dark", "deviceId": device_id})
    );
}

#[test]
fn keeps_local_only_extensions_in_extensions_array() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"extensions": ["/local/ext.ts", "./relative/ext.ts"]}),
    );
    let manager = dirs.manager();
    assert!(manager.get_packages().is_empty());
    assert_eq!(
        manager.get_extension_paths(),
        vec!["/local/ext.ts", "./relative/ext.ts"]
    );
}

#[test]
fn handles_packages_with_filtering_objects() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"packages": ["npm:simple-pkg", {"source": "npm:shitty-extensions", "extensions": ["extensions/oracle.ts"], "skills": []}]}),
    );
    let packages = dirs.manager().get_packages();
    assert_eq!(packages.len(), 2);
    assert_eq!(to_json(&packages[0]), json!("npm:simple-pkg"));
    assert_eq!(
        to_json(&packages[1]),
        json!({"source": "npm:shitty-extensions", "extensions": ["extensions/oracle.ts"], "skills": []})
    );
}

#[tokio::test]
async fn reloads_global_settings_from_disk() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"theme": "dark", "extensions": ["/before.ts"]}),
    );
    let manager = dirs.manager();
    write_json(
        &dirs.global_path(),
        json!({"theme": "light", "extensions": ["/after.ts"], "defaultModel": "claude-sonnet"}),
    );
    manager.reload().await.unwrap();
    assert_eq!(manager.get_theme().as_deref(), Some("light"));
    assert_eq!(manager.get_extension_paths(), vec!["/after.ts"]);
    assert_eq!(manager.get_default_model().as_deref(), Some("claude-sonnet"));
}

#[tokio::test]
async fn keeps_previous_settings_and_reports_path_when_file_invalid() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    let manager = dirs.manager();
    std::fs::write(dirs.global_path(), "{ invalid json").unwrap();
    manager.reload().await.unwrap();
    assert_eq!(manager.get_theme().as_deref(), Some("dark"));
    let errors = manager.drain_errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].scope, SettingsScope::Global);
    assert_eq!(
        errors[0].path.as_deref(),
        Some(dirs.global_path().to_str().unwrap())
    );
}

#[test]
fn stores_slash_separated_theme_settings_separately() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "light/dark"}));
    let manager = dirs.manager();
    assert_eq!(manager.get_theme(), None);
    assert_eq!(manager.get_theme_setting().as_deref(), Some("light/dark"));
    manager.set_theme("solarized-light/tokyo-night");
    assert_eq!(
        read_json(&dirs.global_path())["theme"],
        "solarized-light/tokyo-night"
    );
}

#[test]
fn collects_and_clears_load_errors() {
    let dirs = Dirs::new();
    std::fs::write(dirs.global_path(), "{ invalid global json").unwrap();
    std::fs::write(dirs.project_path(), "{ invalid project json").unwrap();
    let manager = dirs.manager();
    let errors = manager.drain_errors();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].scope, SettingsScope::Global);
    assert_eq!(
        errors[0].path.as_deref(),
        Some(dirs.global_path().to_str().unwrap())
    );
    assert_eq!(errors[1].scope, SettingsScope::Project);
    assert_eq!(
        errors[1].path.as_deref(),
        Some(dirs.project_path().to_str().unwrap())
    );
    assert!(manager.drain_errors().is_empty());
}

#[test]
fn skips_project_settings_when_not_trusted() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "global"}));
    write_json(&dirs.project_path(), json!({"theme": "project"}));
    let manager = dirs.manager_with(false);
    assert!(!manager.is_project_trusted());
    assert_eq!(manager.get_theme().as_deref(), Some("global"));
    assert!(manager.get_project_settings_json().is_empty());
}

#[test]
fn reloads_project_settings_after_trust_changes() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "global"}));
    write_json(&dirs.project_path(), json!({"theme": "project"}));
    let manager = dirs.manager_with(false);
    manager.set_project_trusted(true);
    assert!(manager.is_project_trusted());
    assert_eq!(manager.get_theme().as_deref(), Some("project"));
}

#[test]
fn fails_project_settings_writes_when_not_trusted() {
    let dirs = Dirs::new();
    write_json(&dirs.project_path(), json!({"packages": ["npm:existing"]}));
    let manager = dirs.manager_with(false);
    let err = manager
        .set_project_packages(&[PackageSource::String("npm:new".to_string())])
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Project is not trusted; refusing to write project settings"
    );
    assert!(manager.get_project_settings_json().is_empty());
    assert_eq!(
        read_json(&dirs.project_path()),
        json!({"packages": ["npm:existing"]})
    );
}

#[test]
fn reads_default_project_trust_from_global_settings_only() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"defaultProjectTrust": "always"}));
    write_json(&dirs.project_path(), json!({"defaultProjectTrust": "never"}));
    assert_eq!(
        dirs.manager().get_default_project_trust(),
        DefaultProjectTrust::Always
    );
}

#[test]
fn defaults_invalid_project_trust_settings_to_ask() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"defaultProjectTrust": "sometimes"}));
    assert_eq!(
        dirs.manager().get_default_project_trust(),
        DefaultProjectTrust::Ask
    );
}

#[test]
fn does_not_create_pi_folder_when_only_reading() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    std::fs::remove_dir_all(dirs.project_dir.join(".pi")).unwrap();
    let manager = dirs.manager();
    assert!(!dirs.project_dir.join(".pi").exists());
    assert_eq!(manager.get_theme().as_deref(), Some("dark"));
}

#[test]
fn creates_pi_folder_when_writing_project_settings() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    std::fs::remove_dir_all(dirs.project_dir.join(".pi")).unwrap();
    let manager = dirs.manager();
    assert!(!dirs.project_dir.join(".pi").exists());
    manager
        .set_project_packages(&[serde_json::from_value(json!({"source": "npm:test-pkg"})).unwrap()])
        .unwrap();
    assert!(dirs.project_dir.join(".pi").exists());
    assert!(dirs.project_path().exists());
}

#[test]
fn maps_terminal_capability_overrides() {
    let get = |terminal: Value| in_memory(json!({"terminal": terminal})).get_terminal_capability_overrides();
    assert_eq!(
        get(json!({"images": false, "trueColor": false, "hyperlinks": false})),
        CapabilityOverrides {
            images: Some(None),
            true_color: Some(false),
            hyperlinks: Some(false)
        }
    );
    assert_eq!(
        get(json!({"images": "kitty", "trueColor": true, "hyperlinks": true})),
        CapabilityOverrides {
            images: Some(Some(ImageProtocol::Kitty)),
            true_color: Some(true),
            hyperlinks: Some(true)
        }
    );
    assert_eq!(
        get(json!({"images": "auto", "trueColor": "auto", "hyperlinks": "auto"})),
        CapabilityOverrides::default()
    );
}

#[test]
fn retry_settings_defaults_and_overrides() {
    assert_eq!(
        SettingsManager::in_memory(None, None).get_retry_settings(),
        SettingsManagerGetRetrySettingsResult {
            enabled: true,
            max_retries: 3,
            base_delay_ms: 2000,
            max_agent_delay_ms: 60000,
        }
    );
    assert_eq!(
        in_memory(json!({"retry": {"enabled": true, "maxRetries": 10, "baseDelayMs": 500, "maxAgentDelayMs": 5000}}))
            .get_retry_settings(),
        SettingsManagerGetRetrySettingsResult {
            enabled: true,
            max_retries: 10,
            base_delay_ms: 500,
            max_agent_delay_ms: 5000,
        }
    );
}

#[test]
fn http_idle_timeout_defaults_merges_and_validates() {
    let dirs = Dirs::new();
    assert_eq!(
        dirs.manager().get_http_idle_timeout_ms().unwrap(),
        crate::core::http_dispatcher::DEFAULT_HTTP_IDLE_TIMEOUT_MS
    );
    write_json(&dirs.global_path(), json!({"httpIdleTimeoutMs": 300000}));
    write_json(&dirs.project_path(), json!({"httpIdleTimeoutMs": 0}));
    assert_eq!(dirs.manager().get_http_idle_timeout_ms().unwrap(), 0);

    write_json(&dirs.project_path(), json!({}));
    write_json(&dirs.global_path(), json!({"httpIdleTimeoutMs": -1}));
    let err = dirs.manager().get_http_idle_timeout_ms().unwrap_err();
    assert!(err.to_string().contains("Invalid httpIdleTimeoutMs setting"));

    write_json(&dirs.global_path(), json!({"httpIdleTimeoutMs": " disabled "}));
    assert_eq!(dirs.manager().get_http_idle_timeout_ms().unwrap(), 0);
}

#[test]
fn cache_warming_defaults_and_ignores_project_settings() {
    let dirs = Dirs::new();
    assert_eq!(dirs.manager().get_cache_warming_mode(), CacheWarmingMode::Streaming);
    write_json(&dirs.project_path(), json!({"cacheWarming": "idle"}));
    assert_eq!(dirs.manager().get_cache_warming_mode(), CacheWarmingMode::Streaming);
    write_json(&dirs.global_path(), json!({"cacheWarming": "idle"}));
    assert_eq!(dirs.manager().get_cache_warming_mode(), CacheWarmingMode::Idle);
    write_json(&dirs.global_path(), json!({"cacheWarming": "bogus"}));
    assert_eq!(dirs.manager().get_cache_warming_mode(), CacheWarmingMode::Streaming);
}

#[test]
fn persists_cache_warming_mode_globally() {
    let dirs = Dirs::new();
    dirs.manager().set_cache_warming_mode(&CacheWarmingMode::Off);
    assert_eq!(dirs.manager().get_cache_warming_mode(), CacheWarmingMode::Off);
    assert_eq!(read_json(&dirs.global_path()), json!({"cacheWarming": "off"}));
}

#[test]
fn resolves_external_editor_commands_by_precedence() {
    let _guard = ENV_LOCK.lock().unwrap();
    let original_visual = std::env::var("VISUAL").ok();
    let original_editor = std::env::var("EDITOR").ok();
    let set = |visual: Option<&str>, editor: Option<&str>| unsafe {
        match visual {
            Some(v) => std::env::set_var("VISUAL", v),
            None => std::env::remove_var("VISUAL"),
        }
        match editor {
            Some(v) => std::env::set_var("EDITOR", v),
            None => std::env::remove_var("EDITOR"),
        }
    };
    set(Some("vim"), Some("nano"));
    assert_eq!(
        in_memory(json!({"externalEditor": "code --wait"})).get_external_editor_command(),
        "code --wait"
    );
    assert_eq!(in_memory(json!({})).get_external_editor_command(), "vim");
    set(None, Some("emacs"));
    assert_eq!(in_memory(json!({})).get_external_editor_command(), "emacs");
    set(None, None);
    assert_eq!(
        in_memory(json!({})).get_external_editor_command(),
        if cfg!(windows) { "notepad" } else { "nano" }
    );
    set(original_visual.as_deref(), original_editor.as_deref());
}

#[test]
fn tui_mode_defaults_to_fullscreen_and_persists_regular() {
    let dirs = Dirs::new();
    let manager = dirs.manager();
    assert_eq!(manager.get_tui_mode(), TuiMode::Fullscreen);
    manager.set_tui_mode(&TuiMode::Regular);
    assert_eq!(manager.get_tui_mode(), TuiMode::Regular);
    assert_eq!(read_json(&dirs.global_path())["tuiMode"], "regular");

    write_json(&dirs.global_path(), json!({"tuiMode": "other"}));
    assert_eq!(dirs.manager().get_tui_mode(), TuiMode::Fullscreen);
    write_json(&dirs.global_path(), json!({"uiMode": "regular"}));
    assert_eq!(dirs.manager().get_tui_mode(), TuiMode::Fullscreen);
}

#[test]
fn validates_and_persists_fullscreen_settings() {
    let dirs = Dirs::new();
    let manager = dirs.manager();
    assert_eq!(manager.get_fullscreen_exit_output(), FullscreenExitOutput::Transcript);
    assert_eq!(manager.get_fullscreen_scrollbar(), ScrollViewScrollbar::Auto);
    assert!(manager.get_fullscreen_copy_on_select());

    manager.set_fullscreen_exit_output(&FullscreenExitOutput::ResumeHint);
    manager.set_fullscreen_scrollbar(ScrollViewScrollbar::Hidden);
    manager.set_fullscreen_copy_on_select(false);
    let saved = read_json(&dirs.global_path());
    assert_eq!(saved["fullscreenExitOutput"], "resume-hint");
    assert_eq!(saved["fullscreenScrollbar"], "hidden");
    assert_eq!(saved["fullscreenCopyOnSelect"], false);

    write_json(
        &dirs.global_path(),
        json!({"fullscreenExitOutput": "nothing", "fullscreenScrollbar": "sometimes"}),
    );
    let reloaded = dirs.manager();
    assert_eq!(reloaded.get_fullscreen_exit_output(), FullscreenExitOutput::Transcript);
    assert_eq!(reloaded.get_fullscreen_scrollbar(), ScrollViewScrollbar::Auto);
    assert!(reloaded.get_fullscreen_copy_on_select());
}

#[test]
fn persists_fullscreen_wheel_scroll_lines() {
    let dirs = Dirs::new();
    let manager = dirs.manager();
    assert_eq!(
        manager.get_fullscreen_wheel_scroll_lines(),
        WheelScrollLines::Auto("auto".to_string())
    );
    manager.set_fullscreen_wheel_scroll_lines(&WheelScrollLines::Lines(3));
    assert_eq!(read_json(&dirs.global_path())["fullscreenWheelScrollLines"], 3);

    for (value, expected) in [
        (json!(7.9), WheelScrollLines::Lines(7)),
        (json!(0), WheelScrollLines::Lines(1)),
        (json!(1000), WheelScrollLines::Lines(100)),
        (json!("fast"), WheelScrollLines::Auto("auto".to_string())),
        (Value::Null, WheelScrollLines::Auto("auto".to_string())),
    ] {
        write_json(&dirs.global_path(), json!({"fullscreenWheelScrollLines": value}));
        assert_eq!(dirs.manager().get_fullscreen_wheel_scroll_lines(), expected);
    }
}

#[test]
fn output_pad_defaults_and_persists() {
    let dirs = Dirs::new();
    let manager = dirs.manager();
    assert_eq!(manager.get_output_pad(), 1);
    manager.set_output_pad(0);
    assert_eq!(manager.get_output_pad(), 0);
    assert_eq!(read_json(&dirs.global_path())["outputPad"], 0);
    write_json(&dirs.global_path(), json!({"outputPad": 2}));
    assert_eq!(dirs.manager().get_output_pad(), 1);
}

#[test]
fn mermaid_mode_defaults_and_persists() {
    let dirs = Dirs::new();
    let manager = dirs.manager();
    assert_eq!(manager.get_mermaid_rendering_mode(), MermaidRenderingMode::Streaming);
    manager.set_mermaid_rendering_mode(&MermaidRenderingMode::Final);
    assert_eq!(manager.get_mermaid_rendering_mode(), MermaidRenderingMode::Final);
    assert_eq!(read_json(&dirs.global_path())["markdown"]["mermaid"], "final");
    write_json(&dirs.global_path(), json!({"markdown": {"mermaid": "sometimes"}}));
    assert_eq!(
        dirs.manager().get_mermaid_rendering_mode(),
        MermaidRenderingMode::Streaming
    );
}

#[test]
fn shell_command_prefix_loads_and_is_preserved() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"shellCommandPrefix": "shopt -s expand_aliases"}),
    );
    let manager = dirs.manager();
    assert_eq!(
        manager.get_shell_command_prefix().as_deref(),
        Some("shopt -s expand_aliases")
    );
    manager.set_theme("light");
    let saved = read_json(&dirs.global_path());
    assert_eq!(saved["shellCommandPrefix"], "shopt -s expand_aliases");
    assert_eq!(saved["theme"], "light");

    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    assert_eq!(dirs.manager().get_shell_command_prefix(), None);
}

#[test]
fn default_tools_global_and_project_layers() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"defaultTools": ["read", "bash"]}));
    assert_eq!(dirs.manager().get_default_tools(), Some(vec!["read".into(), "bash".into()]));
    write_json(&dirs.project_path(), json!({"defaultTools": ["grep"]}));
    assert_eq!(dirs.manager().get_default_tools(), Some(vec!["grep".into()]));
}

#[test]
fn default_tools_empty_and_modifiers() {
    assert_eq!(in_memory(json!({"defaultTools": []})).get_default_tools(), Some(vec![]));
    assert_eq!(in_memory(json!({})).get_default_tools(), None);
    assert_eq!(
        in_memory(json!({"defaultTools": ["+codemode", "-write"]})).get_default_tools(),
        Some(vec!["read".into(), "bash".into(), "edit".into(), "codemode".into()])
    );
    assert_eq!(
        in_memory(json!({"defaultTools": ["read", "+grep", "+read"]})).get_default_tools(),
        Some(vec!["read".into(), "grep".into()])
    );
}

#[test]
fn default_tools_project_modifiers_layer_on_global() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"defaultTools": ["read", "bash", "+codemode"]}),
    );
    write_json(
        &dirs.project_path(),
        json!({"defaultTools": ["-codemode", "+tool_search"]}),
    );
    let manager = dirs.manager();
    assert_eq!(
        manager.get_default_tools(),
        Some(vec!["read".into(), "bash".into(), "tool_search".into()])
    );
    manager.apply_overrides(&Settings {
        default_tools: Some(vec!["+codemode".into()]),
        ..Default::default()
    });
    assert_eq!(
        manager.get_default_tools(),
        Some(vec![
            "read".into(),
            "bash".into(),
            "tool_search".into(),
            "codemode".into()
        ])
    );

    let dirs = Dirs::new();
    write_json(&dirs.project_path(), json!({"defaultTools": ["+codemode"]}));
    assert_eq!(
        dirs.manager().get_default_tools(),
        Some(vec![
            "read".into(),
            "bash".into(),
            "edit".into(),
            "write".into(),
            "codemode".into()
        ])
    );
}

#[test]
fn session_dir_resolution() {
    let dirs = Dirs::new();
    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    assert_eq!(dirs.manager().get_session_dir(), None);
    write_json(&dirs.global_path(), json!({"sessionDir": "/tmp/sessions"}));
    assert_eq!(dirs.manager().get_session_dir().as_deref(), Some("/tmp/sessions"));
    write_json(&dirs.global_path(), json!({"sessionDir": "/global/sessions"}));
    write_json(&dirs.project_path(), json!({"sessionDir": "./sessions"}));
    assert_eq!(dirs.manager().get_session_dir().as_deref(), Some("./sessions"));
    write_json(&dirs.project_path(), json!({}));
    write_json(&dirs.global_path(), json!({"sessionDir": "~/sessions"}));
    let home = dirs::home_dir().unwrap();
    assert_eq!(
        dirs.manager().get_session_dir(),
        Some(home.join("sessions").to_string_lossy().into_owned())
    );
}

#[test]
fn shell_path_resolution() {
    let dirs = Dirs::new();
    let home = dirs::home_dir().unwrap();
    write_json(&dirs.global_path(), json!({"theme": "dark"}));
    assert_eq!(dirs.manager().get_shell_path(), None);
    write_json(&dirs.global_path(), json!({"shellPath": "/bin/zsh"}));
    assert_eq!(dirs.manager().get_shell_path().as_deref(), Some("/bin/zsh"));
    write_json(
        &dirs.global_path(),
        json!({"shellPath": "~/.local/bin/agent-shell-sandbox"}),
    );
    assert_eq!(
        dirs.manager().get_shell_path(),
        Some(
            home.join(".local/bin/agent-shell-sandbox")
                .to_string_lossy()
                .into_owned()
        )
    );
    write_json(&dirs.global_path(), json!({"shellPath": "~"}));
    assert_eq!(
        dirs.manager().get_shell_path(),
        Some(home.to_string_lossy().into_owned())
    );
}

#[test]
fn migrates_legacy_keys() {
    let manager = in_memory(json!({
        "queueMode": "all",
        "websockets": true,
        "skills": {"enableSkillCommands": false, "customDirectories": ["/skills"]},
        "retry": {"maxDelayMs": 1234}
    }));
    let global = manager.get_global_settings_json();
    assert_eq!(global.get("queueMode"), None);
    assert_eq!(global["steeringMode"], "all");
    assert_eq!(global["transport"], "websocket");
    assert_eq!(global["skills"], json!(["/skills"]));
    assert_eq!(global["enableSkillCommands"], false);
    assert_eq!(global["retry"], json!({"provider": {"maxRetryDelayMs": 1234}}));
    assert_eq!(manager.get_steering_mode(), SettingsManagerGetSteeringModeResult::All);
    assert_eq!(manager.get_provider_retry_settings().max_retry_delay_ms, 1234);

    let manager = in_memory(json!({"skills": {"customDirectories": []}, "websockets": false}));
    let global = manager.get_global_settings_json();
    assert_eq!(global.get("skills"), None);
    assert_eq!(global["transport"], "sse");
}

#[test]
fn removes_model_thinking_levels() {
    let dirs = Dirs::new();
    let manager = dirs.manager();
    manager.set_model_thinking_level("anthropic", "claude", ThinkingLevel::High);
    assert_eq!(
        manager.get_model_thinking_level("anthropic", "claude"),
        Some(ThinkingLevel::High)
    );
    assert_eq!(
        read_json(&dirs.global_path()),
        json!({"modelThinkingLevels": {"anthropic/claude": "high"}})
    );
    manager.remove_model_thinking_level("anthropic", "claude");
    assert_eq!(manager.get_model_thinking_level("anthropic", "claude"), None);
    assert_eq!(read_json(&dirs.global_path()), json!({}));
}

#[test]
fn get_settings_converts_to_typed_settings() {
    let manager = in_memory(json!({"theme": "dark", "compaction": {"reserveTokens": "bad"}, "custom": 1}));
    let settings = manager.get_settings();
    assert_eq!(settings.theme.as_deref(), Some("dark"));
    assert!(settings.compaction.is_none());
    assert_eq!(settings.extra.get("custom"), Some(&json!(1)));
}

#[test]
fn writes_files_with_two_space_json() {
    let dirs = Dirs::new();
    dirs.manager().set_theme("dark");
    assert_eq!(
        std::fs::read_to_string(dirs.global_path()).unwrap(),
        "{\n  \"theme\": \"dark\"\n}"
    );
}

// ---------------------------------------------------------------------------
// settings-manager-bug.test.ts
// ---------------------------------------------------------------------------

#[test]
fn preserves_file_changes_to_packages_when_changing_unrelated_setting() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"theme": "dark", "packages": ["npm:pi-mcp-adapter"]}),
    );
    let manager = dirs.manager();
    assert_eq!(manager.get_packages().len(), 1);
    let mut current = read_json(&dirs.global_path());
    current["packages"] = json!([]);
    write_json(&dirs.global_path(), current);
    manager.set_theme("light");
    let saved = read_json(&dirs.global_path());
    assert_eq!(saved["packages"], json!([]));
    assert_eq!(saved["theme"], "light");
}

#[test]
fn preserves_file_changes_to_extensions_when_changing_unrelated_setting() {
    let dirs = Dirs::new();
    write_json(
        &dirs.global_path(),
        json!({"theme": "dark", "extensions": ["/old/extension.ts"]}),
    );
    let manager = dirs.manager();
    let mut current = read_json(&dirs.global_path());
    current["extensions"] = json!(["/new/extension.ts"]);
    write_json(&dirs.global_path(), current);
    manager.set_default_thinking_level(ThinkingLevel::High);
    assert_eq!(
        read_json(&dirs.global_path())["extensions"],
        json!(["/new/extension.ts"])
    );
}

#[test]
fn preserves_external_project_changes_when_updating_unrelated_project_field() {
    let dirs = Dirs::new();
    write_json(
        &dirs.project_path(),
        json!({"extensions": ["./old-extension.ts"], "prompts": ["./old-prompt.md"]}),
    );
    let manager = dirs.manager();
    let mut current = read_json(&dirs.project_path());
    current["prompts"] = json!(["./new-prompt.md"]);
    write_json(&dirs.project_path(), current);
    manager
        .set_project_extension_paths(&["./updated-extension.ts".to_string()])
        .unwrap();
    let saved = read_json(&dirs.project_path());
    assert_eq!(saved["prompts"], json!(["./new-prompt.md"]));
    assert_eq!(saved["extensions"], json!(["./updated-extension.ts"]));
}

#[test]
fn in_memory_project_changes_override_external_changes() {
    let dirs = Dirs::new();
    write_json(
        &dirs.project_path(),
        json!({"extensions": ["./initial-extension.ts"]}),
    );
    let manager = dirs.manager();
    let mut current = read_json(&dirs.project_path());
    current["extensions"] = json!(["./external-extension.ts"]);
    write_json(&dirs.project_path(), current);
    manager
        .set_project_extension_paths(&["./in-memory-extension.ts".to_string()])
        .unwrap();
    assert_eq!(
        read_json(&dirs.project_path())["extensions"],
        json!(["./in-memory-extension.ts"])
    );
}

// ---------------------------------------------------------------------------
// settings-manager-compaction.test.ts
// ---------------------------------------------------------------------------

const MODEL_KEY: &str = "provider/family/model";

fn model() -> SettingsManagerGetCompactionSettingsModel {
    SettingsManagerGetCompactionSettingsModel::new("provider", "family/model")
}

fn compaction_defaults() -> SettingsManagerGetCompactionSettingsResult {
    SettingsManagerGetCompactionSettingsResult {
        enabled: true,
        reserve_tokens: 16384,
        keep_recent_tokens: 20000,
    }
}

fn compaction(enabled: bool, reserve: u64, keep: u64) -> SettingsManagerGetCompactionSettingsResult {
    SettingsManagerGetCompactionSettingsResult {
        enabled,
        reserve_tokens: reserve,
        keep_recent_tokens: keep,
    }
}

#[test]
fn compaction_uses_defaults_without_settings() {
    let manager = SettingsManager::in_memory(None, None);
    assert_eq!(manager.get_compaction_settings(None).unwrap(), compaction_defaults());
    assert_eq!(
        manager.get_compaction_settings(Some(&model())).unwrap(),
        compaction_defaults()
    );
}

#[test]
fn compaction_resolves_each_field_independently() {
    let manager = in_memory(json!({"compaction": {
        "reserveTokens": 8192,
        "keepRecentTokens": 10000,
        "modelOverrides": {MODEL_KEY: {"reserveTokens": 400000}}
    }}));
    assert_eq!(
        manager.get_compaction_settings(Some(&model())).unwrap(),
        compaction(true, 400000, 10000)
    );
    assert_eq!(manager.get_compaction_reserve_tokens(Some(&model())).unwrap(), 400000);
    assert_eq!(manager.get_compaction_keep_recent_tokens(Some(&model())).unwrap(), 10000);
    assert_eq!(
        manager.get_compaction_settings(None).unwrap(),
        compaction(true, 8192, 10000)
    );

    manager.apply_overrides_json(
        json!({"compaction": {"modelOverrides": {MODEL_KEY: {"keepRecentTokens": 30000}}}})
            .as_object()
            .unwrap(),
    );
    assert_eq!(manager.get_compaction_keep_recent_tokens(Some(&model())).unwrap(), 30000);
    assert_eq!(manager.get_compaction_reserve_tokens(Some(&model())).unwrap(), 400000);
}

#[test]
fn compaction_falls_back_to_defaults_for_missing_fields() {
    let manager = in_memory(
        json!({"compaction": {"modelOverrides": {MODEL_KEY: {"keepRecentTokens": 1024}}}}),
    );
    assert_eq!(
        manager.get_compaction_settings(Some(&model())).unwrap(),
        compaction(true, 16384, 1024)
    );
}

#[test]
fn compaction_matches_exact_ids() {
    let manager = in_memory(json!({"compaction": {"modelOverrides": {
        MODEL_KEY: {"reserveTokens": 400000},
        "provider/*": {"reserveTokens": 1},
        "family/model": {"reserveTokens": 2}
    }}}));
    assert_eq!(manager.get_compaction_reserve_tokens(Some(&model())).unwrap(), 400000);
    for other in [
        SettingsManagerGetCompactionSettingsModel::new("other", "family/model"),
        SettingsManagerGetCompactionSettingsModel::new("provider", "other"),
        SettingsManagerGetCompactionSettingsModel::new("provider", "family/Model"),
    ] {
        assert_eq!(
            manager.get_compaction_settings(Some(&other)).unwrap(),
            compaction_defaults()
        );
    }
}

#[tokio::test]
async fn compaction_merges_project_overrides_per_field() {
    let storage = storage_with(
        Some(json!({"compaction": {
            "reserveTokens": 8192,
            "modelOverrides": {
                MODEL_KEY: {"reserveTokens": 400000, "keepRecentTokens": 30000},
                "provider/other": {"keepRecentTokens": 4096}
            }
        }})),
        Some(json!({"compaction": {"reserveTokens": 1024, "modelOverrides": {MODEL_KEY: {"keepRecentTokens": 2000}}}})),
    );
    let manager = SettingsManager::from_storage(storage, None);
    assert_eq!(
        manager.get_compaction_settings(Some(&model())).unwrap(),
        compaction(true, 400000, 2000)
    );
    assert_eq!(
        manager
            .get_compaction_settings(Some(&SettingsManagerGetCompactionSettingsModel::new(
                "provider", "other"
            )))
            .unwrap(),
        compaction(true, 1024, 4096)
    );
    manager.reload().await.unwrap();
    assert_eq!(manager.get_compaction_keep_recent_tokens(Some(&model())).unwrap(), 2000);
    manager.set_project_trusted(false);
    assert_eq!(manager.get_compaction_keep_recent_tokens(Some(&model())).unwrap(), 30000);
}

#[tokio::test]
async fn compaction_toggle_keeps_overrides() {
    let storage = storage_with(
        Some(json!({"compaction": {"modelOverrides": {MODEL_KEY: {"enabled": false, "reserveTokens": 400000}}}})),
        None,
    );
    let manager = SettingsManager::from_storage(storage, None);
    assert!(manager.get_compaction_settings(Some(&model())).unwrap().enabled);
    manager.set_compaction_enabled(false);
    manager.flush().await.unwrap();
    manager.reload().await.unwrap();
    assert_eq!(
        manager.get_compaction_settings(Some(&model())).unwrap(),
        compaction(false, 400000, 20000)
    );
}

fn invalid_token_values() -> Vec<(Value, &'static str)> {
    vec![
        (Value::Null, "null"),
        (json!(-1), "-1"),
        (json!(1.5), "1.5"),
        (json!("400000"), "400000"),
        (json!(true), "true"),
        (json!({}), "[object Object]"),
        (json!([]), ""),
        (json!(9007199254740992u64), "9007199254740992"),
    ]
}

#[test]
fn compaction_reports_invalid_model_override_values() {
    for field in ["reserveTokens", "keepRecentTokens"] {
        for (value, string) in invalid_token_values() {
            let storage = storage_with(
                Some(json!({"compaction": {"modelOverrides": {MODEL_KEY: {field: value}}}})),
                None,
            );
            let manager = SettingsManager::from_storage(storage, None);
            let err = manager.get_compaction_settings(Some(&model())).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!(
                    "Invalid compaction.modelOverrides[\"{MODEL_KEY}\"].{field} setting: {string}. Expected a non-negative safe integer."
                )
            );
            assert_eq!(manager.get_compaction_settings(None).unwrap(), compaction_defaults());
            assert_eq!(
                manager
                    .get_compaction_settings(Some(&SettingsManagerGetCompactionSettingsModel::new(
                        "other",
                        "family/model"
                    )))
                    .unwrap(),
                compaction_defaults()
            );
        }
    }
}

#[test]
fn compaction_reports_invalid_ordinary_values() {
    for field in ["reserveTokens", "keepRecentTokens"] {
        for (value, string) in invalid_token_values() {
            let storage = storage_with(
                Some(json!({"compaction": {field: value, "modelOverrides": {MODEL_KEY: {field: 4096}}}})),
                None,
            );
            let manager = SettingsManager::from_storage(storage, None);
            let expected = format!(
                "Invalid compaction.{field} setting: {string}. Expected a non-negative safe integer."
            );
            assert_eq!(
                manager.get_compaction_settings(None).unwrap_err().to_string(),
                expected
            );
            assert_eq!(
                manager
                    .get_compaction_settings(Some(&model()))
                    .unwrap_err()
                    .to_string(),
                expected
            );
        }
    }
}

#[test]
fn compaction_reports_malformed_model_entries() {
    for (entry, string) in [
        (Value::Null, "null"),
        (json!(false), "false"),
        (json!(42), "42"),
        (json!("invalid"), "invalid"),
        (json!([]), ""),
    ] {
        let storage = storage_with(
            Some(json!({"compaction": {"modelOverrides": {MODEL_KEY: entry}}})),
            None,
        );
        let err = SettingsManager::from_storage(storage, None)
            .get_compaction_settings(Some(&model()))
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Invalid compaction.modelOverrides[\"{MODEL_KEY}\"] setting: {string}. Expected an object."
            )
        );
    }
}

#[test]
fn compaction_accepts_zero() {
    let manager = in_memory(json!({"compaction": {"reserveTokens": 0, "keepRecentTokens": 0}}));
    assert_eq!(
        manager.get_compaction_settings(Some(&model())).unwrap(),
        compaction(true, 0, 0)
    );
    manager.apply_overrides_json(
        json!({"compaction": {
            "reserveTokens": 1000,
            "keepRecentTokens": 1000,
            "modelOverrides": {MODEL_KEY: {"reserveTokens": 0, "keepRecentTokens": 0}}
        }})
        .as_object()
        .unwrap(),
    );
    assert_eq!(
        manager.get_compaction_settings(Some(&model())).unwrap(),
        compaction(true, 0, 0)
    );
}

#[test]
fn tool_list_helpers() {
    assert_eq!(get_tool_list_error(&["read".into(), "bash".into()]), None);
    assert_eq!(
        get_tool_list_error(&["read".into(), "+bash".into()]).as_deref(),
        Some("tool names cannot be mixed with +name or -name entries")
    );
    assert_eq!(
        get_tool_list_error(&["+mcp_*".into()]).as_deref(),
        Some("+name and -name entries take exact tool names, not patterns: +mcp_*")
    );
    assert_eq!(
        apply_tool_modifiers(
            &["read".into(), "bash".into()],
            &["-bash".into(), "+grep".into(), "+".into(), "plain".into()]
        ),
        vec!["read".to_string(), "grep".to_string()]
    );
}

// suite/regressions/8337-utf8-bom-parsing.test.ts
#[tokio::test]
async fn loads_frontmatter_and_settings_with_a_leading_bom() {
    let split = crate::utils::text::split_bom("\u{FEFF}content");
    assert_eq!((split.bom.as_str(), split.text.as_str()), ("\u{FEFF}", "content"));
    let parsed = crate::utils::frontmatter::parse_frontmatter("\u{FEFF}---\nname: demo\ndescription: Test\n---\nBody").unwrap();
    assert_eq!(parsed.frontmatter, json!({ "name": "demo", "description": "Test" }));
    assert_eq!(parsed.body, "Body");

    let dirs = Dirs::new();
    std::fs::write(dirs.global_path(), format!("\u{FEFF}{}", json!({ "defaultModel": "global-model" }))).unwrap();
    std::fs::write(dirs.project_path(), format!("\u{FEFF}{}", json!({ "defaultProvider": "project-provider" }))).unwrap();
    let settings = dirs.manager();
    assert_eq!(settings.get_default_model().as_deref(), Some("global-model"));
    assert_eq!(settings.get_default_provider().as_deref(), Some("project-provider"));

    settings.set_theme("dark");
    settings.flush().await.unwrap();
    assert!(!std::fs::read_to_string(dirs.global_path()).unwrap().starts_with('\u{FEFF}'));
}

// suite/regressions/7572-provider-retry-settings-merge.test.ts
#[test]
fn preserves_global_provider_retry_settings_not_overridden_by_the_project() {
    let storage = storage_with(
        Some(json!({ "retry": { "provider": { "timeoutMs": 30000, "maxRetryDelayMs": 45000 } } })),
        Some(json!({ "retry": { "provider": { "maxRetries": 2 } } })),
    );
    let settings = SettingsManager::from_storage(storage, None);
    let retry = settings.get_provider_retry_settings();
    assert_eq!(retry.timeout_ms, Some(30000));
    assert_eq!(retry.max_retries, Some(2));
    assert_eq!(retry.max_retry_delay_ms, 45000);
}

// suite/regressions/3616-settings-inmemory-reload.test.ts
fn in_memory_initial() -> Arc<SettingsManager> {
    in_memory(json!({
        "defaultThinkingLevel": "high",
        "images": { "autoResize": false },
        "compaction": { "enabled": false },
    }))
}

#[tokio::test]
async fn in_memory_settings_survive_direct_reload() {
    let settings = in_memory_initial();
    settings.reload().await.unwrap();
    assert_eq!(settings.get_default_thinking_level(), Some(ThinkingLevel::High));
    assert!(!settings.get_image_auto_resize());
    assert!(!settings.get_compaction_enabled());
    assert_eq!(
        Value::Object(settings.get_global_settings_json()),
        json!({
            "defaultThinkingLevel": "high",
            "images": { "autoResize": false },
            "compaction": { "enabled": false },
        })
    );
}

#[tokio::test]
async fn in_memory_settings_survive_resource_loader_reload() {
    use crate::core::resource_loader::{DefaultResourceLoader, DefaultResourceLoaderOptions, ResourceLoader};
    let settings = in_memory_initial();
    let dirs = Dirs::new();
    let loader = DefaultResourceLoader::new(&DefaultResourceLoaderOptions {
        cwd: dirs.project_dir.to_string_lossy().into_owned(),
        agent_dir: dirs.agent_dir.to_string_lossy().into_owned(),
        settings_manager: Some(settings.clone()),
        no_extensions: Some(true),
        no_skills: Some(true),
        no_prompt_templates: Some(true),
        no_themes: Some(true),
        no_context_files: Some(true),
        ..Default::default()
    });
    loader.reload(None).await.unwrap();
    assert_eq!(settings.get_default_thinking_level(), Some(ThinkingLevel::High));
    assert!(!settings.get_image_auto_resize());
    assert!(!settings.get_compaction_enabled());
}

#[tokio::test]
async fn in_memory_settings_survive_an_unrelated_setter_flush_and_reload() {
    let settings = in_memory(json!({ "images": { "autoResize": false }, "compaction": { "enabled": false } }));
    settings.set_theme("dark");
    settings.flush().await.unwrap();
    settings.reload().await.unwrap();
    assert_eq!(settings.get_theme().as_deref(), Some("dark"));
    assert!(!settings.get_image_auto_resize());
    assert!(!settings.get_compaction_enabled());
    assert_eq!(
        Value::Object(settings.get_global_settings_json()),
        json!({ "images": { "autoResize": false }, "compaction": { "enabled": false }, "theme": "dark" })
    );
}
