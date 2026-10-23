//! SDK classes of `@earendil-works/pi-coding-agent` for JavaScript extensions, backed by the Rust
//! implementations: `SettingsManager` (constructed with its static factories).

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Value, json};

use super::Bridge;
use super::api::{arg, str_arg};
use crate::compat::WheelScrollLines;
use crate::core::settings_manager::*;
use crate::modes::interactive::components::tree_selector::FilterMode;
use pi_agent::types::ThinkingLevel;
use pi_tui::components::scroll_view::ScrollViewScrollbar;

type JsonMap = serde_json::Map<String, Value>;

pub(crate) fn dispatch(bridge: &Arc<Bridge>, op: &str, payload: Value) -> anyhow::Result<Value> {
    match op {
        "settings" => settings_call(bridge, &payload),
        "settings.static" => settings_static(bridge, &payload),
        _ => Err(anyhow::anyhow!("unknown host call {op}")),
    }
}

pub(crate) async fn dispatch_async(bridge: Arc<Bridge>, op: &str, payload: Value) -> anyhow::Result<Value> {
    match op {
        "settings.async" => settings_async(&bridge, &payload).await,
        _ => Err(anyhow::anyhow!("unknown async host call {op}")),
    }
}

fn settings_of(bridge: &Bridge, payload: &Value) -> anyhow::Result<Arc<SettingsManager>> {
    bridge
        .settings_managers
        .get(payload["id"].as_u64().unwrap_or_default())
        .ok_or_else(|| anyhow::anyhow!("This settings manager is no longer available"))
}

/// `SettingsManager.create()` / `inMemory()` / `fromStorage()`: the new manager is owned by its JS
/// object (released when it is collected).
fn settings_static(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let method = str_arg(payload, "m");
    let options: Option<SettingsManagerCreateOptions> = super::api::opt(&payload["options"])?;
    let manager = match method.as_str() {
        "create" => SettingsManager::create(
            payload["cwd"].as_str().unwrap_or(&bridge.cwd),
            payload["agentDir"].as_str(),
            options.as_ref(),
        ),
        "inMemory" => SettingsManager::in_memory_json(
            payload["settings"].as_object().cloned().unwrap_or_default(),
            options.as_ref(),
        ),
        "fromStorage" => {
            let storage = bridge
                .handle(&payload["storage"])
                .ok_or_else(|| anyhow::anyhow!("SettingsManager.fromStorage() expects a storage object"))?;
            SettingsManager::from_storage(Arc::new(JsSettingsStorage { bridge: Arc::downgrade(bridge), storage }), options.as_ref())
        }
        "release" => {
            bridge.settings_managers.remove(payload["id"].as_u64().unwrap_or_default());
            return Ok(Value::Null);
        }
        _ => return Err(anyhow::anyhow!("SettingsManager.{method} is not a function")),
    };
    Ok(json!(bridge.settings_managers.insert(manager)))
}

/// A JavaScript `SettingsStorage` (`{ withLock(scope, fn) }`).
struct JsSettingsStorage {
    bridge: std::sync::Weak<Bridge>,
    storage: pi_jsrt::JsHandle,
}

/// The lock callback for the duration of one `withLock` call into JavaScript.
struct LockFn(*mut (dyn FnMut(Option<String>) -> anyhow::Result<Option<String>> + 'static));
// SAFETY: the pointer is only dereferenced while `with_lock` is running (it is cleared before
// `with_lock` returns), during which the callback it points to is alive and not used elsewhere.
unsafe impl Send for LockFn {}
unsafe impl Sync for LockFn {}

impl SettingsStorage for JsSettingsStorage {
    fn with_lock(&self, scope: SettingsScope, f: SettingsLockFn<'_>) -> anyhow::Result<()> {
        let bridge = self.bridge.upgrade().ok_or_else(|| anyhow::anyhow!("extension runtime stopped"))?;
        let ptr: *mut (dyn FnMut(Option<String>) -> anyhow::Result<Option<String>> + '_) = f;
        // SAFETY: lifetime erasure only; see `LockFn`.
        let ptr: *mut (dyn FnMut(Option<String>) -> anyhow::Result<Option<String>> + 'static) =
            unsafe { std::mem::transmute(ptr) };
        let slot = Arc::new(Mutex::new(Some(LockFn(ptr))));
        let callback_slot = slot.clone();
        let id = bridge.register_callback(Arc::new(move |args: Vec<Value>| {
            let guard = callback_slot.lock();
            let Some(f) = guard.as_ref() else {
                anyhow::bail!("withLock callback called after withLock returned");
            };
            let current = args.first().and_then(Value::as_str).map(str::to_string);
            // SAFETY: see `LockFn`.
            let next = unsafe { (*f.0)(current) }?;
            Ok(json!(next))
        }));
        let scope = serde_json::to_value(scope)?;
        let result = bridge.glue_call_blocking("invokeWithLock", vec![self.storage.to_value(), scope, json!(id)]);
        *slot.lock() = None;
        bridge.callbacks.remove(id);
        result.map(|_| ())
    }
}

fn settings_error_to_js(error: &SettingsError) -> Value {
    let mut out = json!({ "scope": serde_json::to_value(error.scope).unwrap_or(Value::Null) });
    if let Some(path) = &error.path {
        out["path"] = json!(path);
    }
    out["error"] = json!({ "message": crate::core::extensions::runner::error_parts(&error.error).0 });
    out
}

fn capability_overrides_to_js(overrides: &pi_tui::terminal_image::CapabilityOverrides) -> Value {
    let mut out = serde_json::Map::new();
    match overrides.images {
        Some(Some(pi_tui::terminal_image::ImageProtocol::Kitty)) => {
            out.insert("images".into(), json!("kitty"));
        }
        Some(Some(pi_tui::terminal_image::ImageProtocol::Iterm2)) => {
            out.insert("images".into(), json!("iterm2"));
        }
        Some(None) => {
            out.insert("images".into(), Value::Null);
        }
        None => {}
    }
    if let Some(v) = overrides.true_color {
        out.insert("trueColor".into(), json!(v));
    }
    if let Some(v) = overrides.hyperlinks {
        out.insert("hyperlinks".into(), json!(v));
    }
    Value::Object(out)
}

/// Methods of a `SettingsManager` instance (TS names, Rust implementations).
fn settings_call(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let sm = settings_of(bridge, payload)?;
    let method = str_arg(payload, "m");
    let s = |i: usize| arg(payload, i).as_str().unwrap_or_default().to_string();
    let os = |i: usize| arg(payload, i).as_str().map(str::to_string);
    let b = |i: usize| arg(payload, i).as_bool().unwrap_or(false);
    let n = |i: usize| arg(payload, i).as_f64().unwrap_or_default() as i64;
    fn de<T: serde::de::DeserializeOwned>(payload: &Value, i: usize) -> anyhow::Result<T> {
        serde_json::from_value(arg(payload, i)).map_err(|e| anyhow::anyhow!("Invalid argument: {e}"))
    }
    Ok(match method.as_str() {
        "getModelThinkingLevel" => serde_json::to_value(sm.get_model_thinking_level(&s(0), &s(1)))?,
        "getCompactionReserveTokens" => serde_json::to_value(sm.get_compaction_reserve_tokens(de::<Option<SettingsManagerGetCompactionSettingsModel>>(payload, 0)?.as_ref())?)?,
        "getCompactionKeepRecentTokens" => serde_json::to_value(sm.get_compaction_keep_recent_tokens(de::<Option<SettingsManagerGetCompactionSettingsModel>>(payload, 0)?.as_ref())?)?,
        "getCompactionSettings" => serde_json::to_value(sm.get_compaction_settings(de::<Option<SettingsManagerGetCompactionSettingsModel>>(payload, 0)?.as_ref())?)?,
        "drainErrors" => Value::Array(sm.drain_errors().iter().map(settings_error_to_js).collect()),
        "getTerminalCapabilityOverrides" => capability_overrides_to_js(&sm.get_terminal_capability_overrides()),
        "getSettings" => serde_json::to_value(sm.get_settings_json())?,
        "getGlobalSettings" => serde_json::to_value(sm.get_global_settings_json())?,
        "getProjectSettings" => serde_json::to_value(sm.get_project_settings_json())?,
        "isProjectTrusted" => serde_json::to_value(sm.is_project_trusted())?,
        "setProjectTrusted" => { sm.set_project_trusted(b(0)); Value::Null },
        "applyOverrides" => { sm.apply_overrides_json(&de::<JsonMap>(payload, 0)?); Value::Null },
        "getLastChangelogVersion" => serde_json::to_value(sm.get_last_changelog_version())?,
        "setLastChangelogVersion" => { sm.set_last_changelog_version(&s(0)); Value::Null },
        "getSessionDir" => serde_json::to_value(sm.get_session_dir())?,
        "getDefaultProvider" => serde_json::to_value(sm.get_default_provider())?,
        "getDefaultModel" => serde_json::to_value(sm.get_default_model())?,
        "setDefaultProvider" => { sm.set_default_provider(&s(0)); Value::Null },
        "setDefaultModel" => { sm.set_default_model(&s(0)); Value::Null },
        "setDefaultModelAndProvider" => { sm.set_default_model_and_provider(&s(0), &s(1)); Value::Null },
        "getSteeringMode" => serde_json::to_value(sm.get_steering_mode())?,
        "setSteeringMode" => { sm.set_steering_mode(de::<SettingsManagerGetSteeringModeResult>(payload, 0)?); Value::Null },
        "getFollowUpMode" => serde_json::to_value(sm.get_follow_up_mode())?,
        "setFollowUpMode" => { sm.set_follow_up_mode(de::<SettingsManagerGetSteeringModeResult>(payload, 0)?); Value::Null },
        "getThemeSetting" => serde_json::to_value(sm.get_theme_setting())?,
        "getTheme" => serde_json::to_value(sm.get_theme())?,
        "setTheme" => { sm.set_theme(&s(0)); Value::Null },
        "getDefaultThinkingLevel" => serde_json::to_value(sm.get_default_thinking_level())?,
        "setDefaultThinkingLevel" => { sm.set_default_thinking_level(de::<ThinkingLevel>(payload, 0)?); Value::Null },
        "getAllModelThinkingLevels" => serde_json::to_value(sm.get_all_model_thinking_levels())?,
        "setModelThinkingLevel" => { sm.set_model_thinking_level(&s(0), &s(1), de::<ThinkingLevel>(payload, 2)?); Value::Null },
        "removeModelThinkingLevel" => { sm.remove_model_thinking_level(&s(0), &s(1)); Value::Null },
        "getTransport" => serde_json::to_value(sm.get_transport())?,
        "setTransport" => { sm.set_transport(&de::<TransportSetting>(payload, 0)?); Value::Null },
        "getSessionShareDestination" => serde_json::to_value(sm.get_session_share_destination())?,
        "setSessionShareDestination" => { sm.set_session_share_destination(&de::<SessionShareDestination>(payload, 0)?); Value::Null },
        "getCompactionEnabled" => serde_json::to_value(sm.get_compaction_enabled())?,
        "setCompactionEnabled" => { sm.set_compaction_enabled(b(0)); Value::Null },
        "getBranchSummarySettings" => serde_json::to_value(sm.get_branch_summary_settings())?,
        "getBranchSummarySkipPrompt" => serde_json::to_value(sm.get_branch_summary_skip_prompt())?,
        "getRetryEnabled" => serde_json::to_value(sm.get_retry_enabled())?,
        "setRetryEnabled" => { sm.set_retry_enabled(b(0)); Value::Null },
        "getRetrySettings" => serde_json::to_value(sm.get_retry_settings())?,
        "getHttpIdleTimeoutMs" => serde_json::to_value(sm.get_http_idle_timeout_ms()?)?,
        "setHttpIdleTimeoutMs" => { sm.set_http_idle_timeout_ms(n(0))?; Value::Null },
        "getCacheWarmingMode" => serde_json::to_value(sm.get_cache_warming_mode())?,
        "setCacheWarmingMode" => { sm.set_cache_warming_mode(&de::<CacheWarmingMode>(payload, 0)?); Value::Null },
        "getProviderRetrySettings" => serde_json::to_value(sm.get_provider_retry_settings())?,
        "getWebSocketConnectTimeoutMs" => serde_json::to_value(sm.get_web_socket_connect_timeout_ms()?)?,
        "getHideThinkingBlock" => serde_json::to_value(sm.get_hide_thinking_block())?,
        "getShowCacheMissNotices" => serde_json::to_value(sm.get_show_cache_miss_notices())?,
        "getExternalEditorCommand" => serde_json::to_value(sm.get_external_editor_command())?,
        "setHideThinkingBlock" => { sm.set_hide_thinking_block(b(0)); Value::Null },
        "setShowCacheMissNotices" => { sm.set_show_cache_miss_notices(b(0)); Value::Null },
        "getShellPath" => serde_json::to_value(sm.get_shell_path())?,
        "setShellPath" => { sm.set_shell_path(os(0).as_deref()); Value::Null },
        "getQuietStartup" => serde_json::to_value(sm.get_quiet_startup())?,
        "setQuietStartup" => { sm.set_quiet_startup(&de::<QuietStartup>(payload, 0)?); Value::Null },
        "getDefaultProjectTrust" => serde_json::to_value(sm.get_default_project_trust())?,
        "setDefaultProjectTrust" => { sm.set_default_project_trust(&de::<DefaultProjectTrust>(payload, 0)?); Value::Null },
        "getShellCommandPrefix" => serde_json::to_value(sm.get_shell_command_prefix())?,
        "setShellCommandPrefix" => { sm.set_shell_command_prefix(os(0).as_deref()); Value::Null },
        "getNpmCommand" => serde_json::to_value(sm.get_npm_command())?,
        "setNpmCommand" => { sm.set_npm_command(de::<Option<Vec<String>>>(payload, 0)?.as_deref()); Value::Null },
        "getCollapseChangelog" => serde_json::to_value(sm.get_collapse_changelog())?,
        "setCollapseChangelog" => { sm.set_collapse_changelog(b(0)); Value::Null },
        "getEnableInstallTelemetry" => serde_json::to_value(sm.get_enable_install_telemetry())?,
        "setEnableInstallTelemetry" => { sm.set_enable_install_telemetry(b(0)); Value::Null },
        "getEnableAnalytics" => serde_json::to_value(sm.get_enable_analytics())?,
        "getTrackingId" => serde_json::to_value(sm.get_tracking_id())?,
        "setEnableAnalytics" => { sm.set_enable_analytics(b(0)); Value::Null },
        "getOrCreateDeviceId" => serde_json::to_value(sm.get_or_create_device_id())?,
        "getPackages" => serde_json::to_value(sm.get_packages())?,
        "setPackages" => { sm.set_packages(&de::<Vec<PackageSource>>(payload, 0)?); Value::Null },
        "setProjectPackages" => { sm.set_project_packages(&de::<Vec<PackageSource>>(payload, 0)?)?; Value::Null },
        "getExtensionPaths" => serde_json::to_value(sm.get_extension_paths())?,
        "setExtensionPaths" => { sm.set_extension_paths(&de::<Vec<String>>(payload, 0)?); Value::Null },
        "setProjectExtensionPaths" => { sm.set_project_extension_paths(&de::<Vec<String>>(payload, 0)?)?; Value::Null },
        "getSkillPaths" => serde_json::to_value(sm.get_skill_paths())?,
        "setSkillPaths" => { sm.set_skill_paths(&de::<Vec<String>>(payload, 0)?); Value::Null },
        "setProjectSkillPaths" => { sm.set_project_skill_paths(&de::<Vec<String>>(payload, 0)?)?; Value::Null },
        "getPromptTemplatePaths" => serde_json::to_value(sm.get_prompt_template_paths())?,
        "setPromptTemplatePaths" => { sm.set_prompt_template_paths(&de::<Vec<String>>(payload, 0)?); Value::Null },
        "setProjectPromptTemplatePaths" => { sm.set_project_prompt_template_paths(&de::<Vec<String>>(payload, 0)?)?; Value::Null },
        "getThemePaths" => serde_json::to_value(sm.get_theme_paths())?,
        "setThemePaths" => { sm.set_theme_paths(&de::<Vec<String>>(payload, 0)?); Value::Null },
        "setProjectThemePaths" => { sm.set_project_theme_paths(&de::<Vec<String>>(payload, 0)?)?; Value::Null },
        "getEnableSkillCommands" => serde_json::to_value(sm.get_enable_skill_commands())?,
        "setEnableSkillCommands" => { sm.set_enable_skill_commands(b(0)); Value::Null },
        "getThinkingBudgets" => serde_json::to_value(sm.get_thinking_budgets())?,
        "getShowImages" => serde_json::to_value(sm.get_show_images())?,
        "setShowImages" => { sm.set_show_images(b(0)); Value::Null },
        "getImageWidthCells" => serde_json::to_value(sm.get_image_width_cells())?,
        "setImageWidthCells" => { sm.set_image_width_cells(n(0)); Value::Null },
        "getClearOnShrink" => serde_json::to_value(sm.get_clear_on_shrink())?,
        "setClearOnShrink" => { sm.set_clear_on_shrink(b(0)); Value::Null },
        "getShowTerminalProgress" => serde_json::to_value(sm.get_show_terminal_progress())?,
        "setShowTerminalProgress" => { sm.set_show_terminal_progress(b(0)); Value::Null },
        "getTuiMode" => serde_json::to_value(sm.get_tui_mode())?,
        "setTuiMode" => { sm.set_tui_mode(&de::<TuiMode>(payload, 0)?); Value::Null },
        "getFullscreenExitOutput" => serde_json::to_value(sm.get_fullscreen_exit_output())?,
        "setFullscreenExitOutput" => { sm.set_fullscreen_exit_output(&de::<FullscreenExitOutput>(payload, 0)?); Value::Null },
        "getFullscreenScrollbar" => json!(match sm.get_fullscreen_scrollbar() {
            ScrollViewScrollbar::Hidden => "hidden",
            ScrollViewScrollbar::Auto => "auto",
            ScrollViewScrollbar::Always => "always",
        }),
        "setFullscreenScrollbar" => {
            sm.set_fullscreen_scrollbar(match arg(payload, 0).as_str() {
                Some("always") => ScrollViewScrollbar::Always,
                Some("auto") => ScrollViewScrollbar::Auto,
                _ => ScrollViewScrollbar::Hidden,
            });
            Value::Null
        }
        "getFullscreenCopyOnSelect" => serde_json::to_value(sm.get_fullscreen_copy_on_select())?,
        "setFullscreenCopyOnSelect" => { sm.set_fullscreen_copy_on_select(b(0)); Value::Null },
        "getFullscreenWheelScrollLines" => serde_json::to_value(sm.get_fullscreen_wheel_scroll_lines())?,
        "setFullscreenWheelScrollLines" => { sm.set_fullscreen_wheel_scroll_lines(&de::<WheelScrollLines>(payload, 0)?); Value::Null },
        "getImageAutoResize" => serde_json::to_value(sm.get_image_auto_resize())?,
        "setImageAutoResize" => { sm.set_image_auto_resize(b(0)); Value::Null },
        "getBlockImages" => serde_json::to_value(sm.get_block_images())?,
        "setBlockImages" => { sm.set_block_images(b(0)); Value::Null },
        "getEnabledModels" => serde_json::to_value(sm.get_enabled_models())?,
        "getDefaultTools" => serde_json::to_value(sm.get_default_tools())?,
        "setEnabledModels" => { sm.set_enabled_models(de::<Option<Vec<String>>>(payload, 0)?.as_deref()); Value::Null },
        "getDoubleEscapeAction" => serde_json::to_value(sm.get_double_escape_action())?,
        "setDoubleEscapeAction" => { sm.set_double_escape_action(de::<SettingsManagerGetDoubleEscapeActionResult>(payload, 0)?); Value::Null },
        "getTreeFilterMode" => serde_json::to_value(sm.get_tree_filter_mode())?,
        "setTreeFilterMode" => { sm.set_tree_filter_mode(de::<FilterMode>(payload, 0)?); Value::Null },
        "getShowHardwareCursor" => serde_json::to_value(sm.get_show_hardware_cursor())?,
        "setShowHardwareCursor" => { sm.set_show_hardware_cursor(b(0)); Value::Null },
        "getEditorPaddingX" => serde_json::to_value(sm.get_editor_padding_x())?,
        "setEditorPaddingX" => { sm.set_editor_padding_x(n(0)); Value::Null },
        "getOutputPad" => serde_json::to_value(sm.get_output_pad())?,
        "setOutputPad" => { sm.set_output_pad(n(0)); Value::Null },
        "getAutocompleteMaxVisible" => serde_json::to_value(sm.get_autocomplete_max_visible())?,
        "setAutocompleteMaxVisible" => { sm.set_autocomplete_max_visible(n(0)); Value::Null },
        "getCodeBlockIndent" => serde_json::to_value(sm.get_code_block_indent())?,
        "getMermaidRenderingMode" => serde_json::to_value(sm.get_mermaid_rendering_mode())?,
        "setMermaidRenderingMode" => { sm.set_mermaid_rendering_mode(&de::<MermaidRenderingMode>(payload, 0)?); Value::Null },
        "getWarnings" => serde_json::to_value(sm.get_warnings())?,
        "setWarnings" => { sm.set_warnings(&de::<WarningSettings>(payload, 0)?); Value::Null },
        _ => return Err(anyhow::anyhow!("settingsManager.{method} is not a function")),
    })
}

async fn settings_async(bridge: &Arc<Bridge>, payload: &Value) -> anyhow::Result<Value> {
    let sm = settings_of(bridge, payload)?;
    let method = str_arg(payload, "m");
    Ok(match method.as_str() {
        "reload" => { sm.reload().await?; Value::Null },
        "flush" => { sm.flush().await?; Value::Null },

        _ => return Err(anyhow::anyhow!("settingsManager.{method} is not a function")),
    })
}
