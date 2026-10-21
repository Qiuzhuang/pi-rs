//! Fullscreen renderer tests (ported from tui-alt-screen.test.ts).

use std::sync::{Arc, Mutex};

use base64::Engine;
use pi_tui::alt_screen_search::{AltScreenSearchComponent, AltScreenSearchIndex, AltScreenSearchSegment, find_alt_screen_search_matches};
use pi_tui::components::mouse_region::MouseRegion;
use pi_tui::components::scroll_view::{ScrollFollow, ScrollView, ScrollViewOptions};
use pi_tui::components::stack::{HStack, StackChild, StackEntryOptions, StackOptions, VStack};
use pi_tui::components::text::Text;
use pi_tui::event_loop::{CurrentGuard, EventLoop};
use pi_tui::layout_node::StackBasis;
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{Component, ComponentRef, MouseEventType, MouseResult, Tui, TuiOptions, TuiStopOptions, render_fake_cursor};
use pi_tui::tui_alt_screen::{CopySelectionResult, TuiAltScreenOptions};
use pi_tui::utils::{strip_terminal_sequences, visible_width};
use pi_tui::wheel_scroll::{WheelScrollAccelerator, WheelScrollLines};

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

    fn viewport(&self) -> Vec<String> {
        self.terminal.get_viewport().iter().map(|l| l.trim_end().to_string()).collect()
    }

    fn writes(&self) -> String {
        self.terminal.get_writes()
    }
}

impl Drop for Alt {
    fn drop(&mut self) {
        self.tui.stop(TuiStopOptions::default());
    }
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}

fn text(s: &str) -> Arc<Text> {
    Arc::new(Text::new(s, 0, 0, None))
}

fn scroll_view(child: ComponentRef, follow_end: bool, primary: bool) -> Arc<ScrollView> {
    ScrollView::new(
        child,
        ScrollViewOptions {
            follow: if follow_end { Some(ScrollFollow::End) } else { None },
            primary: Some(primary),
            ..Default::default()
        },
    )
}

fn entry(component: ComponentRef, basis: StackBasis, grow: Option<usize>, shrink: Option<usize>, min: Option<usize>) -> StackChild {
    StackChild {
        component,
        options: StackEntryOptions { basis: Some(basis), grow, shrink, min_size: min, ..Default::default() },
    }
}

fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text))
}

/// Records keyboard input.
#[derive(Default)]
struct Recorder {
    inputs: Mutex<Vec<String>>,
    focused: Mutex<bool>,
    label: &'static str,
}

impl Recorder {
    fn new(label: &'static str) -> Arc<Self> {
        Arc::new(Self { label, ..Default::default() })
    }
    fn inputs(&self) -> Vec<String> {
        self.inputs.lock().unwrap().clone()
    }
}

impl Component for Recorder {
    fn render(&self, _width: usize) -> Vec<String> {
        vec![self.label.to_string()]
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        self.inputs.lock().unwrap().push(data.to_string());
    }
    fn is_focusable(&self) -> bool {
        true
    }
    fn set_focused(&self, focused: bool) {
        *self.focused.lock().unwrap() = focused;
    }
    fn focused(&self) -> bool {
        *self.focused.lock().unwrap()
    }
}

#[test]
fn renders_terminal_height_viewport_and_preserves_manual_scroll() {
    let h = Alt::new(20, 4);
    let t = text(&lines(10));
    h.tui.add_child(t.clone());
    h.tui.start();
    h.wait();
    assert_eq!(h.viewport(), ["line 7", "line 8", "line 9", "line 10"]);
    assert!(h.tui.is_following_output());

    h.send("\x1b[<64;1;1M");
    h.wait();
    assert_eq!(h.viewport(), ["line 6", "line 7", "line 8", "line 9"]);
    assert_eq!(h.tui.viewport_top(), 5);
    assert!(!h.tui.is_following_output());

    t.set_text(lines(12));
    h.tui.request_render(false);
    h.wait();
    assert_eq!(h.viewport(), ["line 6", "line 7", "line 8", "line 9"]);
}

#[test]
fn shows_clickable_jump_to_end_indicator() {
    let h = Alt::with(
        30,
        6,
        TuiAltScreenOptions {
            scroll_to_end_indicator: Some(Arc::new(|| "\x1b[7m ↓ Jump to end \x1b[27m".to_string())),
            ..Default::default()
        },
    );
    let transcript = scroll_view(text(&lines(8)), true, true);
    h.tui.set_layout_root(Some(Arc::new(VStack::new(
        vec![
            entry(transcript.clone(), StackBasis::Size(0), Some(1), None, Some(1)),
            entry(text("editor\nfooter"), StackBasis::Auto, None, None, Some(1)),
        ],
        StackOptions::default(),
    ))));
    h.tui.start();
    h.wait();
    assert!(!h.viewport().iter().any(|l| l.contains("Jump to end")));

    h.send("\x1b[<64;1;1M");
    h.wait();
    assert!(!transcript.is_following_end());
    assert_eq!(h.terminal.get_viewport()[3], "line 7  ↓ Jump to end         ");
    assert_eq!(h.viewport()[4], "editor");

    h.send("\x1b[<0;2;4M");
    h.send("\x1b[<0;2;4m");
    h.wait();
    assert!(!transcript.is_following_end());

    h.send("\x1b[<0;15;4M");
    h.send("\x1b[<0;15;4m");
    h.wait();
    assert!(transcript.is_following_end());
    assert_eq!(h.viewport(), ["line 5", "line 6", "line 7", "line 8", "editor", "footer"]);
}

#[test]
fn keeps_explicit_dock_fixed_while_transcript_scrolls() {
    let h = Alt::new(20, 6);
    let transcript_text = text(&lines(8));
    let transcript = scroll_view(transcript_text.clone(), true, true);
    let dock = Arc::new(VStack::new(vec![StackChild::from(text("editor") as ComponentRef), StackChild::from(text("footer") as ComponentRef)], StackOptions::default()));
    h.tui.set_layout_root(Some(Arc::new(VStack::new(
        vec![
            entry(transcript.clone(), StackBasis::Size(0), Some(1), None, Some(1)),
            entry(dock, StackBasis::Auto, None, None, Some(1)),
        ],
        StackOptions::default(),
    ))));
    h.tui.start();
    h.wait();
    assert_eq!(h.viewport(), ["line 5", "line 6", "line 7", "line 8", "editor", "footer"]);

    h.send("\x1b[<64;1;6M");
    h.wait();
    assert_eq!(h.viewport(), ["line 4", "line 5", "line 6", "line 7", "editor", "footer"]);
    assert!(!transcript.is_following_end());

    transcript_text.set_text(lines(10));
    h.tui.request_render(false);
    h.wait();
    assert_eq!(h.viewport(), ["line 4", "line 5", "line 6", "line 7", "editor", "footer"]);

    h.tui.scroll_to_bottom();
    h.wait();
    assert_eq!(h.viewport(), ["line 7", "line 8", "line 9", "line 10", "editor", "footer"]);
}

#[test]
fn routes_wheel_input_to_scroll_view_under_pointer() {
    let h = Alt::new(20, 4);
    let left = scroll_view(text("a1\na2\na3\na4\na5\na6\na7"), true, true);
    let right = scroll_view(text("b1\nb2\nb3\nb4\nb5\nb6\nb7"), true, false);
    h.tui.set_layout_root(Some(Arc::new(HStack::new(
        vec![
            entry(left.clone(), StackBasis::Size(10), None, Some(0), None),
            entry(right.clone(), StackBasis::Size(10), None, Some(0), None),
        ],
        StackOptions::default(),
    ))));
    h.tui.start();
    h.wait();
    h.send("\x1b[<64;15;1M");
    h.wait();
    assert_eq!(left.scroll_top(), 3);
    assert_eq!(right.scroll_top(), 2);
    assert_eq!(h.viewport(), ["a4        b3", "a5        b4", "a6        b5", "a7        b6"]);
}

#[test]
fn scrolls_faster_with_alt_wheel_and_ignores_horizontal_wheel() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text(&lines(12)));
    h.tui.start();
    h.wait();
    assert_eq!(h.tui.viewport_top(), 8);
    h.send("\x1b[<66;1;1M");
    h.send("\x1b[<67;1;1M");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 8);
    h.send("\x1b[<72;1;1M");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 3);
}

#[test]
fn applies_runtime_wheel_line_count_updates() {
    let h = Alt::with(20, 4, TuiAltScreenOptions { wheel_scroll_lines: Some(WheelScrollLines::Lines(3.0)), ..Default::default() });
    let deltas = Arc::new(Mutex::new(Vec::new()));
    let d = deltas.clone();
    h.tui.add_child(Arc::new(MouseRegion::new(
        text("wheel target"),
        Arc::new(move |event| {
            if event.kind != MouseEventType::Wheel {
                return None;
            }
            d.lock().unwrap().push(event.wheel_delta);
            Some(MouseResult::handled())
        }),
    )));
    h.tui.start();
    h.wait();
    h.send("\x1b[<64;1;1M");
    h.tui.set_wheel_scroll_lines(WheelScrollLines::Lines(2.0));
    h.send("\x1b[<65;1;1M");
    h.send("\x1b[<72;1;1M");
    assert_eq!(*deltas.lock().unwrap(), vec![Some(-3), Some(2), Some(-10)]);
}

#[test]
fn chains_unused_wheel_delta_to_outer_scroll_view() {
    let h = Alt::with(20, 4, TuiAltScreenOptions { wheel_scroll_lines: Some(WheelScrollLines::Lines(3.0)), ..Default::default() });
    let inner = scroll_view(text("i1\ni2\ni3\ni4\ni5\ni6"), false, false);
    let outer = scroll_view(
        Arc::new(VStack::new(
            vec![entry(inner.clone(), StackBasis::Size(2), None, None, None), StackChild::from(text("tail1\ntail2\ntail3\ntail4\ntail5") as ComponentRef)],
            StackOptions::default(),
        )),
        false,
        true,
    );
    h.tui.set_layout_root(Some(outer.clone()));
    h.tui.start();
    h.wait();
    h.send("\x1b[<65;1;1M");
    h.wait();
    assert_eq!(inner.scroll_top(), 3);
    assert_eq!(outer.scroll_top(), 0);
    h.send("\x1b[<65;1;1M");
    h.wait();
    assert_eq!(inner.scroll_top(), 4);
    assert_eq!(outer.scroll_top(), 2);
}

#[test]
fn keyboard_viewport_navigation_with_page_overlap() {
    let h = Alt::new(20, 8);
    h.tui.add_child(text(&lines(12)));
    h.tui.start();
    h.wait();
    h.send("\x1b[57421u");
    h.send("\x1b[57421;1:3u");
    h.wait();
    assert_eq!(h.viewport(), (1..=8).map(|i| format!("line {i}")).collect::<Vec<_>>());
    h.send("\x1b[57422u");
    h.send("\x1b[57422;1:3u");
    h.wait();
    assert_eq!(h.viewport(), (5..=12).map(|i| format!("line {i}")).collect::<Vec<_>>());
    h.send("\x1b[7^");
    h.wait();
    assert_eq!(h.viewport(), (1..=8).map(|i| format!("line {i}")).collect::<Vec<_>>());
    h.send("\x1b[8^");
    h.wait();
    assert_eq!(h.viewport(), (5..=12).map(|i| format!("line {i}")).collect::<Vec<_>>());
}

#[test]
fn search_matches_across_rows_and_maps_columns() {
    let m = find_alt_screen_search_matches(&["alpha QUICK".into(), "brown fox".into()], "quick brown");
    assert_eq!(m.len(), 1);
    assert_eq!(
        m[0].segments,
        vec![AltScreenSearchSegment { row: 0, start_col: 6, end_col: 11 }, AltScreenSearchSegment { row: 1, start_col: 0, end_col: 5 }]
    );
    let m = find_alt_screen_search_matches(&["\x1b[31mfoo  bar\x1b[0m".into(), "A界🙂éZ".into()], "oo   bar\nA界🙂é");
    assert_eq!(
        m[0].segments,
        vec![
            AltScreenSearchSegment { row: 0, start_col: 1, end_col: 3 },
            AltScreenSearchSegment { row: 0, start_col: 5, end_col: 8 },
            AltScreenSearchSegment { row: 1, start_col: 0, end_col: 6 },
        ]
    );

    let mut index = AltScreenSearchIndex::new();
    let lines = vec!["alpha needle".to_string(), "omega".to_string()];
    let initial = index.search(&lines, "needle");
    assert!(initial.changed);
    assert_eq!(initial.matches.len(), 1);
    assert!(!index.search(&lines, "needle").changed);
    let changed = index.search(&lines, "omega");
    assert!(changed.changed);
    assert_eq!(changed.matches[0].segments, vec![AltScreenSearchSegment { row: 1, start_col: 0, end_col: 5 }]);
    let changed_lines = index.search(&["alpha needle".to_string(), "no match".to_string()], "omega");
    assert!(changed_lines.changed);
    assert!(changed_lines.matches.is_empty());
}

#[test]
fn renders_search_box_with_placeholder_and_controls() {
    let component = AltScreenSearchComponent::new(Arc::new(|_| {}), None);
    let rendered = component.render(48);
    let lines: Vec<String> = rendered.iter().map(|l| strip_terminal_sequences(l)).collect();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| visible_width(l) == 48));
    assert!(lines[0].starts_with('┌') && lines[0].ends_with('┐'));
    assert!(lines[1].starts_with("│ Find in transcript"));
    assert!(rendered[1].contains("\x1b[2m"));
    assert!(lines[2].ends_with(" ↑ Shift+Enter · ↓ Enter ─┘"), "{}", lines[2]);
    let controls: Vec<char> = lines[2].chars().collect();
    let col = |needle: char| controls.iter().position(|&c| c == needle).unwrap() as i64;
    assert_eq!(component.get_navigation_direction_at(2, col('↑')), Some(-1));
    assert_eq!(component.get_navigation_direction_at(2, col('·')), None);
    assert_eq!(component.get_navigation_direction_at(2, col('↓')), Some(1));

    component.handle_input("n");
    component.set_result(0, 2);
    let populated = component.render(48);
    let plain: Vec<String> = populated.iter().map(|l| strip_terminal_sequences(l)).collect();
    assert!(plain[1].contains('n') && plain[1].contains("1/2"));
    assert!(populated[1].contains("\x1b[2m 1/2 \x1b[22m"));
    assert!(!plain.iter().any(|l| l.contains("Find in transcript")));
}

#[test]
fn searches_transcript_and_restores_editor_focus_on_close() {
    let h = Alt::new(60, 8);
    let transcript_text = text(
        &(0..12)
            .map(|i| match i {
                4 => "line 5 needle one".to_string(),
                9 => "line 10 needle two".to_string(),
                _ => format!("line {}", i + 1),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let transcript = scroll_view(transcript_text, true, true);
    let editor = Recorder::new("editor");
    h.tui.set_layout_root(Some(Arc::new(VStack::new(
        vec![
            entry(transcript.clone(), StackBasis::Size(0), Some(1), None, Some(1)),
            entry(editor.clone(), StackBasis::Size(1), None, Some(0), None),
        ],
        StackOptions::default(),
    ))));
    h.tui.set_focus(Some(editor.clone()));
    h.tui.start();
    h.wait();

    h.send("\x1b[102;6u");
    h.send("needle");
    h.wait();
    assert!(!transcript.is_following_end());
    let vp = h.viewport();
    assert!(vp.iter().any(|l| l.contains("2/2")), "{vp:?}");
    assert!(vp.iter().any(|l| l.contains("↑ Shift+Enter · ↓ Enter")));
    assert!(vp.iter().any(|l| l.contains("line 10 needle two")));
    assert!(editor.inputs().is_empty());
    assert!(h.writes().contains("\x1b[1;7mneedle\x1b[22;27m"));

    for _ in 0..6 {
        h.send("\x1b[<64;1;4M");
    }
    h.wait();
    assert_eq!(transcript.scroll_top(), 0);

    h.send("\x07");
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("1/2")));
    assert!(h.viewport().iter().any(|l| l.contains("line 5 needle one")));

    h.send("\x1b[103;6u");
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("2/2")));

    h.send("\x1b");
    h.send("x");
    h.wait();
    assert!(!h.viewport().iter().any(|l| l.contains("↑ Shift+Enter · ↓ Enter")));
    assert_eq!(editor.inputs(), vec!["x"]);
}

#[test]
fn routes_home_end_to_focused_component_and_ctrl_home_end_to_transcript() {
    let h = Alt::new(20, 6);
    let transcript = scroll_view(text(&lines(12)), true, true);
    let editor = Recorder::new("editor");
    h.tui.set_layout_root(Some(Arc::new(VStack::new(
        vec![
            entry(transcript.clone(), StackBasis::Size(0), Some(1), None, Some(1)),
            entry(editor.clone(), StackBasis::Size(1), None, Some(0), None),
        ],
        StackOptions::default(),
    ))));
    h.tui.set_focus(Some(editor.clone()));
    h.tui.start();
    h.wait();
    let bottom = transcript.scroll_top();
    assert!(bottom > 0);
    let editor_keys = ["\x1bOH", "\x1b[F", "\x1b[57423u", "\x1b[5;5~", "\x1b[6;5~"];
    for key in editor_keys {
        h.send(key);
    }
    h.wait();
    assert_eq!(transcript.scroll_top(), bottom);
    assert_eq!(editor.inputs(), editor_keys);
    h.send("\x1b[1;5H");
    h.wait();
    assert_eq!(transcript.scroll_top(), 0);
    h.send("\x1b[1;5F");
    h.wait();
    assert_eq!(transcript.scroll_top(), bottom);
    assert!(transcript.is_following_end());
    h.send("\x1b[6~");
    h.wait();
    assert_eq!(editor.inputs(), editor_keys);
}

#[test]
fn jumps_between_osc133_prompt_markers() {
    let h = Alt::new(20, 3);
    let content = (1..=4).flat_map(|m| [format!("\x1b]133;A\x07message {m}"), "detail".to_string()]).collect::<Vec<_>>().join("\n");
    h.tui.add_child(text(&content));
    h.tui.start();
    h.wait();
    assert_eq!(h.tui.viewport_top(), 5);
    h.send("\x1b[57419;6u");
    h.send("\x1b[57419;6:3u");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 4);
    assert_eq!(h.viewport()[0], "message 3");
    h.send("\x1b[1;6A");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 2);
    assert_eq!(h.viewport()[0], "message 2");
    h.send("\x1b[1;6B");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 4);
    h.send("\x1b[1;6B");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 5);
    assert_eq!(h.viewport()[1], "message 4");
    assert!(h.tui.is_following_output());
}

#[test]
fn selects_text_and_copies_with_osc52() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text("\x1b[1mal\x1b[0mpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<3;4;2m");
    h.wait();
    let writes = h.writes();
    assert!(writes.contains(&osc52("alpha\nbeta")));
    assert!(writes.contains("al\x1b[0m\x1b[7mpha"), "selection inverse must be reapplied after a reset");
    assert!(h.viewport().iter().any(|l| l.contains("Copied!")));
}

#[test]
fn keeps_selection_highlight_after_fake_cursor() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text(&format!("> {}bcd", render_fake_cursor("a"))));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;6;1M");
    h.send("\x1b[<3;6;1m");
    h.wait();
    assert!(h.writes().contains("\x1b[27m\x1b[7mbcd"));
}

fn copy_recorder(copied: &Arc<Mutex<Vec<String>>>, result: CopySelectionResult) -> TuiAltScreenOptions {
    let c = copied.clone();
    TuiAltScreenOptions {
        copy_selection: Some(Arc::new(move |text: &str| {
            c.lock().unwrap().push(text.to_string());
            pi_tui::event_loop::ready(result.clone())
        })),
        ..Default::default()
    }
}

#[test]
fn uses_injected_copy_selection_handler() {
    let copied = Arc::new(Mutex::new(Vec::new()));
    let h = Alt::with(20, 4, copy_recorder(&copied, CopySelectionResult::Copied));
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert_eq!(*copied.lock().unwrap(), vec!["alpha\nbeta"]);
    assert!(!h.writes().contains("\x1b]52;c;"));
    assert!(h.viewport().iter().any(|l| l.contains("Copied!")));
}

#[test]
fn flashes_specific_copy_error() {
    let copied = Arc::new(Mutex::new(Vec::new()));
    let h = Alt::with(40, 4, copy_recorder(&copied, CopySelectionResult::Error("No clipboard".into())));
    h.tui.add_child(text("alpha\nbeta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("No clipboard")));
}

#[test]
fn leaves_selection_without_copying_when_copy_on_select_disabled() {
    let copied = Arc::new(Mutex::new(Vec::new()));
    let mut options = copy_recorder(&copied, CopySelectionResult::Copied);
    options.copy_on_select = Some(false);
    let h = Alt::with(20, 4, options);
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert!(copied.lock().unwrap().is_empty());
    assert!(h.tui.has_active_selection());
    assert!(h.writes().contains("\x1b[7m"));
    assert!(h.viewport().iter().all(|l| !l.contains("Copied!")));

    let result = h.tui.copy_active_selection_to_clipboard();
    h.lp.run_pending();
    assert_eq!(result.try_take(), Some(true));
    assert_eq!(*copied.lock().unwrap(), vec!["alpha\nbeta"]);
}

#[test]
fn double_click_word_selection() {
    let h = Alt::new(20, 1);
    h.tui.add_child(text("foo  bar"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<0;1;1m");
    h.send("\x1b[<0;3;1M");
    h.wait();
    assert!(h.writes().contains("foo\x1b[27m"));

    for (line, needle) in [("extensions/starline/fixed-editor/compositor.ts", "starline"), ("earendil-works/pi-tui", "works")] {
        let copied = Arc::new(Mutex::new(Vec::new()));
        let h = Alt::with(80, 1, copy_recorder(&copied, CopySelectionResult::Copied));
        h.tui.add_child(text(line));
        h.tui.start();
        h.wait();
        let col = line.find(needle).unwrap() + 1;
        for _ in 0..2 {
            h.send(&format!("\x1b[<0;{col};1M"));
            h.send(&format!("\x1b[<0;{col};1m"));
        }
        h.wait();
        assert_eq!(*copied.lock().unwrap(), vec![line.to_string()]);
    }
}

#[test]
fn word_drag_and_triple_click_line_selection() {
    let h = Alt::new(20, 2);
    h.tui.add_child(text("zero alpha beta\ngamma delta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;6;1M");
    h.send("\x1b[<0;6;1m");
    h.send("\x1b[<0;10;1M");
    h.send("\x1b[<0;10;1m");
    h.wait();
    assert!(h.writes().contains(&osc52("alpha")));

    h.send("\x1b[<0;12;1M");
    h.send("\x1b[<0;12;1m");
    h.send("\x1b[<0;14;1M");
    h.send("\x1b[<32;3;2M");
    h.send("\x1b[<0;3;2m");
    h.wait();
    assert!(h.writes().contains(&osc52("beta\ngamma")));

    for col in [7, 9, 11] {
        h.send(&format!("\x1b[<0;{col};2M"));
        h.send(&format!("\x1b[<0;{col};2m"));
    }
    h.wait();
    assert!(h.writes().contains(&osc52("gamma delta")));
}

#[test]
fn stacks_flash_messages_and_collapses_them() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text("one\ntwo\nthree\nfour"));
    h.tui.start();
    h.wait();
    h.tui.flash("First", Some(80));
    h.tui.flash("Second", Some(500));
    h.wait();
    let vp = h.terminal.get_viewport();
    assert!(vp[0].ends_with(" First "), "{vp:?}");
    assert!(vp[1].ends_with(" Second "));
    h.lp.advance_ms(100);
    h.wait();
    let vp = h.terminal.get_viewport();
    assert!(vp[0].ends_with(" Second "));
    assert!(!vp.iter().any(|l| l.contains("First")));
}

#[test]
fn auto_scrolls_drag_selection_at_viewport_edge() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text(&lines(10)));
    h.tui.start();
    h.wait();
    assert_eq!(h.tui.viewport_top(), 6);
    h.send("\x1b[<0;1;3M");
    h.send("\x1b[<32;1;1M");
    h.lp.advance_ms(130);
    h.wait();
    let top = h.tui.viewport_top();
    assert!(top < 6, "expected auto-scroll, got {top}");
    h.send("\x1b[<0;1;1m");
    h.wait();
    let mut selected: Vec<String> = (0..(8 - top)).map(|i| format!("line {}", top + i + 1)).collect();
    selected.push("l".into());
    assert!(h.writes().contains(&osc52(&selected.join("\n"))));
}

#[test]
fn snaps_selection_to_grapheme_boundaries() {
    let h = Alt::new(20, 2);
    h.tui.add_child(text("A界🙂éZ"));
    h.tui.start();
    h.wait();
    let wide = osc52("界🙂");
    h.send("\x1b[<0;3;1M");
    h.send("\x1b[<32;4;1M");
    h.send("\x1b[<0;4;1m");
    h.wait();
    assert_eq!(h.writes().matches(&wide).count(), 1);
    h.send("\x1b[<0;5;1M");
    h.send("\x1b[<32;2;1M");
    h.send("\x1b[<0;2;1m");
    h.wait();
    assert_eq!(h.writes().matches(&wide).count(), 2);
    h.send("\x1b[<0;6;1M");
    h.send("\x1b[<32;7;1M");
    h.send("\x1b[<0;7;1m");
    h.wait();
    assert!(h.writes().contains(&osc52("éZ")));
}

#[test]
fn prints_full_document_when_leaving_alt_mode() {
    let h = Alt::new(20, 3);
    h.tui.add_child(text("first\nsecond\nthird\nfourth\nfifth\nsixth"));
    h.tui.start();
    h.wait();
    let calls_before = h.terminal.write_calls().len();
    let enter = h.terminal.write_calls().iter().position(|w| w.contains("\x1b[?1049h"));
    assert!(enter.is_some());
    h.tui.stop(TuiStopOptions::default());
    let calls = h.terminal.write_calls();
    let mouse_disable = calls.iter().rposition(|w| w.contains("\x1b[?1006l")).unwrap();
    let restore = calls.iter().rposition(|w| w.contains("\x1b[?1049l")).unwrap();
    assert!(mouse_disable >= calls_before && restore > mouse_disable);
    let data = &calls[restore];
    for word in ["first", "second", "third", "fourth", "fifth", "sixth"] {
        assert!(data.contains(word));
    }
    assert!(data.find("first") < data.find("sixth"));
    assert_eq!(h.tui.get_last_document().len(), 6);
}

#[test]
fn gives_viewport_keys_to_focused_overlay() {
    let h = Alt::new(20, 6);
    h.tui.add_child(text(&lines(12)));
    let overlay = Recorder::new("overlay");
    h.tui.start();
    h.wait();
    let top_before = h.tui.viewport_top();
    let handle = h.tui.show_overlay(overlay.clone(), None);
    h.wait();
    assert!(overlay.focused());
    let keys = ["\x1b[5~", "\x1b[6~", "\x1bOH", "\x1bOF"];
    for key in keys {
        h.send(key);
    }
    h.wait();
    assert_eq!(overlay.inputs(), keys);
    assert_eq!(h.tui.viewport_top(), top_before);
    handle.hide();
    h.wait();
    h.send("\x1b[5~");
    h.wait();
    assert!(h.tui.viewport_top() < top_before);
}

#[test]
fn keeps_viewport_scrolling_while_search_is_focused() {
    let h = Alt::new(20, 6);
    h.tui.add_child(text(&lines(12)));
    h.tui.start();
    h.wait();
    let top_before = h.tui.viewport_top();
    h.send("\x1b[102;6u");
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("↑ ↓")), "{:?}", h.viewport());
    h.send("\x1b[5~");
    h.send("\x1b[<64;1;4M");
    h.wait();
    assert!(h.tui.viewport_top() < top_before);
    assert!(h.viewport().iter().any(|l| l.contains("↑ ↓")));
}

#[test]
fn wheel_scroll_accelerator() {
    let scroll = |a: &mut WheelScrollAccelerator, times: &[f64]| times.iter().map(|t| a.next(1, *t)).collect::<Vec<_>>();
    let mut a = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Lines(3.0), true);
    assert_eq!(scroll(&mut a, &[0.0, 10.0, 20.0, 1000.0]), [3, 3, 3, 3]);
    a.set_lines(WheelScrollLines::Lines(0.5));
    assert_eq!(a.next(1, 2000.0), 1);
    let mut a = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, false);
    assert_eq!(scroll(&mut a, &[0.0, 10.0, 20.0, 30.0]), [1, 1, 1, 1]);
    let mut a = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut a, &[0.0, 150.0, 300.0, 450.0]), [1, 1, 1, 1]);
    assert_eq!(scroll(&mut a, &[1000.0, 1050.0, 1100.0, 1150.0]), [1, 2, 2, 2]);
    assert_eq!(scroll(&mut a, &[2000.0, 2020.0, 2040.0, 2060.0]), [1, 5, 5, 5]);
    assert_eq!(scroll(&mut a, &[3000.0, 3010.0, 3020.0, 3030.0]), [1, 6, 6, 6]);
    let mut a = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut a, &[0.0, 3.0, 6.0, 9.0]), [1, 1, 1, 1]);
    let mut a = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut a, &[0.0, 20.0, 40.0]), [1, 5, 5]);
    assert_eq!(a.next(-1, 60.0), 1);
    assert_eq!(scroll(&mut a, &[500.0, 520.0]), [1, 5]);
    let mut a = WheelScrollAccelerator::with_acceleration(WheelScrollLines::Auto, true);
    assert_eq!(scroll(&mut a, &[0.0, 40.0, 80.0, 120.0, 160.0]), [1, 2, 3, 2, 3]);
}
