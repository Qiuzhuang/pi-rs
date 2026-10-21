//! Port of `packages/coding-agent/src/modes/interactive/components/visual-truncate.ts`.

use pi_tui::tui::Component;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualTruncateResult {
    /// The visual lines to display
    pub visual_lines: Vec<String>,
    /// Number of visual lines that were skipped (hidden)
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub skipped_count: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TruncateToVisualLinesKeep {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "end")]
    End,
}

impl TruncateToVisualLinesKeep {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// Truncate text to a maximum number of visual lines.
/// This accounts for line wrapping based on terminal width.
///
/// @param text - The text content (may contain newlines)
/// @param maxVisualLines - Maximum number of visual lines to show
/// @param width - Terminal/render width
/// @param paddingX - Horizontal padding for Text component (default 0).
/// Use 0 when result will be placed in a Box (Box adds its own padding).
/// Use 1 when result will be placed in a plain Container.
/// @param keep - Which visual lines to keep: the last ones (default) or the first ones.
/// @returns The truncated visual lines and count of skipped lines
pub fn truncate_to_visual_lines(
    text: &str,
    max_visual_lines: i64,
    width: i64,
    padding_x: Option<i64>,
    keep: Option<TruncateToVisualLinesKeep>,
) -> VisualTruncateResult {
    todo!("port modes/interactive/components/visual-truncate.ts:truncateToVisualLines")
}

#[derive(Clone)]
pub struct VisualLinePreviewOptions {
    /// Styled text; may contain newlines.
    pub text: String,
    pub max_visual_lines: i64,
    /// Which visual lines to keep. The hint goes before kept end lines and after kept start lines.
    pub keep: TruncateToVisualLinesKeep,
    /// Styled hint line for the given number of hidden visual lines.
    pub format_hint: Arc<dyn Fn(f64) -> String + Send + Sync>,
}

/// Collapsed tool output limited to a number of visual lines, like bash output. Limiting logical
/// lines instead lets a single long line (such as minified JSON) wrap across the whole screen.
/// Caches its lines per width, since it renders on every frame for every result in the transcript.
/// TS: `implements Component`
pub struct VisualLinePreview {
    // TODO(port): private state (interior mutability; methods take &self)
}

impl VisualLinePreview {
    pub fn new(options: &VisualLinePreviewOptions) -> Self {
        todo!("port modes/interactive/components/visual-truncate.ts:VisualLinePreview.constructor")
    }
}

impl Component for VisualLinePreview {
    fn render(&self, width: usize) -> Vec<String> {
        todo!("port modes/interactive/components/visual-truncate.ts:VisualLinePreview.render")
    }

    fn invalidate(&self) {
        todo!("port modes/interactive/components/visual-truncate.ts:VisualLinePreview.invalidate")
    }
}
