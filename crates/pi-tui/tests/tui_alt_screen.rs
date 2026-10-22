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
    let cached = index.search(&lines, "needle");
    assert!(!cached.changed);
    assert_eq!(cached.matches, initial.matches);
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
    let col_of = |needle: &str, last: bool| {
        let line = &lines[2];
        let byte = if last { line.rfind(needle) } else { line.find(needle) }.unwrap();
        line[..byte].chars().count() as i64
    };
    assert_eq!(component.get_navigation_direction_at(2, col('↑')), Some(-1));
    assert_eq!(component.get_navigation_direction_at(2, col_of("Shift+Enter", false) + 5), Some(-1));
    assert_eq!(component.get_navigation_direction_at(2, col('·')), None);
    assert_eq!(component.get_navigation_direction_at(2, col('↓')), Some(1));
    assert_eq!(component.get_navigation_direction_at(2, col_of("Enter", true) + 2), Some(1));

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
    assert!(h.viewport().iter().any(|l| l.contains("needle") && l.contains("2/2")));

    h.send("\x07");
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("1/2")));
    assert!(h.viewport().iter().any(|l| l.contains("line 5 needle one")));

    h.send("\x1b[103;6u");
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("2/2")));
    assert!(h.viewport().iter().any(|l| l.contains("line 10 needle two")));

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
    h.send("\x1b[57423;5u");
    h.send("\x1b[57423;5:3u");
    h.wait();
    assert_eq!(transcript.scroll_top(), 0);
    h.send("\x1b[6~");
    h.wait();
    assert_eq!(transcript.scroll_top(), 1);
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
    h.send("\x1b[57420;6u");
    h.send("\x1b[57420;6:3u");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 4);
    assert_eq!(h.viewport()[0], "message 3");
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

#[derive(Debug, Clone, PartialEq)]
enum TermEvent {
    Start,
    Write(String),
    Stop,
}

/// `RecordingTerminal`: a virtual terminal that also records start/stop around writes.
struct RecordingTerminal {
    inner: VirtualTerminal,
    events: Mutex<Vec<TermEvent>>,
}

impl pi_tui::terminal::Terminal for RecordingTerminal {
    fn start(&self, on_input: pi_tui::terminal::InputHandler, on_resize: pi_tui::terminal::ResizeHandler) {
        self.events.lock().unwrap().push(TermEvent::Start);
        self.inner.start(on_input, on_resize);
    }
    fn stop(&self) {
        self.events.lock().unwrap().push(TermEvent::Stop);
        self.inner.stop();
    }
    fn write(&self, data: &str) {
        self.events.lock().unwrap().push(TermEvent::Write(data.to_string()));
        self.inner.write(data);
    }
    fn columns(&self) -> usize {
        self.inner.columns()
    }
    fn rows(&self) -> usize {
        self.inner.rows()
    }
    fn kitty_protocol_active(&self) -> bool {
        self.inner.kitty_protocol_active()
    }
    fn hide_cursor(&self) {
        self.inner.hide_cursor();
    }
    fn show_cursor(&self) {
        self.inner.show_cursor();
    }
}

#[test]
fn restores_keyboard_state_before_leaving_alt_mode_and_prints_the_full_document() {
    let lp = EventLoop::new_manual();
    let _guard = lp.make_current();
    let terminal = Arc::new(RecordingTerminal { inner: VirtualTerminal::new(20, 3), events: Mutex::new(Vec::new()) });
    let tui = Tui::alt_screen_with(
        terminal.clone(),
        TuiOptions { event_loop: Some(lp.clone()), ..Default::default() },
        TuiAltScreenOptions::default(),
    );
    tui.add_child(text("first\nsecond\nthird\nfourth\nfifth\nsixth"));
    tui.start();
    lp.run_pending();
    lp.advance_ms(20);
    lp.run_pending();
    tui.stop(TuiStopOptions::default());

    let events = terminal.events.lock().unwrap().clone();
    let write_with = |needle: &str| events.iter().position(|e| matches!(e, TermEvent::Write(d) if d.contains(needle)));
    let start = events.iter().position(|e| *e == TermEvent::Start).unwrap();
    let stop = events.iter().position(|e| *e == TermEvent::Stop).unwrap();
    let enter = write_with("\x1b[?1049h").unwrap();
    let mouse_disable = write_with("\x1b[?1006l").unwrap();
    let restore = write_with("\x1b[?1049l").unwrap();
    assert!(enter < start, "alt screen entered before the terminal starts");
    assert!(mouse_disable < stop, "mouse reporting disabled before the terminal stops");
    assert!(restore > stop, "main screen restored after the terminal stops");

    let TermEvent::Write(data) = &events[restore] else { unreachable!() };
    for word in ["first", "second", "third", "fourth", "fifth", "sixth"] {
        assert!(data.contains(word), "{word}");
    }
    assert!(data.find("first") < data.find("sixth"));
    assert_eq!(tui.get_last_document().len(), 6);
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

// ---- further cases of tui-alt-screen.test.ts --------------------------------------------------

impl Alt {
    /// Writes since `from` write calls, joined.
    fn writes_since(&self, from: usize) -> String {
        self.terminal.write_calls()[from..].concat()
    }
    fn write_count(&self) -> usize {
        self.terminal.write_calls().len()
    }
    fn wait_ms(&self, ms: u64) {
        self.lp.run_pending();
        self.lp.advance_ms(ms);
        self.lp.run_pending();
    }
}

/// A component rendering fixed lines, with optional mouse handling and a render counter.
struct Custom {
    lines: Mutex<Vec<String>>,
    renders: Mutex<usize>,
    invalidated: Mutex<bool>,
    #[allow(clippy::type_complexity)]
    on_mouse: Option<Box<dyn Fn(&pi_tui::tui::MouseEvent) -> Option<MouseResult> + Send + Sync>>,
}

impl Custom {
    fn new(lines: &[&str]) -> Arc<Self> {
        Arc::new(Self::with_mouse(lines, None))
    }
    #[allow(clippy::type_complexity)]
    fn with_mouse(
        lines: &[&str],
        on_mouse: Option<Box<dyn Fn(&pi_tui::tui::MouseEvent) -> Option<MouseResult> + Send + Sync>>,
    ) -> Self {
        Self {
            lines: Mutex::new(lines.iter().map(|s| s.to_string()).collect()),
            renders: Mutex::new(0),
            invalidated: Mutex::new(false),
            on_mouse,
        }
    }
    fn renders(&self) -> usize {
        *self.renders.lock().unwrap()
    }
}

impl Component for Custom {
    fn render(&self, _width: usize) -> Vec<String> {
        *self.renders.lock().unwrap() += 1;
        self.lines.lock().unwrap().clone()
    }
    fn invalidate(&self) {
        *self.invalidated.lock().unwrap() = true;
    }
    fn handle_mouse(&self, event: &pi_tui::tui::MouseEvent) -> Option<MouseResult> {
        self.on_mouse.as_ref().and_then(|f| f(event))
    }
}

fn scroll_view_with(child: ComponentRef, options: ScrollViewOptions) -> Arc<ScrollView> {
    ScrollView::new(child, options)
}

fn transcript_and_dock(transcript: Arc<ScrollView>) -> ComponentRef {
    Arc::new(VStack::new(
        vec![
            entry(transcript, StackBasis::Size(0), Some(1), None, Some(1)),
            entry(text("editor\nfooter"), StackBasis::Auto, None, None, Some(1)),
        ],
        StackOptions::default(),
    ))
}

fn indicator(label: &'static str) -> TuiAltScreenOptions {
    TuiAltScreenOptions { scroll_to_end_indicator: Some(Arc::new(move || label.to_string())), ..Default::default() }
}

#[test]
fn keeps_the_jump_to_end_indicator_centered_as_the_auto_scrollbar_hides_and_reappears() {
    // #9136: auto scrollbar visibility must not move the indicator.
    let label = " ↓ Jump to latest message · End ";
    let h = Alt::with(80, 6, indicator(" ↓ Jump to latest message · End "));
    let transcript = scroll_view_with(
        text(&lines(20)),
        ScrollViewOptions {
            follow: Some(ScrollFollow::End),
            primary: Some(true),
            scrollbar: Some(pi_tui::components::scroll_view::ScrollViewScrollbar::Auto),
            scrollbar_hide_delay_ms: Some(0),
            ..Default::default()
        },
    );
    h.tui.set_layout_root(Some(transcript.clone()));
    h.tui.start();
    h.wait();

    let column = |h: &Alt| {
        let row = &h.terminal.get_viewport()[5];
        row.find(label).map(|byte| row[..byte].chars().count())
    };
    // Scrolling over the track keeps the scrollbar visible until the pointer leaves.
    h.send("\x1b[<64;80;1M");
    h.wait();
    assert!(transcript.is_scrollbar_visible());
    assert!(!transcript.is_following_end());
    let scroll_top = transcript.scroll_top();
    let visible = column(&h);

    // Leaving the track lets the auto-hide timer expire without changing the content.
    h.send("\x1b[<35;79;1M");
    h.wait();
    assert!(!transcript.is_scrollbar_visible());
    assert_eq!(transcript.scroll_top(), scroll_top);
    let hidden = column(&h);

    h.send("\x1b[<35;80;1M");
    h.wait();
    assert!(transcript.is_scrollbar_visible());
    assert_eq!(transcript.scroll_top(), scroll_top);
    let revealed = column(&h);

    assert_eq!([visible, hidden, revealed], [Some(24), Some(24), Some(24)]);
}

#[test]
fn leaves_the_scrollbar_visible_and_clickable_when_the_jump_to_end_indicator_spans_the_transcript() {
    let h = Alt::with(
        30,
        6,
        TuiAltScreenOptions { scroll_to_end_indicator: Some(Arc::new(|| "↓".repeat(30))), ..Default::default() },
    );
    let transcript = scroll_view_with(
        text(&lines(12)),
        ScrollViewOptions {
            follow: Some(ScrollFollow::End),
            primary: Some(true),
            scrollbar: Some(pi_tui::components::scroll_view::ScrollViewScrollbar::Always),
            ..Default::default()
        },
    );
    h.tui.set_layout_root(Some(transcript_and_dock(transcript.clone())));
    h.tui.start();
    h.wait();

    h.send("\x1b[<64;1;1M");
    h.wait();
    assert!(!transcript.is_following_end());

    // The indicator must not intercept a press on the scrollbar's last column.
    assert_eq!(h.terminal.get_viewport()[3], format!("{}┃", "↓".repeat(29)));
    h.send("\x1b[<0;30;4M");
    h.send("\x1b[<0;30;4m");
    h.wait();
    assert!(!transcript.is_following_end());
}

#[test]
fn never_shows_the_jump_to_end_indicator_for_a_primary_scroll_view_without_follow_end() {
    let h = Alt::with(30, 3, indicator(" ↓ Jump to end "));
    let transcript = scroll_view(text("one\ntwo\nthree\nfour\nfive"), false, true);
    h.tui.set_layout_root(Some(transcript.clone()));
    h.tui.start();
    h.wait();
    assert!(!transcript.is_following_end());
    assert!(!h.viewport().iter().any(|l| l.contains("Jump to end")));
}

#[test]
fn invalidates_overlays_with_an_explicit_layout_root() {
    let h = Alt::new(80, 24);
    let overlay = Custom::new(&["overlay"]);
    h.tui.set_layout_root(Some(text("root")));
    h.tui.show_overlay(overlay.clone(), None);
    h.tui.invalidate();
    assert!(*overlay.invalidated.lock().unwrap());
}

#[test]
fn does_not_vertically_redispatch_misses_through_horizontal_layout_containers() {
    use pi_tui::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions, SelectListTheme};
    let h = Alt::new(20, 2);
    let selections = Arc::new(Mutex::new(0));
    let list = Arc::new(SelectList::new(
        vec![SelectItem::new("a", "A", None), SelectItem::new("b", "B", None)],
        2,
        SelectListTheme::plain(),
        SelectListLayoutOptions::default(),
    ));
    let s = selections.clone();
    list.set_on_select(Some(Arc::new(move |_| *s.lock().unwrap() += 1)));
    h.tui.set_layout_root(Some(Arc::new(HStack::new(
        vec![entry(list, StackBasis::Size(10), None, None, None), entry(text("plain"), StackBasis::Size(10), None, None, None)],
        StackOptions::default(),
    ))));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;15;1M");
    h.send("\x1b[<0;15;1m");
    h.wait();
    assert_eq!(*selections.lock().unwrap(), 0);
}

#[test]
fn reveals_an_auto_scrollbar_when_the_pointer_enters_its_hidden_track() {
    let h = Alt::new(10, 5);
    let sv = scroll_view_with(
        text(&lines(20)),
        ScrollViewOptions {
            primary: Some(true),
            scrollbar: Some(pi_tui::components::scroll_view::ScrollViewScrollbar::Auto),
            scrollbar_hide_delay_ms: Some(20),
            ..Default::default()
        },
    );
    h.tui.set_layout_root(Some(sv.clone()));
    h.tui.start();
    h.wait();
    assert!(!sv.is_scrollbar_visible());

    h.send("\x1b[<35;10;3M");
    h.wait();
    assert!(sv.is_scrollbar_visible());
    assert!(sv.is_scrollbar_active());
    assert!(h.terminal.get_viewport().iter().any(|l| l.contains('│') || l.contains('█')));

    h.send("\x1b[<35;9;3M");
    h.wait_ms(40);
    h.wait();
    assert!(!sv.is_scrollbar_visible());
}

#[test]
fn jumps_to_a_scrollbar_track_position_and_continues_dragging_from_there() {
    let h = Alt::new(10, 10);
    let sv = scroll_view_with(
        text(&lines(50)),
        ScrollViewOptions {
            primary: Some(true),
            scrollbar: Some(pi_tui::components::scroll_view::ScrollViewScrollbar::Always),
            ..Default::default()
        },
    );
    h.tui.set_layout_root(Some(sv.clone()));
    h.tui.start();
    h.wait();
    assert_eq!(sv.scroll_top(), 0);

    h.send("\x1b[<0;10;6M");
    h.wait();
    assert_eq!(sv.scroll_top(), 20);

    h.send("\x1b[<32;10;10M");
    h.wait();
    assert_eq!(sv.scroll_top(), 40);

    h.send("\x1b[<0;10;10m");
    h.wait();
    assert!(!h.writes().contains("\x1b]52;c;"));
}

#[test]
fn navigates_transcript_search_with_hoverable_arrow_buttons_and_toggles_it_with_its_shortcut() {
    let h = Alt::with(
        120,
        6,
        TuiAltScreenOptions {
            search_navigation_button_style: Some(Arc::new(|text: &str, hovered: bool| {
                format!("{}{text}\x1b[49m", if hovered { "\x1b[45m" } else { "\x1b[44m" })
            })),
            ..Default::default()
        },
    );
    h.tui.add_child(text("needle one\nmiddle\nneedle two\nend"));
    h.tui.start();
    h.wait();

    h.send("\x1b[102;6u");
    h.send("needle");
    h.wait();
    let viewport = h.terminal.get_viewport();
    assert!(viewport.iter().any(|l| l.contains("1/2")));
    assert!(viewport.iter().any(|l| l.contains("↑ Shift+Enter · ↓ Enter")));

    let char_col = |line: &str, byte: usize| line[..byte].chars().count();
    let arrow_row = viewport.iter().position(|l| l.contains('↑') && l.contains('↓')).unwrap();
    let arrow_col = char_col(&viewport[arrow_row], viewport[arrow_row].rfind("Enter").unwrap());
    let before = h.write_count();
    h.send(&format!("\x1b[<35;{};{}M", arrow_col + 1, arrow_row + 1));
    h.wait();
    assert!(h.writes_since(before).contains("\x1b[45m↓ Enter\x1b[49m"));
    h.send(&format!("\x1b[<0;{};{}M", arrow_col + 1, arrow_row + 1));
    h.wait();
    let viewport = h.terminal.get_viewport();
    assert!(viewport.iter().any(|l| l.contains("2/2")));
    assert!(viewport.iter().any(|l| l.contains("↑ Shift+Enter · ↓ Enter")));

    let arrow_row = viewport.iter().position(|l| l.contains('↑') && l.contains('↓')).unwrap();
    let arrow_col = char_col(&viewport[arrow_row], viewport[arrow_row].find("Shift+Enter").unwrap()) + 3;
    h.send(&format!("\x1b[<0;{};{}M", arrow_col + 1, arrow_row + 1));
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("1/2")));
    assert!(h.viewport().iter().any(|l| l.contains("↑ Shift+Enter · ↓ Enter")));

    h.send("\x1b[102;6u");
    h.wait();
    assert!(!h.viewport().iter().any(|l| l.contains("↑ Shift+Enter · ↓ Enter")));
}

#[test]
fn does_not_treat_transcript_box_drawing_as_search_navigation_buttons() {
    let h = Alt::new(80, 10);
    h.tui.add_child(text(
        &[
            "needle one",
            "middle",
            "needle two",
            "filler",
            "┌────────────────────────────────────────┐",
            "│ box                                    │",
            "└────────────────────────────────────────┘",
            "end",
        ]
        .join("\n"),
    ));
    h.tui.start();
    h.wait();

    h.send("\x1b[102;6u");
    h.send("needle");
    h.wait();
    let viewport = h.viewport();
    assert!(viewport.iter().any(|l| l.contains("1/2")));
    assert!(!viewport.iter().any(|l| l.contains("2/2")));

    let box_bottom = viewport.iter().position(|l| l.starts_with('└')).unwrap();
    h.send(&format!("\x1b[<0;24;{}M", box_bottom + 1));
    h.wait();
    let viewport = h.viewport();
    assert!(viewport.iter().any(|l| l.contains("1/2")));
    assert!(!viewport.iter().any(|l| l.contains("2/2")));
}

#[test]
fn uses_configured_styles_for_current_and_non_current_search_matches() {
    let h = Alt::with(
        60,
        4,
        TuiAltScreenOptions {
            search_match_style: Some(Arc::new(|t: &str| format!("\x1b[41m{t}\x1b[49m"))),
            search_current_match_style: Some(Arc::new(|t: &str| format!("\x1b[42m{t}\x1b[49m"))),
            ..Default::default()
        },
    );
    h.tui.add_child(text("needle first\nmiddle\nneedle second\nend"));
    h.tui.start();
    h.wait();
    h.send("\x1b[102;6u");
    h.send("needle");
    h.wait();
    assert!(h.writes().contains("\x1b[42mneedle\x1b[49m"));
    assert!(h.writes().contains("\x1b[41mneedle\x1b[49m"));
}

#[test]
fn opens_an_osc_8_hyperlink_with_specific_or_generic_release_codes_but_not_on_drag() {
    let opened = Arc::new(Mutex::new(Vec::<String>::new()));
    let o = opened.clone();
    let h = Alt::with(
        20,
        3,
        TuiAltScreenOptions {
            open_url: Some(Arc::new(move |url: &str| o.lock().unwrap().push(url.to_string()))),
            ..Default::default()
        },
    );
    let url = "https://example.com/path?q=1";
    let bel_url = "https://example.com/bel";
    let emoji_url = "https://example.com/emoji";
    h.tui.add_child(text(&format!(
        "{}\n\x1b]8;;{bel_url}\x07link\x1b]8;;\x07\n{}",
        pi_tui::terminal_image::hyperlink("link", url),
        pi_tui::terminal_image::hyperlink("🙂", emoji_url)
    )));
    h.tui.start();
    h.wait();

    h.send("\x1b[<0;2;1M");
    h.send("\x1b[<3;2;1m");
    h.wait();
    assert_eq!(*opened.lock().unwrap(), [url]);

    h.send("\x1b[<0;2;2M");
    h.send("\x1b[<0;2;2m");
    h.wait();
    assert_eq!(*opened.lock().unwrap(), [url, bel_url]);

    h.send("\x1b[<0;2;3M");
    h.send("\x1b[<0;2;3m");
    h.wait();
    assert_eq!(*opened.lock().unwrap(), [url, bel_url, emoji_url]);

    h.send("\x1b[<0;2;1M");
    h.send("\x1b[<32;4;1M");
    h.send("\x1b[<0;4;1m");
    h.wait();
    assert_eq!(*opened.lock().unwrap(), [url, bel_url, emoji_url]);
}

#[test]
fn copies_an_active_selection_programmatically() {
    let copied = Arc::new(Mutex::new(Vec::new()));
    let h = Alt::with(20, 4, copy_recorder(&copied, CopySelectionResult::Copied));
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();

    assert!(!h.tui.has_active_selection());
    let result = h.tui.copy_active_selection_to_clipboard();
    h.lp.run_pending();
    assert_eq!(result.try_take(), Some(false));

    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert_eq!(*copied.lock().unwrap(), ["alpha\nbeta"]);
    assert!(h.tui.has_active_selection());

    copied.lock().unwrap().clear();
    let result = h.tui.copy_active_selection_to_clipboard();
    h.wait();
    assert_eq!(result.try_take(), Some(true));
    assert_eq!(*copied.lock().unwrap(), ["alpha\nbeta"]);
    assert!(h.viewport().iter().any(|l| l.contains("Copied!")));
}

#[test]
fn flashes_an_error_when_the_injected_copy_selection_handler_fails() {
    let copied = Arc::new(Mutex::new(Vec::new()));
    let h = Alt::with(20, 4, copy_recorder(&copied, CopySelectionResult::Failed));
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert!(h.viewport().iter().any(|l| l.contains("Copy failed")));
    assert!(!h.writes().contains("\x1b]52;c;"), "must not emit OSC 52 when a copySelection handler is provided");
}

#[test]
fn flashes_a_specific_error_returned_by_the_injected_copy_selection_handler() {
    // #9618. The specific message is flashed for 5 seconds.
    let copied = Arc::new(Mutex::new(Vec::new()));
    let mut options =
        copy_recorder(&copied, CopySelectionResult::Error("Clipboard unavailable: install wl-clipboard".into()));
    options.copy_on_select = Some(false);
    let h = Alt::with(80, 4, options);
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    let result = h.tui.copy_active_selection_to_clipboard();
    h.wait();
    assert_eq!(result.try_take(), Some(false));
    assert!(h.viewport().iter().any(|l| l.contains("Clipboard unavailable: install wl-clipboard")));
    assert!(h.viewport().iter().all(|l| !l.contains("Copy failed")));
    h.wait_ms(4900);
    assert!(h.viewport().iter().any(|l| l.contains("Clipboard unavailable")), "still shown before 5000ms");
    h.wait_ms(200);
    assert!(h.viewport().iter().all(|l| !l.contains("Clipboard unavailable")), "gone after 5000ms");
}

#[test]
fn highlights_a_complete_whitespace_segment_during_a_word_drag() {
    let h = Alt::new(20, 1);
    h.tui.add_child(text("foo  bar"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<0;1;1m");
    h.send("\x1b[<0;2;1M");
    h.send("\x1b[<32;4;1M");
    h.wait();
    assert!(h.writes().contains("foo  \x1b[27m"));
}

#[test]
fn does_not_repaint_idle_or_zero_width_selections_on_focus_loss() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    let clipboard_writes = |h: &Alt| h.terminal.write_calls().iter().filter(|w| w.contains("\x1b]52;c;")).count();

    let idle = h.write_count();
    h.send("\x1b[O");
    h.send("\x1b[I");
    h.wait();
    assert_eq!(h.write_count(), idle);

    // A completed click leaves a zero-width anchor; orphaned drag/release events must not extend it.
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<0;1;1m");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert_eq!(clipboard_writes(&h), 0);

    // Losing focus after a press without a drag cancels the press without repainting.
    h.send("\x1b[<0;1;3M");
    h.wait();
    let pressed = h.write_count();
    h.send("\x1b[O");
    h.send("\x1b[I");
    h.wait();
    assert_eq!(h.write_count(), pressed);
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert_eq!(clipboard_writes(&h), 0);
    assert!(h.writes().contains("\x1b[?1004h"));

    h.tui.stop(TuiStopOptions::default());
    assert!(h.writes().contains("\x1b[?1004l"));
}

#[test]
fn clears_an_active_visible_selection_on_focus_loss_and_ignores_orphan_events() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.wait();
    let before = h.write_count();
    h.send("\x1b[O");
    h.send("\x1b[I");
    h.wait();
    let focus_loss = h.writes_since(before);
    assert!(focus_loss.contains("alpha"));
    assert!(focus_loss.contains("beta"));
    assert!(!focus_loss.contains("\x1b[7m"));

    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert!(!h.writes().contains("\x1b]52;c;"));
}

#[test]
fn retains_a_completed_visible_selection_across_focus_changes() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text("alpha\nbeta\ngamma\ndelta"));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    let completed = h.write_count();
    h.send("\x1b[O");
    h.send("\x1b[I");
    h.wait();
    assert_eq!(h.write_count(), completed);

    let before = h.write_count();
    h.tui.render_now(true);
    let redraw = h.writes_since(before);
    assert!(redraw.contains("alpha"));
    assert!(redraw.contains("beta"));
    assert!(redraw.contains("\x1b[7m"));
}

#[test]
fn ignores_horizontal_trackpad_wheel_events() {
    let h = Alt::new(20, 4);
    h.tui.add_child(text(&lines(8)));
    h.tui.start();
    h.wait();
    h.send("\x1b[<66;1;1M");
    h.send("\x1b[<67;1;1M");
    h.wait();
    assert_eq!(h.tui.viewport_top(), 4);
    assert_eq!(h.viewport(), ["line 5", "line 6", "line 7", "line 8"]);
}

#[test]
fn dispatches_clicks_to_nested_mouse_regions_without_breaking_drag_selection() {
    let h = Alt::new(20, 2);
    let clicks = Arc::new(Mutex::new(0));
    let c = clicks.clone();
    h.tui.add_child(Arc::new(MouseRegion::new(
        text("clickable\nselectable"),
        Arc::new(move |event| {
            if event.kind != MouseEventType::Click {
                return None;
            }
            *c.lock().unwrap() += 1;
            Some(MouseResult::handled())
        }),
    )));
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;2;1M");
    h.send("\x1b[<0;2;1m");
    h.wait();
    assert_eq!(*clicks.lock().unwrap(), 1);

    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;4;2M");
    h.send("\x1b[<0;4;2m");
    h.wait();
    assert_eq!(*clicks.lock().unwrap(), 1);
    assert!(h.writes().contains("\x1b]52;c;"));
}

#[test]
fn scrolls_the_transcript_on_wheel_input_a_focused_overlay_does_not_handle() {
    // #10769
    let h = Alt::new(20, 6);
    h.tui.add_child(text(&lines(12)));
    let overlay = Recorder::new("overlay");
    h.tui.start();
    h.wait();
    let top_before = h.tui.viewport_top();
    h.tui.show_overlay(
        overlay.clone(),
        Some(pi_tui::tui::OverlayOptions {
            anchor: Some(pi_tui::tui::OverlayAnchor::TopLeft),
            width: Some(pi_tui::tui::SizeValue::Abs(10)),
            ..Default::default()
        }),
    );
    h.wait();
    assert!(overlay.focused());
    h.send("\x1b[<64;3;1M");
    h.wait();
    assert!(h.tui.viewport_top() < top_before);
    assert!(overlay.inputs().is_empty());
}

#[test]
fn focuses_and_captures_drag_gestures_for_mouse_aware_components() {
    let h = Alt::new(20, 2);
    let events = Arc::new(Mutex::new(Vec::new()));
    let e = events.clone();
    let component = Arc::new(Custom::with_mouse(
        &["control"],
        Some(Box::new(move |event| {
            e.lock().unwrap().push(event.kind);
            Some(if event.kind == MouseEventType::Press {
                MouseResult { handled: true, capture: true, focus: true, ..Default::default() }
            } else {
                MouseResult::handled()
            })
        })),
    ));
    h.tui.add_child(component.clone());
    h.tui.start();
    h.wait();
    h.send("\x1b[<0;1;1M");
    h.send("\x1b[<32;5;2M");
    h.send("\x1b[<0;5;2m");
    h.wait();
    assert_eq!(*events.lock().unwrap(), [MouseEventType::Press, MouseEventType::Drag, MouseEventType::Release]);
    let focused = h.tui.get_focused_component().expect("focused");
    assert!(pi_tui::tui::same_component(&focused, &(component as ComponentRef)));
}

#[test]
fn reports_consecutive_click_counts_to_component_owned_controls() {
    let h = Alt::new(20, 1);
    let counts = Arc::new(Mutex::new(Vec::new()));
    let c = counts.clone();
    h.tui.add_child(Arc::new(Custom::with_mouse(
        &["control"],
        Some(Box::new(move |event| match event.kind {
            MouseEventType::Press => Some(MouseResult::handled()),
            MouseEventType::Click => {
                c.lock().unwrap().push(event.click_count.unwrap_or(0));
                Some(MouseResult::handled())
            }
            _ => None,
        })),
    )));
    h.tui.start();
    h.wait();
    for _ in 0..3 {
        h.send("\x1b[<0;1;1M");
        h.send("\x1b[<0;1;1m");
    }
    h.wait();
    assert_eq!(*counts.lock().unwrap(), [1, 2, 3]);
}

#[test]
fn does_not_rerender_for_handled_no_op_pointer_motion() {
    let h = Alt::new(20, 2);
    let component = Arc::new(Custom::with_mouse(
        &["hover target"],
        Some(Box::new(|event| (event.kind == MouseEventType::Move).then(MouseResult::handled))),
    ));
    h.tui.add_child(component.clone());
    h.tui.start();
    h.wait();
    let renders = component.renders();
    let writes = h.write_count();
    h.send("\x1b[<35;1;1M");
    h.wait();
    assert_eq!(component.renders(), renders);
    assert_eq!(h.write_count(), writes);
}

#[test]
fn lets_mouse_aware_components_consume_wheel_events_before_viewport_scrolling() {
    let h = Alt::new(20, 3);
    let wheel_events = Arc::new(Mutex::new(0));
    let w = wheel_events.clone();
    h.tui.add_child(Arc::new(MouseRegion::new(
        text(&lines(8)),
        Arc::new(move |event| {
            if event.kind != MouseEventType::Wheel {
                return None;
            }
            *w.lock().unwrap() += 1;
            Some(MouseResult::handled())
        }),
    )));
    h.tui.start();
    h.wait();
    let top = h.tui.viewport_top();
    h.send("\x1b[<64;1;1M");
    h.wait();
    assert_eq!(*wheel_events.lock().unwrap(), 1);
    assert_eq!(h.tui.viewport_top(), top);
}

#[test]
fn keeps_viewport_scrolling_when_an_overlay_is_not_focused() {
    let h = Alt::new(20, 6);
    let editor = Recorder::new("editor");
    h.tui.add_child(text(&lines(12)));
    h.tui.set_focus(Some(editor.clone()));
    h.tui.start();
    h.wait();
    let top_before = h.tui.viewport_top();

    let hidden = h.tui.show_overlay(Recorder::new("overlay"), None);
    hidden.set_hidden(true);
    let non_capturing = Recorder::new("overlay");
    h.tui.show_overlay(non_capturing.clone(), Some(pi_tui::tui::OverlayOptions { non_capturing: true, ..Default::default() }));
    let unfocused = Recorder::new("overlay");
    let unfocused_handle = h.tui.show_overlay(unfocused.clone(), None);
    unfocused_handle.unfocus(None);
    h.wait();
    assert!(!non_capturing.focused());
    assert!(!unfocused.focused());

    h.send("\x1b[5~");
    h.send("\x1b[<64;10;3M");
    h.wait();
    assert!(h.tui.viewport_top() < top_before);
    assert!(non_capturing.inputs().is_empty());
    assert!(unfocused.inputs().is_empty());
}
