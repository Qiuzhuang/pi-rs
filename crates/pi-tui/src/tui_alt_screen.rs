//! Fullscreen (alternate screen) renderer with an application-owned, scrollable viewport,
//! mouse support, text selection and transcript search. Port of `tui-alt-screen.ts`.
//!
//! Create with [`Tui::alt_screen`] / [`Tui::alt_screen_with`]. Viewport methods such as
//! [`Tui::scroll_by`] are implemented here and are no-ops in regular (main screen) mode.

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use base64::Engine;

use crate::StyleFn;
use crate::alt_screen_search::{
    AltScreenSearchComponent, AltScreenSearchIndex, AltScreenSearchMatch, NavigationButtonStyle,
    get_alt_screen_search_match_key,
};
use crate::components::alt_screen_flash::AltScreenFlashContainer;
use crate::components::scroll_view::{Overscroll, ScrollFollow, ScrollView, ScrollViewOptions, ScrollViewScrollToOptions};
use crate::event_loop::{self, BoxFuture, EventLoop, TimerId};
use crate::keybindings::get_keybindings;
use crate::keys::is_key_release;
use crate::layout::{
    LayoutFrame, ScrollbarGeometry, get_layout_boxes_at, get_scroll_view_box, get_scroll_views_at, get_scrollbar_geometry,
    render_layout_frame, starts_with_osc133_prompt, strip_osc133_zone_prefix,
};
use crate::layout_node::get_layout_node;
use crate::terminal::Terminal;
use crate::terminal_image::{
    ImageProtocol, TerminalCapabilities, delete_all_kitty_images, delete_all_kitty_placements, delete_kitty_image,
    get_capabilities, get_kitty_image_placement, get_kitty_image_placement_rows, is_image_line, set_capabilities,
};
use crate::tui::{
    CURSOR_MARKER, Component, ComponentRef, InputListenerResult, MouseButton, MouseDispatchTarget, MouseEvent,
    MouseEventType, MouseResult, OverlayAnchor, OverlayHandle, OverlayMargin, OverlayOptions, Screen, SizeValue, Tui,
    TuiMode, TuiOptions, TuiStopOptions, apply_line_resets, component_ptr, composite_tui_line, dispatch_mouse_event,
    extract_cursor_position, resolve_fake_cursors, retarget_mouse_event, same_component,
};
use crate::utils::{
    extract_ansi_code, get_grapheme_cell_range, get_osc8_link_at_column, js_trim_end, slice_by_column,
    strip_terminal_sequences, truncate_to_width, visible_width, word_segments,
};
use crate::wheel_scroll::{WheelScrollAccelerator, WheelScrollLines};

const ENTER_ALT_SCREEN: &str = "\x1b[?1049h";
const EXIT_ALT_SCREEN: &str = "\x1b[?1049l";
const DISABLE_AUTOWRAP: &str = "\x1b[?7l";
const ENABLE_AUTOWRAP: &str = "\x1b[?7h";
const ENABLE_BUTTON_MOTION_MOUSE: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1004h\x1b[?1006h";
const ENABLE_ALL_MOTION_MOUSE: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1004h\x1b[?1006h";
const DISABLE_MOUSE: &str = "\x1b[?1006l\x1b[?1004l\x1b[?1003l\x1b[?1002l\x1b[?1000l";
const FOCUS_IN: &str = "\x1b[I";
const FOCUS_OUT: &str = "\x1b[O";
const BEGIN_SYNCHRONIZED_OUTPUT: &str = "\x1b[?2026h";
const END_SYNCHRONIZED_OUTPUT: &str = "\x1b[?2026l";
const PAGE_SCROLL_OVERLAP: i64 = 4;
const ALT_WHEEL_SCROLL_MULTIPLIER: i64 = 5;
const MAX_CACHED_OFFSCREEN_KITTY_IMAGES: usize = 16;
const MAX_CACHED_OFFSCREEN_KITTY_TRANSMISSION_BYTES: usize = 32 * 1024 * 1024;
const MAX_CACHED_OFFSCREEN_KITTY_DECODED_BYTES: u64 = 64 * 1024 * 1024;
const DOUBLE_CLICK_INTERVAL_MS: f64 = 500.0;
const COPY_ERROR_FLASH_DURATION_MS: u64 = 5000;
/// Fullscreen owns mouse selection, so mirror common terminal word selection by keeping
/// paths and kebab-case tokens whole.
const TERMINAL_WORD_SELECTION_JOINERS: [&str; 2] = ["/", "-"];

/// Result of a custom clipboard copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopySelectionResult {
    Copied,
    Failed,
    /// Failure with a message to display.
    Error(String),
}

/// Copies selected text to the system clipboard.
pub type CopySelectionFn = Arc<dyn Fn(&str) -> BoxFuture<CopySelectionResult> + Send + Sync>;

/// Options for the fullscreen renderer.
#[derive(Clone, Default)]
pub struct TuiAltScreenOptions {
    /// Logical lines moved per mouse-wheel event (default: 1). Alt+wheel moves five times as far.
    pub wheel_scroll_lines: Option<WheelScrollLines>,
    /// Capture mouse events for viewport scrolling and application-owned selection (default: true).
    pub mouse: Option<bool>,
    /// Style a non-current transcript search match.
    pub search_match_style: Option<StyleFn>,
    /// Style the current transcript search match.
    pub search_current_match_style: Option<StyleFn>,
    /// Style a transcript search navigation button.
    pub search_navigation_button_style: Option<NavigationButtonStyle>,
    /// Render a clickable jump-to-end label for a follow-end primary scroll view scrolled away from its end.
    pub scroll_to_end_indicator: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// Open an OSC 8 hyperlink activated with a primary-button click.
    pub open_url: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    /// Handle an unmodified secondary-button press for clipboard paste (Windows only).
    pub on_right_click_paste: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Copy selected text on mouse release (default: true).
    pub copy_on_select: Option<bool>,
    /// Copy text to the clipboard; when omitted an OSC 52 write is used.
    pub copy_selection: Option<CopySelectionFn>,
}

#[derive(Debug, Clone, Copy)]
struct CachedKittyImage {
    transmission_generation: u64,
    transmission_bytes: usize,
    estimated_decoded_bytes: u64,
}

#[derive(Clone)]
struct SelectionPoint {
    row: i64,
    col: i64,
    scroll_view: Option<Arc<ScrollView>>,
    /// The point lies between cells rather than on a cell.
    boundary: bool,
}

#[derive(Clone)]
struct SelectionRange {
    start: SelectionPoint,
    end: SelectionPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionGranularity {
    Character,
    Word,
    Line,
}

#[derive(Clone)]
struct ClickTarget {
    timestamp: f64,
    count: u32,
    row: i64,
    scroll_view: Option<Arc<ScrollView>>,
    word_start: i64,
    word_end: i64,
}

#[derive(Debug, Clone, Copy)]
struct SgrMouseEvent {
    button: i64,
    x: i64,
    y: i64,
    release: bool,
}

#[derive(Debug, Clone, Copy)]
struct WheelEvent {
    direction: i32,
    x: i64,
    y: i64,
    button: i64,
}

#[derive(Clone)]
struct ScrollbarDrag {
    scroll_view: Arc<ScrollView>,
    grab_offset: i64,
}

struct ScrollbarTarget {
    scroll_view: Arc<ScrollView>,
    geometry: ScrollbarGeometry,
}

#[derive(Debug, Clone, Copy)]
struct ScrollToEndIndicatorRect {
    row: i64,
    column: i64,
    width: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchSelectionMode {
    Query,
    Retain,
    Next,
    Previous,
}

struct ActiveSearch {
    component: Arc<AltScreenSearchComponent>,
    index: AltScreenSearchIndex,
    overlay: Option<OverlayHandle>,
    query: String,
    matches: Vec<AltScreenSearchMatch>,
    selected_index: i64,
    selected_key: Option<String>,
    anchor_row: i64,
    selection_mode: SearchSelectionMode,
}

#[derive(Debug, Clone, Copy)]
struct SearchHighlightRange {
    start_col: i64,
    end_col: i64,
    current: bool,
}

#[derive(Clone)]
struct ComponentClick {
    timestamp: f64,
    count: u32,
    component: ComponentRef,
    x: i64,
    y: i64,
}

struct AltState {
    previous_screen: Vec<String>,
    last_document: Vec<String>,
    previous_screen_width: usize,
    previous_screen_height: usize,
    layout_root: Option<ComponentRef>,
    current_layout: Option<Arc<LayoutFrame>>,
    alt_screen_active: bool,
    image_protocol: Option<ImageProtocol>,
    saved_capabilities: Option<TerminalCapabilities>,
    /// Insertion-ordered like a JavaScript `Map`.
    uploaded_kitty_images: Vec<(u32, CachedKittyImage)>,
    selection_anchor: Option<SelectionPoint>,
    selection_focus: Option<SelectionPoint>,
    selection_granularity: SelectionGranularity,
    selection_initial_range: Option<SelectionRange>,
    last_click: Option<ClickTarget>,
    selection_drag_pointer: Option<(i64, i64)>,
    selection_auto_scroll_direction: i64,
    selection_auto_scroll_timer: Option<TimerId>,
    selection_press_active: bool,
    scrollbar_drag: Option<ScrollbarDrag>,
    scrollbar_hover: Option<Arc<ScrollView>>,
    scroll_to_end_indicator_rect: Option<ScrollToEndIndicatorRect>,
    active_search: Option<ActiveSearch>,
    pressed_url: Option<String>,
    selection_dragged: bool,
    mouse_capture: Option<MouseDispatchTarget>,
    mouse_press_target: Option<MouseDispatchTarget>,
    mouse_press_point: Option<(i64, i64)>,
    mouse_press_moved: bool,
    last_component_click: Option<ComponentClick>,
    wheel_scroll: WheelScrollAccelerator,
    copy_on_select: bool,
}

struct Config {
    mouse_enabled: bool,
    search_match_style: StyleFn,
    search_current_match_style: StyleFn,
    search_navigation_button_style: NavigationButtonStyle,
    scroll_to_end_indicator: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    open_url: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    on_right_click_paste: Option<Arc<dyn Fn() + Send + Sync>>,
    copy_selection: Option<CopySelectionFn>,
}

/// The implicit document: the TUI's children rendered as one column.
struct ImplicitDocument {
    tui: Weak<Tui>,
}

impl Component for ImplicitDocument {
    fn render(&self, width: usize) -> Vec<String> {
        self.tui.upgrade().map_or_else(Vec::new, |t| t.container.render_children(width))
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.tui.upgrade().and_then(|t| t.container.handle_mouse_with_owner(event, None))
    }

    fn invalidate(&self) {
        if let Some(t) = self.tui.upgrade() {
            for child in t.container.children() {
                child.invalidate();
            }
        }
    }
}

/// Fullscreen renderer state (see module docs).
pub(crate) struct AltScreen {
    st: Mutex<AltState>,
    implicit_scroll_view: Arc<ScrollView>,
    flashes: AltScreenFlashContainer,
    cfg: Config,
    tui: Weak<Tui>,
}

fn same_sv(a: &Option<Arc<ScrollView>>, b: &Option<Arc<ScrollView>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

fn env_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

fn decode_mouse_button(button: i64) -> MouseButton {
    match button & 3 {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        _ => MouseButton::None,
    }
}

fn parse_sgr(data: &str) -> Option<(i64, i64, i64, bool)> {
    let body = data.strip_prefix("\x1b[<")?;
    let (body, release) = if let Some(b) = body.strip_suffix('M') {
        (b, false)
    } else {
        (body.strip_suffix('m')?, true)
    };
    let mut parts = body.split(';');
    let mut num = || -> Option<i64> {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()
    };
    let (b, x, y) = (num()?, num()?, num()?);
    if parts.next().is_some() {
        return None;
    }
    Some((b, x, y, release))
}

fn parse_x10(data: &str) -> Option<(i64, i64, i64)> {
    let chars: Vec<char> = data.chars().collect();
    if chars.len() != 6 || !data.starts_with("\x1b[M") {
        return None;
    }
    Some((chars[3] as i64 - 32, chars[4] as i64 - 33, chars[5] as i64 - 33))
}

fn parse_wheel_event(data: &str) -> Option<WheelEvent> {
    if let Some((button, x, y, _)) = parse_sgr(data) {
        if button & 64 == 0 {
            return None;
        }
        let direction = button & 3;
        if direction != 0 && direction != 1 {
            return None;
        }
        return Some(WheelEvent { direction: if direction == 0 { -1 } else { 1 }, x: x - 1, y: y - 1, button });
    }
    if let Some((button, x, y)) = parse_x10(data) {
        if button & 64 == 0 {
            return None;
        }
        let direction = button & 3;
        if direction != 0 && direction != 1 {
            return None;
        }
        return Some(WheelEvent { direction: if direction == 0 { -1 } else { 1 }, x, y, button });
    }
    None
}

fn parse_sgr_mouse_event(data: &str) -> Option<SgrMouseEvent> {
    let (button, x, y, release) = parse_sgr(data)?;
    Some(SgrMouseEvent { button, x: x - 1, y: y - 1, release })
}

fn is_mouse_sequence(data: &str) -> bool {
    parse_sgr(data).is_some() || parse_x10(data).is_some()
}

impl AltScreen {
    fn new(tui: Weak<Tui>, options: TuiAltScreenOptions, event_loop: EventLoop) -> Self {
        let document: ComponentRef = Arc::new(ImplicitDocument { tui: tui.clone() });
        let implicit_scroll_view = ScrollView::new(
            document,
            ScrollViewOptions {
                follow: Some(ScrollFollow::End),
                primary: Some(true),
                event_loop: Some(event_loop.clone()),
                ..Default::default()
            },
        );
        let weak = tui.clone();
        let flashes = AltScreenFlashContainer::new(
            Arc::new(move || {
                if let Some(t) = weak.upgrade() {
                    t.request_render(false);
                }
            }),
            event_loop,
        );
        Self {
            st: Mutex::new(AltState {
                previous_screen: Vec::new(),
                last_document: Vec::new(),
                previous_screen_width: 0,
                previous_screen_height: 0,
                layout_root: None,
                current_layout: None,
                alt_screen_active: false,
                image_protocol: None,
                saved_capabilities: None,
                uploaded_kitty_images: Vec::new(),
                selection_anchor: None,
                selection_focus: None,
                selection_granularity: SelectionGranularity::Character,
                selection_initial_range: None,
                last_click: None,
                selection_drag_pointer: None,
                selection_auto_scroll_direction: 0,
                selection_auto_scroll_timer: None,
                selection_press_active: false,
                scrollbar_drag: None,
                scrollbar_hover: None,
                scroll_to_end_indicator_rect: None,
                active_search: None,
                pressed_url: None,
                selection_dragged: false,
                mouse_capture: None,
                mouse_press_target: None,
                mouse_press_point: None,
                mouse_press_moved: false,
                last_component_click: None,
                wheel_scroll: WheelScrollAccelerator::new(options.wheel_scroll_lines.unwrap_or(WheelScrollLines::Lines(1.0))),
                copy_on_select: options.copy_on_select.unwrap_or(true),
            }),
            implicit_scroll_view,
            flashes,
            cfg: Config {
                mouse_enabled: options.mouse.unwrap_or(true),
                search_match_style: options
                    .search_match_style
                    .unwrap_or_else(|| Arc::new(|t: &str| format!("\x1b[4m{t}\x1b[24m"))),
                search_current_match_style: options
                    .search_current_match_style
                    .unwrap_or_else(|| Arc::new(|t: &str| format!("\x1b[1;7m{t}\x1b[22;27m"))),
                search_navigation_button_style: options
                    .search_navigation_button_style
                    .unwrap_or_else(|| Arc::new(|t: &str, _| t.to_string())),
                scroll_to_end_indicator: options.scroll_to_end_indicator,
                open_url: options.open_url,
                on_right_click_paste: options.on_right_click_paste,
                copy_selection: options.copy_selection,
            },
            tui,
        }
    }

    fn st(&self) -> MutexGuard<'_, AltState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn layout(&self) -> Option<Arc<LayoutFrame>> {
        self.st().current_layout.clone()
    }

    fn primary_scroll_view(&self) -> Arc<ScrollView> {
        self.st()
            .current_layout
            .as_ref()
            .and_then(|l| l.primary_scroll_view.clone())
            .unwrap_or_else(|| self.implicit_scroll_view.clone())
    }

    fn request_render(&self, tui: &Tui) {
        tui.request_render(false);
    }

    // ---- hooks used by Tui ---------------------------------------------------------

    pub(crate) fn mounted_roots(&self, tui: &Tui) -> Vec<ComponentRef> {
        let root = self.st().layout_root.clone();
        match root {
            Some(root) => vec![root],
            None => tui.container.children(),
        }
    }

    pub(crate) fn render_root(&self, tui: &Tui, width: usize) -> Vec<String> {
        let root = self.st().layout_root.clone();
        match root {
            Some(root) => root.render(width),
            None => tui.container.render_children(width),
        }
    }

    pub(crate) fn before_terminal_start(&self, tui: &Tui) {
        self.stop_selection_auto_scroll(tui);
        self.st().selection_press_active = false;
        self.stop_scrollbar_hover();
        self.st().scrollbar_drag = None;
        self.flashes.dispose();
        let capabilities = get_capabilities();
        let mut invalidate = false;
        {
            let mut s = self.st();
            s.alt_screen_active = true;
            s.image_protocol = capabilities.images;
            s.uploaded_kitty_images.clear();
            if capabilities.images == Some(ImageProtocol::Iterm2) {
                s.saved_capabilities = Some(capabilities);
                invalidate = true;
            }
        }
        if invalidate {
            set_capabilities(TerminalCapabilities { images: None, ..capabilities });
            tui.invalidate();
        }
        {
            let mut s = self.st();
            s.last_document.clear();
            s.selection_anchor = None;
            s.selection_focus = None;
            s.selection_granularity = SelectionGranularity::Character;
            s.selection_initial_range = None;
            s.last_click = None;
            s.pressed_url = None;
            s.selection_dragged = false;
            s.mouse_capture = None;
            s.mouse_press_target = None;
            s.mouse_press_point = None;
            s.mouse_press_moved = false;
            s.last_component_click = None;
        }
        self.reset_render_state();
        let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
        let mouse_sequence = if env_is_set("TMUX")
            || env_is_set("ZELLIJ")
            || env_is_set("STY")
            || term.starts_with("tmux")
            || term.starts_with("screen")
        {
            ENABLE_BUTTON_MOTION_MOUSE
        } else {
            ENABLE_ALL_MOTION_MOUSE
        };
        tui.terminal().write(&format!(
            "{ENTER_ALT_SCREEN}{DISABLE_AUTOWRAP}{}\x1b[2J\x1b[H\x1b[?25l",
            if self.cfg.mouse_enabled { mouse_sequence } else { "" }
        ));
    }

    pub(crate) fn before_terminal_stop(&self, tui: &Tui, _options: TuiStopOptions) {
        self.close_search(tui);
        self.stop_selection_auto_scroll(tui);
        self.st().selection_press_active = false;
        self.stop_scrollbar_hover();
        {
            let mut s = self.st();
            s.scrollbar_drag = None;
            s.mouse_capture = None;
            s.mouse_press_target = None;
            s.mouse_press_point = None;
            s.mouse_press_moved = false;
        }
        self.flashes.dispose();
        if !self.st().alt_screen_active {
            return;
        }
        let delete = self.delete_kitty_images();
        tui.terminal().write(&format!(
            "{BEGIN_SYNCHRONIZED_OUTPUT}{delete}{}{ENABLE_AUTOWRAP}{END_SYNCHRONIZED_OUTPUT}",
            if self.cfg.mouse_enabled { DISABLE_MOUSE } else { "" }
        ));
        self.st().uploaded_kitty_images.clear();
    }

    pub(crate) fn after_terminal_stop(&self, tui: &Tui, options: TuiStopOptions) {
        if !self.st().alt_screen_active {
            return;
        }
        self.st().alt_screen_active = false;
        if options.preserve_screen {
            tui.terminal()
                .write(&format!("{BEGIN_SYNCHRONIZED_OUTPUT}{EXIT_ALT_SCREEN}\x1b[?25h{END_SYNCHRONIZED_OUTPUT}"));
        } else {
            let width = tui.terminal().columns().max(1);
            let rendered = self.render_root(tui, width);
            let document_lines: Vec<String> = resolve_fake_cursors(rendered, tui.get_show_hardware_cursor())
                .into_iter()
                .map(|l| strip_osc133_zone_prefix(&l).replace(CURSOR_MARKER, ""))
                .collect();
            let mut lines = document_lines;
            apply_line_resets(&mut lines);
            let last_document: Vec<String> = lines
                .into_iter()
                .map(|line| {
                    if is_image_line(&line) || visible_width(&line) <= width {
                        line
                    } else {
                        slice_by_column(&line, 0, width, true)
                    }
                })
                .collect();
            let mut buffer = format!("{BEGIN_SYNCHRONIZED_OUTPUT}{EXIT_ALT_SCREEN}{DISABLE_AUTOWRAP}");
            for (row, line) in last_document.iter().enumerate() {
                if row > 0 {
                    buffer.push_str("\r\n");
                }
                buffer.push_str("\r\x1b[2K");
                buffer.push_str(line);
            }
            buffer.push_str(&format!("\x1b[0m{ENABLE_AUTOWRAP}\r\n\x1b[?25h{END_SYNCHRONIZED_OUTPUT}"));
            self.st().last_document = last_document;
            tui.terminal().write(&buffer);
        }
        let saved = self.st().saved_capabilities.take();
        if let Some(saved) = saved {
            set_capabilities(saved);
        }
    }

    pub(crate) fn exit_restore_prefix(&self, base: String) -> String {
        if !self.st().alt_screen_active {
            return base;
        }
        format!(
            "{END_SYNCHRONIZED_OUTPUT}{}{ENABLE_AUTOWRAP}{base}",
            if self.cfg.mouse_enabled { DISABLE_MOUSE } else { "" }
        )
    }

    pub(crate) fn exit_restore_suffix(&self) -> String {
        if self.st().alt_screen_active { format!("{EXIT_ALT_SCREEN}\x1b[?25h") } else { String::new() }
    }

    fn delete_kitty_images(&self) -> String {
        if self.st().image_protocol == Some(ImageProtocol::Kitty) { delete_all_kitty_images() } else { String::new() }
    }

    fn prepare_kitty_screen(&self, screen: &[String]) -> (Vec<String>, String) {
        let mut s = self.st();
        let mut visible_ids: Vec<u32> = Vec::new();
        let lines: Vec<String> = screen
            .iter()
            .map(|line| {
                let Some(placement) = get_kitty_image_placement(line) else { return line.clone() };
                visible_ids.push(placement.image_id);
                let cached = s.uploaded_kitty_images.iter().position(|(id, _)| *id == placement.image_id);
                let cached_image = cached.map(|i| s.uploaded_kitty_images.remove(i).1);
                s.uploaded_kitty_images.push((
                    placement.image_id,
                    CachedKittyImage {
                        transmission_generation: placement.transmission_generation,
                        transmission_bytes: placement.transmission_bytes,
                        estimated_decoded_bytes: placement.estimated_decoded_bytes,
                    },
                ));
                if cached_image.is_some_and(|c| c.transmission_generation == placement.transmission_generation) {
                    placement.replacement_line
                } else {
                    line.clone()
                }
            })
            .collect();
        let mut offscreen_count = 0usize;
        let mut offscreen_transmission = 0usize;
        let mut offscreen_decoded = 0u64;
        for (id, c) in &s.uploaded_kitty_images {
            if visible_ids.contains(id) {
                continue;
            }
            offscreen_count += 1;
            offscreen_transmission += c.transmission_bytes;
            offscreen_decoded += c.estimated_decoded_bytes;
        }
        let mut evicted = String::new();
        let snapshot = s.uploaded_kitty_images.clone();
        for (id, c) in snapshot {
            if offscreen_count <= MAX_CACHED_OFFSCREEN_KITTY_IMAGES
                && offscreen_transmission <= MAX_CACHED_OFFSCREEN_KITTY_TRANSMISSION_BYTES
                && offscreen_decoded <= MAX_CACHED_OFFSCREEN_KITTY_DECODED_BYTES
            {
                break;
            }
            if visible_ids.contains(&id) {
                continue;
            }
            evicted.push_str(&delete_kitty_image(id));
            s.uploaded_kitty_images.retain(|(i, _)| *i != id);
            offscreen_count -= 1;
            offscreen_transmission -= c.transmission_bytes;
            offscreen_decoded -= c.estimated_decoded_bytes;
        }
        (lines, evicted)
    }

    pub(crate) fn reset_render_state(&self) {
        let mut s = self.st();
        s.previous_screen.clear();
        s.previous_screen_width = 0;
        s.previous_screen_height = 0;
        s.current_layout = None;
    }

    // ---- viewport API ----------------------------------------------------------------

    fn set_layout_root(&self, tui: &Tui, component: Option<ComponentRef>) {
        {
            let mut s = self.st();
            let same = match (&s.layout_root, &component) {
                (None, None) => true,
                (Some(a), Some(b)) => same_component(a, b),
                _ => false,
            };
            if same {
                return;
            }
            s.layout_root = component;
            s.current_layout = None;
        }
        self.request_render(tui);
    }

    fn scroll_by(&self, tui: &Tui, lines: i64) {
        self.primary_scroll_view().scroll_by(lines);
        self.request_render(tui);
    }

    fn scroll_to_top(&self, tui: &Tui) {
        self.primary_scroll_view().scroll_to_start();
        self.request_render(tui);
    }

    fn scroll_to_bottom(&self, tui: &Tui) {
        self.primary_scroll_view().scroll_to_end();
        self.request_render(tui);
    }

    fn scroll_to_prompt(&self, tui: &Tui, direction: i64) {
        let Some(layout) = self.layout() else { return };
        let scroll_view = self.primary_scroll_view();
        let Some(lines) = get_scroll_view_box(&layout, &scroll_view).and_then(|b| b.scroll_content_lines.clone()) else {
            return;
        };
        let mut row = scroll_view.scroll_top() + direction;
        while row >= 0 && (row as usize) < lines.len() {
            if starts_with_osc133_prompt(&lines[row as usize]) {
                scroll_view.scroll_to(row, ScrollViewScrollToOptions::default());
                self.request_render(tui);
                return;
            }
            row += direction;
        }
    }

    // ---- search ----------------------------------------------------------------------

    fn toggle_search(&self, tui: &Tui) {
        if self.st().active_search.is_some() {
            self.close_search(tui);
            return;
        }
        let weak = self.tui.clone();
        let component = Arc::new(AltScreenSearchComponent::new(
            Arc::new(move |query: &str| {
                if let Some(t) = weak.upgrade()
                    && let Screen::Alt(alt) = &t.screen
                {
                    alt.update_search_query(&t, query);
                }
            }),
            Some(self.cfg.search_navigation_button_style.clone()),
        ));
        let anchor_row = self.primary_scroll_view().scroll_top();
        self.st().active_search = Some(ActiveSearch {
            component: component.clone(),
            index: AltScreenSearchIndex::new(),
            overlay: None,
            query: String::new(),
            matches: Vec::new(),
            selected_index: -1,
            selected_key: None,
            anchor_row,
            selection_mode: SearchSelectionMode::Query,
        });
        let handle = tui.show_overlay(
            component,
            Some(OverlayOptions {
                anchor: Some(OverlayAnchor::TopRight),
                width: Some(SizeValue::from("40%")),
                min_width: Some(32),
                margin: Some(OverlayMargin::all(1)),
                ..Default::default()
            }),
        );
        if let Some(search) = self.st().active_search.as_mut() {
            search.overlay = Some(handle);
        }
    }

    fn close_search(&self, tui: &Tui) {
        let Some(search) = self.st().active_search.take() else { return };
        if let Some(overlay) = &search.overlay {
            overlay.hide();
        }
        self.request_render(tui);
    }

    fn update_search_query(&self, tui: &Tui, query: &str) {
        let primary_top = self.primary_scroll_view().scroll_top();
        let component = {
            let mut s = self.st();
            let Some(search) = s.active_search.as_mut() else { return };
            if query == search.query {
                return;
            }
            let selected_row = usize::try_from(search.selected_index)
                .ok()
                .and_then(|i| search.matches.get(i))
                .and_then(|m| m.segments.first())
                .map(|seg| seg.row as i64);
            search.anchor_row = selected_row.unwrap_or(primary_top);
            search.query = query.to_string();
            search.selection_mode = SearchSelectionMode::Query;
            search.component.clone()
        };
        component.set_result(-1, 0);
        self.request_render(tui);
    }

    fn navigate_search(&self, tui: &Tui, direction: i64) {
        {
            let mut s = self.st();
            let Some(search) = s.active_search.as_mut() else { return };
            if search.query.is_empty() {
                return;
            }
            search.selection_mode = if direction < 0 { SearchSelectionMode::Previous } else { SearchSelectionMode::Next };
        }
        self.request_render(tui);
    }

    fn search_navigation_direction_at(&self, x: i64, y: i64) -> Option<i32> {
        let (component, overlay) = {
            let s = self.st();
            let search = s.active_search.as_ref()?;
            (search.component.clone(), search.overlay.clone()?)
        };
        let bounds = overlay.get_bounds()?;
        if x < bounds.col || x >= bounds.col + bounds.width || y < bounds.row || y >= bounds.row + bounds.height {
            return None;
        }
        component.get_navigation_direction_at(y - bounds.row, x - bounds.col)
    }

    fn handle_search_mouse_event(&self, tui: &Tui, event: &SgrMouseEvent) -> bool {
        let Some(component) = self.st().active_search.as_ref().map(|s| s.component.clone()) else { return false };
        let direction = self.search_navigation_direction_at(event.x, event.y);
        if component.set_hovered_navigation_direction(direction) {
            self.request_render(tui);
        }
        let Some(direction) = direction else { return false };
        if event.release || event.button & 32 != 0 || event.button & 3 != 0 {
            return false;
        }
        self.navigate_search(tui, direction as i64);
        true
    }

    fn refresh_search(&self, layout: &LayoutFrame) -> bool {
        let scroll_view = layout.primary_scroll_view.clone().unwrap_or_else(|| self.implicit_scroll_view.clone());
        let b = get_scroll_view_box(layout, &scroll_view);
        let lines = b.and_then(|b| b.scroll_content_lines.clone());
        let mut s = self.st();
        let Some(search) = s.active_search.as_mut() else { return false };
        let Some(lines) = lines.filter(|_| !search.query.trim().is_empty()) else {
            search.matches.clear();
            search.selected_index = -1;
            search.selected_key = None;
            search.selection_mode = SearchSelectionMode::Retain;
            search.component.set_result(-1, 0);
            return false;
        };
        let should_reveal = search.selection_mode != SearchSelectionMode::Retain;
        let result = search.index.search(&lines, &search.query);
        let matches = result.matches;
        search.matches = matches.clone();
        if !result.changed && search.selection_mode == SearchSelectionMode::Retain {
            return false;
        }
        let exact_index: i64 = if result.changed {
            match &search.selected_key {
                Some(key) => {
                    matches.iter().position(|m| get_alt_screen_search_match_key(m) == *key).map_or(-1, |i| i as i64)
                }
                None => -1,
            }
        } else {
            search.selected_index
        };
        let n = matches.len() as i64;
        let mut selected_index = -1i64;
        if n > 0 {
            selected_index = match search.selection_mode {
                SearchSelectionMode::Query => {
                    let (mut low, mut high) = (0usize, matches.len());
                    while low < high {
                        let middle = low + (high - low) / 2;
                        if (matches[middle].segments.first().map_or(0, |s| s.row) as i64) < search.anchor_row {
                            low = middle + 1;
                        } else {
                            high = middle;
                        }
                    }
                    if low < matches.len() { low as i64 } else { 0 }
                }
                SearchSelectionMode::Next => {
                    let base = if exact_index >= 0 { exact_index } else { search.selected_index.min(n - 1) };
                    if base < 0 { 0 } else { (base + 1) % n }
                }
                SearchSelectionMode::Previous => {
                    let base = if exact_index >= 0 { exact_index } else { search.selected_index.min(n - 1) };
                    if base < 0 { n - 1 } else { (base - 1 + n) % n }
                }
                SearchSelectionMode::Retain => {
                    if exact_index >= 0 { exact_index } else { search.selected_index.max(0).min(n - 1) }
                }
            };
        }
        search.selected_index = selected_index;
        search.selected_key =
            if selected_index >= 0 { Some(get_alt_screen_search_match_key(&matches[selected_index as usize])) } else { None };
        search.selection_mode = SearchSelectionMode::Retain;
        search.component.set_result(selected_index, matches.len());
        drop(s);
        if !should_reveal {
            return false;
        }
        let Some(selected) = usize::try_from(selected_index).ok().and_then(|i| matches.get(i)) else { return false };
        let (Some(first), Some(last)) = (selected.segments.first(), selected.segments.last()) else { return false };
        if b.is_none() || scroll_view.viewport_height() <= 0 {
            return false;
        }
        let before = scroll_view.scroll_top();
        let visible_bottom = before + scroll_view.viewport_height() - 1;
        let mut target = before;
        if (first.row as i64) < before || (last.row as i64) > visible_bottom {
            target = first.row as i64 - scroll_view.viewport_height() / 3;
        }
        scroll_view.scroll_to(target, ScrollViewScrollToOptions { disable_follow: true });
        scroll_view.scroll_top() != before
    }

    fn flash(&self, message: &str, duration_ms: Option<u64>) {
        self.flashes.flash(message, duration_ms);
    }

    fn should_defer_viewport_input_to_overlay(&self, tui: &Tui) -> bool {
        let search_focused = self
            .st()
            .active_search
            .as_ref()
            .and_then(|s| s.overlay.clone())
            .is_some_and(|o| o.is_focused());
        tui.is_overlay_focused() && !search_focused
    }

    fn clear_component_mouse_gesture(&self) {
        let mut s = self.st();
        s.mouse_capture = None;
        s.mouse_press_target = None;
        s.mouse_press_point = None;
        s.mouse_press_moved = false;
    }

    // ---- input -----------------------------------------------------------------------

    pub(crate) fn handle_viewport_input(&self, tui: &Tui, data: &str) -> Option<InputListenerResult> {
        let consume = Some(InputListenerResult { consume: true, data: None });
        if data == FOCUS_OUT {
            let had_active_selection = self.st().selection_press_active;
            let had_non_empty = had_active_selection && self.get_selection_bounds().is_some();
            self.st().selection_press_active = false;
            self.stop_selection_auto_scroll(tui);
            self.stop_scrollbar_hover();
            let search_component = self.st().active_search.as_ref().map(|s| s.component.clone());
            if search_component.is_some_and(|c| c.set_hovered_navigation_direction(None)) {
                self.request_render(tui);
            }
            {
                let mut s = self.st();
                s.scrollbar_drag = None;
                s.pressed_url = None;
                s.selection_dragged = false;
            }
            self.clear_component_mouse_gesture();
            {
                let mut s = self.st();
                s.last_component_click = None;
                if had_active_selection {
                    s.selection_anchor = None;
                    s.selection_focus = None;
                    s.selection_granularity = SelectionGranularity::Character;
                    s.selection_initial_range = None;
                }
                s.last_click = None;
            }
            if had_active_selection && had_non_empty {
                self.request_render(tui);
            }
            return consume;
        }
        if data == FOCUS_IN {
            return consume;
        }

        if let Some(wheel) = parse_wheel_event(data) {
            let now = tui.event_loop().now_ms();
            let lines = self.st().wheel_scroll.next(wheel.direction, now);
            let wheel_delta = wheel.direction as i64 * if wheel.button & 8 != 0 { lines * ALT_WHEEL_SCROLL_MULTIPLIER } else { lines };
            let event = self.create_mouse_event(tui, MouseEventType::Wheel, wheel.button, wheel.x, wheel.y, Some(wheel_delta), None);
            let (hit, overlay_result) = tui.dispatch_mouse_to_overlay(&event);
            let result = overlay_result.or_else(|| if hit { None } else { self.dispatch_mouse_to_layout(&event) });
            if let Some(result) = result {
                if self.apply_mouse_dispatch_result(tui, &event, &result) {
                    self.request_render(tui);
                }
                return consume;
            }
            self.route_wheel(tui, &wheel, wheel_delta);
            return consume;
        }
        if let Some(mouse) = parse_sgr_mouse_event(data) {
            self.handle_mouse_event(tui, mouse);
            return consume;
        }
        if is_mouse_sequence(data) {
            return consume;
        }

        let keybindings = get_keybindings();
        let is_release = is_key_release(data);
        if keybindings.matches(data, "tui.altScreen.search") {
            if !is_release {
                self.toggle_search(tui);
            }
            return consume;
        }
        let search_focused = self
            .st()
            .active_search
            .as_ref()
            .and_then(|s| s.overlay.clone())
            .is_some_and(|o| o.is_focused());
        if search_focused {
            if keybindings.matches(data, "tui.altScreen.searchNext") {
                if !is_release {
                    self.navigate_search(tui, 1);
                }
                return consume;
            }
            if keybindings.matches(data, "tui.altScreen.searchPrevious") {
                if !is_release {
                    self.navigate_search(tui, -1);
                }
                return consume;
            }
            if keybindings.matches(data, "tui.altScreen.searchClose") {
                if !is_release {
                    self.close_search(tui);
                }
                return consume;
            }
        }
        if self.should_defer_viewport_input_to_overlay(tui) {
            return None;
        }
        let viewport_height = || self.primary_scroll_view().viewport_height();
        let actions: [(&str, &dyn Fn()); 12] = [
            ("tui.altScreen.pageUp", &|| self.scroll_by(tui, -(viewport_height() - PAGE_SCROLL_OVERLAP).max(1))),
            ("tui.altScreen.pageDown", &|| self.scroll_by(tui, (viewport_height() - PAGE_SCROLL_OVERLAP).max(1))),
            ("tui.altScreen.halfPageUp", &|| self.scroll_by(tui, -(viewport_height() / 2).max(1))),
            ("tui.altScreen.halfPageDown", &|| self.scroll_by(tui, (viewport_height() / 2).max(1))),
            ("tui.altScreen.lineUp", &|| self.scroll_by(tui, -1)),
            ("tui.altScreen.lineDown", &|| self.scroll_by(tui, 1)),
            ("tui.altScreen.previousPrompt", &|| self.scroll_to_prompt(tui, -1)),
            ("tui.altScreen.nextPrompt", &|| self.scroll_to_prompt(tui, 1)),
            ("tui.altScreen.top", &|| self.scroll_to_top(tui)),
            ("tui.altScreen.bottom", &|| self.scroll_to_bottom(tui)),
            ("", &|| {}),
            ("", &|| {}),
        ];
        for (id, action) in actions {
            if !id.is_empty() && keybindings.matches(data, id) {
                if !is_release {
                    action();
                }
                return consume;
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn create_mouse_event(
        &self,
        tui: &Tui,
        kind: MouseEventType,
        button: i64,
        x: i64,
        y: i64,
        wheel_delta: Option<i64>,
        click_count: Option<u32>,
    ) -> MouseEvent {
        MouseEvent {
            kind,
            button: if kind == MouseEventType::Wheel { MouseButton::None } else { decode_mouse_button(button) },
            x,
            y,
            screen_x: x,
            screen_y: y,
            width: tui.terminal().columns().max(1) as i64,
            height: tui.terminal().rows().max(1) as i64,
            shift: button & 4 != 0,
            alt: button & 8 != 0,
            ctrl: button & 16 != 0,
            wheel_delta,
            click_count,
        }
    }

    fn dispatch_mouse_to_layout(&self, event: &MouseEvent) -> Option<MouseResult> {
        let layout = self.layout()?;
        let mut visited: Vec<*const ()> = Vec::new();
        for b in get_layout_boxes_at(&layout, event.screen_x, event.screen_y) {
            let ptr = component_ptr(&b.component);
            if visited.contains(&ptr) {
                continue;
            }
            if get_layout_node(&b.component).is_some() && b.component.uses_container_mouse() {
                continue;
            }
            visited.push(ptr);
            let local = MouseEvent {
                x: event.screen_x - b.rect.x,
                y: event.screen_y - b.rect.y,
                width: b.rect.width,
                height: b.rect.height,
                ..*event
            };
            if let Some(result) = dispatch_mouse_event(&b.component, &local) {
                return Some(result);
            }
        }
        None
    }

    fn apply_mouse_dispatch_result(&self, tui: &Tui, event: &MouseEvent, result: &MouseResult) -> bool {
        let target_component = result.focus_target.clone().or_else(|| result.target.as_ref().map(|t| t.component.clone()));
        let focus_target = target_component.map(|c| tui.resolve_mouse_focus_target(&c));
        let focus_changed = result.focus
            && match (&tui.get_focused_component(), &focus_target) {
                (Some(a), Some(b)) => !same_component(a, b),
                (None, None) => false,
                _ => true,
            };
        if result.focus {
            tui.set_focus(focus_target);
        }
        if result.capture {
            self.st().mouse_capture = result.target.clone();
        }
        result.render.unwrap_or(
            focus_changed
                || matches!(
                    event.kind,
                    MouseEventType::Press | MouseEventType::Click | MouseEventType::Drag | MouseEventType::Wheel
                ),
        )
    }

    fn dispatch_mouse_to_target(&self, event: &MouseEvent, target: &MouseDispatchTarget) -> Option<MouseResult> {
        dispatch_mouse_event(&target.component, &retarget_mouse_event(event, target))
    }

    fn component_click_count(&self, tui: &Tui, target: &MouseDispatchTarget, x: i64, y: i64) -> u32 {
        let now = tui.event_loop().now_ms();
        let mut s = self.st();
        let count = match &s.last_component_click {
            Some(p)
                if now - p.timestamp <= DOUBLE_CLICK_INTERVAL_MS
                    && same_component(&p.component, &target.component)
                    && p.x == x
                    && p.y == y =>
            {
                (p.count % 3) + 1
            }
            _ => 1,
        };
        s.last_component_click = Some(ComponentClick { timestamp: now, count, component: target.component.clone(), x, y });
        count
    }

    fn clear_text_selection(&self, tui: &Tui) {
        self.stop_selection_auto_scroll(tui);
        let mut s = self.st();
        s.selection_press_active = false;
        s.selection_anchor = None;
        s.selection_focus = None;
        s.selection_granularity = SelectionGranularity::Character;
        s.selection_initial_range = None;
        s.pressed_url = None;
        s.selection_dragged = false;
    }

    fn handle_mouse_event(&self, tui: &Tui, raw: SgrMouseEvent) {
        let is_motion = raw.button & 32 != 0;
        let kind = if raw.release {
            MouseEventType::Release
        } else if is_motion {
            if decode_mouse_button(raw.button) == MouseButton::None { MouseEventType::Move } else { MouseEventType::Drag }
        } else {
            MouseEventType::Press
        };
        let event = self.create_mouse_event(tui, kind, raw.button, raw.x, raw.y, None, None);

        let capture_target = {
            let s = self.st();
            s.mouse_capture.clone().or_else(|| s.mouse_press_target.clone())
        };
        if let Some(target) = capture_target {
            {
                let mut s = self.st();
                if let Some((px, py)) = s.mouse_press_point
                    && (raw.x != px || raw.y != py)
                {
                    s.mouse_press_moved = true;
                    s.last_component_click = None;
                }
            }
            let mut render = false;
            if let Some(r) = self.dispatch_mouse_to_target(&event, &target) {
                render = self.apply_mouse_dispatch_result(tui, &event, &r);
            }
            if raw.release {
                let (moved, point) = {
                    let s = self.st();
                    (s.mouse_press_moved, s.mouse_press_point)
                };
                if !moved && point == Some((raw.x, raw.y)) {
                    let count = self.component_click_count(tui, &target, raw.x, raw.y);
                    let click = self.create_mouse_event(tui, MouseEventType::Click, raw.button, raw.x, raw.y, None, Some(count));
                    if let Some(r) = self.dispatch_mouse_to_target(&click, &target) {
                        render = self.apply_mouse_dispatch_result(tui, &click, &r) || render;
                    }
                }
                self.clear_component_mouse_gesture();
            }
            if render {
                self.request_render(tui);
            }
            return;
        }

        if self.handle_search_mouse_event(tui, &raw) {
            return;
        }

        let (hit, overlay_result) = tui.dispatch_mouse_to_overlay(&event);
        if !hit {
            if self.handle_scroll_to_end_indicator_mouse_event(tui, &raw) {
                return;
            }
            let scrollbar_handled = self.handle_scrollbar_mouse_event(tui, &raw);
            if self.st().scrollbar_drag.is_none() {
                self.update_scrollbar_hover(tui, raw.x, raw.y);
            }
            if scrollbar_handled {
                return;
            }
        } else {
            self.stop_scrollbar_hover();
        }

        let result = overlay_result.or_else(|| if hit { None } else { self.dispatch_mouse_to_layout(&event) });
        if let Some(result) = result {
            let render = self.apply_mouse_dispatch_result(tui, &event, &result);
            if kind == MouseEventType::Press {
                self.clear_text_selection(tui);
                let mut s = self.st();
                s.mouse_press_target = result.target.clone();
                s.mouse_press_point = Some((raw.x, raw.y));
                s.mouse_press_moved = false;
            }
            if render {
                self.request_render(tui);
            }
            return;
        }

        if self.handle_right_click_paste(&raw) {
            return;
        }
        self.handle_selection_mouse_event(tui, &raw);
    }

    fn route_wheel(&self, tui: &Tui, event: &WheelEvent, delta: i64) {
        let mut remaining = delta;
        let mut seen: Vec<Arc<ScrollView>> = Vec::new();
        if let Some(layout) = self.layout() {
            for scroll_view in get_scroll_views_at(&layout, event.x, event.y) {
                seen.push(scroll_view.clone());
                remaining = scroll_view.scroll_by(remaining);
                if remaining == 0 || scroll_view.overscroll == Overscroll::Contain {
                    break;
                }
            }
        }
        let primary = self.primary_scroll_view();
        if remaining != 0 && !seen.iter().any(|s| Arc::ptr_eq(s, &primary)) {
            primary.scroll_by(remaining);
        }
        self.update_scrollbar_hover(tui, event.x, event.y);
        self.request_render(tui);
    }

    fn handle_right_click_paste(&self, event: &SgrMouseEvent) -> bool {
        let Some(paste) = &self.cfg.on_right_click_paste else { return false };
        if !cfg!(windows)
            || std::env::var("TERM_PROGRAM").is_ok_and(|v| v.eq_ignore_ascii_case("vscode"))
            || event.release
            || event.button != 2
        {
            return false;
        }
        let paste = paste.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| paste()));
        true
    }

    fn handle_scroll_to_end_indicator_mouse_event(&self, tui: &Tui, event: &SgrMouseEvent) -> bool {
        let Some(rect) = self.st().scroll_to_end_indicator_rect else { return false };
        if event.release || event.button & 32 != 0 || event.button & 3 != 0 {
            return false;
        }
        if event.y != rect.row || event.x < rect.column || event.x >= rect.column + rect.width {
            return false;
        }
        self.scroll_to_bottom(tui);
        true
    }

    fn scrollbar_target_at(&self, tui: &Tui, x: i64, y: i64, include_hidden_auto: bool) -> Option<ScrollbarTarget> {
        if tui.has_overlay() {
            return None;
        }
        let layout = self.layout()?;
        for scroll_view in get_scroll_views_at(&layout, x, y) {
            let geometry = get_scroll_view_box(&layout, &scroll_view).and_then(|b| get_scrollbar_geometry(b, include_hidden_auto));
            if let Some(geometry) = geometry
                && x == geometry.column
                && y >= geometry.track_top
                && y < geometry.track_top + geometry.track_height
            {
                return Some(ScrollbarTarget { scroll_view, geometry });
            }
        }
        None
    }

    fn set_scrollbar_hover(&self, scroll_view: Option<Arc<ScrollView>>) {
        let previous = {
            let mut s = self.st();
            if same_sv(&s.scrollbar_hover, &scroll_view) {
                return;
            }
            std::mem::replace(&mut s.scrollbar_hover, scroll_view.clone())
        };
        if let Some(p) = previous {
            p.set_scrollbar_active(false);
        }
        if let Some(n) = scroll_view {
            n.set_scrollbar_active(true);
        }
    }

    fn update_scrollbar_hover(&self, tui: &Tui, x: i64, y: i64) {
        self.set_scrollbar_hover(self.scrollbar_target_at(tui, x, y, true).map(|t| t.scroll_view));
    }

    fn stop_scrollbar_hover(&self) {
        self.set_scrollbar_hover(None);
    }

    fn scroll_scrollbar_to_pointer(scroll_view: &ScrollView, geometry: &ScrollbarGeometry, pointer_y: i64, grab_offset: i64) {
        let max_thumb_offset = geometry.track_height - geometry.thumb_height;
        let thumb_offset = (pointer_y - geometry.track_top - grab_offset).min(max_thumb_offset).max(0);
        let scroll_top = if max_thumb_offset == 0 {
            0
        } else {
            (thumb_offset as f64 / max_thumb_offset as f64 * geometry.max_scroll_top as f64 + 0.5).floor() as i64
        };
        scroll_view.scroll_to(scroll_top, ScrollViewScrollToOptions::default());
    }

    fn handle_scrollbar_mouse_event(&self, tui: &Tui, event: &SgrMouseEvent) -> bool {
        let drag = self.st().scrollbar_drag.clone();
        if let Some(drag) = drag {
            if event.release {
                self.st().scrollbar_drag = None;
                return true;
            }
            if let Some(layout) = self.layout()
                && let Some(geometry) =
                    get_scroll_view_box(&layout, &drag.scroll_view).and_then(|b| get_scrollbar_geometry(b, false))
            {
                Self::scroll_scrollbar_to_pointer(&drag.scroll_view, &geometry, event.y, drag.grab_offset);
            }
            return true;
        }
        if event.release || event.button & 32 != 0 || event.button & 3 != 0 {
            return false;
        }
        let Some(target) = self.scrollbar_target_at(tui, event.x, event.y, false) else { return false };
        self.stop_selection_auto_scroll(tui);
        {
            let mut s = self.st();
            s.selection_press_active = false;
            s.selection_anchor = None;
            s.selection_focus = None;
            s.selection_granularity = SelectionGranularity::Character;
            s.selection_initial_range = None;
            s.last_click = None;
            s.pressed_url = None;
            s.selection_dragged = false;
        }
        self.set_scrollbar_hover(Some(target.scroll_view.clone()));
        let g = &target.geometry;
        let on_thumb = event.y >= g.thumb_top && event.y < g.thumb_top + g.thumb_height;
        let grab_offset = if on_thumb { event.y - g.thumb_top } else { g.thumb_height / 2 };
        if !on_thumb {
            Self::scroll_scrollbar_to_pointer(&target.scroll_view, g, event.y, grab_offset);
        }
        self.st().scrollbar_drag = Some(ScrollbarDrag { scroll_view: target.scroll_view, grab_offset });
        true
    }

    // ---- selection -------------------------------------------------------------------

    fn scroll_selection_point(&self, tui: &Tui, scroll_view: &Arc<ScrollView>, x: i64, y: i64) -> Option<SelectionPoint> {
        let layout = self.layout()?;
        let b = get_scroll_view_box(&layout, scroll_view)?;
        if b.rect.height <= 0 || b.clip.height <= 0 {
            return None;
        }
        let visible_top = 0.max(b.rect.y).max(b.clip.y);
        let visible_bottom = (tui.terminal().rows() as i64 - 1)
            .min(b.rect.y + b.rect.height - 1)
            .min(b.clip.y + b.clip.height - 1);
        if visible_bottom < visible_top {
            return None;
        }
        let pointer_row = y.min(visible_bottom).max(visible_top);
        let max_content_row = (b.scroll_content_lines.as_ref().map_or(1, |l| l.len()) as i64 - 1).max(0);
        Some(SelectionPoint {
            row: (scroll_view.scroll_top() + pointer_row - b.rect.y).min(max_content_row).max(0),
            col: (x - b.rect.x).min(b.rect.width - 1).max(0),
            scroll_view: Some(scroll_view.clone()),
            boundary: false,
        })
    }

    fn selection_point(&self, tui: &Tui, event: &SgrMouseEvent, scroll_view: Option<&Arc<ScrollView>>) -> SelectionPoint {
        if let Some(sv) = scroll_view
            && let Some(p) = self.scroll_selection_point(tui, sv, event.x, event.y)
        {
            return p;
        }
        SelectionPoint {
            row: event.y.min(tui.terminal().rows() as i64 - 1).max(0),
            col: event.x.min(tui.terminal().columns() as i64 - 1).max(0),
            scroll_view: None,
            boundary: false,
        }
    }

    fn selection_source_line(&self, point: &SelectionPoint) -> String {
        if let Some(sv) = &point.scroll_view
            && let Some(layout) = self.layout()
            && let Some(lines) = get_scroll_view_box(&layout, sv).and_then(|b| b.scroll_content_lines.clone())
        {
            return lines.get(point.row as usize).cloned().unwrap_or_default();
        }
        self.st().previous_screen.get(point.row as usize).cloned().unwrap_or_default()
    }

    fn word_selection(&self, point: &SelectionPoint) -> Option<SelectionRange> {
        struct Seg {
            start: i64,
            end: i64,
            selectable: bool,
            joiner: bool,
        }
        let line = strip_terminal_sequences(&self.selection_source_line(point));
        let mut segments: Vec<Seg> = Vec::new();
        let mut start = 0i64;
        for seg in word_segments(&line) {
            let end = start + visible_width(seg.segment) as i64;
            let joiner = TERMINAL_WORD_SELECTION_JOINERS.contains(&seg.segment);
            segments.push(Seg { start, end, selectable: seg.is_word_like || joiner, joiner });
            start = end;
        }
        let clicked = segments.iter().position(|s| point.col >= s.start && point.col < s.end)?;
        let can_join = |l: &Seg, r: &Seg| l.selectable && r.selectable && (l.joiner || r.joiner);
        let mut selection_start = segments[clicked].start;
        let mut selection_end = segments[clicked].end;
        let mut index = clicked;
        while index > 0 && can_join(&segments[index - 1], &segments[index]) {
            selection_start = segments[index - 1].start;
            index -= 1;
        }
        let mut index = clicked;
        while index + 1 < segments.len() && can_join(&segments[index], &segments[index + 1]) {
            selection_end = segments[index + 1].end;
            index += 1;
        }
        Some(SelectionRange {
            start: SelectionPoint { col: selection_start, ..point.clone() },
            end: SelectionPoint { col: selection_end, boundary: true, ..point.clone() },
        })
    }

    fn line_selection(&self, point: &SelectionPoint) -> SelectionRange {
        SelectionRange {
            start: SelectionPoint { col: 0, ..point.clone() },
            end: SelectionPoint {
                col: visible_width(&self.selection_source_line(point)) as i64,
                boundary: true,
                ..point.clone()
            },
        }
    }

    fn update_selection_focus(&self, point: SelectionPoint) {
        let (granularity, initial) = {
            let s = self.st();
            (s.selection_granularity, s.selection_initial_range.clone())
        };
        let Some(initial) = initial.filter(|_| granularity != SelectionGranularity::Character) else {
            self.st().selection_focus = Some(point);
            return;
        };
        let range =
            if granularity == SelectionGranularity::Word { self.word_selection(&point) } else { Some(self.line_selection(&point)) };
        let Some(range) = range else { return };
        let target_before_initial = range.start.row < initial.start.row
            || (range.start.row == initial.start.row && range.start.col < initial.start.col);
        let mut s = self.st();
        if target_before_initial {
            s.selection_anchor = Some(initial.end);
            s.selection_focus = Some(range.start);
        } else {
            s.selection_anchor = Some(initial.start);
            s.selection_focus = Some(range.end);
        }
    }

    fn click_count(&self, tui: &Tui, point: &SelectionPoint, word: Option<&SelectionRange>) -> u32 {
        let now = tui.event_loop().now_ms();
        let mut s = self.st();
        let count = match (word, &s.last_click) {
            (Some(word), Some(previous))
                if now - previous.timestamp <= DOUBLE_CLICK_INTERVAL_MS
                    && previous.row == point.row
                    && same_sv(&previous.scroll_view, &point.scroll_view)
                    && previous.word_start == word.start.col
                    && previous.word_end == word.end.col =>
            {
                (previous.count % 3) + 1
            }
            _ => 1,
        };
        s.last_click = word.map(|word| ClickTarget {
            timestamp: now,
            count,
            row: point.row,
            scroll_view: point.scroll_view.clone(),
            word_start: word.start.col,
            word_end: word.end.col,
        });
        count
    }

    fn update_selection_auto_scroll(&self, tui: &Tui, event: &SgrMouseEvent) {
        let scroll_view = self.st().selection_anchor.as_ref().and_then(|a| a.scroll_view.clone());
        let (Some(scroll_view), Some(layout)) = (scroll_view, self.layout()) else {
            self.stop_selection_auto_scroll(tui);
            return;
        };
        let Some(b) = get_scroll_view_box(&layout, &scroll_view).filter(|b| b.rect.height > 0 && b.clip.height > 0) else {
            self.stop_selection_auto_scroll(tui);
            return;
        };
        let visible_top = 0.max(b.rect.y).max(b.clip.y);
        let visible_bottom = (tui.terminal().rows() as i64 - 1)
            .min(b.rect.y + b.rect.height - 1)
            .min(b.clip.y + b.clip.height - 1);
        let direction = if event.y <= visible_top {
            -1
        } else if event.y >= visible_bottom {
            1
        } else {
            0
        };
        {
            let mut s = self.st();
            s.selection_drag_pointer = Some((event.x, event.y));
            s.selection_auto_scroll_direction = direction;
        }
        if direction == 0 {
            self.stop_selection_auto_scroll(tui);
            return;
        }
        if self.st().selection_auto_scroll_timer.is_some() {
            return;
        }
        let weak = self.tui.clone();
        let timer = tui.event_loop().set_interval(Duration::from_millis(50), move || {
            if let Some(t) = weak.upgrade()
                && let Screen::Alt(alt) = &t.screen
            {
                alt.auto_scroll_selection(&t);
            }
        });
        self.st().selection_auto_scroll_timer = Some(timer);
    }

    fn auto_scroll_selection(&self, tui: &Tui) {
        let (scroll_view, pointer, direction) = {
            let s = self.st();
            (
                s.selection_anchor.as_ref().and_then(|a| a.scroll_view.clone()),
                s.selection_drag_pointer,
                s.selection_auto_scroll_direction,
            )
        };
        let (Some(scroll_view), Some((px, py))) = (scroll_view, pointer) else {
            self.stop_selection_auto_scroll(tui);
            return;
        };
        if direction == 0 {
            self.stop_selection_auto_scroll(tui);
            return;
        }
        let remaining = scroll_view.scroll_by(direction);
        if remaining == direction {
            self.stop_selection_auto_scroll(tui);
            return;
        }
        if let Some(point) = self.scroll_selection_point(tui, &scroll_view, px, py) {
            self.update_selection_focus(point);
        }
        self.request_render(tui);
    }

    fn stop_selection_auto_scroll(&self, tui: &Tui) {
        let timer = {
            let mut s = self.st();
            s.selection_auto_scroll_direction = 0;
            s.selection_drag_pointer = None;
            s.selection_auto_scroll_timer.take()
        };
        if let Some(timer) = timer {
            tui.event_loop().clear_timer(timer);
        }
    }

    fn handle_selection_mouse_event(&self, tui: &Tui, event: &SgrMouseEvent) {
        let button = event.button & 3;
        if button != 0 && !(event.release && button == 3) {
            return;
        }
        let anchor_scroll_view = self.st().selection_anchor.as_ref().and_then(|a| a.scroll_view.clone());
        let point = self.selection_point(tui, event, anchor_scroll_view.as_ref());
        if event.release {
            if !self.st().selection_press_active {
                return;
            }
            self.st().selection_press_active = false;
            self.stop_selection_auto_scroll(tui);
            if self.st().selection_anchor.is_none() {
                return;
            }
            self.update_selection_focus(point.clone());
            let (is_click, clicked_url) = {
                let mut s = self.st();
                let anchor = s.selection_anchor.clone().unwrap();
                let is_click = !s.selection_dragged
                    && same_sv(&anchor.scroll_view, &point.scroll_view)
                    && anchor.row == point.row
                    && anchor.col == point.col;
                let url = if is_click { s.pressed_url.clone() } else { None };
                s.pressed_url = None;
                (is_click, url)
            };
            if let (Some(url), Some(open)) = (clicked_url, self.cfg.open_url.clone()) {
                {
                    let mut s = self.st();
                    s.selection_anchor = None;
                    s.selection_focus = None;
                }
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open(&url)));
                self.request_render(tui);
                return;
            }
            if is_click {
                let count = self.st().last_click.as_ref().map_or(1, |c| c.count);
                let click = self.create_mouse_event(tui, MouseEventType::Click, event.button, event.x, event.y, None, Some(count));
                let (hit, overlay_result) = tui.dispatch_mouse_to_overlay(&click);
                let result = overlay_result.or_else(|| if hit { None } else { self.dispatch_mouse_to_layout(&click) });
                if let Some(result) = result {
                    let render = self.apply_mouse_dispatch_result(tui, &click, &result);
                    self.clear_text_selection(tui);
                    if render {
                        self.request_render(tui);
                    }
                    return;
                }
            }
            if self.st().copy_on_select {
                self.copy_selection_to_clipboard(tui);
            }
            self.request_render(tui);
            return;
        }
        if event.button & 32 != 0 {
            {
                let mut s = self.st();
                if !s.selection_press_active || s.selection_anchor.is_none() {
                    return;
                }
                s.selection_dragged = true;
                s.last_click = None;
                s.pressed_url = None;
            }
            self.update_selection_focus(point);
            self.update_selection_auto_scroll(tui, event);
            self.request_render(tui);
            return;
        }
        self.stop_selection_auto_scroll(tui);
        self.st().selection_press_active = true;
        let scroll_view = if !tui.has_overlay() {
            self.layout().and_then(|l| get_scroll_views_at(&l, event.x, event.y).into_iter().next())
        } else {
            None
        };
        let anchor = self.selection_point(tui, event, scroll_view.as_ref());
        let word = self.word_selection(&anchor);
        let click_count = self.click_count(tui, &anchor, word.as_ref());
        let range = match click_count {
            2 => word,
            3 => Some(self.line_selection(&anchor)),
            _ => None,
        };
        let pressed_url = if range.is_some() {
            None
        } else {
            let row = event.y.min(tui.terminal().rows() as i64 - 1).max(0) as usize;
            let col = event.x.min(tui.terminal().columns() as i64 - 1).max(0) as usize;
            let line = self.st().previous_screen.get(row).cloned().unwrap_or_default();
            get_osc8_link_at_column(&line, col)
        };
        {
            let mut s = self.st();
            s.selection_granularity = match (&range, click_count) {
                (Some(_), 2) => SelectionGranularity::Word,
                (Some(_), _) => SelectionGranularity::Line,
                (None, _) => SelectionGranularity::Character,
            };
            s.selection_anchor = Some(range.as_ref().map_or_else(|| anchor.clone(), |r| r.start.clone()));
            s.selection_focus = Some(range.as_ref().map_or_else(|| anchor.clone(), |r| r.end.clone()));
            s.selection_initial_range = range;
            s.selection_dragged = false;
            s.pressed_url = pressed_url;
        }
        self.request_render(tui);
    }

    fn get_selection_bounds(&self) -> Option<SelectionRange> {
        let s = self.st();
        let (anchor, focus) = (s.selection_anchor.clone()?, s.selection_focus.clone()?);
        if !same_sv(&anchor.scroll_view, &focus.scroll_view) {
            return None;
        }
        if anchor.row == focus.row && anchor.col == focus.col {
            return None;
        }
        let anchor_before = anchor.row < focus.row || (anchor.row == focus.row && anchor.col < focus.col);
        Some(if anchor_before { SelectionRange { start: anchor, end: focus } } else { SelectionRange { start: focus, end: anchor } })
    }

    fn selection_columns(
        line: &str,
        row: i64,
        selection: &SelectionRange,
        min_column: i64,
        max_column: Option<i64>,
    ) -> (i64, i64) {
        let line_width = visible_width(line) as i64;
        let max_column = max_column.unwrap_or(line_width);
        let mut start = min_column.max(0);
        let mut end = line_width.min(max_column);
        if row == selection.start.row {
            start = get_grapheme_cell_range(line, selection.start.col.max(0) as usize)
                .map_or(selection.start.col.min(line_width), |r| r.start as i64);
        }
        if row == selection.end.row {
            end = if selection.end.boundary {
                selection.end.col.min(line_width)
            } else {
                get_grapheme_cell_range(line, selection.end.col.max(0) as usize)
                    .map_or((selection.end.col + 1).min(line_width), |r| r.end as i64)
            };
        }
        (start.max(min_column), end.min(max_column))
    }

    fn active_selection_text(&self) -> Option<String> {
        let selection = self.get_selection_bounds()?;
        let source_lines: Vec<String> = if let Some(sv) = &selection.start.scroll_view {
            let layout = self.layout()?;
            get_scroll_view_box(&layout, sv)?.scroll_content_lines.as_ref()?.to_vec()
        } else {
            self.st().previous_screen.clone()
        };
        let mut lines = Vec::new();
        let mut row = selection.start.row;
        while row <= selection.end.row {
            let line = source_lines.get(row.max(0) as usize).cloned().unwrap_or_default();
            let (start, end) = Self::selection_columns(&line, row, &selection, 0, None);
            let sliced = slice_by_column(&line, start.max(0) as usize, (end - start).max(0) as usize, true);
            lines.push(js_trim_end(&strip_terminal_sequences(&sliced)).to_string());
            row += 1;
        }
        let text = lines.join("\n");
        if text.is_empty() { None } else { Some(text) }
    }

    fn copy_selection_to_clipboard(&self, tui: &Tui) {
        if let Some(text) = self.active_selection_text() {
            // `void this.copySelectionToClipboard()`: the copy already runs on the event loop;
            // the handle only reports its outcome.
            drop(self.copy_text_to_clipboard(tui, text));
        }
    }

    /// Copy text using the configured clipboard path; resolves to whether it succeeded.
    fn copy_text_to_clipboard(&self, tui: &Tui, text: String) -> event_loop::Deferred<bool> {
        let (resolver, deferred) = event_loop::deferred::<bool>();
        if let Some(copy) = self.cfg.copy_selection.clone() {
            let weak = self.tui.clone();
            let fut = copy(&text);
            event_loop::spawn(tui.event_loop(), async move {
                let result = fut.await;
                let ok = result == CopySelectionResult::Copied;
                if let Some(t) = weak.upgrade()
                    && let Screen::Alt(alt) = &t.screen
                {
                    match result {
                        CopySelectionResult::Copied => alt.flash("Copied!", None),
                        CopySelectionResult::Error(message) => alt.flash(&message, Some(COPY_ERROR_FLASH_DURATION_MS)),
                        CopySelectionResult::Failed => alt.flash("Copy failed", Some(COPY_ERROR_FLASH_DURATION_MS)),
                    }
                }
                resolver.resolve(ok);
            });
            return deferred;
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        tui.terminal().write(&format!("\x1b]52;c;{encoded}\x07"));
        self.flash("Copied!", None);
        resolver.resolve(true);
        deferred
    }

    // ---- painting --------------------------------------------------------------------

    fn apply_search_text_highlight(&self, text: &str, current: bool) -> String {
        let style = if current { &self.cfg.search_current_match_style } else { &self.cfg.search_match_style };
        let mut result = String::new();
        let mut plain_start = 0;
        let mut index = 0;
        while index < text.len() {
            match extract_ansi_code(text, index) {
                None => {
                    index += text[index..].chars().next().map_or(1, char::len_utf8);
                }
                Some((code, len)) => {
                    if index > plain_start {
                        result.push_str(&style(&text[plain_start..index]));
                    }
                    result.push_str(code);
                    index += len;
                    plain_start = index;
                }
            }
        }
        if plain_start < text.len() {
            result.push_str(&style(&text[plain_start..]));
        }
        result
    }

    fn apply_search_highlights(&self, tui: &Tui, screen: Vec<String>, layout: &LayoutFrame) -> Vec<String> {
        let (matches, selected_index) = {
            let s = self.st();
            let Some(search) = s.active_search.as_ref() else { return screen };
            if search.selected_index < 0 || search.matches.is_empty() {
                return screen;
            }
            (search.matches.clone(), search.selected_index)
        };
        let scroll_view = layout.primary_scroll_view.clone().unwrap_or_else(|| self.implicit_scroll_view.clone());
        let Some(b) = get_scroll_view_box(layout, &scroll_view) else { return screen };
        let scrollbar_column = get_scrollbar_geometry(b, false).map(|g| g.column);
        let min_row = 0.max(b.rect.y).max(b.clip.y);
        let max_row = (screen.len() as i64).min(b.rect.y + b.rect.height).min(b.clip.y + b.clip.height);
        let min_column = 0.max(b.rect.x).max(b.clip.x);
        let mut max_column =
            (tui.terminal().columns() as i64).min(b.rect.x + b.rect.width).min(b.clip.x + b.clip.width);
        if let Some(c) = scrollbar_column {
            max_column = max_column.min(c);
        }
        let scroll_top = scroll_view.scroll_top();
        let min_content_row = scroll_top + min_row - b.rect.y;
        let max_content_row = scroll_top + max_row - b.rect.y - 1;
        let (mut low, mut high) = (0usize, matches.len());
        while low < high {
            let middle = low + (high - low) / 2;
            let last_row = matches[middle].segments.last().map_or(-1, |s| s.row as i64);
            if last_row < min_content_row {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let mut ranges_by_row: Vec<(i64, Vec<SearchHighlightRange>)> = Vec::new();
        for (match_index, m) in matches.iter().enumerate().skip(low) {
            if m.segments.first().map_or(0, |s| s.row as i64) > max_content_row {
                break;
            }
            for segment in &m.segments {
                let row = b.rect.y + segment.row as i64 - scroll_top;
                if row < min_row || row >= max_row {
                    continue;
                }
                let start_col = min_column.max(b.rect.x + segment.start_col as i64);
                let end_col = max_column.min(b.rect.x + segment.end_col as i64);
                if end_col <= start_col {
                    continue;
                }
                let range = SearchHighlightRange { start_col, end_col, current: match_index as i64 == selected_index };
                match ranges_by_row.iter_mut().find(|(r, _)| *r == row) {
                    Some((_, ranges)) => ranges.push(range),
                    None => ranges_by_row.push((row, vec![range])),
                }
            }
        }
        let mut result = screen;
        for (row, mut ranges) in ranges_by_row {
            let Some(mut line) = result.get(row as usize).cloned() else { continue };
            if is_image_line(&line) {
                continue;
            }
            let line_width = visible_width(&line) as i64;
            ranges.sort_by_key(|range| std::cmp::Reverse(range.start_col));
            for range in ranges {
                let start_col = range.start_col.min(line_width);
                let end_col = range.end_col.min(line_width);
                if end_col <= start_col {
                    continue;
                }
                let before = slice_by_column(&line, 0, start_col as usize, true);
                let highlighted = slice_by_column(&line, start_col as usize, (end_col - start_col) as usize, true);
                let after = slice_by_column(&line, end_col as usize, (line_width - end_col).max(0) as usize, true);
                line = format!("{before}{}{after}", self.apply_search_text_highlight(&highlighted, range.current));
            }
            result[row as usize] = line;
        }
        result
    }

    fn apply_selection_highlight(text: &str) -> String {
        let mut result = String::from("\x1b[7m");
        let mut index = 0;
        while index < text.len() {
            match extract_ansi_code(text, index) {
                None => {
                    let c = text[index..].chars().next().unwrap();
                    result.push(c);
                    index += c.len_utf8();
                }
                Some((code, len)) => {
                    result.push_str(code);
                    if code.ends_with('m') {
                        result.push_str("\x1b[7m");
                    }
                    index += len;
                }
            }
        }
        result.push_str("\x1b[27m");
        result
    }

    fn apply_selection(&self, tui: &Tui, screen: Vec<String>, layout: Option<&LayoutFrame>) -> Vec<String> {
        let Some(selection) = self.get_selection_bounds() else { return screen };
        let mut screen_selection = selection.clone();
        let mut min_row = 0i64;
        let mut max_row = screen.len() as i64 - 1;
        let mut min_column = 0i64;
        let mut max_column = tui.terminal().columns() as i64;
        if let Some(sv) = &selection.start.scroll_view {
            let Some(layout) = layout else { return screen };
            let Some(b) = get_scroll_view_box(layout, sv) else { return screen };
            min_row = 0.max(b.rect.y).max(b.clip.y);
            max_row = (screen.len() as i64 - 1).min(b.rect.y + b.rect.height - 1).min(b.clip.y + b.clip.height - 1);
            min_column = 0.max(b.rect.x).max(b.clip.x);
            max_column = (tui.terminal().columns() as i64).min(b.rect.x + b.rect.width).min(b.clip.x + b.clip.width);
            let top = sv.scroll_top();
            screen_selection.start.row = b.rect.y + selection.start.row - top;
            screen_selection.start.col = b.rect.x + selection.start.col;
            screen_selection.end.row = b.rect.y + selection.end.row - top;
            screen_selection.end.col = b.rect.x + selection.end.col;
        }
        screen
            .into_iter()
            .enumerate()
            .map(|(row, line)| {
                let row = row as i64;
                if row < min_row
                    || row > max_row
                    || row < screen_selection.start.row
                    || row > screen_selection.end.row
                    || is_image_line(&line)
                {
                    return line;
                }
                let line_width = visible_width(&line) as i64;
                let (start, end) = Self::selection_columns(&line, row, &screen_selection, min_column, Some(max_column));
                if end <= start {
                    return line;
                }
                let before = slice_by_column(&line, 0, start as usize, true);
                let selected = slice_by_column(&line, start as usize, (end - start) as usize, true);
                let after = slice_by_column(&line, end as usize, (line_width - end).max(0) as usize, true);
                format!("{before}{}{after}", Self::apply_selection_highlight(&selected))
            })
            .collect()
    }

    fn composite_scroll_to_end_indicator(&self, screen: Vec<String>, layout: &LayoutFrame, width: usize) -> Vec<String> {
        self.st().scroll_to_end_indicator_rect = None;
        let scroll_view = layout.primary_scroll_view.clone().unwrap_or_else(|| self.implicit_scroll_view.clone());
        let Some(indicator) = &self.cfg.scroll_to_end_indicator else { return screen };
        if !scroll_view.follow_end || scroll_view.is_following_end() {
            return screen;
        }
        let b = get_scroll_view_box(layout, &scroll_view);
        let Some(clip) = b.map(|b| b.clip).filter(|c| c.width > 0 && c.height > 0) else { return screen };
        let row = clip.y + clip.height - 1;
        if row < 0 || row as usize >= screen.len() || is_image_line(&screen[row as usize]) {
            return screen;
        }
        let scrollbar_column = b.and_then(|b| get_scrollbar_geometry(b, false)).map(|g| g.column);
        let label = truncate_to_width(&indicator(), clip.width as usize, "", false);
        let label_width = visible_width(&label) as i64;
        let column = clip.x + (clip.width - label_width).div_euclid(2);
        let right_edge = scrollbar_column.unwrap_or(clip.x + clip.width);
        let available_width = (right_edge - column).max(0);
        let text = truncate_to_width(&label, available_width as usize, "", false);
        let text_width = visible_width(&text);
        if text_width == 0 {
            return screen;
        }
        let mut result = screen;
        result[row as usize] = composite_tui_line(&result[row as usize], &text, column.max(0) as usize, text_width, width);
        self.st().scroll_to_end_indicator_rect = Some(ScrollToEndIndicatorRect { row, column, width: text_width as i64 });
        result
    }

    fn composite_flashes(&self, screen: Vec<String>, width: usize, height: usize) -> Vec<String> {
        let mut flash_lines = self.flashes.render(width);
        if flash_lines.len() > height {
            flash_lines.drain(..flash_lines.len() - height);
        }
        if flash_lines.is_empty() {
            return screen;
        }
        let mut result = screen;
        while result.len() < height {
            result.push(String::new());
        }
        for (row, line) in flash_lines.iter().enumerate() {
            let flash_width = visible_width(line);
            if flash_width == 0 {
                continue;
            }
            result[row] = composite_tui_line(&result[row], line, width.saturating_sub(flash_width), flash_width, width);
        }
        result
    }

    pub(crate) fn do_render(&self, tui: &Tui) {
        if tui.is_stopped() || !self.st().alt_screen_active {
            return;
        }
        let width = tui.terminal().columns().max(1);
        let height = tui.terminal().rows().max(1);
        let root: ComponentRef = self.st().layout_root.clone().unwrap_or_else(|| self.implicit_scroll_view.clone());
        let weak = self.tui.clone();
        let request: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(t) = weak.upgrade() {
                t.request_render(false);
            }
        });
        let mut next_layout = render_layout_frame(&root, width, height, request.clone());
        if self.refresh_search(&next_layout) {
            next_layout = render_layout_frame(&root, width, height, request);
        }
        let stripped: Vec<String> = next_layout.lines.iter().map(|l| strip_osc133_zone_prefix(l).to_string()).collect();
        let mut screen = resolve_fake_cursors(stripped, tui.get_show_hardware_cursor());
        screen = self.apply_search_highlights(tui, screen, &next_layout);
        screen = self.composite_scroll_to_end_indicator(screen, &next_layout, width);
        screen = tui.composite_overlays(screen, width, height);
        if screen.len() > height {
            screen.drain(..screen.len() - height);
        }
        screen = self.apply_selection(tui, screen, Some(&next_layout));
        screen = self.composite_flashes(screen, width, height);

        let cursor_pos = extract_cursor_position(&mut screen, height);
        apply_line_resets(&mut screen);
        let screen: Vec<String> = screen
            .into_iter()
            .map(|line| {
                if is_image_line(&line) || visible_width(&line) <= width { line } else { slice_by_column(&line, 0, width, true) }
            })
            .collect();

        let (previous_screen, prev_w, prev_h, image_protocol, had_uploaded) = {
            let s = self.st();
            (
                s.previous_screen.clone(),
                s.previous_screen_width,
                s.previous_screen_height,
                s.image_protocol,
                !s.uploaded_kitty_images.is_empty(),
            )
        };
        let full_redraw = previous_screen.is_empty() || prev_w != width || prev_h != height;
        let changed_rows: Vec<bool> = screen.iter().enumerate().map(|(row, line)| previous_screen.get(row) != Some(line)).collect();
        let image_anchors_need_redraw = screen.iter().enumerate().any(|(row, line)| {
            changed_rows[row] && (is_image_line(line) || previous_screen.get(row).is_some_and(|p| is_image_line(p)))
        });
        let is_wezterm = env_is_set("WEZTERM_PANE")
            || std::env::var("TERM_PROGRAM").is_ok_and(|v| v.eq_ignore_ascii_case("wezterm"));
        let image_cells_need_redraw = !image_anchors_need_redraw
            && is_wezterm
            && image_protocol == Some(ImageProtocol::Kitty)
            && changed_rows.iter().any(|c| *c)
            && screen.iter().enumerate().any(|(row, line)| {
                let Some(placement_rows) = get_kitty_image_placement_rows(line) else { return false };
                (row..row + placement_rows).any(|r| changed_rows.get(r).copied().unwrap_or(false))
            });
        let images_need_redraw = image_anchors_need_redraw || image_cells_need_redraw;
        let redraw_images = full_redraw || images_need_redraw;
        let (prepared_lines, evicted) = if redraw_images && image_protocol == Some(ImageProtocol::Kitty) {
            self.prepare_kitty_screen(&screen)
        } else {
            (screen.clone(), String::new())
        };

        let mut buffer = String::from(BEGIN_SYNCHRONIZED_OUTPUT);
        if full_redraw {
            tui.full_redraw_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let clear_images = if image_protocol == Some(ImageProtocol::Kitty) && had_uploaded {
                delete_all_kitty_placements()
            } else {
                self.delete_kitty_images()
            };
            buffer.push_str(&clear_images);
            buffer.push_str("\x1b[2J");
        } else if images_need_redraw {
            match image_protocol {
                Some(ImageProtocol::Iterm2) => buffer.push_str("\x1b[2J"),
                Some(ImageProtocol::Kitty) => buffer.push_str(&delete_all_kitty_placements()),
                None => {}
            }
        }
        buffer.push_str(&evicted);

        let row_changed = |row: usize| full_redraw || images_need_redraw || screen.get(row) != previous_screen.get(row);
        let prepared = |row: usize| prepared_lines.get(row).map_or("", |s| s.as_str());
        let draw_kitty_images_last =
            redraw_images && image_protocol == Some(ImageProtocol::Kitty) && screen.iter().any(|l| is_image_line(l)) && is_wezterm;
        if draw_kitty_images_last {
            for row in 0..height {
                if row_changed(row) {
                    buffer.push_str(&format!("\x1b[{};1H\x1b[2K", row + 1));
                }
            }
            for row in 0..height {
                if row_changed(row) && !is_image_line(prepared(row)) {
                    buffer.push_str(&format!("\x1b[{};1H{}", row + 1, prepared(row)));
                }
            }
            for row in 0..height {
                if row_changed(row) && is_image_line(prepared(row)) {
                    buffer.push_str(&format!("\x1b[{};1H{}", row + 1, prepared(row)));
                }
            }
        } else {
            for row in 0..height {
                if row_changed(row) {
                    buffer.push_str(&format!("\x1b[{};1H\x1b[2K{}", row + 1, prepared(row)));
                }
            }
        }

        match cursor_pos {
            Some((row, col)) => {
                buffer.push_str(&format!("\x1b[{};{}H", row + 1, col.min(width) + 1));
                buffer.push_str(if tui.get_show_hardware_cursor() { "\x1b[?25h" } else { "\x1b[?25l" });
            }
            None => buffer.push_str("\x1b[?25l"),
        }
        buffer.push_str(END_SYNCHRONIZED_OUTPUT);
        tui.terminal().write(&buffer);

        let mut s = self.st();
        s.previous_screen = screen;
        s.previous_screen_width = width;
        s.previous_screen_height = height;
        s.current_layout = Some(Arc::new(next_layout));
    }
}

// =============================================================================
// Tui API for fullscreen mode
// =============================================================================

impl Tui {
    /// A fullscreen TUI on the alternate screen with an application-owned viewport.
    pub fn alt_screen(terminal: Arc<dyn Terminal>) -> Arc<Tui> {
        Self::alt_screen_with(terminal, TuiOptions::default(), TuiAltScreenOptions::default())
    }

    pub fn alt_screen_with(terminal: Arc<dyn Terminal>, mut options: TuiOptions, alt: TuiAltScreenOptions) -> Arc<Tui> {
        let lp = options.event_loop.clone().unwrap_or_else(event_loop::current);
        options.event_loop = Some(lp.clone());
        let tui = Tui::new_with_screen_fn(terminal, options, TuiMode::Fullscreen, move |weak| {
            Screen::Alt(Box::new(AltScreen::new(weak.clone(), alt, lp)))
        });
        let weak = Arc::downgrade(&tui);
        tui.add_input_listener(Arc::new(move |data: &str| {
            let tui = weak.upgrade()?;
            match &tui.screen {
                Screen::Alt(alt) => alt.handle_viewport_input(&tui, data),
                Screen::Main(_) => None,
            }
        }));
        tui
    }

    fn alt(&self) -> Option<&AltScreen> {
        match &self.screen {
            Screen::Alt(alt) => Some(alt),
            Screen::Main(_) => None,
        }
    }

    /// First visible row of the primary scroll view (fullscreen only; 0 otherwise).
    pub fn viewport_top(&self) -> i64 {
        self.alt().map_or(0, |a| a.primary_scroll_view().scroll_top())
    }

    /// Whether the primary scroll view follows new output (fullscreen only).
    pub fn is_following_output(&self) -> bool {
        self.alt().is_some_and(|a| a.primary_scroll_view().is_following_end())
    }

    pub fn set_wheel_scroll_lines(&self, lines: WheelScrollLines) {
        if let Some(a) = self.alt() {
            a.st().wheel_scroll.set_lines(lines);
        }
    }

    pub fn get_copy_on_select(&self) -> bool {
        self.alt().is_some_and(|a| a.st().copy_on_select)
    }

    pub fn set_copy_on_select(&self, enabled: bool) {
        if let Some(a) = self.alt() {
            a.st().copy_on_select = enabled;
        }
    }

    /// Whether the fullscreen viewport has a non-empty text selection.
    pub fn has_active_selection(&self) -> bool {
        self.alt().is_some_and(|a| a.active_selection_text().is_some())
    }

    /// Copy the active selection using the configured clipboard path.
    pub fn copy_active_selection_to_clipboard(&self) -> event_loop::Deferred<bool> {
        if let Some(a) = self.alt()
            && let Some(text) = a.active_selection_text()
        {
            return a.copy_text_to_clipboard(self, text);
        }
        let (resolver, deferred) = event_loop::deferred();
        resolver.resolve(false);
        deferred
    }

    /// Drop the text selection and multi-click history.
    pub fn reset_text_selection(&self) {
        if let Some(a) = self.alt() {
            a.clear_text_selection(self);
            a.st().last_click = None;
        }
    }

    /// The lines of the last rendered fullscreen frame, one per terminal row.
    pub fn get_screen_lines(&self) -> Vec<String> {
        self.alt().map_or_else(Vec::new, |a| a.st().previous_screen.clone())
    }

    /// The document written to the main screen when the fullscreen TUI stopped.
    pub fn get_last_document(&self) -> Vec<String> {
        self.alt().map_or_else(Vec::new, |a| a.st().last_document.clone())
    }

    /// Use `component` (typically a stack or scroll view) as the fullscreen layout root.
    pub fn set_layout_root(&self, component: Option<ComponentRef>) {
        if let Some(a) = self.alt() {
            a.set_layout_root(self, component);
        }
    }

    pub fn scroll_by(&self, lines: i64) {
        if let Some(a) = self.alt() {
            a.scroll_by(self, lines);
        }
    }

    pub fn scroll_to_top(&self) {
        if let Some(a) = self.alt() {
            a.scroll_to_top(self);
        }
    }

    pub fn scroll_to_bottom(&self) {
        if let Some(a) = self.alt() {
            a.scroll_to_bottom(self);
        }
    }

    /// Show a transient message in the fullscreen flash stack.
    pub fn flash(&self, message: &str, duration_ms: Option<u64>) {
        if let Some(a) = self.alt() {
            a.flash(message, duration_ms);
        }
    }
}
