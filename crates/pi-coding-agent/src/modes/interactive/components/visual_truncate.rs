//! Port of `packages/coding-agent/src/modes/interactive/components/visual-truncate.ts`.
//!
//! Shared utility for truncating text to visual lines (accounting for line wrapping).
//! Used by tool renderers and bash-execution.rs for consistent behavior.

use parking_lot::Mutex;
use pi_tui::components::text::Text;
use pi_tui::tui::Component;
use pi_tui::utils::truncate_to_width;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualTruncateResult {
    /// The visual lines to display
    pub visual_lines: Vec<String>,
    /// Number of visual lines that were skipped (hidden)
    pub skipped_count: i64,
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
    if text.is_empty() {
        return VisualTruncateResult { visual_lines: Vec::new(), skipped_count: 0 };
    }
    let padding_x = padding_x.unwrap_or(0).max(0) as usize;
    let keep = keep.unwrap_or(TruncateToVisualLinesKeep::End);

    // Create a temporary Text component to render and get visual lines
    let temp_text = Text::new(text, padding_x, 0, None);
    let all_visual_lines = temp_text.render(width.max(0) as usize);

    let max = max_visual_lines.max(0) as usize;
    if all_visual_lines.len() <= max {
        return VisualTruncateResult { visual_lines: all_visual_lines, skipped_count: 0 };
    }

    let truncated_lines = match keep {
        TruncateToVisualLinesKeep::Start => all_visual_lines[..max].to_vec(),
        // `slice(-0)` is the whole array in JS.
        TruncateToVisualLinesKeep::End if max == 0 => all_visual_lines.clone(),
        TruncateToVisualLinesKeep::End => all_visual_lines[all_visual_lines.len() - max..].to_vec(),
    };
    let skipped_count = all_visual_lines.len() as i64 - max_visual_lines;

    VisualTruncateResult { visual_lines: truncated_lines, skipped_count }
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
    options: VisualLinePreviewOptions,
    cache: Mutex<Option<(usize, Vec<String>)>>,
}

impl VisualLinePreview {
    pub fn new(options: &VisualLinePreviewOptions) -> Self {
        Self { options: options.clone(), cache: Mutex::new(None) }
    }
}

impl Component for VisualLinePreview {
    fn render(&self, width: usize) -> Vec<String> {
        if let Some((cached_width, lines)) = &*self.cache.lock()
            && *cached_width == width
        {
            return lines.clone();
        }
        let VisualLinePreviewOptions { text, max_visual_lines, keep, format_hint } = &self.options;
        let preview = truncate_to_visual_lines(text, *max_visual_lines, width as i64, Some(0), Some(*keep));
        let lines = preview.visual_lines;
        let result = if preview.skipped_count > 0 {
            let hint = truncate_to_width(&format_hint(preview.skipped_count as f64), width, "...", false);
            match keep {
                TruncateToVisualLinesKeep::Start => {
                    let mut out = lines;
                    out.push(hint);
                    out
                }
                TruncateToVisualLinesKeep::End => {
                    let mut out = vec![hint];
                    out.extend(lines);
                    out
                }
            }
        } else {
            lines
        };
        *self.cache.lock() = Some((width, result.clone()));
        result
    }

    fn invalidate(&self) {
        *self.cache.lock() = None;
    }
}
