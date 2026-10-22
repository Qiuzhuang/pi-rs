//! Port of `packages/coding-agent/src/core/tools/render-utils.ts`.

use crate::core::extensions::types::{ToolRenderContext, ToolRenderState};
use crate::core::tools::js_util::{js_number_string, js_trim, utf16_len, utf16_prefix};
use crate::modes::interactive::theme::theme::Theme;
use crate::modes::interactive::theme::theme_tokens::ThemeColor;
use pi_ai::types::UserContentBlock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn shorten_path(path: Value) -> String {
    match path {
        Value::String(path) => shorten_path_str(&path),
        _ => String::new(),
    }
}

/// [`shorten_path`] for a string.
pub fn shorten_path_str(path: &str) -> String {
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !home.is_empty()
        && let Some(rest) = path.strip_prefix(&home)
    {
        return format!("~{rest}");
    }
    path.to_string()
}

/// `pathToFileURL(path).href`.
pub(crate) fn path_to_file_url(path: &str) -> String {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| format!("file://{path}"))
}

pub fn link_path(styled_text: &str, raw_path: &str, cwd: &str) -> String {
    if !pi_tui::terminal_image::get_capabilities().hyperlinks {
        return styled_text.to_string();
    }
    let absolute_path = crate::core::tools::path_utils::resolve_path_with_options(raw_path, cwd, &Default::default());
    pi_tui::terminal_image::hyperlink(styled_text, &path_to_file_url(&absolute_path))
}

/// `str(value)`: strings as-is, `null`/`undefined` as `""`, anything else `None` (invalid).
pub fn str(value: Value) -> Option<String> {
    str_ref(Some(&value))
}

/// [`str`] for an optional argument (`None` is `undefined`).
pub fn str_ref(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => None,
    }
}

/// `Number(string)` for the numeric strings tool arguments use.
fn js_string_to_number(text: &str) -> f64 {
    let trimmed = js_trim(text);
    if trimmed.is_empty() {
        return 0.0;
    }
    let lower = trimmed.to_ascii_lowercase();
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = lower.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).map(|v| v as f64).unwrap_or(f64::NAN);
        }
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if lower.contains("inf") || lower.contains("nan") {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

fn parse_line_number(value: &Value) -> Option<f64> {
    let number = match value {
        Value::String(s) if !js_trim(s).is_empty() => js_string_to_number(s),
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

fn is_nullish(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

/// Format finite numbers or numeric strings as a read line range, omitting malformed ranges.
pub fn format_read_line_range(offset: Value, limit: Value) -> String {
    format_read_line_range_ref(Some(&offset), Some(&limit))
}

/// [`format_read_line_range`] for optional arguments (`None` is `undefined`).
pub fn format_read_line_range_ref(offset: Option<&Value>, limit: Option<&Value>) -> String {
    // Strict schemas represent omitted optional arguments as null.
    if is_nullish(offset) && is_nullish(limit) {
        return String::new();
    }
    let start = match offset {
        None | Some(Value::Null) => 1.0,
        Some(value) => match parse_line_number(value) {
            Some(start) => start,
            None => return String::new(),
        },
    };
    let Some(limit) = limit.filter(|l| !l.is_null()) else {
        return format!(":{}", js_number_string(start));
    };
    let Some(count) = parse_line_number(limit) else {
        return String::new();
    };
    let end = start + count - 1.0;
    let end_part = if end != 0.0 && !end.is_nan() {
        format!("-{}", js_number_string(end))
    } else {
        String::new()
    };
    format!(":{}{end_part}", js_number_string(start))
}

pub fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
}

pub fn normalize_display_text(text: &str) -> String {
    text.replace('\r', "")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTextOutputResultContent {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTextOutputResult {
    pub content: Vec<GetTextOutputResultContent>,
}

/// Text of a tool result for display: text blocks (ANSI stripped, sanitized) and, when inline
/// images are not shown, a fallback line per image.
pub fn get_text_output(result: Option<&GetTextOutputResult>, show_images: bool) -> String {
    let Some(result) = result else {
        return String::new();
    };
    let blocks: Vec<UserContentBlock> = result
        .content
        .iter()
        .filter_map(|c| match c.r#type.as_str() {
            "text" => Some(UserContentBlock::text(c.text.clone().unwrap_or_default())),
            "image" => Some(UserContentBlock::Image(pi_ai::types::ImageContent {
                data: c.data.clone().unwrap_or_default(),
                mime_type: c.mime_type.clone().unwrap_or_default(),
            })),
            _ => None,
        })
        .collect();
    get_content_text_output(Some(&blocks), show_images)
}

/// [`get_text_output`] for content blocks.
pub fn get_content_text_output(content: Option<&[UserContentBlock]>, show_images: bool) -> String {
    let Some(content) = content else {
        return String::new();
    };
    let mut output = content
        .iter()
        .filter_map(UserContentBlock::as_text)
        .map(|c| {
            let stripped = crate::utils::ansi::strip_ansi(&c.text).unwrap_or_else(|_| c.text.clone());
            crate::utils::shell::sanitize_binary_output(&stripped).replace('\r', "")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let images: Vec<_> = content.iter().filter_map(UserContentBlock::as_image).collect();
    let caps = pi_tui::terminal_image::get_capabilities();
    if !images.is_empty() && (caps.images.is_none() || !show_images) {
        let indicators = images
            .iter()
            .map(|img| {
                let mime_type = if img.mime_type.is_empty() { "image/unknown" } else { img.mime_type.as_str() };
                let dims = if !img.data.is_empty() && !img.mime_type.is_empty() {
                    pi_tui::terminal_image::get_image_dimensions(&img.data, &img.mime_type)
                } else {
                    None
                };
                pi_tui::terminal_image::image_fallback(mime_type, dims, None)
            })
            .collect::<Vec<_>>()
            .join("\n");
        output = if output.is_empty() {
            indicators
        } else {
            format!("{output}\n{indicators}")
        };
    }
    output
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRenderResultLike {
    pub content: Vec<UserContentBlock>,
    pub details: Value,
}

const COLLAPSED_ARGS_CHARS: usize = 100;

/// `JSON.stringify(value)` (compact).
fn json_stringify(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// Generic tool call header: the title followed by the arguments. Collapsed, they are `key=value`
/// pairs on the title line, cut to {@link COLLAPSED_ARGS_CHARS}. Expanded, each is a `key: value`
/// line below the title, with strings shown raw and continuation lines indented.
pub fn format_tool_call_with_args(title: &str, args: Value, theme: Arc<Theme>, expanded: bool) -> String {
    format_tool_call_with_args_ref(title, Some(&args), &theme, expanded)
}

/// [`format_tool_call_with_args`] with borrowed arguments.
pub fn format_tool_call_with_args_ref(title: &str, args: Option<&Value>, theme: &Theme, expanded: bool) -> String {
    let header = theme.fg(&ThemeColor::ToolTitle, &theme.bold(title));
    let Some(args) = args.filter(|a| !a.is_null()) else {
        return header;
    };
    let entries: Vec<(String, Value)> = match args {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        other => vec![("args".to_string(), other.clone())],
    };
    if entries.is_empty() {
        return header;
    }
    if expanded {
        let lines: Vec<String> = entries
            .iter()
            .map(|(key, value)| {
                let text = match value {
                    Value::String(s) => s.clone(),
                    other => serde_json::to_string_pretty(other).unwrap_or_default(),
                };
                format!(
                    "  {key}: {}",
                    replace_tabs(&text).replace('\r', "").split('\n').collect::<Vec<_>>().join("\n    ")
                )
            })
            .collect();
        return format!("{header}\n{}", theme.fg(&ThemeColor::Muted, &lines.join("\n")));
    }
    let pairs = entries
        .iter()
        .map(|(key, value)| format!("{key}={}", json_stringify(value)))
        .collect::<Vec<_>>()
        .join(" ");
    let preview = if utf16_len(&pairs) > COLLAPSED_ARGS_CHARS {
        format!("{}...", utf16_prefix(&pairs, COLLAPSED_ARGS_CHARS - 3))
    } else {
        pairs
    };
    format!("{header} {}", theme.fg(&ThemeColor::Muted, &preview))
}

/// Duration as `4.2s` below a minute, then `1m 30s` and `2h 3m 4s`.
pub fn format_tool_duration(ms: f64) -> String {
    let seconds = ms / 1000.0;
    if seconds < 60.0 {
        return format!("{}s", crate::core::tools::js_util::js_to_fixed(seconds, 1));
    }
    let total_seconds = seconds.floor() as i64;
    let minutes = total_seconds / 60;
    let remainder = total_seconds % 60;
    if minutes < 60 {
        return format!("{minutes}m {remainder}s");
    }
    format!("{}h {}m {remainder}s", minutes / 60, minutes % 60)
}

/// Per-row renderer state: one value per type, so helpers such as the tool timer and a
/// renderer's own state can share `ToolRenderContext.state` like fields of the TS state object.
#[derive(Default)]
pub struct RenderStateMap {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

/// Run `f` with the `T` stored in a row's renderer state, creating it with `Default` first.
pub fn with_render_state<T, R>(state: &ToolRenderState, f: impl FnOnce(&mut T) -> R) -> R
where
    T: Default + Send + Sync + 'static,
{
    let mut slot = state.lock();
    if !slot.as_ref().is_some_and(|b| b.is::<RenderStateMap>()) {
        *slot = Some(Box::new(RenderStateMap::default()));
    }
    let map = slot
        .as_mut()
        .and_then(|b| b.downcast_mut::<RenderStateMap>())
        .expect("render state map");
    let value = map
        .values
        .entry(TypeId::of::<T>())
        .or_insert_with(|| Box::new(T::default()));
    f(value.downcast_mut::<T>().expect("render state type"))
}

/// Stops a running interval when dropped or stopped.
#[derive(Debug, Clone, Default)]
pub struct IntervalHandle {
    stopped: Arc<AtomicBool>,
}

impl IntervalHandle {
    /// Call `tick` every `period` until stopped (TS `setInterval`).
    pub fn start(period: std::time::Duration, tick: Arc<dyn Fn() + Send + Sync>) -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(period);
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                tick();
            }
        });
        Self { stopped }
    }

    /// `clearInterval`.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

/// Renderer state of {@link startToolTimer} and {@link toolTimerText}, kept in `ToolRenderContext.state`.
#[derive(Debug, Clone, Default)]
pub struct ToolTimerState {
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub interval: Option<IntervalHandle>,
}

type ClockFn = Arc<dyn Fn() -> i64 + Send + Sync>;

static TOOL_TIMER_CLOCK: parking_lot::RwLock<Option<ClockFn>> = parking_lot::RwLock::new(None);

/// Replace the clock (epoch milliseconds) that tool timers read, or restore the system clock with
/// `None`. The equivalent of the TS tests' `vi.useFakeTimers()` / `vi.setSystemTime()`.
#[doc(hidden)]
pub fn set_tool_timer_clock(clock: Option<ClockFn>) {
    *TOOL_TIMER_CLOCK.write() = clock;
}

fn now_ms() -> i64 {
    let clock = TOOL_TIMER_CLOCK.read().clone();
    match clock {
        Some(clock) => clock(),
        None => chrono::Utc::now().timestamp_millis(),
    }
}

/// Start the row's clock once execution starts. Call from `renderCall`.
pub fn start_tool_timer(context: &ToolRenderContext) {
    let execution_started = context.execution_started;
    with_render_state::<ToolTimerState, _>(&context.state, |state| {
        if execution_started && state.started_at.is_none() {
            state.started_at = Some(now_ms());
            state.ended_at = None;
        }
    });
}

/// The row's timing line, from `renderResult`: `Elapsed 4.2s` while the result is partial, redrawn
/// every second, and `Took 4.2s` once it is final. Undefined when the row has no start time and no
/// recorded duration.
pub fn tool_timer_text(context: &ToolRenderContext, is_partial: bool) -> Option<String> {
    let invalidate = context.invalidate.clone();
    let is_error = context.is_error;
    let (started_at, ended_at) = with_render_state::<ToolTimerState, _>(&context.state, |state| {
        if state.started_at.is_some() && is_partial && state.interval.is_none() {
            let invalidate = invalidate.clone();
            state.interval = Some(IntervalHandle::start(
                std::time::Duration::from_secs(1),
                Arc::new(move || invalidate()),
            ));
        }
        if !is_partial || is_error {
            if state.ended_at.is_none() {
                state.ended_at = Some(now_ms());
            }
            if let Some(interval) = state.interval.take() {
                interval.stop();
            }
        }
        (state.started_at, state.ended_at)
    });
    // A final result's recorded duration wins: it is monotonic and survives reloads. The renderer's
    // own clock is the fallback for live progress and for results stored without one.
    if !is_partial && let Some(duration_ms) = context.duration_ms {
        return Some(format!("Took {}", format_tool_duration(duration_ms as f64)));
    }
    let started_at = started_at?;
    let end_time = ended_at.unwrap_or_else(now_ms);
    Some(format!(
        "{} {}",
        if is_partial { "Elapsed" } else { "Took" },
        format_tool_duration((end_time - started_at) as f64)
    ))
}

pub fn invalid_arg_text(theme: Arc<Theme>) -> String {
    invalid_arg_text_ref(&theme)
}

/// [`invalid_arg_text`] with a borrowed theme.
pub fn invalid_arg_text_ref(theme: &Theme) -> String {
    theme.fg(&ThemeColor::Error, "[invalid arg]")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderToolPathOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_fallback: Option<String>,
}

/// `raw_path` is `None` for an invalid (non-string) argument.
pub fn render_tool_path(
    raw_path: Option<&str>,
    theme: Arc<Theme>,
    cwd: &str,
    options: Option<&RenderToolPathOptions>,
) -> String {
    render_tool_path_ref(raw_path, &theme, cwd, options)
}

/// [`render_tool_path`] with a borrowed theme.
pub fn render_tool_path_ref(
    raw_path: Option<&str>,
    theme: &Theme,
    cwd: &str,
    options: Option<&RenderToolPathOptions>,
) -> String {
    let Some(raw_path) = raw_path else {
        return invalid_arg_text_ref(theme);
    };
    let value = if raw_path.is_empty() {
        options.and_then(|o| o.empty_fallback.clone()).unwrap_or_default()
    } else {
        raw_path.to_string()
    };
    if value.is_empty() {
        return theme.fg(&ThemeColor::ToolOutput, "...");
    }
    link_path(&theme.fg(&ThemeColor::Accent, &shorten_path_str(&value)), &value, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_line_range_formatting() {
        let f = format_read_line_range_ref;
        assert_eq!(f(None, None), "");
        assert_eq!(format_read_line_range(json!(null), json!(null)), "");
        assert_eq!(f(Some(&json!(5)), None), ":5");
        assert_eq!(f(Some(&json!(5)), Some(&json!(10))), ":5-14");
        assert_eq!(f(None, Some(&json!(10))), ":1-10");
        assert_eq!(f(Some(&json!("5")), Some(&json!("10"))), ":5-14");
        assert_eq!(f(Some(&json!("abc")), Some(&json!(10))), "");
        assert_eq!(f(Some(&json!(5)), Some(&json!("x"))), "");
        assert_eq!(f(Some(&json!(1)), Some(&json!(0))), ":1");
        assert_eq!(f(Some(&json!(true)), None), "");
    }

    #[test]
    fn durations() {
        assert_eq!(format_tool_duration(4200.0), "4.2s");
        assert_eq!(format_tool_duration(90_000.0), "1m 30s");
        assert_eq!(format_tool_duration(7_384_000.0), "2h 3m 4s");
    }

    #[test]
    fn str_values() {
        assert_eq!(str_ref(None), Some(String::new()));
        assert_eq!(str(json!(null)), Some(String::new()));
        assert_eq!(str(json!("a")), Some("a".into()));
        assert_eq!(str(json!(1)), None);
    }
}
