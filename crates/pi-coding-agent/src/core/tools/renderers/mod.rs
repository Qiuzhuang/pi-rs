//! Port of `packages/coding-agent/src/core/tools/renderers/index.ts`.
//!
//! Built-in tool renderers, without the tools themselves.

pub mod bash;
pub mod edit;
pub mod find;
pub mod grep;
pub mod ls;
pub mod read;
pub mod write;

use crate::core::extensions::types::{ToolRenderContext, ToolRenderers};
use crate::core::tools::ToolName;
use indexmap::IndexMap;
use pi_tui::components::text::Text;
use pi_tui::tui::{ComponentRef, Container, downcast_component};
use serde_json::Value;
use std::sync::Arc;

pub use bash::create_shell_renderers;
pub use edit::edit_renderers;
pub use find::find_renderers;
pub use grep::grep_renderers;
pub use ls::ls_renderers;
pub use read::read_renderers;
pub use write::write_renderers;

/// Renderers for every built-in tool, keyed by tool name.
pub fn create_all_tool_renderers() -> IndexMap<ToolName, ToolRenderers> {
    IndexMap::from([
        (ToolName::Read, read_renderers()),
        (ToolName::Bash, create_shell_renderers("$")),
        (ToolName::Powershell, create_shell_renderers("PS>")),
        (ToolName::Edit, edit_renderers()),
        (ToolName::Write, write_renderers()),
        (ToolName::Grep, grep_renderers()),
        (ToolName::Find, find_renderers()),
        (ToolName::Ls, ls_renderers()),
    ])
}

/// Merge built-in renderers into a tool definition that does not supply its own.
///
/// `ToolExecutionComponent` used to do this lookup itself, which forced every presentation to import
/// the tool implementations. Callers do it now, so a process that renders can import renderers alone.
pub fn with_built_in_renderers(tool_name: &str, definition: Option<ToolRenderers>) -> Option<ToolRenderers> {
    let name: Option<ToolName> = serde_json::from_value(Value::String(tool_name.to_string())).ok();
    let built_in = name.and_then(|name| create_all_tool_renderers().shift_remove(&name));
    let Some(definition) = definition else {
        return built_in;
    };
    let Some(built_in) = built_in else {
        return Some(definition);
    };
    Some(ToolRenderers {
        render_shell: definition.render_shell,
        render_call: definition.render_call.or(built_in.render_call),
        render_result: definition.render_result.or(built_in.render_result),
    })
}

/// `(context.lastComponent as Text | undefined) ?? new Text("", 0, 0)`.
pub(crate) fn reuse_text(context: &ToolRenderContext) -> Arc<Text> {
    context
        .last_component
        .as_ref()
        .and_then(downcast_component::<Text>)
        .unwrap_or_else(|| Arc::new(Text::new("", 0, 0, None)))
}

/// `(context.lastComponent as Container | undefined) ?? new Container()`.
pub(crate) fn reuse_container(context: &ToolRenderContext) -> Arc<Container> {
    context
        .last_component
        .as_ref()
        .and_then(downcast_component::<Container>)
        .unwrap_or_else(|| Arc::new(Container::new()))
}

/// A text component handle as a `ComponentRef`.
pub(crate) fn as_component<T: pi_tui::tui::Component>(component: Arc<T>) -> ComponentRef {
    component
}

/// `args?.[key]`.
pub(crate) fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.as_object().and_then(|map| map.get(key))
}

/// `args?.file_path ?? args?.path` (nullish coalescing).
pub(crate) fn path_arg(args: &Value) -> Option<&Value> {
    match arg(args, "file_path") {
        Some(value) if !value.is_null() => Some(value),
        _ => arg(args, "path"),
    }
}

/// Lines of a result's text output for the list-style renderers (find/grep/ls).
pub(crate) fn format_list_output(
    output: &str,
    expanded: bool,
    collapsed_max: usize,
    theme: &crate::modes::interactive::theme::theme::Theme,
) -> String {
    use crate::modes::interactive::components::keybinding_hints::key_hint;
    use crate::modes::interactive::theme::theme_tokens::ThemeColor;
    let mut text = String::new();
    if !output.is_empty() {
        let lines: Vec<&str> = output.split('\n').collect();
        let max_lines = if expanded { lines.len() } else { collapsed_max };
        let display: Vec<String> = lines
            .iter()
            .take(max_lines)
            .map(|line| theme.fg(&ThemeColor::ToolOutput, line))
            .collect();
        text.push_str(&format!("\n{}", display.join("\n")));
        if lines.len() > max_lines {
            let remaining = lines.len() - max_lines;
            text.push_str(&format!(
                "{} {}{}",
                theme.fg(&ThemeColor::Muted, &format!("\n... ({remaining} more lines,")),
                key_hint("app.tools.expand".to_string(), "to expand"),
                theme.fg(&ThemeColor::Muted, ")")
            ));
        }
    }
    text
}
