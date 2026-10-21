//! Port of `packages/coding-agent/src/modes/interactive/components/mermaid.ts`.

use crate::core::extensions::types::MarkdownTransformer;
use crate::core::settings_schema::MermaidRenderingMode;
use crate::modes::interactive::theme::theme::Theme;
use std::sync::Arc;

#[derive(Clone)]
pub struct MermaidTransformerOptions {
    pub get_mode: Arc<dyn Fn() -> MermaidRenderingMode + Send + Sync>,
    pub theme: Option<Arc<Theme>>,
}

/// Create a transformer that replaces top-level Mermaid code blocks with Unicode terminal diagrams.
pub fn create_mermaid_markdown_transformer(
    options: &MermaidTransformerOptions,
) -> MarkdownTransformer {
    todo!("port modes/interactive/components/mermaid.ts:createMermaidMarkdownTransformer")
}
