//! Cases of `tui-alt-screen.test.ts` that change process-global state (environment variables,
//! terminal capabilities, Kitty image metadata, keybindings). They live in their own test
//! binary and hold [`LOCK`] so they neither race each other nor the tests in `tui_alt_screen.rs`.

use std::sync::{Arc, Mutex, MutexGuard};

use pi_tui::components::image::{Image, ImageOptions, ImageTheme};
use pi_tui::components::scroll_view::{ScrollView, ScrollViewOptions};
use pi_tui::components::stack::{HStack, StackChild, StackEntryOptions, StackOptions, VStack};
use pi_tui::components::text::Text;
use pi_tui::event_loop::{CurrentGuard, EventLoop};
use pi_tui::keybindings::{KeySpec, KeybindingsManager, TUI_KEYBINDINGS, get_keybindings, set_keybindings};
use pi_tui::layout_node::StackBasis;
use pi_tui::terminal_image::{
    ImageDimensions, ImageProtocol, KittyEncodeOptions, KittyImageMetadata, TerminalCapabilities, encode_kitty,
    register_kitty_image_metadata, reset_capabilities_cache, set_capabilities,
};
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, ComponentRef, Tui, TuiOptions, TuiStopOptions};
use pi_tui::tui_alt_screen::TuiAltScreenOptions;

static LOCK: Mutex<()> = Mutex::new(());

/// Holds [`LOCK`]; restores the saved environment variables and resets capabilities on drop.
struct Global {
    saved: Vec<(&'static str, Option<String>)>,
    _lock: MutexGuard<'static, ()>,
}

fn global(env_keys: &[&'static str]) -> Global {
    let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Global { saved: env_keys.iter().map(|k| (*k, std::env::var(k).ok())).collect(), _lock: lock }
}

impl Global {
    fn set(&self, key: &str, value: Option<&str>) {
        // SAFETY: environment access in this binary is serialized by LOCK.
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

impl Drop for Global {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            self.set(k, v.as_deref());
        }
        reset_capabilities_cache();
    }
}

struct Alt {
    lp: EventLoop,
    _guard: CurrentGuard,
    terminal: VirtualTerminal,
    tui: Arc<Tui>,
}

impl Alt {
    fn new(cols: usize, rows: usize) -> Self {
        Self::with(cols, rows, TuiAltScreenOptions::default())
    }
    fn with(cols: usize, rows: usize, options: TuiAltScreenOptions) -> Self {
        let lp = EventLoop::new_manual();
        let guard = lp.make_current();
        let terminal = VirtualTerminal::new(cols, rows);
        let tui = Tui::alt_screen_with(
            Arc::new(terminal.clone()),
            TuiOptions { event_loop: Some(lp.clone()), ..Default::default() },
            options,
        );
        Self { lp, _guard: guard, terminal, tui }
    }
    fn wait(&self) {
        self.lp.run_pending();
        self.lp.advance_ms(20);
        self.lp.run_pending();
    }
    fn send(&self, data: &str) {
        self.terminal.send_input(data);
        self.lp.run_pending();
    }
    fn writes(&self) -> String {
        self.terminal.get_writes()
    }
    fn write_count(&self) -> usize {
        self.terminal.write_calls().len()
    }
    fn writes_since(&self, from: usize) -> String {
        self.terminal.write_calls()[from..].concat()
    }
}

impl Drop for Alt {
    fn drop(&mut self) {
        self.tui.stop(TuiStopOptions::default());
    }
}

/// A component rendering fixed lines.
struct Lines(Mutex<Vec<String>>);

impl Lines {
    fn new(lines: Vec<String>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(lines)))
    }
    fn set(&self, index: usize, line: &str) {
        self.0.lock().unwrap()[index] = line.to_string();
    }
}

impl Component for Lines {
    fn render(&self, _width: usize) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn text(s: &str) -> Arc<Text> {
    Arc::new(Text::new(s, 0, 0, None))
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}

fn kitty() {
    set_capabilities(TerminalCapabilities { images: Some(ImageProtocol::Kitty), true_color: true, hyperlinks: true });
}

fn iterm2() {
    set_capabilities(TerminalCapabilities { images: Some(ImageProtocol::Iterm2), true_color: true, hyperlinks: true });
}

fn kitty_line(image_id: u32, columns: usize, rows: usize, width_px: u32, height_px: u32) -> String {
    register_kitty_image_metadata(KittyImageMetadata { image_id, columns, rows, width_px, height_px });
    encode_kitty(
        "AAAA",
        KittyEncodeOptions { columns: Some(columns), rows: Some(rows), image_id: Some(image_id), move_cursor: Some(false) },
    )
}

fn primary_scroll(child: ComponentRef) -> Arc<ScrollView> {
    ScrollView::new(child, ScrollViewOptions { primary: Some(true), ..Default::default() })
}

fn entry(component: ComponentRef, basis: StackBasis) -> StackChild {
    StackChild { component, options: StackEntryOptions { basis: Some(basis), ..Default::default() } }
}

const MUX_KEYS: &[&str] = &["TMUX", "ZELLIJ", "STY", "TERM"];

#[test]
fn uses_button_motion_tracking_inside_terminal_multiplexers() {
    let g = global(MUX_KEYS);
    for key in MUX_KEYS {
        g.set(key, None);
    }
    g.set("TERM", Some("xterm-256color"));
    {
        let h = Alt::new(80, 24);
        h.tui.start();
        assert!(h.writes().contains("\x1b[?1003h"));
    }

    let multiplexers: &[(&str, &[(&str, &str)])] = &[
        ("tmux environment", &[("TMUX", "/tmp/tmux/default,1,0")]),
        ("tmux TERM", &[("TERM", "tmux-256color")]),
        ("Zellij environment", &[("ZELLIJ", "0")]),
        ("Screen environment", &[("STY", "123.session")]),
        ("Screen TERM", &[("TERM", "screen-256color")]),
    ];
    for (name, environment) in multiplexers {
        for key in MUX_KEYS {
            g.set(key, None);
        }
        for (k, v) in *environment {
            g.set(k, Some(v));
        }
        let h = Alt::new(80, 24);
        h.tui.start();
        let writes = h.writes();
        assert!(writes.contains("\x1b[?1002h"), "{name} should enable button-motion tracking");
        assert!(!writes.contains("\x1b[?1003h"), "{name} should not enable all-motion tracking");
        assert!(writes.contains("\x1b[?1006h"), "{name} should enable SGR mouse encoding");
    }
}

#[test]
fn invokes_the_right_click_paste_handler_only_on_windows_outside_vs_code() {
    // The platform is fixed at compile time in Rust: on Windows the TS sequence applies
    // (paste outside VS Code only); elsewhere the handler is never invoked.
    let g = global(&["TERM_PROGRAM"]);
    let pastes = Arc::new(Mutex::new(0));
    let p = pastes.clone();
    let h = Alt::with(
        80,
        24,
        TuiAltScreenOptions { on_right_click_paste: Some(Arc::new(move || *p.lock().unwrap() += 1)), ..Default::default() },
    );
    g.set("TERM_PROGRAM", None);
    h.tui.start();
    h.send("\x1b[<2;1;1M");
    h.send("\x1b[<2;1;1m");
    let expected = if cfg!(windows) { 1 } else { 0 };
    assert_eq!(*pastes.lock().unwrap(), expected);

    g.set("TERM_PROGRAM", Some("vscode"));
    h.send("\x1b[<2;1;1M");
    assert_eq!(*pastes.lock().unwrap(), expected);
}

fn with_bindings<T>(bindings: &[(&str, &str)], f: impl FnOnce() -> T) -> T {
    let original = get_keybindings();
    set_keybindings(Arc::new(KeybindingsManager::new(
        TUI_KEYBINDINGS.clone(),
        bindings.iter().map(|(k, v)| (k.to_string(), Some(KeySpec::One(v.to_string())))).collect(),
    )));
    struct Restore(Arc<KeybindingsManager>);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_keybindings(self.0.clone());
        }
    }
    let _restore = Restore(original);
    f()
}

#[test]
fn scrolls_the_transcript_by_half_a_page_with_custom_bindings() {
    let _g = global(&[]);
    with_bindings(&[("tui.altScreen.halfPageUp", "ctrl+u"), ("tui.altScreen.halfPageDown", "ctrl+d")], || {
        let h = Alt::new(20, 10);
        h.tui.add_child(text(&lines(30)));
        h.tui.start();
        h.wait();
        assert_eq!(h.tui.viewport_top(), 20);
        h.send("\x15");
        h.wait();
        assert_eq!(h.tui.viewport_top(), 15);
        h.send("\x04");
        h.wait();
        assert_eq!(h.tui.viewport_top(), 20);
    });
}

#[test]
fn scrolls_the_transcript_by_one_line_with_custom_bindings() {
    let _g = global(&[]);
    with_bindings(&[("tui.altScreen.lineUp", "ctrl+y"), ("tui.altScreen.lineDown", "ctrl+e")], || {
        let h = Alt::new(20, 10);
        h.tui.add_child(text(&lines(30)));
        h.tui.start();
        h.wait();
        assert_eq!(h.tui.viewport_top(), 20);
        h.send("\x19");
        h.wait();
        assert_eq!(h.tui.viewport_top(), 19);
        h.send("\x05");
        h.wait();
        assert_eq!(h.tui.viewport_top(), 20);
    });
}

#[test]
fn does_not_emit_kitty_graphics_commands_or_osc_133_zones_in_iterm2() {
    let _g = global(&[]);
    iterm2();
    let h = Alt::new(20, 3);
    h.tui.add_child(Lines::new(strings(&["\x1b]133;B\x07\x1b]133;C\x07\x1b]133;A\x07content"])));
    h.tui.add_child(Arc::new(Image::new(
        "AAAA",
        "image/png",
        ImageTheme { fallback_color: Arc::new(|v: &str| v.to_string()) },
        ImageOptions { filename: Some("example.png".into()), ..Default::default() },
        Some(ImageDimensions { width_px: 10, height_px: 10 }),
    )));
    h.tui.start();
    h.wait();
    h.tui.stop(TuiStopOptions::default());
    let writes = h.writes();
    assert!(!writes.contains("\x1b_G"));
    assert!(!writes.contains("\x1b]133;"));
    assert!(!writes.contains("\x1b]1337;File="));
    assert!(writes.contains("[Image:"));
}

#[test]
fn clears_stale_iterm2_image_placements_when_they_leave_the_viewport() {
    let _g = global(&[]);
    iterm2();
    let h = Alt::new(20, 3);
    let image_line = "\x1b]1337;File=inline=1;width=2;height=auto:AAAA\x07";
    h.tui.add_child(Lines::new(strings(&[image_line, "", "", "after", "more", "end"])));
    h.tui.start();
    h.wait();
    h.tui.scroll_to_top();
    h.wait();
    let before = h.write_count();
    h.tui.scroll_by(1);
    h.wait();
    assert!(h.writes_since(before).contains("\x1b[2J"));
}

#[test]
fn crops_a_kitty_image_whose_first_line_is_above_the_viewport() {
    let _g = global(&[]);
    let h = Alt::new(20, 3);
    let image_line = kitty_line(123, 2, 3, 100, 100);
    h.tui.add_child(Lines::new(vec!["before".into(), image_line, String::new(), String::new(), "after".into(), "end".into()]));
    h.tui.start();
    h.wait();
    assert_eq!(h.tui.viewport_top(), 3);
    assert!(h.terminal.write_calls().iter().any(|w| w.contains("i=123") && w.contains("y=66,h=34,r=1")));
}

#[test]
fn redraws_wezterm_kitty_images_after_writes_to_covered_rows() {
    // #10319: a scrollbar update below an unchanged image anchor erased its cells.
    let g = global(&["WEZTERM_PANE", "TERM_PROGRAM"]);
    g.set("WEZTERM_PANE", Some("1"));
    kitty();
    let h = Alt::new(20, 4);
    let image_line = kitty_line(10319, 2, 3, 100, 100);
    let content = Lines::new(vec![image_line, String::new(), String::new(), "after".into()]);
    h.tui.set_layout_root(Some(content.clone()));
    h.tui.start();
    h.wait();
    let before = h.write_count();

    content.set(1, "changed");
    h.tui.request_render(false);
    h.wait();
    let redraw = h.writes_since(before);
    let placement = redraw.find("\x1b_Ga=p,q=2").expect("placement-only command");
    assert!(redraw.contains("\x1b_Ga=d,d=a,q=2\x1b\\"));
    assert!(placement > redraw.find("changed").unwrap());
    assert!(!redraw.contains("\x1b_Ga=T"));
}

#[test]
fn reuses_moved_kitty_images_without_dropping_hstack_siblings() {
    let _g = global(&[]);
    kitty();
    let h = Alt::new(20, 6);
    let label = text("left");
    let image = Arc::new(Image::new(
        "A".repeat(8192),
        "image/png",
        ImageTheme { fallback_color: Arc::new(|v: &str| v.to_string()) },
        ImageOptions::default(),
        Some(ImageDimensions { width_px: 100, height_px: 100 }),
    ));
    let header = text("header");
    let row = Arc::new(HStack::new(
        vec![entry(label.clone(), StackBasis::Size(10)), entry(image, StackBasis::Size(10))],
        StackOptions::default(),
    ));
    h.tui.set_layout_root(Some(Arc::new(VStack::new(
        vec![entry(header.clone(), StackBasis::Auto), entry(row, StackBasis::Size(4))],
        StackOptions::default(),
    ))));
    h.tui.start();
    h.wait();
    assert!(h.writes().contains("\x1b_Ga=T"));

    let before = h.write_count();
    label.set_text("changed");
    header.set_text("header\nsecond");
    h.tui.request_render(false);
    h.wait();
    let redraw = h.writes_since(before);
    let placement = redraw.find("\x1b_Ga=p,q=2").expect("placement-only command");
    assert!(redraw.contains("\x1b_Ga=d,d=a,q=2\x1b\\"));
    assert!(placement > redraw.find("changed").unwrap());
    assert!(!redraw.contains("\x1b_Ga=T"));
    assert!(redraw.len() < 2000, "expected placement-only redraw, got {} bytes", redraw.len());
    assert!(h.terminal.get_viewport().iter().any(|l| l.trim_end() == "changed"));
}

#[test]
fn retains_recently_offscreen_kitty_images_for_placement_only_reuse() {
    let _g = global(&[]);
    kitty();
    let h = Alt::new(20, 1);
    let image_id = 321;
    let image_line = kitty_line(image_id, 2, 1, 100, 50);
    h.tui.set_layout_root(Some(primary_scroll(Lines::new(vec![image_line, "after".into()]))));
    h.tui.start();
    h.wait();
    assert!(h.writes().contains("\x1b_Ga=T"));

    let before = h.write_count();
    h.tui.scroll_by(1);
    h.wait();
    h.tui.scroll_by(-1);
    h.wait();
    let reentry = h.writes_since(before);
    assert!(reentry.contains("\x1b_Ga=p,q=2"));
    assert!(!reentry.contains("\x1b_Ga=T"));
    assert!(!reentry.contains(&format!("\x1b_Ga=d,d=I,i={image_id},q=2\x1b\\")));
}

#[test]
fn evicts_the_least_recently_visible_kitty_image_when_the_cache_is_full() {
    let _g = global(&[]);
    kitty();
    let h = Alt::new(20, 1);
    let first = 500;
    let image_lines: Vec<String> = (0..18).map(|i| kitty_line(first + i, 2, 1, 100, 50)).collect();
    let count = image_lines.len();
    h.tui.set_layout_root(Some(primary_scroll(Lines::new(image_lines))));
    h.tui.start();
    h.wait();
    for _ in 1..count {
        h.tui.scroll_by(1);
        h.wait();
    }
    assert!(h.writes().contains(&format!("\x1b_Ga=d,d=I,i={first},q=2\x1b\\")));

    let before = h.write_count();
    h.tui.scroll_to_top();
    h.wait();
    assert!(h.writes_since(before).contains("\x1b_Ga=T"));
}

#[test]
fn evicts_offscreen_kitty_images_when_decoded_raster_memory_exceeds_the_cache_quota() {
    let _g = global(&[]);
    kitty();
    let h = Alt::new(20, 1);
    let first = 600;
    let image_lines: Vec<String> = (0..4).map(|i| kitty_line(first + i, 2, 1, 3840, 2160)).collect();
    let count = image_lines.len();
    h.tui.set_layout_root(Some(primary_scroll(Lines::new(image_lines))));
    h.tui.start();
    h.wait();
    for _ in 1..count {
        h.tui.scroll_by(1);
        h.wait();
    }
    assert!(h.writes().contains(&format!("\x1b_Ga=d,d=I,i={first},q=2\x1b\\")));
}
