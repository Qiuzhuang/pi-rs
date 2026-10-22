//! Builders for `selectors.json` (scripts/golden/gen-selectors.mjs).

use std::sync::Arc;

use pi_ai::auth::types::{AuthCheck, AuthType};
use pi_coding_agent::core::trust_manager::ProjectTrustStoreEntry;
use pi_coding_agent::modes::interactive::components::oauth_selector::{
    AuthSelectorProvider, OAuthSelectorComponent, OAuthSelectorComponentMode,
};
use pi_coding_agent::modes::interactive::components::settings_selector::{
    SettingsCallbacks, SettingsConfig, SettingsSelectorComponent,
};
use pi_coding_agent::modes::interactive::components::show_images_selector::ShowImagesSelectorComponent;
use pi_coding_agent::modes::interactive::components::theme_selector::ThemeSelectorComponent;
use pi_coding_agent::modes::interactive::components::thinking_selector::ThinkingSelectorComponent;
use pi_coding_agent::modes::interactive::components::trust_selector::{TrustSelectorComponent, TrustSelectorOptions};
use pi_coding_agent::modes::interactive::components::user_message_selector::{
    UserMessageItem, UserMessageSelectorComponent,
};
use pi_tui::components::scroll_view::ScrollViewScrollbar;
use pi_tui::tui::Component;
use serde_json::Value;

use super::harness::*;

pub fn feed(component: &dyn Component, input: &Value) {
    for key in input.get("keys").and_then(Value::as_array).into_iter().flatten() {
        component.handle_input(key.as_str().unwrap_or(""));
    }
}

fn settings_config(c: &Value) -> SettingsConfig {
    SettingsConfig {
        auto_compact: b(c, "autoCompact"),
        default_model: s(c, "defaultModel").into(),
        current_model: opt_from(c, "currentModel"),
        available_default_models: from(c, "availableDefaultModels"),
        show_images: b(c, "showImages"),
        image_width_cells: opt_i(c, "imageWidthCells").unwrap(),
        auto_resize_images: b(c, "autoResizeImages"),
        block_images: b(c, "blockImages"),
        enable_skill_commands: b(c, "enableSkillCommands"),
        steering_mode: from(c, "steeringMode"),
        follow_up_mode: from(c, "followUpMode"),
        transport: from(c, "transport"),
        session_share_destination: from(c, "sessionShareDestination"),
        http_idle_timeout_ms: opt_i(c, "httpIdleTimeoutMs").unwrap(),
        cache_warming_mode: from(c, "cacheWarmingMode"),
        thinking_level: from(c, "thinkingLevel"),
        available_thinking_levels: from(c, "availableThinkingLevels"),
        model_thinking_levels: from(c, "modelThinkingLevels"),
        current_theme: s(c, "currentTheme").into(),
        terminal_theme: from(c, "terminalTheme"),
        available_themes: from(c, "availableThemes"),
        hide_thinking_block: b(c, "hideThinkingBlock"),
        mermaid_rendering_mode: from(c, "mermaidRenderingMode"),
        show_cache_miss_notices: b(c, "showCacheMissNotices"),
        collapse_changelog: b(c, "collapseChangelog"),
        enable_install_telemetry: b(c, "enableInstallTelemetry"),
        double_escape_action: from(c, "doubleEscapeAction"),
        tree_filter_mode: from(c, "treeFilterMode"),
        show_hardware_cursor: b(c, "showHardwareCursor"),
        editor_padding_x: opt_i(c, "editorPaddingX").unwrap(),
        output_pad: opt_i(c, "outputPad").unwrap(),
        autocomplete_max_visible: opt_i(c, "autocompleteMaxVisible").unwrap(),
        quiet_startup: from(c, "quietStartup"),
        default_project_trust: from(c, "defaultProjectTrust"),
        clear_on_shrink: b(c, "clearOnShrink"),
        show_terminal_progress: b(c, "showTerminalProgress"),
        tui_mode: from(c, "tuiMode"),
        fullscreen_exit_output: from(c, "fullscreenExitOutput"),
        fullscreen_scrollbar: match s(c, "fullscreenScrollbar") {
            "always" => ScrollViewScrollbar::Always,
            "hidden" => ScrollViewScrollbar::Hidden,
            _ => ScrollViewScrollbar::Auto,
        },
        fullscreen_copy_on_select: b(c, "fullscreenCopyOnSelect"),
        fullscreen_wheel_scroll_lines: from(c, "fullscreenWheelScrollLines"),
        warnings: from(c, "warnings"),
    }
}

fn noop_settings_callbacks() -> SettingsCallbacks {
    SettingsCallbacks {
        on_auto_compact_change: Arc::new(|_| {}),
        on_show_images_change: Arc::new(|_| {}),
        on_image_width_cells_change: Arc::new(|_| {}),
        on_auto_resize_images_change: Arc::new(|_| {}),
        on_block_images_change: Arc::new(|_| {}),
        on_enable_skill_commands_change: Arc::new(|_| {}),
        on_steering_mode_change: Arc::new(|_| {}),
        on_follow_up_mode_change: Arc::new(|_| {}),
        on_transport_change: Arc::new(|_| {}),
        on_session_share_destination_change: Arc::new(|_| {}),
        on_http_idle_timeout_ms_change: Arc::new(|_| {}),
        on_cache_warming_mode_change: Arc::new(|_| {}),
        on_model_thinking_level_change: Arc::new(|_, _, _| {}),
        on_model_thinking_level_remove: Arc::new(|_, _| {}),
        on_theme_change: Arc::new(|_| {}),
        on_theme_preview: Some(Arc::new(|_| {})),
        on_hide_thinking_block_change: Arc::new(|_| {}),
        on_mermaid_rendering_mode_change: Arc::new(|_| {}),
        on_show_cache_miss_notices_change: Arc::new(|_| {}),
        on_collapse_changelog_change: Arc::new(|_| {}),
        on_enable_install_telemetry_change: Arc::new(|_| {}),
        on_double_escape_action_change: Arc::new(|_| {}),
        on_tree_filter_mode_change: Arc::new(|_| {}),
        on_show_hardware_cursor_change: Arc::new(|_| {}),
        on_editor_padding_x_change: Arc::new(|_| {}),
        on_output_pad_change: Arc::new(|_| {}),
        on_autocomplete_max_visible_change: Arc::new(|_| {}),
        on_quiet_startup_change: Arc::new(|_| {}),
        on_default_project_trust_change: Arc::new(|_| {}),
        on_clear_on_shrink_change: Arc::new(|_| {}),
        on_show_terminal_progress_change: Arc::new(|_| {}),
        on_tui_mode_change: Arc::new(|_| {}),
        on_fullscreen_exit_output_change: Arc::new(|_| {}),
        on_fullscreen_scrollbar_change: Arc::new(|_| {}),
        on_fullscreen_copy_on_select_change: Arc::new(|_| {}),
        on_fullscreen_wheel_scroll_lines_change: Arc::new(|_| {}),
        on_warnings_change: Arc::new(|_| {}),
        on_cancel: Arc::new(|| {}),
    }
}

fn auth_provider(p: &Value) -> AuthSelectorProvider {
    AuthSelectorProvider {
        id: s(p, "id").into(),
        name: s(p, "name").into(),
        auth_type: from(p, "authType"),
        method: None,
        status: p.get("status").map(|st| AuthCheck {
            source: opt_s(st, "source").map(str::to_string),
            auth_type: serde_json::from_value::<AuthType>(st["type"].clone()).unwrap(),
        }),
        subscription: p.get("subscription").and_then(Value::as_bool),
    }
}

pub fn build(kind: &str, input: &Value) -> Option<Built> {
    Some(match kind {
        "thinking_selector" => {
            let levels: Vec<_> = from(input, "levels");
            let c = ThinkingSelectorComponent::new(
                from(input, "current"),
                &levels,
                Arc::new(|_| {}),
                Arc::new(|| {}),
                b(input, "withDefault").then(|| Arc::new(|_| {}) as Arc<dyn Fn(_) + Send + Sync>),
                opt_from(input, "defaultLevel"),
            );
            feed(&c, input);
            component(c)
        }
        "theme_selector" => {
            let c = ThemeSelectorComponent::new(s(input, "current"), Arc::new(|_| {}), Arc::new(|| {}), Arc::new(|_| {}));
            feed(c.get_select_list().as_ref(), input);
            component(c)
        }
        "show_images_selector" => {
            component(ShowImagesSelectorComponent::new(b(input, "current"), Arc::new(|_| {}), Arc::new(|| {})))
        }
        "trust_selector" => {
            let saved: Option<ProjectTrustStoreEntry> = opt_from(input, "savedDecision");
            let c = TrustSelectorComponent::new(&TrustSelectorOptions {
                cwd: s(input, "cwd").into(),
                saved_decision: saved,
                project_trusted: b(input, "projectTrusted"),
                on_select: Arc::new(|_| {}),
                on_cancel: Arc::new(|| {}),
            });
            feed(&c, input);
            component(c)
        }
        "user_message_selector" => {
            let messages: Vec<UserMessageItem> = from(input, "messages");
            let c = UserMessageSelectorComponent::new(&messages, Arc::new(|_| {}), Arc::new(|| {}), opt_s(input, "initial"));
            feed(c.get_message_list().as_ref(), input);
            component(c)
        }
        "oauth_selector" => {
            let mode: OAuthSelectorComponentMode = from(input, "mode");
            let providers: Vec<_> = input["providers"].as_array().unwrap().iter().map(auth_provider).collect();
            let c = OAuthSelectorComponent::new(mode, &providers, Arc::new(|_, _| {}), Arc::new(|| {}), opt_s(input, "search"));
            feed(&c, input);
            component(c)
        }
        "settings_selector" => {
            let mut config = serde_json::json!({
                "autoCompact": true, "defaultModel": "anthropic/claude-sonnet-4-5", "availableDefaultModels": [],
                "showImages": true, "imageWidthCells": 60, "autoResizeImages": true, "blockImages": false,
                "enableSkillCommands": true, "steeringMode": "one-at-a-time", "followUpMode": "one-at-a-time",
                "transport": "sse", "sessionShareDestination": "auto", "httpIdleTimeoutMs": 300000,
                "cacheWarmingMode": "off", "thinkingLevel": "medium",
                "availableThinkingLevels": ["off", "minimal", "low", "medium", "high"], "modelThinkingLevels": {},
                "currentTheme": "dark", "terminalTheme": "dark", "availableThemes": ["dark", "light"],
                "hideThinkingBlock": false, "mermaidRenderingMode": "streaming", "showCacheMissNotices": false,
                "collapseChangelog": false, "enableInstallTelemetry": true, "doubleEscapeAction": "tree",
                "treeFilterMode": "default", "showHardwareCursor": false, "editorPaddingX": 0, "outputPad": 1,
                "autocompleteMaxVisible": 5, "quietStartup": false, "defaultProjectTrust": "ask",
                "clearOnShrink": false, "showTerminalProgress": true, "tuiMode": "regular",
                "fullscreenExitOutput": "transcript", "fullscreenScrollbar": "auto", "fullscreenCopyOnSelect": true,
                "fullscreenWheelScrollLines": 7, "warnings": {}
            });
            for (k, v) in input["config"].as_object().unwrap() {
                config[k] = v.clone();
            }
            let c = SettingsSelectorComponent::new(&settings_config(&config), &noop_settings_callbacks());
            feed(c.get_settings_list().as_ref(), input);
            component(c)
        }
        _ => return None,
    })
}
