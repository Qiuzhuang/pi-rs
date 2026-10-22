//! Port of `packages/coding-agent/src/cli/project-trust.ts`.

use std::sync::Arc;

use serde_json::Value;

use crate::cli::startup_ui::{ShowStartupSelectorOptions, show_startup_input, show_startup_selector};
use crate::core::extensions::types::*;
use crate::core::project_trust::AppMode;
use crate::core::settings_manager::SettingsManager;
use crate::modes::interactive::theme::theme::Theme;
use crate::utils::chalk;

#[derive(Clone)]
pub struct CreateProjectTrustContextOptions {
    pub cwd: String,
    pub mode: AppMode,
    pub settings_manager: Arc<SettingsManager>,
    pub has_ui: bool,
}

/// The startup UI offered to `project_trust` handlers: dialogs run as standalone startup TUIs
/// in interactive mode; everything else is a no-op (TS exposes only select/confirm/input/notify).
struct StartupProjectTrustUI {
    mode: AppMode,
    settings_manager: Arc<SettingsManager>,
    has_ui: bool,
}

impl StartupProjectTrustUI {
    fn dialogs_available(&self) -> bool {
        self.has_ui && self.mode == AppMode::Interactive
    }
}

#[async_trait::async_trait]
impl ExtensionUIContext for StartupProjectTrustUI {
    async fn select(&self, title: &str, options: Vec<String>, _opts: ExtensionUIDialogOptions) -> Option<String> {
        if !self.dialogs_available() {
            return None;
        }
        let entries: Vec<ShowStartupSelectorOptions> = options
            .iter()
            .map(|option| ShowStartupSelectorOptions { label: option.clone(), value: Value::String(option.clone()) })
            .collect();
        show_startup_selector(self.settings_manager.clone(), title, &entries)
            .await
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(str::to_string))
    }

    async fn confirm(&self, title: &str, message: &str, _opts: ExtensionUIDialogOptions) -> bool {
        if !self.dialogs_available() {
            return false;
        }
        let entries = [
            ShowStartupSelectorOptions { label: "Yes".into(), value: Value::Bool(true) },
            ShowStartupSelectorOptions { label: "No".into(), value: Value::Bool(false) },
        ];
        show_startup_selector(self.settings_manager.clone(), &format!("{title}\n{message}"), &entries)
            .await
            .ok()
            .flatten()
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    async fn input(&self, title: &str, placeholder: Option<&str>, _opts: ExtensionUIDialogOptions) -> Option<String> {
        if !self.dialogs_available() {
            return None;
        }
        show_startup_input(self.settings_manager.clone(), title, placeholder).await.ok().flatten()
    }

    fn notify(&self, message: &str, kind: NotifyType) {
        if self.mode != AppMode::Interactive {
            let text = match kind {
                NotifyType::Error => chalk::red(message),
                NotifyType::Warning => chalk::yellow(message),
                NotifyType::Info => chalk::cyan(message),
            };
            eprintln!("{text}");
        }
    }

    fn on_terminal_input(&self, _handler: TerminalInputHandler) -> Unsubscribe {
        Arc::new(|| {})
    }
    fn set_status(&self, _key: &str, _text: Option<&str>) {}
    fn set_working_message(&self, _message: Option<&str>) {}
    fn set_working_visible(&self, _visible: bool) {}
    fn set_working_indicator(&self, _options: Option<WorkingIndicatorOptions>) {}
    fn set_hidden_thinking_label(&self, _label: Option<&str>) {}
    fn set_widget(&self, _key: &str, _content: Option<WidgetContent>, _options: ExtensionWidgetOptions) {}
    fn set_footer(&self, _factory: Option<FooterFactory>) {}
    fn set_header(&self, _factory: Option<ComponentFactory>) {}
    fn set_title(&self, _title: &str) {}
    async fn custom(&self, _factory: CustomComponentFactory, _options: CustomUiOptions) -> anyhow::Result<CustomUiResult> {
        Ok(Box::new(()))
    }
    fn paste_to_editor(&self, _text: &str) {}
    fn set_editor_text(&self, _text: &str) {}
    fn get_editor_text(&self) -> String {
        String::new()
    }
    async fn editor(&self, _title: &str, _prefill: Option<&str>) -> Option<String> {
        None
    }
    fn add_autocomplete_provider(&self, _factory: AutocompleteProviderFactory) {}
    fn set_editor_component(&self, _factory: Option<EditorFactory>) {}
    fn get_editor_component(&self) -> Option<EditorFactory> {
        None
    }
    fn theme(&self) -> Arc<Theme> {
        crate::modes::interactive::theme::theme::theme()
    }
    fn get_all_themes(&self) -> Vec<ThemeInfo> {
        Vec::new()
    }
    fn get_theme(&self, _name: &str) -> Option<Arc<Theme>> {
        None
    }
    fn set_theme(&self, _theme: ThemeSelection) -> SetThemeResult {
        SetThemeResult { success: false, error: Some("UI not available".to_string()) }
    }
    fn get_tools_expanded(&self) -> bool {
        false
    }
    fn set_tools_expanded(&self, _expanded: bool) {}
}

pub fn create_project_trust_context(options: &CreateProjectTrustContextOptions) -> ProjectTrustContext {
    ProjectTrustContext {
        cwd: options.cwd.clone(),
        mode: match options.mode {
            AppMode::Interactive => ExtensionMode::Tui,
            AppMode::Print => ExtensionMode::Print,
            AppMode::Json => ExtensionMode::Json,
            AppMode::Rpc => ExtensionMode::Rpc,
        },
        has_ui: options.has_ui,
        ui: Arc::new(StartupProjectTrustUI {
            mode: options.mode,
            settings_manager: options.settings_manager.clone(),
            has_ui: options.has_ui,
        }),
    }
}
