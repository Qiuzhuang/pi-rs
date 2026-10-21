//! Port of `packages/coding-agent/src/utils/syntax-highlight.ts`.

use indexmap::IndexMap;
use std::sync::Arc;

pub async fn load_all_highlight_languages() -> anyhow::Result<()> {
    todo!("port utils/syntax-highlight.ts:loadAllHighlightLanguages")
}

pub type HighlightFormatter = Arc<dyn Fn(String) -> String + Send + Sync>;

pub type HighlightTheme = IndexMap<String, HighlightFormatter>;

#[derive(Clone)]
pub struct HighlightOptions {
    pub language: Option<String>,
    pub ignore_illegals: Option<bool>,
    pub language_subset: Option<Vec<String>>,
    pub theme: Option<HighlightTheme>,
}

pub fn render_highlighted_html(html: &str, theme: Option<&HighlightTheme>) -> String {
    todo!("port utils/syntax-highlight.ts:renderHighlightedHtml")
}

pub fn highlight(code: &str, options: Option<&HighlightOptions>) -> String {
    todo!("port utils/syntax-highlight.ts:highlight")
}

pub fn supports_language(name: &str) -> bool {
    todo!("port utils/syntax-highlight.ts:supportsLanguage")
}
