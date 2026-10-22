//! Port of `packages/coding-agent/src/extensions/llama/ui.ts`.

use crate::core::extensions::types::{CustomUiOptions, ExtensionCommandContext, NotifyType};
use crate::core::keybindings::KeybindingsManager;
use crate::extensions::llama::client::{LlamaModelInfo, LlamaModelStatus, LlamaProgress};
use crate::extensions::llama::huggingface::HuggingFaceModel;
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_tui::StyleFn;
use pi_tui::components::input::Input;
use pi_tui::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions, SelectListTheme};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::fuzzy::fuzzy_filter;
use pi_tui::tui::{Component, ComponentRef, Container, Tui};
use pi_tui::utils::{truncate_to_width, visible_width};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const DOWNLOAD_VALUE: &str = "\0download";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
// Mirrors a TS union; boxing the large variants would add indirection to every
// construction and match across the crate for little memory saved.
#[allow(clippy::large_enum_variant)]
pub enum LlamaManagerAction {
    #[serde(rename = "model")]
    Model { model: LlamaModelInfo },
    #[serde(rename = "download")]
    Download {},
    #[serde(rename = "close")]
    Close {},
}

// TS: extends LlamaProgress (inherited fields inlined)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressState {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "pi_ai::serde_util::opt_js_f64")]
    pub ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub title: String,
    pub model: String,
}

impl ProgressState {
    /// `Object.assign(state, progress)`: fields the progress leaves undefined stay as they were.
    fn apply(&mut self, progress: LlamaProgress) {
        self.message = progress.message;
        if progress.ratio.is_some() {
            self.ratio = progress.ratio;
        }
        if progress.detail.is_some() {
            self.detail = progress.detail;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LlamaUiConnectionErrorResult {
    #[serde(rename = "retry")]
    Retry,
    #[serde(rename = "close")]
    Close,
}

impl LlamaUiConnectionErrorResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Close => "close",
        }
    }
}

/// Hugging Face search: `(query, signal) => Promise<HuggingFaceModel[]>`.
pub type HuggingFaceSearchFn =
    Arc<dyn Fn(String, CancellationToken) -> BoxFuture<'static, anyhow::Result<Vec<HuggingFaceModel>>> + Send + Sync>;

#[async_trait::async_trait]
pub trait LlamaUi: Send + Sync {
    async fn show_models(&self, server_url: &str, models: &[LlamaModelInfo]) -> anyhow::Result<LlamaManagerAction>;
    async fn select(&self, title: &str, options: &[String]) -> anyhow::Result<Option<String>>;
    async fn confirm(&self, title: &str, message: &str) -> anyhow::Result<bool>;
    async fn connection_error(&self, server_url: &str, message: &str) -> anyhow::Result<LlamaUiConnectionErrorResult>;
    async fn search_models(&self, search: HuggingFaceSearchFn) -> anyhow::Result<Option<String>>;
    fn show_status(&self, title: &str, message: &str);
    async fn progress(&self, state: &ProgressState) -> anyhow::Result<()>;
    fn update_progress(&self, state: &ProgressState);
}

fn round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn context_label(model: &LlamaModelInfo) -> Option<String> {
    let label = |value: f64| if value >= 1000.0 { format!("{}k", round(value / 1000.0)) } else { format!("{value}") };
    let meta = model.meta.as_ref();
    if let Some(context) = meta.and_then(|m| m.n_ctx.or(m.n_ctx_train)).filter(|n| *n != 0.0 && !n.is_nan()) {
        return Some(label(context));
    }
    let args = model.status.args.as_deref().unwrap_or_default();
    for index in 0..args.len().saturating_sub(1) {
        if args[index] != "--ctx-size" && args[index] != "-c" && args[index] != "-ctx" {
            continue;
        }
        let text = args[index + 1].trim();
        let value: f64 = if text.is_empty() { 0.0 } else { text.parse().unwrap_or(f64::NAN) };
        if value.is_finite() && value > 0.0 {
            return Some(label(value));
        }
    }
    None
}

fn model_description(model: &LlamaModelInfo) -> String {
    let mut details: Vec<String> = Vec::new();
    let loaded = matches!(model.status.value, LlamaModelStatus::Loaded | LlamaModelStatus::Sleeping);
    if loaded {
        details.push("loaded".into());
    } else if model.status.value != LlamaModelStatus::Unloaded {
        details.push(model.status.value.as_str().into());
    }
    if loaded && let Some(context) = context_label(model) {
        details.push(format!("{context} context"));
    }
    details.join(" · ")
}

fn style(theme: &Arc<Theme>, color: ThemeColor) -> StyleFn {
    let theme = theme.clone();
    Arc::new(move |text: &str| theme.fg(&color, text))
}

fn select_theme(theme: &Arc<Theme>) -> SelectListTheme {
    SelectListTheme {
        selected_prefix: style(theme, ThemeColor::Accent),
        selected_text: style(theme, ThemeColor::Accent),
        description: style(theme, ThemeColor::Muted),
        scroll_info: style(theme, ThemeColor::Dim),
        no_match: style(theme, ThemeColor::Warning),
    }
}

fn border(theme: &Arc<Theme>) -> ComponentRef {
    let theme = theme.clone();
    Arc::new(DynamicBorder::new(Some(Arc::new(move |text: String| theme.fg(&ThemeColor::Accent, &text)))))
}

fn frame(theme: &Arc<Theme>, title: &str, body: Vec<ComponentRef>, footer: Option<String>) -> Arc<Container> {
    let container = Container::new_ref();
    container.add_child(border(theme));
    container.add_child(Arc::new(Text::new(theme.fg(&ThemeColor::Accent, &theme.bold(title)), 1, 0, None)));
    for child in body {
        container.add_child(child);
    }
    if let Some(footer) = footer.filter(|f| !f.is_empty()) {
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(Text::new(theme.fg(&ThemeColor::Dim, &footer), 1, 0, None)));
    }
    container.add_child(border(theme));
    container
}

fn hint(keybinding: &str, description: &str) -> String {
    key_hint(keybinding.to_string(), description)
}

fn compact_count(value: f64) -> String {
    use crate::core::tools::js_util::{js_number_string, js_to_fixed};
    if value >= 1_000_000.0 {
        return format!("{}M", js_to_fixed(value / 1_000_000.0, if value >= 10_000_000.0 { 0 } else { 1 }));
    }
    if value >= 1_000.0 {
        return format!("{}k", js_to_fixed(value / 1_000.0, if value >= 100_000.0 { 0 } else { 1 }));
    }
    js_number_string(value)
}

struct SearchState {
    results: Vec<HuggingFaceModel>,
    filtered_results: Vec<HuggingFaceModel>,
    selected_index: usize,
    query: String,
    status: String,
    debounce: Option<CancellationToken>,
    request: Option<CancellationToken>,
    closed: bool,
    focused: bool,
}

/// TS: `extends Container implements Focusable`
pub struct HuggingFaceSearch {
    this: Weak<HuggingFaceSearch>,
    tui: Arc<Tui>,
    theme: Arc<Theme>,
    keybindings: Arc<KeybindingsManager>,
    search: HuggingFaceSearchFn,
    cache: Arc<Mutex<HashMap<String, Vec<HuggingFaceModel>>>>,
    on_select_model: Arc<dyn Fn(Option<String>) + Send + Sync>,
    container: Container,
    input: Arc<Input>,
    results_container: Arc<Container>,
    state: Mutex<SearchState>,
}

impl HuggingFaceSearch {
    pub fn new(
        tui: Arc<Tui>,
        theme: Arc<Theme>,
        keybindings: Arc<KeybindingsManager>,
        search: HuggingFaceSearchFn,
        cache: Arc<Mutex<HashMap<String, Vec<HuggingFaceModel>>>>,
        on_select_model: Arc<dyn Fn(Option<String>) + Send + Sync>,
    ) -> Arc<Self> {
        let component = Arc::new_cyclic(|this| {
            let input = Arc::new(Input::default());
            let results_container = Container::new_ref();
            let container = Container::new();
            container.add_child(Arc::new(Text::new(
                theme.fg(&ThemeColor::Dim, "Model name or owner/repository[:quant]"),
                1,
                0,
                None,
            )));
            container.add_child(input.clone());
            container.add_child(Arc::new(Spacer::new(1)));
            container.add_child(results_container.clone());
            HuggingFaceSearch {
                this: this.clone(),
                tui,
                theme,
                keybindings,
                search,
                cache,
                on_select_model,
                container,
                input,
                results_container,
                state: Mutex::new(SearchState {
                    results: Vec::new(),
                    filtered_results: Vec::new(),
                    selected_index: 0,
                    query: String::new(),
                    status: "Type at least 2 characters".to_string(),
                    debounce: None,
                    request: None,
                    closed: false,
                    focused: false,
                }),
            }
        });
        component.update_results();
        component
    }

    fn update_results(&self) {
        let (lines, total) = {
            let state = self.state.lock();
            let theme = &self.theme;
            let max_visible: usize = 10;
            let count = state.filtered_results.len();
            let start = (state.selected_index as i64 - (max_visible / 2) as i64)
                .min(count as i64 - max_visible as i64)
                .max(0) as usize;
            let end = (start + max_visible).min(count);
            let mut lines: Vec<String> = Vec::new();
            for index in start..end {
                let model = &state.filtered_results[index];
                let prefix = if index == state.selected_index { "→ " } else { "  " };
                let details = format!("{} downloads", compact_count(model.downloads));
                lines.push(if index == state.selected_index {
                    theme.fg(&ThemeColor::Accent, &format!("{prefix}{}  {details}", model.id))
                } else {
                    format!("{prefix}{}{}", model.id, theme.fg(&ThemeColor::Muted, &format!("  {details}")))
                });
            }
            if start > 0 || end < count {
                lines.push(theme.fg(&ThemeColor::Dim, &format!("  ({}/{})", state.selected_index + 1, count)));
            }
            if count == 0 || state.status == "Searching Hugging Face…" {
                lines.push(theme.fg(&ThemeColor::Dim, &format!("  {}", state.status)));
            }
            (lines, count)
        };
        let _ = total;
        self.results_container.clear();
        for line in lines {
            self.results_container.add_child(Arc::new(Text::new(line, 0, 0, None)));
        }
        self.tui.request_render(false);
    }

    fn filter_results(&self) {
        {
            let mut state = self.state.lock();
            if !state.query.is_empty() {
                let matches: std::collections::HashSet<String> =
                    fuzzy_filter(&state.results, &state.query, |model| model.id.clone())
                        .into_iter()
                        .map(|model| model.id)
                        .collect();
                state.filtered_results = state.results.iter().filter(|m| matches.contains(&m.id)).cloned().collect();
            } else {
                state.filtered_results = state.results.clone();
            }
            state.selected_index = state.selected_index.min(state.filtered_results.len().saturating_sub(1));
        }
        self.update_results();
    }

    fn schedule_search(&self) {
        let query = {
            let mut state = self.state.lock();
            if let Some(debounce) = state.debounce.take() {
                debounce.cancel();
            }
            if let Some(request) = state.request.take() {
                request.cancel();
            }
            if state.query.chars().count() < 2 {
                state.status = "Type at least 2 characters".to_string();
                None
            } else if let Some(cached) = self.cache.lock().get(&state.query.to_lowercase()).cloned() {
                state.status = if cached.is_empty() { "No GGUF models found".to_string() } else { String::new() };
                state.results = cached;
                None
            } else {
                state.status = "Searching Hugging Face…".to_string();
                Some(state.query.clone())
            }
        };
        self.filter_results();
        let Some(query) = query else { return };
        let debounce = CancellationToken::new();
        self.state.lock().debounce = Some(debounce.clone());
        let this = self.this.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
                _ = debounce.cancelled() => return,
            }
            if let Some(this) = this.upgrade() {
                this.run_search(query).await;
            }
        });
    }

    async fn run_search(&self, query: String) {
        let request = CancellationToken::new();
        self.state.lock().request = Some(request.clone());
        let result = (self.search)(query.clone(), request.clone()).await;
        let stale = |state: &SearchState| state.closed || request.is_cancelled() || state.query != query;
        match result {
            Ok(results) => {
                self.cache.lock().insert(query.to_lowercase(), results.clone());
                {
                    let mut state = self.state.lock();
                    if !stale(&state) {
                        state.status = if results.is_empty() { "No GGUF models found".into() } else { String::new() };
                        state.results = results;
                        state.selected_index = 0;
                    } else {
                        Self::clear_request(&mut state, &request);
                        return;
                    }
                    Self::clear_request(&mut state, &request);
                }
                self.filter_results();
            }
            Err(error) => {
                {
                    let mut state = self.state.lock();
                    let is_stale = stale(&state);
                    Self::clear_request(&mut state, &request);
                    if is_stale {
                        return;
                    }
                    state.results = Vec::new();
                    state.status = error.to_string();
                }
                self.filter_results();
            }
        }
    }

    fn clear_request(state: &mut SearchState, request: &CancellationToken) {
        if state.request.as_ref().is_some_and(|r| r == request) {
            state.request = None;
        }
    }

    fn close(&self, model: Option<String>) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.closed = true;
            if let Some(debounce) = state.debounce.take() {
                debounce.cancel();
            }
            if let Some(request) = state.request.take() {
                request.cancel();
            }
        }
        (self.on_select_model)(model);
    }
}

fn is_exact_model(query: &str) -> bool {
    static EXACT: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^[^/\s]+/[^:\s]+(?::[^\s:]+)?$").unwrap());
    EXACT.is_match(query)
}

impl Component for HuggingFaceSearch {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn handle_input(&self, data: &str) {
        let keys = &self.keybindings;
        if keys.matches(data, "tui.select.up") {
            let changed = {
                let mut state = self.state.lock();
                let count = state.filtered_results.len();
                if count > 0 {
                    state.selected_index = if state.selected_index == 0 { count - 1 } else { state.selected_index - 1 };
                }
                count > 0
            };
            if changed {
                self.update_results();
            }
            return;
        }
        if keys.matches(data, "tui.select.down") {
            let changed = {
                let mut state = self.state.lock();
                let count = state.filtered_results.len();
                if count > 0 {
                    state.selected_index = if state.selected_index == count - 1 { 0 } else { state.selected_index + 1 };
                }
                count > 0
            };
            if changed {
                self.update_results();
            }
            return;
        }
        if keys.matches(data, "tui.select.confirm") {
            let selected = {
                let state = self.state.lock();
                if is_exact_model(&state.query) {
                    Some(state.query.clone())
                } else {
                    state.filtered_results.get(state.selected_index).map(|m| m.id.clone())
                }
            };
            if let Some(selected) = selected.filter(|s| !s.is_empty()) {
                self.close(Some(selected));
            }
            return;
        }
        if keys.matches(data, "tui.select.cancel") {
            self.close(None);
            return;
        }
        self.input.handle_input(data);
        let query = self.input.get_value().trim().to_string();
        {
            let mut state = self.state.lock();
            if query == state.query {
                return;
            }
            state.query = query;
        }
        self.schedule_search();
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        self.container.invalidate();
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.input.set_focused(focused);
    }

    fn focused(&self) -> bool {
        self.state.lock().focused
    }
}

struct ViewState {
    content: ComponentRef,
    input_handler: Option<ComponentRef>,
    input_target: Option<ComponentRef>,
    /// Pending `progress()` calls, resolved when the user asks to stop.
    progress_waiters: Vec<oneshot::Sender<()>>,
    progress_active: bool,
    showing_progress: bool,
    focused: bool,
}

/// TS: `implements LlamaUi, Focusable`
pub struct LlamaView {
    tui: Arc<Tui>,
    theme: Arc<Theme>,
    keybindings: Arc<KeybindingsManager>,
    search_cache: Arc<Mutex<HashMap<String, Vec<HuggingFaceModel>>>>,
    state: Mutex<ViewState>,
}

/// Resolves a oneshot once; later sends are ignored.
fn once_sender<T: Send + 'static>(sender: oneshot::Sender<T>) -> Arc<dyn Fn(T) + Send + Sync> {
    let slot = Mutex::new(Some(sender));
    Arc::new(move |value| {
        if let Some(sender) = slot.lock().take() {
            let _ = sender.send(value);
        }
    })
}

/// A receiver whose sender was dropped never resolves, like a promise nobody settles.
async fn settle<T>(receiver: oneshot::Receiver<T>) -> T {
    match receiver.await {
        Ok(value) => value,
        Err(_) => std::future::pending().await,
    }
}

impl LlamaView {
    pub fn new(tui: Arc<Tui>, theme: Arc<Theme>, keybindings: Arc<KeybindingsManager>) -> Self {
        let content: ComponentRef = frame(
            &theme,
            "llama.cpp models",
            vec![Arc::new(Text::new(theme.fg(&ThemeColor::Muted, "Loading…"), 1, 1, None))],
            None,
        );
        Self {
            tui,
            theme,
            keybindings,
            search_cache: Arc::new(Mutex::new(HashMap::new())),
            state: Mutex::new(ViewState {
                content,
                input_handler: None,
                input_target: None,
                progress_waiters: Vec::new(),
                progress_active: false,
                showing_progress: false,
                focused: false,
            }),
        }
    }

    fn set_content(&self, content: ComponentRef, input_handler: Option<ComponentRef>, input_target: Option<ComponentRef>) {
        let (previous, focused) = {
            let mut state = self.state.lock();
            let previous = state.input_target.take();
            state.progress_waiters.clear();
            state.progress_active = false;
            state.showing_progress = false;
            state.content = content;
            state.input_handler = input_handler;
            state.input_target = input_target.clone();
            (previous, state.focused)
        };
        if let Some(previous) = previous {
            previous.set_focused(false);
        }
        if let Some(target) = input_target {
            target.set_focused(focused);
        }
        self.tui.request_render(false);
    }

    fn select_list(&self, items: Vec<SelectItem>, layout: SelectListLayoutOptions) -> Arc<SelectList> {
        let visible = items.len().min(12);
        Arc::new(SelectList::new(items, visible, select_theme(&self.theme), layout))
    }
}

#[async_trait::async_trait]
impl LlamaUi for LlamaView {
    async fn show_models(&self, server_url: &str, models: &[LlamaModelInfo]) -> anyhow::Result<LlamaManagerAction> {
        let mut sorted = models.to_vec();
        sorted.sort_by(|left, right| {
            let loaded = |m: &LlamaModelInfo| (m.status.value == LlamaModelStatus::Loaded) as i32;
            (loaded(right) - loaded(left))
                .cmp(&0)
                .then_with(|| crate::core::tools::js_util::locale_compare(&left.id, &right.id))
        });
        let by_id: HashMap<String, LlamaModelInfo> = sorted.iter().map(|m| (m.id.clone(), m.clone())).collect();
        let mut items: Vec<SelectItem> = sorted
            .iter()
            .map(|model| SelectItem::new(model.id.clone(), model.id.clone(), Some(model_description(model))))
            .collect();
        items.push(SelectItem::new(
            DOWNLOAD_VALUE,
            "Download model…",
            Some("Hugging Face owner/repository[:quant]".to_string()),
        ));
        let (tx, rx) = oneshot::channel();
        let resolve = once_sender(tx);
        let list = self.select_list(
            items,
            SelectListLayoutOptions {
                min_primary_column_width: Some(36),
                max_primary_column_width: Some(56),
                truncate_primary: None,
            },
        );
        let on_select = resolve.clone();
        list.set_on_select(Some(Arc::new(move |item: SelectItem| {
            if item.value == DOWNLOAD_VALUE {
                on_select(LlamaManagerAction::Download {});
            } else if let Some(model) = by_id.get(&item.value) {
                on_select(LlamaManagerAction::Model { model: model.clone() });
            }
        })));
        list.set_on_cancel(Some(Arc::new(move || resolve(LlamaManagerAction::Close {}))));
        let content = frame(
            &self.theme,
            "llama.cpp models",
            vec![
                Arc::new(Text::new(self.theme.fg(&ThemeColor::Dim, server_url), 1, 0, None)),
                Arc::new(Spacer::new(1)),
                list.clone(),
            ],
            Some(format!("{} • {}", hint("tui.select.confirm", "load/unload/download"), hint("tui.select.cancel", "close"))),
        );
        self.set_content(content, Some(list), None);
        Ok(settle(rx).await)
    }

    async fn select(&self, title: &str, options: &[String]) -> anyhow::Result<Option<String>> {
        let (tx, rx) = oneshot::channel();
        let resolve = once_sender(tx);
        let list = self.select_list(
            options.iter().map(|o| SelectItem::new(o.clone(), o.clone(), None)).collect(),
            SelectListLayoutOptions::default(),
        );
        let on_select = resolve.clone();
        list.set_on_select(Some(Arc::new(move |item: SelectItem| on_select(Some(item.value)))));
        list.set_on_cancel(Some(Arc::new(move || resolve(None))));
        let content = frame(
            &self.theme,
            title,
            vec![Arc::new(Spacer::new(1)), list.clone()],
            Some(format!("{} • {}", hint("tui.select.confirm", "select"), hint("tui.select.cancel", "cancel"))),
        );
        self.set_content(content, Some(list), None);
        Ok(settle(rx).await)
    }

    async fn confirm(&self, title: &str, message: &str) -> anyhow::Result<bool> {
        Ok(self.select(&format!("{title}\n{message}"), &["Yes".to_string(), "No".to_string()]).await?.as_deref()
            == Some("Yes"))
    }

    async fn connection_error(&self, server_url: &str, message: &str) -> anyhow::Result<LlamaUiConnectionErrorResult> {
        let choice = self
            .select(&format!("llama.cpp unavailable\n{server_url}\n\n{message}"), &["Retry".to_string(), "Close".to_string()])
            .await?;
        Ok(if choice.as_deref() == Some("Retry") {
            LlamaUiConnectionErrorResult::Retry
        } else {
            LlamaUiConnectionErrorResult::Close
        })
    }

    async fn search_models(&self, search: HuggingFaceSearchFn) -> anyhow::Result<Option<String>> {
        let (tx, rx) = oneshot::channel();
        let resolve = once_sender(tx);
        let component = HuggingFaceSearch::new(
            self.tui.clone(),
            self.theme.clone(),
            self.keybindings.clone(),
            search,
            self.search_cache.clone(),
            resolve,
        );
        let content = frame(
            &self.theme,
            "Download model",
            vec![Arc::new(Spacer::new(1)), component.clone()],
            Some(format!("{} • {}", hint("tui.select.confirm", "select"), hint("tui.select.cancel", "back"))),
        );
        self.set_content(content, Some(component.clone()), Some(component));
        Ok(settle(rx).await)
    }

    fn show_status(&self, title: &str, message: &str) {
        let content = frame(
            &self.theme,
            title,
            vec![Arc::new(Spacer::new(1)), Arc::new(Text::new(self.theme.fg(&ThemeColor::Muted, message), 1, 0, None))],
            None,
        );
        self.set_content(content, None, None);
    }

    async fn progress(&self, state: &ProgressState) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        {
            let mut view = self.state.lock();
            view.progress_waiters.push(tx);
            view.progress_active = true;
            view.showing_progress = true;
        }
        self.update_progress(state);
        settle(rx).await;
        Ok(())
    }

    fn update_progress(&self, state: &ProgressState) {
        if !self.state.lock().showing_progress {
            return;
        }
        let theme = &self.theme;
        let mut body: Vec<ComponentRef> = vec![
            Arc::new(Text::new(theme.fg(&ThemeColor::Text, &state.model), 1, 0, None)),
            Arc::new(Spacer::new(1)),
            Arc::new(Text::new(theme.fg(&ThemeColor::Muted, &state.message), 1, 0, None)),
        ];
        if let Some(ratio) = state.ratio {
            let available = 40usize;
            let filled = round(ratio.clamp(0.0, 1.0) * available as f64) as usize;
            body.push(Arc::new(Text::new(
                theme.fg(
                    &ThemeColor::Accent,
                    &format!("{}{} {}%", "█".repeat(filled), "─".repeat(available - filled), round(ratio * 100.0)),
                ),
                1,
                0,
                None,
            )));
        }
        if let Some(detail) = state.detail.as_deref().filter(|d| !d.is_empty()) {
            body.push(Arc::new(Text::new(theme.fg(&ThemeColor::Dim, detail), 1, 0, None)));
        }
        let content = frame(theme, &state.title, body, Some(hint("tui.select.cancel", "stop")));
        {
            let mut view = self.state.lock();
            view.content = content;
            view.input_handler = None;
        }
        self.tui.request_render(false);
    }
}

impl Component for LlamaView {
    fn render(&self, width: usize) -> Vec<String> {
        let content = self.state.lock().content.clone();
        content
            .render(width)
            .into_iter()
            .map(|line| if visible_width(&line) > width { truncate_to_width(&line, width, "", false) } else { line })
            .collect()
    }

    fn handle_input(&self, data: &str) {
        let waiters = {
            let mut state = self.state.lock();
            if state.progress_active && self.keybindings.matches(data, "tui.select.cancel") {
                state.progress_active = false;
                Some(std::mem::take(&mut state.progress_waiters))
            } else {
                None
            }
        };
        if let Some(waiters) = waiters {
            for waiter in waiters {
                let _ = waiter.send(());
            }
            return;
        }
        let handler = self.state.lock().input_handler.clone();
        if let Some(handler) = handler {
            handler.handle_input(data);
        }
        self.tui.request_render(false);
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn invalidate(&self) {
        let content = self.state.lock().content.clone();
        content.invalidate();
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        let target = {
            let mut state = self.state.lock();
            state.focused = focused;
            state.input_target.clone()
        };
        if let Some(target) = target {
            target.set_focused(focused);
        }
    }

    fn focused(&self) -> bool {
        self.state.lock().focused
    }
}

/// The body of `/llama`, run with the view.
pub type LlamaUiRun = Arc<dyn Fn(Arc<dyn LlamaUi>) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

pub async fn show_llama_ui(ctx: &ExtensionCommandContext, run: LlamaUiRun) -> anyhow::Result<()> {
    let ui = ctx.ui()?;
    let notify_ui = ui.clone();
    let factory: crate::core::extensions::types::CustomComponentFactory =
        Arc::new(move |tui, theme, keybindings, done| {
            let view = Arc::new(LlamaView::new(tui, theme, keybindings));
            let run = run.clone();
            let notify_ui = notify_ui.clone();
            let task_view = view.clone();
            tokio::spawn(async move {
                if let Err(error) = run(task_view).await {
                    notify_ui.notify(&error.to_string(), NotifyType::Error);
                }
                done(Box::new(()));
            });
            Box::pin(async move { Ok(view as ComponentRef) })
        });
    ui.custom(factory, CustomUiOptions::default()).await?;
    Ok(())
}

pub type ProgressRunFn<T> = Arc<
    dyn Fn(CancellationToken, Arc<dyn Fn(LlamaProgress) + Send + Sync>) -> BoxFuture<'static, anyhow::Result<T>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct RunWithProgressOptions<T> {
    pub title: String,
    pub model: String,
    pub initial_message: String,
    pub cancel_title: String,
    pub cancel_message: String,
    pub run: ProgressRunFn<T>,
    pub cancel: Arc<dyn Fn() -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>,
}

/// `{ cancelled: true } | { cancelled: false; value: T }`.
#[derive(Debug, Clone, PartialEq)]
pub enum RunWithProgressResult<T> {
    Cancelled,
    Completed(T),
}

pub async fn run_with_progress<T: Send + 'static>(
    ui: Arc<dyn LlamaUi>,
    options: &RunWithProgressOptions<T>,
) -> anyhow::Result<RunWithProgressResult<T>> {
    let controller = CancellationToken::new();
    let state = Arc::new(Mutex::new(ProgressState {
        title: options.title.clone(),
        model: options.model.clone(),
        message: options.initial_message.clone(),
        ..Default::default()
    }));
    let update: Arc<dyn Fn(LlamaProgress) + Send + Sync> = {
        let (state, ui) = (state.clone(), ui.clone());
        Arc::new(move |progress| {
            let snapshot = {
                let mut state = state.lock();
                state.apply(progress);
                state.clone()
            };
            ui.update_progress(&snapshot);
        })
    };
    let mut settled = tokio::spawn((options.run)(controller.clone(), update));
    let result = loop {
        if settled.is_finished() {
            break (&mut settled).await;
        }
        let snapshot = state.lock().clone();
        tokio::select! {
            result = &mut settled => break result,
            stop = ui.progress(&snapshot) => { stop?; }
        }
        let stop = ui.confirm(&options.cancel_title, &options.cancel_message).await?;
        if !stop || settled.is_finished() {
            continue;
        }
        let cancelled = (options.cancel)().await;
        controller.cancel();
        cancelled?;
        let _ = (&mut settled).await;
        return Ok(RunWithProgressResult::Cancelled);
    };
    let value = result.map_err(|error| anyhow::anyhow!("{error}"))??;
    Ok(RunWithProgressResult::Completed(value))
}
