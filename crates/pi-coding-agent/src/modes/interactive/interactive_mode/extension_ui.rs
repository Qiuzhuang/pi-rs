//! Extension integration: binding, autocomplete, shortcuts, widgets, header/footer, dialogs and the
//! TUI implementation of `ExtensionUIContext`.

use super::*;
use crate::core::agent_session::ExtensionBindings;
use crate::core::agent_session_runtime::{
    AgentSessionRuntimeForkOptions, AgentSessionRuntimeNewSessionOptions,
};
use crate::core::extensions::types::{
    ComponentFactory, CustomComponentFactory, CustomUiOptions, CustomUiResult, ExtensionCommandContextActions,
    ExtensionMode, ExtensionUIContext, ExtensionUIDialogOptions, ExtensionWidgetOptions, FooterFactory, ForkOptions,
    NavigateTreeOptions, NewSessionOptions, NotifyType, OverlayOptionsSource, ProjectTrustContext,
    SessionReplaceOutcome, SetThemeResult, SwitchSessionOptions, TerminalInputHandler, ThemeInfo, ThemeSelection,
    Unsubscribe, WidgetContent, WidgetPlacement,
};
use crate::core::session_cwd::{MissingSessionCwdError, format_missing_session_cwd_prompt};
use crate::modes::interactive::components::extension_editor::ExtensionEditorComponent;
use crate::modes::interactive::components::extension_input::{ExtensionInputComponent, ExtensionInputOptions};
use crate::modes::interactive::components::extension_selector::{ExtensionSelectorComponent, ExtensionSelectorOptions};
use crate::modes::interactive::model_search::{ModelSearchItem, get_model_search_text};
use crate::modes::interactive::theme::theme::Theme;
use pi_tui::autocomplete::{
    AutocompleteItem, AutocompleteSuggestions, CombinedAutocompleteProvider, CommandEntry, CompletionResult,
    SlashCommand, SuggestOptions,
};
use pi_tui::components::text::Text;
use tokio::sync::oneshot;

/// A provider with an explicit set of trigger characters (TS assigns `provider.triggerCharacters`).
struct TriggerCharactersProvider {
    inner: Arc<dyn AutocompleteProvider>,
    trigger_characters: Vec<String>,
}

impl AutocompleteProvider for TriggerCharactersProvider {
    fn trigger_characters(&self) -> Vec<String> {
        self.trigger_characters.clone()
    }
    fn get_suggestions(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        options: SuggestOptions,
    ) -> pi_tui::event_loop::BoxFuture<Option<AutocompleteSuggestions>> {
        self.inner.get_suggestions(lines, cursor_line, cursor_col, options)
    }
    fn apply_completion(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> CompletionResult {
        self.inner.apply_completion(lines, cursor_line, cursor_col, item, prefix)
    }
    fn should_trigger_file_completion(&self, lines: &[String], cursor_line: usize, cursor_col: usize) -> bool {
        self.inner.should_trigger_file_completion(lines, cursor_line, cursor_col)
    }
    fn has_should_trigger_file_completion(&self) -> bool {
        self.inner.has_should_trigger_file_completion()
    }
}

fn fuzzy_items<T: Clone>(
    items: &[T],
    prefix: &str,
    get_search_text: impl Fn(&T) -> String,
    to_item: impl Fn(&T) -> AutocompleteItem,
) -> Option<Vec<AutocompleteItem>> {
    let filtered = pi_tui::fuzzy::fuzzy_filter(items, prefix, get_search_text);
    if filtered.is_empty() {
        return None;
    }
    Some(filtered.iter().map(to_item).collect())
}

/// The TUI implementation of `ExtensionUIContext`.
pub(crate) struct TuiExtensionUIContext {
    pub(crate) mode: Weak<Inner>,
}

impl TuiExtensionUIContext {
    fn inner(&self) -> Option<Arc<Inner>> {
        self.mode.upgrade()
    }
}

#[async_trait::async_trait]
impl ExtensionUIContext for TuiExtensionUIContext {
    async fn select(&self, title: &str, options: Vec<String>, opts: ExtensionUIDialogOptions) -> Option<String> {
        let inner = self.inner()?;
        inner.show_extension_selector(title, options, opts, None).await
    }

    async fn confirm(&self, title: &str, message: &str, opts: ExtensionUIDialogOptions) -> bool {
        let Some(inner) = self.inner() else { return false };
        inner.show_extension_confirm(title, message, opts).await
    }

    async fn input(&self, title: &str, placeholder: Option<&str>, opts: ExtensionUIDialogOptions) -> Option<String> {
        let inner = self.inner()?;
        inner.show_extension_input(title, placeholder, opts).await
    }

    fn notify(&self, message: &str, kind: NotifyType) {
        if let Some(inner) = self.inner() {
            match kind {
                NotifyType::Error => inner.show_error(message),
                NotifyType::Warning => inner.show_warning(message),
                NotifyType::Info => inner.show_status(message),
            }
        }
    }

    fn on_terminal_input(&self, handler: TerminalInputHandler) -> Unsubscribe {
        match self.inner() {
            Some(inner) => inner.add_extension_terminal_input_listener(handler),
            None => Arc::new(|| {}),
        }
    }

    fn set_status(&self, key: &str, text: Option<&str>) {
        if let Some(inner) = self.inner() {
            inner.footer_data_provider.set_extension_status(key, text);
            inner.request_render();
        }
    }

    fn set_working_message(&self, message: Option<&str>) {
        if let Some(inner) = self.inner() {
            let active = {
                let mut s = inner.state.lock();
                s.working_message = message.map(str::to_string);
                s.active_status_indicator.clone()
            };
            if let Some(active) = active
                && active.kind() == StatusIndicatorKind::Working
            {
                active.set_message(message.unwrap_or(DEFAULT_WORKING_MESSAGE));
            }
        }
    }

    fn set_working_visible(&self, visible: bool) {
        if let Some(inner) = self.inner() {
            inner.set_working_visible(visible);
        }
    }

    fn set_working_indicator(&self, options: Option<WorkingIndicatorOptions>) {
        if let Some(inner) = self.inner() {
            inner.set_working_indicator(options);
        }
    }

    fn set_hidden_thinking_label(&self, label: Option<&str>) {
        if let Some(inner) = self.inner() {
            inner.set_hidden_thinking_label(label);
        }
    }

    fn set_widget(&self, key: &str, content: Option<WidgetContent>, options: ExtensionWidgetOptions) {
        if let Some(inner) = self.inner() {
            inner.set_extension_widget(key, content, options);
        }
    }

    fn set_footer(&self, factory: Option<FooterFactory>) {
        if let Some(inner) = self.inner() {
            inner.set_extension_footer(factory);
        }
    }

    fn set_header(&self, factory: Option<ComponentFactory>) {
        if let Some(inner) = self.inner() {
            inner.set_extension_header(factory);
        }
    }

    fn set_title(&self, title: &str) {
        if let Some(inner) = self.inner() {
            inner.ui().terminal().set_title(title);
        }
    }

    async fn custom(&self, factory: CustomComponentFactory, options: CustomUiOptions) -> anyhow::Result<CustomUiResult> {
        let inner = self.inner().ok_or_else(|| anyhow::anyhow!("interactive mode is gone"))?;
        inner.show_extension_custom(factory, options).await
    }

    fn paste_to_editor(&self, text: &str) {
        if let Some(inner) = self.inner() {
            inner.editor().handle_input(&format!("\x1b[200~{text}\x1b[201~"));
        }
    }

    fn set_editor_text(&self, text: &str) {
        if let Some(inner) = self.inner() {
            inner.editor().set_text(text);
        }
    }

    fn get_editor_text(&self) -> String {
        self.inner().map(|inner| inner.editor().get_expanded_text()).unwrap_or_default()
    }

    async fn editor(&self, title: &str, prefill: Option<&str>) -> Option<String> {
        let inner = self.inner()?;
        inner.show_extension_editor(title, prefill).await
    }

    fn add_autocomplete_provider(&self, factory: AutocompleteProviderFactory) {
        if let Some(inner) = self.inner() {
            inner.state.lock().autocomplete_provider_wrappers.push(factory);
            inner.setup_autocomplete_provider();
        }
    }

    fn set_editor_component(&self, factory: Option<EditorFactory>) {
        if let Some(inner) = self.inner() {
            inner.set_custom_editor_component(factory);
        }
    }

    fn get_editor_component(&self) -> Option<EditorFactory> {
        self.inner().and_then(|inner| inner.state.lock().editor_component_factory.clone())
    }

    fn theme(&self) -> Arc<Theme> {
        theme()
    }

    fn get_all_themes(&self) -> Vec<ThemeInfo> {
        crate::modes::interactive::theme::theme::get_available_themes_with_paths()
            .into_iter()
            .map(|t| ThemeInfo { name: t.name, path: t.path })
            .collect()
    }

    fn get_theme(&self, name: &str) -> Option<Arc<Theme>> {
        crate::modes::interactive::theme::theme::get_theme_by_name(name)
    }

    fn set_theme(&self, selection: ThemeSelection) -> SetThemeResult {
        let Some(inner) = self.inner() else {
            return SetThemeResult { success: false, error: Some("interactive mode is gone".to_string()) };
        };
        match selection {
            ThemeSelection::Theme(theme) => {
                let result = inner.theme_controller.set_theme_instance(theme);
                SetThemeResult { success: result.success, error: result.error }
            }
            ThemeSelection::Name(name) => {
                let result = inner.theme_controller.set_theme_name(&name, None);
                if result.success {
                    let settings = inner.settings_manager();
                    if settings.get_theme().as_deref() != Some(name.as_str()) {
                        settings.set_theme(&name);
                    }
                }
                SetThemeResult { success: result.success, error: result.error }
            }
        }
    }

    fn get_tools_expanded(&self) -> bool {
        self.inner().map(|inner| inner.state.lock().tool_output_expanded).unwrap_or(false)
    }

    fn set_tools_expanded(&self, expanded: bool) {
        if let Some(inner) = self.inner() {
            inner.set_tools_expanded(expanded);
        }
    }
}

/// Session-replacing actions for extension commands.
struct CommandContextActions {
    mode: Weak<Inner>,
}

#[async_trait::async_trait]
impl ExtensionCommandContextActions for CommandContextActions {
    async fn wait_for_idle(&self) -> anyhow::Result<()> {
        let inner = self.mode.upgrade().ok_or_else(|| anyhow::anyhow!("interactive mode is gone"))?;
        inner.session().wait_for_idle().await
    }

    async fn new_session(&self, options: NewSessionOptions) -> anyhow::Result<SessionReplaceOutcome> {
        let inner = self.mode.upgrade().ok_or_else(|| anyhow::anyhow!("interactive mode is gone"))?;
        inner.clear_status_indicator(None);
        let result = inner
            .runtime_host
            .new_session(Some(&AgentSessionRuntimeNewSessionOptions {
                parent_session: options.parent_session,
                setup: options.setup,
                with_session: options.with_session,
            }))
            .await;
        match result {
            Ok(result) => Ok(SessionReplaceOutcome { cancelled: result.cancelled }),
            Err(error) => inner.handle_fatal_runtime_error("Failed to create session", &error),
        }
    }

    async fn fork(&self, entry_id: &str, options: ForkOptions) -> anyhow::Result<SessionReplaceOutcome> {
        let inner = self.mode.upgrade().ok_or_else(|| anyhow::anyhow!("interactive mode is gone"))?;
        let result = inner
            .runtime_host
            .fork(
                entry_id,
                Some(&AgentSessionRuntimeForkOptions {
                    position: options.position.map(|p| serde_json::from_value(serde_json::to_value(p).unwrap_or_default()))
                        .and_then(Result::ok),
                    with_session: options.with_session,
                }),
            )
            .await;
        match result {
            Ok(result) => {
                if !result.cancelled {
                    inner.editor().set_text(result.selected_text.as_deref().unwrap_or(""));
                    inner.show_status("Forked to new session");
                }
                Ok(SessionReplaceOutcome { cancelled: result.cancelled })
            }
            Err(error) => inner.handle_fatal_runtime_error("Failed to fork session", &error),
        }
    }

    async fn navigate_tree(&self, target_id: &str, options: NavigateTreeOptions) -> anyhow::Result<SessionReplaceOutcome> {
        let inner = self.mode.upgrade().ok_or_else(|| anyhow::anyhow!("interactive mode is gone"))?;
        let result = inner
            .session()
            .navigate_tree(
                target_id,
                Some(&crate::core::agent_session::AgentSessionNavigateTreeOptions {
                    summarize: options.summarize,
                    custom_instructions: options.custom_instructions,
                    replace_instructions: options.replace_instructions,
                    label: options.label,
                }),
            )
            .await?;
        if result.cancelled {
            return Ok(SessionReplaceOutcome { cancelled: true });
        }
        inner.chat_container.clear();
        inner.render_initial_messages();
        if let Some(text) = &result.editor_text
            && inner.editor().get_text().trim().is_empty()
        {
            inner.editor().set_text(text);
        }
        inner.show_status("Navigated to selected point");
        let this = inner.clone();
        inner.spawn(async move { this.flush_compaction_queue(false).await });
        Ok(SessionReplaceOutcome { cancelled: false })
    }

    async fn switch_session(&self, session_path: &str, options: SwitchSessionOptions) -> anyhow::Result<SessionReplaceOutcome> {
        let inner = self.mode.upgrade().ok_or_else(|| anyhow::anyhow!("interactive mode is gone"))?;
        Ok(inner.handle_resume_session(session_path, options.with_session).await)
    }

    async fn reload(&self) -> anyhow::Result<()> {
        let inner = self.mode.upgrade().ok_or_else(|| anyhow::anyhow!("interactive mode is gone"))?;
        inner.handle_reload_command().await;
        Ok(())
    }
}

impl Inner {
    pub(crate) fn create_extension_ui_context(&self) -> Arc<dyn ExtensionUIContext> {
        Arc::new(TuiExtensionUIContext { mode: self.weak.clone() })
    }

    pub(crate) fn create_project_trust_context(&self, cwd: String) -> ProjectTrustContext {
        ProjectTrustContext { cwd, mode: ExtensionMode::Tui, has_ui: true, ui: self.create_extension_ui_context() }
    }

    /// Initialize the extension system with TUI-based UI context.
    pub(crate) async fn bind_current_session_extensions(self: Arc<Self>) -> anyhow::Result<()> {
        let weak_abort = self.weak.clone();
        let weak_shutdown = self.weak.clone();
        let weak_error = self.weak.clone();
        let bindings = ExtensionBindings {
            ui_context: Some(self.create_extension_ui_context()),
            mode: Some(ExtensionMode::Tui),
            command_context_actions: Some(Arc::new(CommandContextActions { mode: self.weak.clone() })),
            abort_handler: Some(Arc::new(move || {
                if let Some(this) = weak_abort.upgrade() {
                    this.restore_queued_messages_to_editor(true, None);
                }
            })),
            shutdown_handler: Some(Arc::new(move || {
                if let Some(this) = weak_shutdown.upgrade() {
                    this.state.lock().shutdown_requested = true;
                    if this.session().is_idle() {
                        let target = this.clone();
                        this.spawn(async move { target.shutdown(false).await });
                    }
                }
            })),
            on_error: Some(Arc::new(move |error| {
                if let Some(this) = weak_error.upgrade() {
                    this.show_extension_error(&error.extension_path, &error.error, error.stack.as_deref());
                }
            })),
        };
        self.session().bind_extensions(&bindings).await?;

        set_registered_themes(self.session().resource_loader().get_themes().themes);
        self.setup_autocomplete_provider();
        self.setup_extension_shortcuts();
        self.show_loaded_resources(None, false, true);
        self.show_startup_notices_if_needed();
        Ok(())
    }

    fn create_base_autocomplete_provider(&self) -> Arc<dyn AutocompleteProvider> {
        let session = self.session();
        let mut slash_commands: Vec<SlashCommand> = crate::core::slash_commands::BUILTIN_SLASH_COMMANDS
            .iter()
            .map(|c| SlashCommand {
                name: c.name.clone(),
                description: Some(c.description.clone()),
                argument_hint: c.argument_hint.clone(),
                get_argument_completions: None,
            })
            .collect();

        for command in &mut slash_commands {
            let weak = self.weak.clone();
            match command.name.as_str() {
                "model" => {
                    command.get_argument_completions = Some(Arc::new(move |prefix: &str| {
                        let result = weak.upgrade().and_then(|this| {
                            let session = this.session();
                            let scoped = session.scoped_models();
                            let models: Vec<pi_ai::types::Model> = if !scoped.is_empty() {
                                scoped.into_iter().map(|s| s.model).collect()
                            } else {
                                session.model_runtime().get_available_snapshot()
                            };
                            if models.is_empty() {
                                return None;
                            }
                            let items: Vec<(ModelSearchItem, String)> = models
                                .iter()
                                .map(|m| {
                                    (
                                        ModelSearchItem {
                                            id: m.id.clone(),
                                            provider: m.provider.to_string(),
                                            name: Some(m.name.clone()),
                                        },
                                        format!("{}/{}", m.provider, m.id),
                                    )
                                })
                                .collect();
                            fuzzy_items(&items, prefix, |(item, _)| get_model_search_text(item), |(item, label)| {
                                AutocompleteItem {
                                    value: label.clone(),
                                    label: item.id.clone(),
                                    description: Some(item.provider.clone()),
                                }
                            })
                        });
                        Box::pin(async move { result })
                    }));
                }
                "thinking" => {
                    command.get_argument_completions = Some(Arc::new(move |prefix: &str| {
                        let result = weak.upgrade().and_then(|this| {
                            let levels: Vec<String> =
                                this.session().get_available_thinking_levels().iter().map(|l| l.as_str().to_string()).collect();
                            fuzzy_items(&levels, prefix, |l| l.clone(), |l| AutocompleteItem::new(l.clone(), l.clone()))
                        });
                        Box::pin(async move { result })
                    }));
                }
                "login" => {
                    command.get_argument_completions = Some(Arc::new(move |prefix: &str| {
                        let result = weak.upgrade().and_then(|this| {
                            let providers =
                                super::selectors::get_login_provider_completion_options(&this.get_login_provider_options(None));
                            fuzzy_items(
                                &providers,
                                prefix,
                                super::selectors::get_login_provider_search_text,
                                |provider| AutocompleteItem {
                                    value: provider.id.clone(),
                                    label: provider.id.clone(),
                                    description: Some(super::selectors::format_login_provider_completion_description(provider)),
                                },
                            )
                        });
                        Box::pin(async move { result })
                    }));
                }
                _ => {}
            }
        }

        // Convert prompt templates to SlashCommand format for autocomplete
        let template_commands: Vec<SlashCommand> = session
            .prompt_templates()
            .into_iter()
            .map(|t| SlashCommand {
                name: t.name.clone(),
                description: self.prefix_autocomplete_description(Some(&t.description), Some(&t.source_info)),
                argument_hint: t.argument_hint.clone(),
                get_argument_completions: None,
            })
            .collect();

        // Convert extension commands to SlashCommand format
        let builtin_names: HashSet<String> = slash_commands.iter().map(|c| c.name.clone()).collect();
        let extension_commands: Vec<SlashCommand> = session
            .extension_runner()
            .get_registered_commands()
            .into_iter()
            .filter(|c| !builtin_names.contains(&c.name))
            .map(|c| {
                let completions = c.get_argument_completions.clone().map(|f| {
                    let f: pi_tui::autocomplete::ArgumentCompletions = Arc::new(move |prefix: &str| {
                        let fut = f(prefix.to_string());
                        Box::pin(fut) as pi_tui::event_loop::BoxFuture<Option<Vec<AutocompleteItem>>>
                    });
                    f
                });
                SlashCommand {
                    name: c.invocation_name.clone(),
                    description: self.prefix_autocomplete_description(c.description.as_deref(), Some(&c.source_info)),
                    argument_hint: None,
                    get_argument_completions: completions,
                }
            })
            .collect();

        // Build skill commands from session.skills (if enabled)
        let mut skill_command_list = Vec::new();
        let mut skill_commands = IndexMap::new();
        if self.settings_manager().get_enable_skill_commands() {
            for skill in session.resource_loader().get_skills().skills {
                let command_name = format!("skill:{}", skill.name);
                skill_commands.insert(command_name.clone(), skill.file_path.clone());
                skill_command_list.push(SlashCommand {
                    name: command_name,
                    description: self.prefix_autocomplete_description(Some(&skill.description), Some(&skill.source_info)),
                    argument_hint: None,
                    get_argument_completions: None,
                });
            }
        }
        let fd_path = {
            let mut s = self.state.lock();
            s.skill_commands = skill_commands;
            s.fd_path.clone()
        };

        let commands: Vec<CommandEntry> = slash_commands
            .into_iter()
            .chain(template_commands)
            .chain(extension_commands)
            .chain(skill_command_list)
            .map(CommandEntry::Command)
            .collect();
        Arc::new(CombinedAutocompleteProvider::new(commands, self.session_manager().get_cwd(), fd_path))
    }

    fn get_autocomplete_source_tag(&self, source_info: Option<&crate::core::source_info::SourceInfo>) -> Option<String> {
        let info = source_info?;
        // Built-in extension commands are untagged, like built-in commands.
        if info.source == "builtin" {
            return None;
        }
        let scope_prefix = match info.scope {
            crate::core::source_info::SourceScope::User => "u",
            crate::core::source_info::SourceScope::Project => "p",
            crate::core::source_info::SourceScope::Temporary => "t",
        };
        let source = info.source.trim();
        if source == "auto" || source == "local" || source == "cli" {
            return Some(scope_prefix.to_string());
        }
        if source.starts_with("npm:") {
            return Some(format!("{scope_prefix}:{source}"));
        }
        if let Some(git) = crate::utils::git::parse_git_url(source) {
            let r = git.r#ref.as_ref().map(|r| format!("@{r}")).unwrap_or_default();
            return Some(format!("{scope_prefix}:git:{}/{}{r}", git.host, git.path));
        }
        Some(scope_prefix.to_string())
    }

    fn prefix_autocomplete_description(
        &self,
        description: Option<&str>,
        source_info: Option<&crate::core::source_info::SourceInfo>,
    ) -> Option<String> {
        let description = description.filter(|d| !d.is_empty());
        let Some(tag) = self.get_autocomplete_source_tag(source_info) else {
            return description.map(str::to_string);
        };
        Some(match description {
            Some(d) => format!("[{tag}] {d}"),
            None => format!("[{tag}]"),
        })
    }

    pub(crate) fn setup_autocomplete_provider(&self) {
        let mut provider = self.create_base_autocomplete_provider();
        let wrappers = self.state.lock().autocomplete_provider_wrappers.clone();
        let mut trigger_characters: Vec<String> = Vec::new();
        for wrap in wrappers {
            provider = wrap(provider);
            for c in provider.trigger_characters() {
                if !trigger_characters.contains(&c) {
                    trigger_characters.push(c);
                }
            }
        }
        if !trigger_characters.is_empty() {
            provider = Arc::new(TriggerCharactersProvider { inner: provider, trigger_characters });
        }
        let (editor, is_default) = {
            let mut s = self.state.lock();
            s.autocomplete_provider = Some(provider.clone());
            (s.editor.clone(), s.editor_is_default)
        };
        self.default_editor.set_autocomplete_provider(provider.clone());
        if !is_default {
            editor.set_autocomplete_provider(provider);
        }
    }

    pub(crate) fn show_startup_notices_if_needed(&self) {
        use crate::modes::interactive::components::dynamic_border::DynamicBorder;
        let changelog = {
            let mut s = self.state.lock();
            if s.startup_notices_shown {
                return;
            }
            s.startup_notices_shown = true;
            s.changelog_markdown.clone()
        };
        let Some(changelog) = changelog else { return };
        if !self.chat_container.children().is_empty() {
            self.add_spacer();
        }
        self.chat_container.add_child(Arc::new(DynamicBorder::new(None)));
        if self.settings_manager().get_collapse_changelog() {
            static VERSION_RE: std::sync::LazyLock<regex::Regex> =
                std::sync::LazyLock::new(|| regex::Regex::new(r"##\s+\[?(\d+\.\d+\.\d+)\]?").unwrap());
            let latest = VERSION_RE
                .captures(&changelog)
                .map(|c| c[1].to_string())
                .unwrap_or_else(|| self.version.clone());
            let condensed = format!("Updated to v{latest}. Use {} to view full changelog.", theme().bold("/changelog"));
            self.chat_container.add_child(Arc::new(Text::new(condensed, 1, 0, None)));
        } else {
            self.add_text(
                &self.chat_container,
                Arc::new(|| theme().bold(&theme().fg(&ThemeColor::Accent, "What's New"))),
                1,
                0,
            );
            self.add_spacer();
            self.chat_container.add_child(Arc::new(pi_tui::components::markdown::Markdown::new(
                changelog.trim().to_string(),
                1,
                0,
                self.get_markdown_theme_with_settings(),
                None,
                None,
            )));
            self.add_spacer();
        }
        self.chat_container.add_child(Arc::new(DynamicBorder::new(None)));
    }

    /// Set up keyboard shortcuts registered by extensions.
    pub(crate) fn setup_extension_shortcuts(&self) {
        let runner = self.session().extension_runner();
        let shortcuts = runner.get_shortcuts(&self.keybindings.get_effective_config());
        if shortcuts.is_empty() {
            return;
        }
        let weak = self.weak.clone();
        self.default_editor.set_on_extension_shortcut(Some(Arc::new(move |data: String| {
            for (shortcut_str, shortcut) in &shortcuts {
                if pi_tui::keys::matches_key(&data, shortcut_str) {
                    let Some(this) = weak.upgrade() else { return false };
                    let handler = shortcut.handler.clone();
                    let ctx = this.session().extension_runner().create_context();
                    let target = this.clone();
                    this.spawn(async move {
                        if let Err(error) = handler(ctx).await {
                            target.show_error(&format!("Shortcut handler error: {error}"));
                        }
                    });
                    return true;
                }
            }
            false
        })));
    }

    // ------------------------------------------------------------------
    // Widgets, header and footer
    // ------------------------------------------------------------------

    fn set_extension_widget(&self, key: &str, content: Option<WidgetContent>, options: ExtensionWidgetOptions) {
        let placement = options.placement.unwrap_or_default();
        {
            let mut s = self.state.lock();
            s.extension_widgets_above.shift_remove(key);
            s.extension_widgets_below.shift_remove(key);
        }
        let Some(content) = content else {
            self.render_widgets();
            return;
        };
        let component: ComponentRef = match content {
            WidgetContent::Lines(lines) => {
                // Wrap string array in a Container with Text components
                let container = Container::new_ref();
                for line in lines.iter().take(MAX_WIDGET_LINES) {
                    container.add_child(Arc::new(Text::new(line.clone(), 1, 0, None)));
                }
                if lines.len() > MAX_WIDGET_LINES {
                    container.add_child(Arc::new(ThemedText::new(
                        Arc::new(|| theme().fg(&ThemeColor::Muted, "... (widget truncated)")),
                        Some(1),
                        Some(0),
                    )));
                }
                container
            }
            WidgetContent::Component(factory) => factory(self.ui(), theme()),
        };
        {
            let mut s = self.state.lock();
            match placement {
                WidgetPlacement::BelowEditor => s.extension_widgets_below.insert(key.to_string(), component),
                WidgetPlacement::AboveEditor => s.extension_widgets_above.insert(key.to_string(), component),
            };
        }
        self.render_widgets();
    }

    fn clear_extension_widgets(&self) {
        {
            let mut s = self.state.lock();
            s.extension_widgets_above.clear();
            s.extension_widgets_below.clear();
        }
        self.render_widgets();
    }

    /// Render all extension widgets to the widget containers.
    pub(crate) fn render_widgets(&self) {
        let (above, below) = {
            let s = self.state.lock();
            (
                s.extension_widgets_above.values().cloned().collect::<Vec<_>>(),
                s.extension_widgets_below.values().cloned().collect::<Vec<_>>(),
            )
        };
        render_widget_container(&self.widget_container_above, &above, true, true);
        render_widget_container(&self.widget_container_below, &below, false, false);
        self.request_render();
    }

    fn set_extension_footer(&self, factory: Option<FooterFactory>) {
        self.footer_container.clear();
        match factory {
            Some(factory) => {
                let footer = factory(self.ui(), theme(), self.footer_data_provider.clone());
                self.state.lock().custom_footer = Some(footer.clone());
                self.footer_container.add_child(footer);
            }
            None => {
                self.state.lock().custom_footer = None;
                self.footer_container.add_child(self.footer.clone());
            }
        }
        self.request_render();
    }

    fn set_extension_header(&self, factory: Option<ComponentFactory>) {
        let (built_in, custom, expanded) = {
            let s = self.state.lock();
            (s.built_in_header.clone(), s.custom_header.clone(), s.tool_output_expanded)
        };
        // Header may not be initialized yet if called during early initialization
        let Some(built_in) = built_in else { return };
        let current = custom.unwrap_or_else(|| built_in.clone());
        let children = self.header_container.children();
        let index = children.iter().position(|c| pi_tui::tui::same_component(c, &current));
        let mut children = children;
        match factory {
            Some(factory) => {
                let header = factory(self.ui(), theme());
                set_expanded_on(&header, expanded);
                self.state.lock().custom_header = Some(header.clone());
                match index {
                    Some(index) => children[index] = header,
                    None => children.insert(0, header),
                }
            }
            None => {
                self.state.lock().custom_header = None;
                set_expanded_on(&built_in, expanded);
                if let Some(index) = index {
                    children[index] = built_in;
                }
            }
        }
        self.header_container.set_children(children);
        self.request_render();
    }

    fn add_extension_terminal_input_listener(&self, handler: TerminalInputHandler) -> Unsubscribe {
        let id = self.next_id();
        let listener_id = self.ui().add_input_listener(to_input_listener(handler.clone()));
        self.state.lock().extension_terminal_input_subscriptions.push(TerminalInputSubscription {
            id,
            handler,
            listener_id,
        });
        let weak = self.weak.clone();
        Arc::new(move || {
            if let Some(this) = weak.upgrade() {
                let removed = {
                    let mut s = this.state.lock();
                    let index = s.extension_terminal_input_subscriptions.iter().position(|sub| sub.id == id);
                    index.map(|i| s.extension_terminal_input_subscriptions.remove(i))
                };
                if let Some(sub) = removed {
                    this.ui().remove_input_listener(sub.listener_id);
                }
            }
        })
    }

    pub(crate) fn rebind_extension_terminal_input_listeners(&self) {
        let ui = self.ui();
        let mut s = self.state.lock();
        for sub in s.extension_terminal_input_subscriptions.iter_mut() {
            ui.remove_input_listener(sub.listener_id);
            sub.listener_id = ui.add_input_listener(to_input_listener(sub.handler.clone()));
        }
    }

    pub(crate) fn clear_extension_terminal_input_listeners(&self) {
        let subs = std::mem::take(&mut self.state.lock().extension_terminal_input_subscriptions);
        let ui = self.ui();
        for sub in subs {
            ui.remove_input_listener(sub.listener_id);
        }
    }

    pub(crate) fn reset_extension_ui(&self) {
        let dialogs = std::mem::take(&mut self.state.lock().extension_dialogs);
        if !dialogs.is_empty() {
            for dialog in &dialogs {
                if let Some(dispose) = &dialog.dispose {
                    dispose();
                }
            }
            self.show_top_extension_dialog();
        }
        self.ui().hide_overlay();
        self.clear_extension_terminal_input_listeners();
        self.set_extension_footer(None);
        self.set_extension_header(None);
        self.clear_extension_widgets();
        self.footer_data_provider.clear_extension_statuses();
        self.footer.invalidate();
        self.state.lock().autocomplete_provider_wrappers.clear();
        self.set_custom_editor_component(None);
        self.setup_autocomplete_provider();
        self.default_editor.set_on_extension_shortcut(None);
        self.update_terminal_title();
        {
            let mut s = self.state.lock();
            s.working_message = None;
            s.working_visible = true;
        }
        self.set_working_indicator(None);
        let active = self.state.lock().active_status_indicator.clone();
        if let Some(active) = active
            && active.kind() == StatusIndicatorKind::Working
        {
            active.set_message(&format!(
                "{DEFAULT_WORKING_MESSAGE} ({} to interrupt)",
                crate::modes::interactive::components::keybinding_hints::key_text("app.interrupt".to_string())
            ));
        }
        self.set_hidden_thinking_label(None);
    }

    // ------------------------------------------------------------------
    // Dialogs
    // ------------------------------------------------------------------

    /// Show an extension dialog in the editor slot. Dialogs form a stack: a new dialog hides the visible
    /// one, and closing the visible dialog shows the previous one again.
    pub(crate) fn push_extension_dialog(
        &self,
        component: ComponentRef,
        blocked: Option<BlockedStatus>,
        dispose: Option<Handler>,
    ) -> u64 {
        self.dispose_active_selector();
        let id = self.next_id();
        self.state.lock().extension_dialogs.push(DialogEntry { id, component, blocked, dispose });
        self.show_top_extension_dialog();
        id
    }

    /// Remove an extension dialog. If it was visible, show the previous dialog or the editor.
    pub(crate) fn remove_extension_dialog(&self, id: u64) {
        let was_top = {
            let mut s = self.state.lock();
            let Some(index) = s.extension_dialogs.iter().position(|d| d.id == id) else { return };
            s.extension_dialogs.remove(index);
            index == s.extension_dialogs.len()
        };
        if was_top {
            self.show_top_extension_dialog();
        }
    }

    pub(crate) fn show_top_extension_dialog(&self) {
        let (component, blocked) = {
            let s = self.state.lock();
            match s.extension_dialogs.last() {
                Some(top) => (top.component.clone(), top.blocked.clone()),
                None => {
                    let editor: ComponentRef = s.editor.clone();
                    (editor, None)
                }
            }
        };
        self.editor_container.clear();
        self.editor_container.add_child(component.clone());
        let ui = self.ui();
        ui.set_focus(Some(component));
        self.program_status.set_blocked("extension-dialog", blocked.as_ref());
        ui.request_render(false);
    }

    /// Show a selector for extensions.
    pub(crate) async fn show_extension_selector(
        &self,
        title: &str,
        options: Vec<String>,
        opts: ExtensionUIDialogOptions,
        blocked: Option<BlockedStatus>,
    ) -> Option<String> {
        if opts.signal.as_ref().is_some_and(|s| s.is_cancelled()) {
            return None;
        }
        let blocked = blocked.unwrap_or(BlockedStatus {
            kind: crate::modes::interactive::program_status_reporter::BlockedStatusKind::Question,
            message: title.to_string(),
        });
        let (tx, rx) = oneshot::channel::<Option<String>>();
        let finisher = DialogFinisher::new(self.weak.clone(), tx);
        let on_select = {
            let f = finisher.clone();
            Arc::new(move |option: String| f.finish(Some(option)))
        };
        let on_cancel = {
            let f = finisher.clone();
            Arc::new(move || f.finish(None))
        };
        let weak = self.weak.clone();
        let selector = Arc::new(ExtensionSelectorComponent::new(
            title,
            &options,
            on_select,
            on_cancel,
            Some(&ExtensionSelectorOptions {
                tui: Some(self.ui()),
                timeout: opts.timeout.map(|t| t as i64),
                on_toggle_tools_expanded: Some(Arc::new(move || {
                    if let Some(this) = weak.upgrade() {
                        this.toggle_tool_output_expansion();
                    }
                })),
                description: None,
            }),
        ));
        let dispose_target = selector.clone();
        let id = self.push_extension_dialog(
            selector,
            Some(blocked),
            Some(Arc::new(move || dispose_target.dispose())),
        );
        finisher.attach(id);
        finisher.watch_signal(opts.signal.clone());
        rx.await.ok().flatten()
    }

    /// Show a confirmation dialog for extensions.
    pub(crate) async fn show_extension_confirm(&self, title: &str, message: &str, opts: ExtensionUIDialogOptions) -> bool {
        let result = self
            .show_extension_selector(
                &format!("{title}\n{message}"),
                vec!["Yes".to_string(), "No".to_string()],
                opts,
                Some(BlockedStatus {
                    kind: crate::modes::interactive::program_status_reporter::BlockedStatusKind::Permission,
                    message: title.to_string(),
                }),
            )
            .await;
        result.as_deref() == Some("Yes")
    }

    pub(crate) async fn prompt_for_missing_session_cwd(&self, error: &MissingSessionCwdError) -> Option<String> {
        let issue = error.issue();
        let confirmed = self
            .show_extension_confirm(
                "Session cwd not found",
                &format_missing_session_cwd_prompt(&issue),
                ExtensionUIDialogOptions::default(),
            )
            .await;
        confirmed.then_some(issue.fallback_cwd)
    }

    /// Show a text input for extensions.
    pub(crate) async fn show_extension_input(
        &self,
        title: &str,
        placeholder: Option<&str>,
        opts: ExtensionUIDialogOptions,
    ) -> Option<String> {
        if opts.signal.as_ref().is_some_and(|s| s.is_cancelled()) {
            return None;
        }
        let (tx, rx) = oneshot::channel::<Option<String>>();
        let finisher = DialogFinisher::new(self.weak.clone(), tx);
        let on_submit = {
            let f = finisher.clone();
            Arc::new(move |value: String| f.finish(Some(value)))
        };
        let on_cancel = {
            let f = finisher.clone();
            Arc::new(move || f.finish(None))
        };
        let input = Arc::new(ExtensionInputComponent::new(
            title,
            placeholder,
            on_submit,
            on_cancel,
            Some(&ExtensionInputOptions {
                tui: Some(self.ui()),
                timeout: opts.timeout.map(|t| t as i64),
                initial_value: None,
                description: None,
            }),
        ));
        let dispose_target = input.clone();
        let id = self.push_extension_dialog(
            input,
            Some(BlockedStatus {
                kind: crate::modes::interactive::program_status_reporter::BlockedStatusKind::Question,
                message: title.to_string(),
            }),
            Some(Arc::new(move || dispose_target.dispose())),
        );
        finisher.attach(id);
        finisher.watch_signal(opts.signal.clone());
        rx.await.ok().flatten()
    }

    /// Show a multi-line editor for extensions (with Ctrl+G support).
    pub(crate) async fn show_extension_editor(&self, title: &str, prefill: Option<&str>) -> Option<String> {
        let (tx, rx) = oneshot::channel::<Option<String>>();
        let finisher = DialogFinisher::new(self.weak.clone(), tx);
        let on_submit = {
            let f = finisher.clone();
            Arc::new(move |value: String| f.finish(Some(value)))
        };
        let on_cancel = {
            let f = finisher.clone();
            Arc::new(move || f.finish(None))
        };
        let external = self.settings_manager().get_external_editor_command();
        let editor = Arc::new(ExtensionEditorComponent::new(
            self.ui(),
            self.keybindings.clone(),
            title,
            prefill,
            on_submit,
            on_cancel,
            None,
            Some(&external),
        ));
        let id = self.push_extension_dialog(
            editor,
            Some(BlockedStatus {
                kind: crate::modes::interactive::program_status_reporter::BlockedStatusKind::Question,
                message: title.to_string(),
            }),
            None,
        );
        finisher.attach(id);
        rx.await.ok().flatten()
    }

    /// Set a custom editor component from an extension. `None` restores the default editor.
    pub(crate) fn set_custom_editor_component(&self, factory: Option<EditorFactory>) {
        let current_text = self.editor().get_text();
        self.state.lock().editor_component_factory = factory.clone();
        self.dispose_active_selector();
        self.editor_container.clear();

        let new_editor: Arc<dyn EditorComponent> = match factory {
            Some(factory) => {
                let new_editor = factory(self.ui(), (*get_editor_theme()).clone(), self.keybindings.clone());
                // Wire up callbacks from the default editor
                self.install_submit_callback_on(&new_editor);
                new_editor.set_on_change(Some(self.on_change_callback()));
                new_editor.set_text(&current_text);
                let border_color = self.state.lock().editor_border_color.clone();
                if let Some(color) = border_color {
                    new_editor.set_border_color(color);
                }
                new_editor.set_padding_x(self.default_editor.get_padding_x());
                new_editor.set_autocomplete_max_visible(self.default_editor.get_autocomplete_max_visible());
                let autocomplete_provider = self.state.lock().autocomplete_provider.clone();
                if let Some(provider) = autocomplete_provider {
                    new_editor.set_autocomplete_provider(provider);
                }
                // If extending CustomEditor, copy app-level handlers
                let component: ComponentRef = new_editor.clone();
                if let Some(custom) = pi_tui::tui::downcast_component::<CustomEditor>(&component) {
                    if custom.on_escape().is_none() {
                        let d = self.default_editor.clone();
                        custom.set_on_escape(Some(Arc::new(move || {
                            if let Some(f) = d.on_escape() {
                                f();
                            }
                        })));
                    }
                    if custom.on_ctrl_d().is_none() {
                        let d = self.default_editor.clone();
                        custom.set_on_ctrl_d(Some(Arc::new(move || {
                            if let Some(f) = d.on_ctrl_d() {
                                f();
                            }
                        })));
                    }
                    if custom.on_paste_image().is_none() {
                        let d = self.default_editor.clone();
                        custom.set_on_paste_image(Some(Arc::new(move || {
                            if let Some(f) = d.on_paste_image() {
                                f();
                            }
                        })));
                    }
                    if custom.on_extension_shortcut().is_none() {
                        let d = self.default_editor.clone();
                        custom.set_on_extension_shortcut(Some(Arc::new(move |data: String| {
                            d.on_extension_shortcut().is_some_and(|f| f(data))
                        })));
                    }
                    let mut handlers = custom.action_handlers();
                    for (action, handler) in self.default_editor.action_handlers() {
                        handlers.insert(action, handler);
                    }
                    custom.set_action_handlers(handlers);
                }
                self.state.lock().editor_is_default = false;
                new_editor
            }
            None => {
                self.default_editor.set_text(&current_text);
                self.state.lock().editor_is_default = true;
                self.default_editor.clone()
            }
        };
        self.state.lock().editor = new_editor.clone();
        let component: ComponentRef = new_editor;
        self.editor_container.add_child(component.clone());
        let indicator = self.state.lock().active_status_indicator.clone();
        if let Some(indicator) = indicator {
            self.status_container.clear();
            let embedded = self.set_editor_working_status_indicator_public(Some(indicator.clone()));
            self.state.lock().active_working_indicator_embedded = embedded;
            if !embedded {
                self.status_container.add_child(indicator);
            }
        }
        let ui = self.ui();
        ui.set_focus(Some(component));
        ui.request_render(false);
    }

    fn set_editor_working_status_indicator_public(&self, indicator: Option<Arc<StatusIndicator>>) -> bool {
        self.default_editor.set_working_status_indicator(None);
        let editor = self.editor();
        let component: ComponentRef = editor;
        match pi_tui::tui::downcast_component::<CustomEditor>(&component) {
            Some(custom) if custom.embed_working_status() => {
                custom.set_working_status_indicator(indicator);
                true
            }
            _ => false,
        }
    }

    /// Show a custom component with keyboard focus. Overlay mode renders on top of existing content.
    pub(crate) async fn show_extension_custom(
        &self,
        factory: CustomComponentFactory,
        options: CustomUiOptions,
    ) -> anyhow::Result<CustomUiResult> {
        let saved_text = self.editor().get_text();
        let is_overlay = options.overlay.unwrap_or(false);
        let (tx, rx) = oneshot::channel::<CustomUiResult>();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let closed = Arc::new(AtomicBool::new(false));
        let dialog_id: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
        let overlay_handle: Arc<Mutex<Option<pi_tui::tui::OverlayHandle>>> = Arc::new(Mutex::new(None));

        let done: crate::core::extensions::types::CustomUiDone = {
            let weak = self.weak.clone();
            let closed = closed.clone();
            let dialog_id = dialog_id.clone();
            let overlay_handle = overlay_handle.clone();
            let saved_text = saved_text.clone();
            let tx = tx.clone();
            Arc::new(move |result: CustomUiResult| {
                if closed.swap(true, Ordering::SeqCst) {
                    return;
                }
                if let Some(this) = weak.upgrade() {
                    // Remove this overlay, not the topmost one: another overlay may have opened above it.
                    if is_overlay {
                        if let Some(handle) = overlay_handle.lock().take() {
                            handle.hide();
                        }
                    } else {
                        if let Some(id) = dialog_id.lock().take() {
                            this.remove_extension_dialog(id);
                        }
                        this.editor().set_text(&saved_text);
                        this.request_render();
                    }
                }
                if let Some(tx) = tx.lock().take() {
                    let _ = tx.send(result);
                }
            })
        };

        let component = factory(self.ui(), theme(), self.keybindings.clone(), done).await;
        match component {
            Ok(component) => {
                if !closed.load(Ordering::SeqCst) {
                    if is_overlay {
                        let overlay_options = options.overlay_options.as_ref().map(|o| match o {
                            OverlayOptionsSource::Static(options) => options.clone(),
                            OverlayOptionsSource::Dynamic(f) => f(),
                        });
                        let handle = self.ui().show_overlay(component, overlay_options);
                        *overlay_handle.lock() = Some(handle.clone());
                        // Expose handle to caller for visibility control
                        if let Some(on_handle) = &options.on_handle {
                            on_handle(handle);
                        }
                    } else {
                        let id = self.push_extension_dialog(component, None, None);
                        *dialog_id.lock() = Some(id);
                    }
                }
            }
            Err(error) => {
                if !closed.swap(true, Ordering::SeqCst) {
                    if !is_overlay {
                        self.editor().set_text(&saved_text);
                        self.request_render();
                    }
                    return Err(error);
                }
            }
        }
        rx.await.map_err(|_| anyhow::anyhow!("custom UI component was dropped"))
    }

    /// Show an extension error in the UI.
    pub(crate) fn show_extension_error(&self, extension_path: &str, error: &str, stack: Option<&str>) {
        let message = format!("Extension \"{extension_path}\" error: {error}");
        self.add_text(&self.chat_container, Arc::new(move || theme().fg(&ThemeColor::Error, &message)), 1, 0);
        if let Some(stack) = stack {
            // Show stack trace in dim color, indented (skip the first line, which duplicates the message)
            let lines: Vec<String> = stack.split('\n').skip(1).map(str::to_string).collect();
            if !lines.is_empty() {
                self.add_text(
                    &self.chat_container,
                    Arc::new(move || {
                        lines.iter().map(|l| theme().fg(&ThemeColor::Dim, &format!("  {}", l.trim()))).collect::<Vec<_>>().join("\n")
                    }),
                    1,
                    0,
                );
            }
        }
        self.request_render();
    }
}

fn render_widget_container(container: &Container, widgets: &[ComponentRef], spacer_when_empty: bool, leading_spacer: bool) {
    container.clear();
    if widgets.is_empty() {
        if spacer_when_empty {
            container.add_child(Arc::new(Spacer::new(1)));
        }
        return;
    }
    if leading_spacer {
        container.add_child(Arc::new(Spacer::new(1)));
    }
    for widget in widgets {
        container.add_child(widget.clone());
    }
}

fn to_input_listener(handler: TerminalInputHandler) -> pi_tui::tui::InputListener {
    Arc::new(move |data: &str| {
        handler(data).map(|r| pi_tui::tui::InputListenerResult { consume: r.consume.unwrap_or(false), data: r.data })
    })
}

/// Resolves a dialog once: removes it from the dialog stack and sends the value.
pub(crate) struct DialogFinisher<T: Send + 'static> {
    mode: Weak<Inner>,
    tx: Arc<Mutex<Option<oneshot::Sender<Option<T>>>>>,
    id: Arc<Mutex<Option<u64>>>,
    pending: Arc<Mutex<Option<Option<T>>>>,
}

impl<T: Send + 'static> Clone for DialogFinisher<T> {
    fn clone(&self) -> Self {
        Self { mode: self.mode.clone(), tx: self.tx.clone(), id: self.id.clone(), pending: self.pending.clone() }
    }
}

impl<T: Send + 'static> DialogFinisher<T> {
    pub(crate) fn new(mode: Weak<Inner>, tx: oneshot::Sender<Option<T>>) -> Self {
        Self { mode, tx: Arc::new(Mutex::new(Some(tx))), id: Arc::new(Mutex::new(None)), pending: Arc::new(Mutex::new(None)) }
    }

    /// Record the dialog id once it is shown. A value delivered before that is applied now.
    pub(crate) fn attach(&self, id: u64) {
        *self.id.lock() = Some(id);
        let pending = self.pending.lock().take();
        if let Some(value) = pending {
            self.finish(value);
        }
    }

    pub(crate) fn finish(&self, value: Option<T>) {
        let Some(id) = *self.id.lock() else {
            *self.pending.lock() = Some(value);
            return;
        };
        let Some(tx) = self.tx.lock().take() else { return };
        if let Some(mode) = self.mode.upgrade() {
            let dispose = mode.state.lock().extension_dialogs.iter().find(|d| d.id == id).and_then(|d| d.dispose.clone());
            if let Some(dispose) = dispose {
                dispose();
            }
            mode.remove_extension_dialog(id);
        }
        let _ = tx.send(value);
    }

    /// Dismiss the dialog when the signal fires.
    pub(crate) fn watch_signal(&self, signal: Option<tokio_util::sync::CancellationToken>) {
        let Some(signal) = signal else { return };
        let this = self.clone();
        if let Some(mode) = self.mode.upgrade() {
            mode.spawn(async move {
                signal.cancelled().await;
                this.finish(None);
            });
        }
    }
}

