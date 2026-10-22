//! Port of `packages/coding-agent/src/core/tools/renderers/edit.ts`.
//!
//! Presentation for the edit tool.

use super::{as_component, path_arg, reuse_container};
use crate::core::extensions::types::{ToolRenderContext, ToolRenderResultOptions, ToolRenderers};
use crate::core::tools::edit_diff::{ComputeEditsDiffResult, Edit, EditDiffError, EditDiffResult, compute_edits_diff};
use crate::core::tools::render_utils::{render_tool_path_ref, str_ref, with_render_state};
use crate::modes::interactive::components::diff::{RenderDiffOptions, render_diff};
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::interactive::theme::theme_tokens::{ThemeBg, ThemeColor};
use pi_agent::types::AgentToolResult;
use pi_tui::components::box_component::BoxComponent;
use pi_tui::components::spacer::Spacer;
use pi_tui::components::text::Text;
use pi_tui::tui::{Component, ComponentRef, MouseEvent, MouseResult, downcast_component};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EditPreview {
    EditDiffResult(EditDiffResult),
    EditDiffError(EditDiffError),
}

impl From<ComputeEditsDiffResult> for EditPreview {
    fn from(result: ComputeEditsDiffResult) -> Self {
        match result {
            ComputeEditsDiffResult::EditDiffResult(r) => EditPreview::EditDiffResult(r),
            ComputeEditsDiffResult::EditDiffError(e) => EditPreview::EditDiffError(e),
        }
    }
}

/// Renderer state of the edit tool rows.
#[derive(Clone, Default)]
pub struct EditRenderState {
    pub call_component: Option<Arc<EditCallRenderComponent>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderableEditArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "file_path", default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edits: Option<Vec<Edit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_text: Option<String>,
}

#[derive(Default)]
struct EditCallState {
    preview: Option<EditPreview>,
    preview_args_key: Option<String>,
    preview_pending: bool,
    settled_error: bool,
}

/// The edit call row: a `Box` with the preview fields of the TS `EditCallRenderComponent`.
pub struct EditCallRenderComponent {
    inner: BoxComponent,
    state: parking_lot::Mutex<EditCallState>,
}

impl EditCallRenderComponent {
    fn new() -> Self {
        Self {
            inner: BoxComponent::new(1, 1, Some(Arc::new(|text: &str| text.to_string()))),
            state: parking_lot::Mutex::new(EditCallState::default()),
        }
    }
}

impl Component for EditCallRenderComponent {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.render(width)
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.inner.handle_mouse(event)
    }

    fn invalidate(&self) {
        self.inner.invalidate();
    }
}

fn get_or_create_call_component(context: &ToolRenderContext) -> Arc<EditCallRenderComponent> {
    if let Some(component) = context
        .last_component
        .as_ref()
        .and_then(downcast_component::<EditCallRenderComponent>)
    {
        let stored = component.clone();
        with_render_state::<EditRenderState, _>(&context.state, |state| state.call_component = Some(stored));
        return component;
    }
    with_render_state::<EditRenderState, _>(&context.state, |state| {
        state
            .call_component
            .get_or_insert_with(|| Arc::new(EditCallRenderComponent::new()))
            .clone()
    })
}

struct PreviewInput {
    path: String,
    edits: Vec<Edit>,
}

fn get_renderable_preview_input(args: &Value) -> Option<PreviewInput> {
    let map = args.as_object()?;
    let path = match map.get("path") {
        Some(Value::String(p)) => p.clone(),
        _ => match map.get("file_path") {
            Some(Value::String(p)) => p.clone(),
            _ => return None,
        },
    };
    if path.is_empty() {
        return None;
    }
    if let Some(Value::Array(edits)) = map.get("edits")
        && !edits.is_empty()
    {
        let parsed: Option<Vec<Edit>> = edits
            .iter()
            .map(|edit| {
                let old_text = edit.get("oldText")?.as_str()?;
                let new_text = edit.get("newText")?.as_str()?;
                Some(Edit {
                    old_text: old_text.to_string(),
                    new_text: new_text.to_string(),
                })
            })
            .collect();
        if let Some(edits) = parsed {
            return Some(PreviewInput { path, edits });
        }
    }
    if let (Some(Value::String(old_text)), Some(Value::String(new_text))) = (map.get("oldText"), map.get("newText")) {
        return Some(PreviewInput {
            path,
            edits: vec![Edit {
                old_text: old_text.clone(),
                new_text: new_text.clone(),
            }],
        });
    }
    None
}

/// `JSON.stringify({ path, edits })`.
fn preview_args_key(input: &PreviewInput) -> String {
    serde_json::to_string(&serde_json::json!({ "path": input.path, "edits": input.edits })).unwrap_or_default()
}

fn format_edit_call(args: &Value, theme: &Theme, cwd: &str) -> String {
    let path_display = render_tool_path_ref(str_ref(path_arg(args)).as_deref(), theme, cwd, None);
    format!("{} {path_display}", theme.fg(&ThemeColor::ToolTitle, &theme.bold("edit")))
}

fn format_edit_result(
    args: &Value,
    preview: Option<&EditPreview>,
    result: &AgentToolResult,
    theme: &Theme,
    is_error: bool,
) -> Option<String> {
    let raw_path = str_ref(path_arg(args));
    let preview_diff = match preview {
        Some(EditPreview::EditDiffResult(r)) => Some(r.diff.as_str()),
        _ => None,
    };
    let preview_error = match preview {
        Some(EditPreview::EditDiffError(e)) => Some(e.error.as_str()),
        _ => None,
    };
    if is_error {
        let error_text = result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if error_text.is_empty() || Some(error_text.as_str()) == preview_error {
            return None;
        }
        return Some(theme.fg(&ThemeColor::Error, &error_text));
    }

    let result_diff = result
        .details
        .as_ref()
        .and_then(|d| d.get("diff"))
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    if let Some(result_diff) = result_diff
        && Some(result_diff.as_str()) != preview_diff
    {
        return Some(render_diff(
            &result_diff,
            Some(&RenderDiffOptions {
                file_path: raw_path.filter(|p| !p.is_empty()),
            }),
        ));
    }
    None
}

fn get_edit_header_bg(preview: Option<&EditPreview>, settled_error: bool, theme: &Arc<Theme>) -> pi_tui::StyleFn {
    let theme = theme.clone();
    let bg = match preview {
        Some(EditPreview::EditDiffError(_)) => ThemeBg::ToolErrorBg,
        Some(EditPreview::EditDiffResult(_)) => ThemeBg::ToolSuccessBg,
        None if settled_error => ThemeBg::ToolErrorBg,
        None => ThemeBg::ToolPendingBg,
    };
    Arc::new(move |text: &str| theme.bg(&bg, text))
}

fn build_edit_call_component(
    component: &EditCallRenderComponent,
    args: &Value,
    theme: &Arc<Theme>,
    cwd: &str,
    output_pad: usize,
) {
    let (preview, settled_error) = {
        let state = component.state.lock();
        (state.preview.clone(), state.settled_error)
    };
    component
        .inner
        .set_bg_fn(Some(get_edit_header_bg(preview.as_ref(), settled_error, theme)));
    component.inner.set_padding_x(output_pad);
    component.inner.clear();
    component
        .inner
        .add_child(Arc::new(Text::new(format_edit_call(args, theme, cwd), 0, 0, None)));

    let Some(preview) = preview else {
        return;
    };
    let body = match &preview {
        EditPreview::EditDiffError(e) => theme.fg(&ThemeColor::Error, &e.error),
        EditPreview::EditDiffResult(r) => render_diff(&r.diff, None),
    };
    component.inner.add_child(Arc::new(Spacer::new(1)));
    component.inner.add_child(Arc::new(Text::new(body, 0, 0, None)));
}

fn set_edit_preview(component: &EditCallRenderComponent, preview: EditPreview, args_key: Option<String>) -> bool {
    let mut state = component.state.lock();
    let changed = match (&state.preview, &preview) {
        (None, _) => true,
        (Some(EditPreview::EditDiffError(a)), EditPreview::EditDiffError(b)) => a.error != b.error,
        (Some(EditPreview::EditDiffResult(a)), EditPreview::EditDiffResult(b)) => {
            a.diff != b.diff || a.first_changed_line != b.first_changed_line
        }
        _ => true,
    };
    state.preview = Some(preview);
    state.preview_args_key = args_key;
    state.preview_pending = false;
    changed
}

/// Run a preview computation off the render path (TS `void computeEditsDiff(...).then(...)`).
fn spawn_preview<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(future);
        }
        Err(_) => {
            crate::utils::uncaught::spawn_thread(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    runtime.block_on(future);
                }
            });
        }
    }
}

pub fn edit_renderers() -> ToolRenderers {
    ToolRenderers {
        render_shell: None,
        render_call: Some(Arc::new(|args: &Value, theme: &Arc<Theme>, context: &ToolRenderContext| {
            let component = get_or_create_call_component(context);
            let preview_input = get_renderable_preview_input(args);
            let args_key = preview_input.as_ref().map(preview_args_key);

            let should_compute = {
                let mut state = component.state.lock();
                if state.preview_args_key != args_key {
                    state.preview = None;
                    state.preview_args_key = args_key.clone();
                    state.preview_pending = false;
                    state.settled_error = false;
                }
                let should = context.args_complete
                    && preview_input.is_some()
                    && state.preview.is_none()
                    && !state.preview_pending;
                if should {
                    state.preview_pending = true;
                }
                should
            };

            if should_compute && let Some(input) = preview_input {
                let request_key = args_key.clone();
                let request_component = component.clone();
                let invalidate = context.invalidate.clone();
                let cwd = context.cwd.clone();
                spawn_preview(async move {
                    let preview = match compute_edits_diff(&input.path, &input.edits, &cwd).await {
                        Ok(result) => EditPreview::from(result),
                        Err(err) => EditPreview::EditDiffError(EditDiffError { error: err.to_string() }),
                    };
                    let current_key = request_component.state.lock().preview_args_key.clone();
                    if current_key == request_key {
                        set_edit_preview(&request_component, preview, request_key);
                        invalidate();
                    }
                });
            }

            build_edit_call_component(&component, args, theme, &context.cwd, context.output_pad);
            as_component(component)
        })),
        render_result: Some(Arc::new(
            |result: &AgentToolResult, _options: ToolRenderResultOptions, theme: &Arc<Theme>, context: &ToolRenderContext| {
                let call_component =
                    with_render_state::<EditRenderState, _>(&context.state, |state| state.call_component.clone());
                let preview_input = get_renderable_preview_input(&context.args);
                let args_key = preview_input.as_ref().map(preview_args_key);
                let result_diff = if context.is_error {
                    None
                } else {
                    result.details.as_ref().and_then(|d| d.get("diff")).and_then(Value::as_str).map(str::to_string)
                };
                let first_changed_line = result
                    .details
                    .as_ref()
                    .and_then(|d| d.get("firstChangedLine"))
                    .and_then(Value::as_i64);
                if let Some(call_component) = &call_component {
                    let mut changed = false;
                    if let Some(result_diff) = result_diff {
                        changed = set_edit_preview(
                            call_component,
                            EditPreview::EditDiffResult(EditDiffResult {
                                diff: result_diff,
                                first_changed_line,
                            }),
                            args_key,
                        ) || changed;
                    }
                    {
                        let mut state = call_component.state.lock();
                        if state.settled_error != context.is_error {
                            state.settled_error = context.is_error;
                            changed = true;
                        }
                    }
                    if changed {
                        build_edit_call_component(call_component, &context.args, theme, &context.cwd, context.output_pad);
                    }
                }

                let preview = call_component.as_ref().and_then(|c| c.state.lock().preview.clone());
                let output = format_edit_result(&context.args, preview.as_ref(), result, theme, context.is_error);
                let component = reuse_container(context);
                component.clear();
                let Some(output) = output else {
                    return as_component(component);
                };
                component.add_child(Arc::new(Spacer::new(1)));
                let child: ComponentRef = Arc::new(Text::new(output, context.output_pad, 0, None));
                component.add_child(child);
                as_component(component)
            },
        )),
    }
}
