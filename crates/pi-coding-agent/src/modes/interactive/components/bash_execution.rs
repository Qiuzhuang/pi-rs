//! Port of `packages/coding-agent/src/modes/interactive/components/bash-execution.ts`.
//!
//! Component for displaying bash command execution with streaming output.

use crate::core::tools::truncate::{
    DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncationOptions, TruncationResult, truncate_tail,
};
use crate::modes::interactive::components::dynamic_border::DynamicBorder;
use crate::modes::interactive::components::keybinding_hints::{key_hint, key_text};
use crate::modes::interactive::components::visual_truncate::truncate_to_visual_lines;
use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use crate::utils::ansi::strip_ansi;
use parking_lot::Mutex;
use pi_tui::components::loader::Loader;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, ComponentRef, Container, MouseEvent, MouseResult, Tui};
use std::sync::Arc;

// Preview line limit when not expanded (matches tool execution behavior)
const PREVIEW_LINES: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Complete,
    Cancelled,
    Error,
}

struct State {
    output_lines: Vec<String>,
    status: Status,
    exit_code: Option<i64>,
    truncation_result: Option<TruncationResult>,
    full_output_path: Option<String>,
    expanded: bool,
    output_pad: usize,
}

/// Collapsed output preview, re-wrapped for the width it renders at.
struct PreviewLines {
    styled_input: String,
    output_pad: usize,
    cache: Mutex<Option<(usize, Vec<String>)>>,
}

impl Component for PreviewLines {
    fn render(&self, width: usize) -> Vec<String> {
        let mut cache = self.cache.lock();
        if let Some((w, lines)) = &*cache
            && *w == width
        {
            return lines.clone();
        }
        let result =
            truncate_to_visual_lines(&self.styled_input, PREVIEW_LINES, width as i64, Some(self.output_pad as i64), None);
        *cache = Some((width, result.visual_lines.clone()));
        result.visual_lines
    }

    fn invalidate(&self) {
        *self.cache.lock() = None;
    }
}

/// TS: `extends Container`
pub struct BashExecutionComponent {
    container: Container,
    command: String,
    loader: Arc<Loader>,
    content_container: Arc<Container>,
    /// `dim` marks `!!` commands, whose output is excluded from the model context.
    color_key: ThemeColor,
    st: Mutex<State>,
}

impl BashExecutionComponent {
    pub fn new(
        command: &str,
        ui: Arc<Tui>,
        exclude_from_context: Option<bool>,
        output_pad: Option<i64>,
    ) -> Self {
        let color_key = if exclude_from_context.unwrap_or(false) { ThemeColor::Dim } else { ThemeColor::BashMode };
        let border_color: Arc<dyn Fn(String) -> String + Send + Sync> =
            Arc::new(move |s: String| theme().fg(&color_key, &s));
        let container = Container::new();

        // Add spacer
        container.add_child(Arc::new(Spacer::new(1)));

        // Top border
        container.add_child(Arc::new(DynamicBorder::new(Some(border_color.clone()))));

        // Content container (holds dynamic content between borders)
        let content_container = Arc::new(Container::new());
        container.add_child(content_container.clone() as ComponentRef);

        let loader = Loader::new(
            &ui,
            Arc::new(move |spinner: &str| theme().fg(&color_key, spinner)),
            Arc::new(|text: &str| theme().fg(&ThemeColor::Muted, text)),
            format!("Running... ({} to cancel)", key_text("tui.select.cancel".to_string())), // Plain text for loader
            None,
        );

        // Bottom border
        container.add_child(Arc::new(DynamicBorder::new(Some(border_color))));

        let component = Self {
            container,
            command: command.to_string(),
            loader,
            content_container,
            color_key,
            st: Mutex::new(State {
                output_lines: Vec::new(),
                status: Status::Running,
                exit_code: None,
                truncation_result: None,
                full_output_path: None,
                expanded: false,
                output_pad: output_pad.unwrap_or(1).max(0) as usize,
            }),
        };
        component.update_display();
        component
    }

    /// Set whether the output is expanded (shows full output) or collapsed (preview only).
    pub fn set_expanded(&self, expanded: bool) {
        self.st.lock().expanded = expanded;
        self.update_display();
    }

    pub fn set_output_pad(&self, output_pad: i64) {
        self.st.lock().output_pad = output_pad.max(0) as usize;
        self.update_display();
    }

    pub fn append_output(&self, chunk: &str) {
        // Strip ANSI codes and normalize line endings
        // Note: binary data is already sanitized in tui-renderer.ts executeBashCommand
        let clean = strip_ansi(chunk).unwrap_or_else(|_| chunk.to_string()).replace("\r\n", "\n").replace('\r', "\n");

        // Append to output lines
        let new_lines: Vec<&str> = clean.split('\n').collect();
        {
            let mut st = self.st.lock();
            if !st.output_lines.is_empty() && !new_lines.is_empty() {
                // Append first chunk to last line (incomplete line continuation)
                if let Some(last) = st.output_lines.last_mut() {
                    last.push_str(new_lines[0]);
                }
                st.output_lines.extend(new_lines[1..].iter().map(|s| s.to_string()));
            } else {
                st.output_lines.extend(new_lines.iter().map(|s| s.to_string()));
            }
        }

        self.update_display();
    }

    pub fn set_complete(
        &self,
        exit_code: Option<i64>,
        cancelled: bool,
        truncation_result: Option<&TruncationResult>,
        full_output_path: Option<&str>,
    ) {
        {
            let mut st = self.st.lock();
            st.exit_code = exit_code;
            st.status = if cancelled {
                Status::Cancelled
            } else if exit_code.is_some_and(|c| c != 0) {
                Status::Error
            } else {
                Status::Complete
            };
            st.truncation_result = truncation_result.cloned();
            st.full_output_path = full_output_path.map(str::to_string);
        }

        // Stop loader
        self.loader.stop();

        self.update_display();
    }

    fn update_display(&self) {
        let t = theme();
        let (full_output, status, exit_code, expanded, output_pad, truncated_before, full_output_path) = {
            let st = self.st.lock();
            (
                st.output_lines.join("\n"),
                st.status,
                st.exit_code,
                st.expanded,
                st.output_pad,
                st.truncation_result.as_ref().is_some_and(|r| r.truncated),
                st.full_output_path.clone(),
            )
        };

        // Apply truncation for LLM context limits (same limits as bash tool)
        let context_truncation = truncate_tail(
            &full_output,
            Some(&TruncationOptions { max_lines: Some(DEFAULT_MAX_LINES), max_bytes: Some(DEFAULT_MAX_BYTES) }),
        );

        // Get the lines to potentially display (after context truncation)
        let available_lines: Vec<&str> = if context_truncation.content.is_empty() {
            Vec::new()
        } else {
            context_truncation.content.split('\n').collect()
        };

        // Apply preview truncation based on expanded state
        let preview_start = available_lines.len().saturating_sub(PREVIEW_LINES as usize);
        let preview_logical_lines = &available_lines[preview_start..];
        let hidden_line_count = available_lines.len() - preview_logical_lines.len();

        // Rebuild content container
        let content = &self.content_container;
        content.clear();

        // Command header
        let header = Text::new(t.fg(&self.color_key, &t.bold(&format!("$ {}", self.command))), output_pad, 0, None);
        content.add_child(Arc::new(header));

        // Output
        if !available_lines.is_empty() {
            if expanded {
                // Show all lines
                let display_text =
                    available_lines.iter().map(|line| t.fg(&ThemeColor::Muted, line)).collect::<Vec<_>>().join("\n");
                content.add_child(Arc::new(Text::new(format!("\n{display_text}"), output_pad, 0, None)));
            } else {
                // Use shared visual truncation utility with width-aware caching
                let styled_output =
                    preview_logical_lines.iter().map(|line| t.fg(&ThemeColor::Muted, line)).collect::<Vec<_>>().join("\n");
                content.add_child(Arc::new(PreviewLines {
                    styled_input: format!("\n{styled_output}"),
                    output_pad,
                    cache: Mutex::new(None),
                }));
            }
        }

        // Loader or status
        if status == Status::Running {
            content.add_child(self.loader.clone() as ComponentRef);
        } else {
            let mut status_parts: Vec<String> = Vec::new();

            // Show how many lines are hidden (collapsed preview)
            if hidden_line_count > 0 {
                if expanded {
                    status_parts.push(format!(
                        "{}{}{}",
                        t.fg(&ThemeColor::Muted, "("),
                        key_hint("app.tools.expand".to_string(), "to collapse"),
                        t.fg(&ThemeColor::Muted, ")")
                    ));
                } else {
                    status_parts.push(format!(
                        "{}{}{}",
                        t.fg(&ThemeColor::Muted, &format!("... {hidden_line_count} more lines (")),
                        key_hint("app.tools.expand".to_string(), "to expand"),
                        t.fg(&ThemeColor::Muted, ")")
                    ));
                }
            }

            if status == Status::Cancelled {
                status_parts.push(t.fg(&ThemeColor::Warning, "(cancelled)"));
            } else if status == Status::Error {
                let code = exit_code.map(|c| c.to_string()).unwrap_or_else(|| "undefined".to_string());
                status_parts.push(t.fg(&ThemeColor::Error, &format!("(exit {code})")));
            }

            // Add truncation warning (context truncation, not preview truncation)
            let was_truncated = truncated_before || context_truncation.truncated;
            if was_truncated && let Some(path) = full_output_path.filter(|p| !p.is_empty()) {
                status_parts.push(t.fg(&ThemeColor::Warning, &format!("Output truncated. Full output: {path}")));
            }

            if !status_parts.is_empty() {
                content.add_child(Arc::new(Text::new(format!("\n{}", status_parts.join("\n")), output_pad, 0, None)));
            }
        }
    }

    /// Get the raw output for creating BashExecutionMessage.
    pub fn get_output(&self) -> String {
        self.st.lock().output_lines.join("\n")
    }

    /// Get the command that was executed.
    pub fn get_command(&self) -> String {
        self.command.clone()
    }
}

impl Component for BashExecutionComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render(width)
    }

    fn invalidate(&self) {
        self.container.invalidate();
        self.update_display();
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }

    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        self.container.child_components()
    }
}
