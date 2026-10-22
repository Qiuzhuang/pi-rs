//! Builders for `chrome.json` (scripts/golden/gen-chrome.mjs).

use std::sync::Arc;

use futures::FutureExt;
use parking_lot::Mutex;
use pi_agent::types::ThinkingLevel;
use pi_ai::auth::extension_oauth_types::OAuthDeviceCodeInfo;
use pi_ai::auth::types::AuthInfoLink;
use pi_ai::types::Model;
use pi_coding_agent::core::agent_session::AgentSessionRoutedModel;
use pi_coding_agent::core::extensions::types::{ContextUsage, WorkingIndicatorOptions};
use pi_coding_agent::core::footer_data_provider::FooterDataProvider;
use pi_coding_agent::core::keybindings::KeybindingsManager;
use pi_coding_agent::core::session_manager::SessionEntry;
use pi_coding_agent::modes::interactive::components::auth_url::AuthUrlComponent;
use pi_coding_agent::modes::interactive::components::bordered_loader::{BorderedLoader, BorderedLoaderOptions};
use pi_coding_agent::modes::interactive::components::custom_editor::{CustomEditor, CustomEditorOptions};
use pi_coding_agent::modes::interactive::components::dynamic_border::DynamicBorder;
use pi_coding_agent::modes::interactive::components::earendil_announcement::EarendilAnnouncementComponent;
use pi_coding_agent::modes::interactive::components::extension_editor::{
    ExtensionEditorComponent, ExtensionEditorOptions,
};
use pi_coding_agent::modes::interactive::components::extension_input::{ExtensionInputComponent, ExtensionInputOptions};
use pi_coding_agent::modes::interactive::components::extension_selector::{
    ExtensionSelectorComponent, ExtensionSelectorOptions,
};
use pi_coding_agent::modes::interactive::components::first_time_setup::{
    FirstTimeSetupComponent, FirstTimeSetupOptions,
};
use pi_coding_agent::modes::interactive::components::footer::{FooterComponent, FooterSession};
use pi_coding_agent::modes::interactive::components::keybinding_hints::{key_hint, key_text, raw_key_hint};
use pi_coding_agent::modes::interactive::components::login_dialog::LoginDialogComponent;
use pi_coding_agent::modes::interactive::components::pi_logo::{pi_logo_lines, pi_wordmark};
use pi_coding_agent::modes::interactive::components::status_indicator::{
    BranchSummaryStatusIndicator, CompactionStatusIndicator, CompactionStatusReason, IdleStatus, RetryStatusIndicator,
    StatusIndicator, WorkingStatusIndicator,
};
use pi_coding_agent::modes::interactive::theme::theme::{get_editor_theme, theme};
use pi_tui::tui::Component;
use serde_json::Value;

use super::harness::*;

fn feed(component: &dyn Component, input: &Value) {
    for key in input.get("keys").and_then(Value::as_array).into_iter().flatten() {
        component.handle_input(key.as_str().unwrap_or(""));
    }
}

fn keybindings() -> Arc<KeybindingsManager> {
    Arc::new(KeybindingsManager::new(None, None))
}

// --- footer --------------------------------------------------------------------------------------

struct FakeSession {
    cwd: String,
    session_name: Option<String>,
    model: Option<Model>,
    thinking_level: ThinkingLevel,
    entries: Vec<SessionEntry>,
    context_usage: Option<ContextUsage>,
    routed: Option<AgentSessionRoutedModel>,
    using_subscription: bool,
}

impl FooterSession for FakeSession {
    fn entries(&self) -> Vec<SessionEntry> {
        self.entries.clone()
    }
    fn entry_count(&self) -> i64 {
        self.entries.len() as i64
    }
    fn session_id(&self) -> String {
        "golden-session".into()
    }
    fn leaf_id(&self) -> Option<String> {
        None
    }
    fn session_name(&self) -> Option<String> {
        self.session_name.clone()
    }
    fn cwd(&self) -> String {
        self.cwd.clone()
    }
    fn model(&self) -> Option<Model> {
        self.model.clone()
    }
    fn thinking_level(&self) -> ThinkingLevel {
        self.thinking_level
    }
    fn routed_model(&self) -> Option<AgentSessionRoutedModel> {
        self.routed.clone()
    }
    fn context_usage(&self) -> Option<ContextUsage> {
        self.context_usage.clone()
    }
    fn is_using_subscription(&self, _provider: &str) -> bool {
        self.using_subscription
    }
}

/// Keeps git fixture directories alive for the test run.
static GIT_DIRS: Mutex<Vec<tempfile::TempDir>> = Mutex::new(Vec::new());

fn footer_data(input: &Value) -> Arc<FooterDataProvider> {
    let provider = match opt_s(input, "branch") {
        Some(branch) => {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".git")).unwrap();
            std::fs::write(dir.path().join(".git/HEAD"), format!("ref: refs/heads/{branch}\n")).unwrap();
            let provider = FooterDataProvider::new(dir.path().to_str().unwrap());
            GIT_DIRS.lock().push(dir);
            provider
        }
        None => FooterDataProvider::new("/home/golden/no-git-here"),
    };
    provider.set_available_provider_count(opt_i(input, "providerCount").unwrap_or(1));
    if let Some(statuses) = input.get("statuses").and_then(Value::as_object) {
        for (k, v) in statuses {
            provider.set_extension_status(k, v.as_str());
        }
    }
    Arc::new(provider)
}

fn model(v: &Value) -> Option<Model> {
    let v = v.as_object()?;
    Some(Model {
        id: v["id"].as_str().unwrap().into(),
        provider: v["provider"].as_str().unwrap().into(),
        reasoning: v["reasoning"].as_bool().unwrap_or(false),
        context_window: v["contextWindow"].as_u64().unwrap_or(0),
        ..Default::default()
    })
}

fn build_footer(input: &Value) -> Built {
    let thinking_level: ThinkingLevel = opt_from(input, "thinkingLevel").unwrap_or(ThinkingLevel::Off);
    let session = FakeSession {
        cwd: s(input, "cwd").into(),
        session_name: opt_s(input, "sessionName").map(str::to_string),
        model: input.get("model").and_then(model),
        thinking_level,
        entries: from(input, "entries"),
        context_usage: opt_from(input, "contextUsage"),
        routed: input.get("routed").filter(|r| !r.is_null()).map(|r| AgentSessionRoutedModel {
            model: Model { id: s(r, "id").into(), ..Default::default() },
            thinking_level: opt_from(r, "thinkingLevel"),
        }),
        using_subscription: b(input, "usingSubscription"),
    };
    let footer = FooterComponent::with_session(Arc::new(session), &footer_data(input));
    if input.get("autoCompact").and_then(Value::as_bool) == Some(false) {
        footer.set_auto_compact_enabled(false);
    }
    component(footer)
}

// --- status indicators ---------------------------------------------------------------------------

fn build_status(input: &Value) -> Built {
    let ui = test_tui();
    let indicator: Arc<StatusIndicator> = match s(input, "type") {
        "working" => {
            let options: Option<WorkingIndicatorOptions> = opt_from(input, "indicator");
            WorkingStatusIndicator::new(ui, s(input, "message"), options.as_ref(), None).indicator()
        }
        "retry" => RetryStatusIndicator::new(
            ui,
            opt_i(input, "attempt").unwrap(),
            opt_i(input, "maxAttempts").unwrap(),
            opt_i(input, "delayMs").unwrap(),
        )
        .indicator(),
        "compaction" => {
            let reason = match s(input, "reason") {
                "manual" => CompactionStatusReason::Manual,
                "overflow" => CompactionStatusReason::Overflow,
                _ => CompactionStatusReason::Threshold,
            };
            CompactionStatusIndicator::new(ui, &reason, None).indicator()
        }
        "branchSummary" => BranchSummaryStatusIndicator::new(ui).indicator(),
        _ => return Built::Render(Box::new(|w| IdleStatus::new().render(w))),
    };
    Built::Render(Box::new(move |w| {
        let mut lines = indicator.render(w);
        lines.push(indicator.render_in_border(w as i64));
        lines.push(indicator.render_spinner_in_border(w as i64));
        lines
    }))
}

// --- editor --------------------------------------------------------------------------------------

fn build_editor(input: &Value) -> Built {
    let editor = CustomEditor::new(
        test_tui(),
        get_editor_theme(),
        keybindings(),
        Some(&CustomEditorOptions {
            embed_working_status: Some(b(input, "embed")),
            padding_x: opt_i(input, "paddingX"),
            ..Default::default()
        }),
    );
    if let Some(text) = opt_s(input, "text") {
        editor.editor().set_text(text);
    }
    if b(input, "focused") {
        editor.set_focused(true);
    }
    feed(&editor, input);
    if let Some(status) = opt_s(input, "status") {
        editor.set_working_status_indicator(Some(WorkingStatusIndicator::new(test_tui(), status, None, None).indicator()));
    }
    component(editor)
}

// --- login dialog --------------------------------------------------------------------------------

fn build_login(input: &Value) -> Built {
    let c = LoginDialogComponent::new(
        test_tui(),
        s(input, "provider"),
        Arc::new(|_, _| {}),
        opt_s(input, "providerName"),
        opt_s(input, "title"),
    );
    for step in input.get("steps").and_then(Value::as_array).into_iter().flatten() {
        if let Some(dc) = step.get("deviceCode") {
            c.show_device_code(OAuthDeviceCodeInfo {
                user_code: s(dc, "userCode").into(),
                verification_uri: s(dc, "verificationUri").into(),
                interval_seconds: None,
                expires_in_seconds: None,
            });
        }
        if let Some(details) = step.get("details") {
            let lines: Vec<String> = serde_json::from_value(details.clone()).unwrap();
            c.show_details(&lines);
        }
        if let Some(info) = step.get("info") {
            let links: Vec<AuthInfoLink> = info["links"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| AuthInfoLink { url: s(l, "url").into(), label: opt_s(l, "label").map(str::to_string) })
                .collect();
            c.show_info(s(info, "message"), Some(&links), Some(b(info, "closeHint")));
        }
        if let Some(waiting) = opt_s(step, "waiting") {
            c.show_waiting(waiting);
        }
        if let Some(progress) = opt_s(step, "progress") {
            c.show_progress(progress);
        }
        if let Some(prompt) = step.get("prompt") {
            // The prompt adds its rows on the first poll, then waits for input.
            let _ = c.show_prompt(s(prompt, "message"), opt_s(prompt, "placeholder")).now_or_never();
        }
        if let Some(manual) = opt_s(step, "manual") {
            let _ = c.show_manual_input(manual).now_or_never();
        }
        if step.get("keys").is_some() {
            feed(&c, step);
        }
    }
    component(c)
}

pub fn build(kind: &str, input: &Value) -> Option<Built> {
    let noop = Arc::new(|| {});
    Some(match kind {
        "footer" => build_footer(input),
        "status_indicator" => build_status(input),
        "dynamic_border" => component(DynamicBorder::new(None)),
        "bordered_loader" => component(BorderedLoader::new(
            test_tui(),
            theme(),
            s(input, "message"),
            Some(&BorderedLoaderOptions { cancellable: Some(b(input, "cancellable")) }),
        )),
        "auth_url" => component(AuthUrlComponent::new(test_tui(), s(input, "url"))),
        "key_hints" => {
            let hints: Vec<Vec<String>> = from(input, "hints");
            Built::Render(Box::new(move |_| {
                hints
                    .iter()
                    .map(|h| match h[0].as_str() {
                        "keyHint" => key_hint(h[1].clone().into(), &h[2]),
                        "keyText" => key_text(h[1].clone().into()),
                        _ => raw_key_hint(&h[1], &h[2]),
                    })
                    .collect()
            }))
        }
        "pi_logo" => Built::Render(Box::new(|_| {
            let (top, bottom) = pi_logo_lines();
            vec![top, bottom, pi_wordmark()]
        })),
        "earendil_announcement" => component(EarendilAnnouncementComponent::new()),
        "extension_selector" => {
            let options: Vec<String> = from(input, "options");
            let c = ExtensionSelectorComponent::new(
                s(input, "title"),
                &options,
                Arc::new(|_| {}),
                noop,
                Some(&ExtensionSelectorOptions {
                    tui: None,
                    timeout: None,
                    on_toggle_tools_expanded: None,
                    description: opt_s(input, "description").map(str::to_string),
                }),
            );
            feed(&c, input);
            component(c)
        }
        "extension_input" => {
            let c = ExtensionInputComponent::new(
                s(input, "title"),
                None,
                Arc::new(|_| {}),
                noop,
                Some(&ExtensionInputOptions {
                    tui: None,
                    timeout: None,
                    initial_value: opt_s(input, "initialValue").map(str::to_string),
                    description: opt_s(input, "description").map(str::to_string),
                }),
            );
            if b(input, "focused") {
                c.set_focused(true);
            }
            feed(&c, input);
            component(c)
        }
        "extension_editor" => {
            let c = ExtensionEditorComponent::new(
                test_tui(),
                keybindings(),
                s(input, "title"),
                opt_s(input, "prefill"),
                Arc::new(|_| {}),
                noop,
                Some(&ExtensionEditorOptions {
                    description: opt_s(input, "description").map(str::to_string),
                    ..Default::default()
                }),
                Some("vi"),
            );
            if b(input, "focused") {
                c.set_focused(true);
            }
            feed(&c, input);
            component(c)
        }
        "custom_editor" => build_editor(input),
        "login_dialog" => build_login(input),
        "first_time_setup" => {
            let c = FirstTimeSetupComponent::new(&FirstTimeSetupOptions {
                on_theme_preview: Arc::new(|_| {}),
                on_submit: Arc::new(|_| {}),
                on_cancel: Arc::new(|| {}),
            });
            feed(&c, input);
            component(c)
        }
        _ => return None,
    })
}
