//! Port of `packages/coding-agent/src/modes/interactive/components/bordered-loader.ts`.

use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::key_hint;
use crate::modes::interactive::theme::theme::{Theme, ThemeColor};
use pi_tui::StyleFn;
use pi_tui::components::loader::{CancellableLoader, Loader};
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, Tui};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BorderedLoaderOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellable: Option<bool>,
}

enum LoaderKind {
    Cancellable(Arc<CancellableLoader>),
    Plain(Arc<Loader>),
}

/// Loader wrapped with borders for extension UI
/// TS: `extends Container`
pub struct BorderedLoader {
    container: Container,
    loader: LoaderKind,
    signal: CancellationToken,
}

impl BorderedLoader {
    pub fn new(
        tui: Arc<Tui>,
        theme: Arc<Theme>,
        message: &str,
        options: Option<&BorderedLoaderOptions>,
    ) -> Self {
        let cancellable = options.and_then(|o| o.cancellable).unwrap_or(true);
        let container = Container::new();
        let border_theme = theme.clone();
        let border_color: Arc<dyn Fn(String) -> String + Send + Sync> =
            Arc::new(move |s: String| border_theme.fg(&ThemeColor::Border, &s));
        container.add_child(Arc::new(DynamicBorder::new(Some(border_color.clone()))));
        let spinner_theme = theme.clone();
        let spinner: StyleFn = Arc::new(move |s: &str| spinner_theme.fg(&ThemeColor::Accent, s));
        let message_theme = theme.clone();
        let message_color: StyleFn = Arc::new(move |s: &str| message_theme.fg(&ThemeColor::Muted, s));
        let signal = CancellationToken::new();
        let loader = if cancellable {
            let loader = CancellableLoader::new(&tui, spinner, message_color, message, None);
            let token = signal.clone();
            loader.signal().on_abort(move || token.cancel());
            container.add_child(loader.clone() as ComponentRef);
            LoaderKind::Cancellable(loader)
        } else {
            let loader = Loader::new(&tui, spinner, message_color, message, None);
            container.add_child(loader.clone() as ComponentRef);
            LoaderKind::Plain(loader)
        };
        if cancellable {
            container.add_child(Arc::new(Spacer::new(1)));
            container.add_child(Arc::new(Text::new(key_hint("tui.select.cancel".to_string(), "cancel"), 1, 0, None)));
        }
        container.add_child(Arc::new(Spacer::new(1)));
        container.add_child(Arc::new(DynamicBorder::new(Some(border_color))));
        Self { container, loader, signal }
    }

    pub fn signal(&self) -> CancellationToken {
        self.signal.clone()
    }

    pub fn set_on_abort(&self, value: Option<Arc<dyn Fn() + Send + Sync>>) {
        if let LoaderKind::Cancellable(loader) = &self.loader {
            loader.set_on_abort(value);
        }
    }

    pub fn dispose(&self) {
        match &self.loader {
            LoaderKind::Cancellable(loader) => loader.dispose(),
            LoaderKind::Plain(loader) => loader.stop(),
        }
    }
}

impl Component for BorderedLoader {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn invalidate(&self) {
        self.container.invalidate();
    }

    fn handle_input(&self, data: &str) {
        if let LoaderKind::Cancellable(loader) = &self.loader {
            loader.handle_input(data);
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }
}
