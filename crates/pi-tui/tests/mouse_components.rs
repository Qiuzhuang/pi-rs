//! Mouse-aware component tests (ported from mouse-components.test.ts).

use std::sync::{Arc, Mutex, Weak};

use pi_tui::components::editor::{Editor, EditorOptions, EditorTheme};
use pi_tui::components::input::Input;
use pi_tui::components::select_list::{SelectItem, SelectList, SelectListLayoutOptions, SelectListTheme};
use pi_tui::components::settings_list::{SettingItem, SettingsList, SettingsListOptions, SettingsListTheme, SubmenuDone};
use pi_tui::event_loop::{CurrentGuard, EventLoop};
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{
    Component, ComponentRef, Container, MouseButton, MouseEvent, MouseEventType, MouseResult, OverlayAnchor, OverlayOptions,
    SizeValue, Tui, TuiOptions, TuiStopOptions, same_component,
};
use pi_tui::tui_alt_screen::{CopySelectionResult, TuiAltScreenOptions};

fn mouse(kind: MouseEventType, x: i64, y: i64, width: i64, height: i64) -> MouseEvent {
    MouseEvent {
        kind,
        button: MouseButton::Left,
        x,
        y,
        screen_x: x,
        screen_y: y,
        width,
        height,
        shift: false,
        alt: false,
        ctrl: false,
        wheel_delta: None,
        click_count: if kind == MouseEventType::Click { Some(1) } else { None },
    }
}

fn select_theme() -> SelectListTheme {
    SelectListTheme::plain()
}

fn settings_theme() -> SettingsListTheme {
    SettingsListTheme {
        label: Arc::new(|t: &str, _| t.to_string()),
        value: Arc::new(|t: &str, _| t.to_string()),
        description: Arc::new(|t: &str| t.to_string()),
        cursor: "> ".into(),
        hint: Arc::new(|t: &str| t.to_string()),
    }
}

fn items(labels: &[&str]) -> Vec<SelectItem> {
    labels.iter().map(|l| SelectItem::new(l.to_lowercase(), *l, None)).collect()
}

fn setting(id: &str, label: &str, current: &str, values: &[&str]) -> SettingItem {
    SettingItem {
        id: id.into(),
        label: label.into(),
        current_value: current.into(),
        values: Some(values.iter().map(|v| v.to_string()).collect()),
        ..Default::default()
    }
}

struct Alt {
    lp: EventLoop,
    _guard: CurrentGuard,
    terminal: VirtualTerminal,
    tui: Arc<Tui>,
}

impl Alt {
    fn new(cols: usize, rows: usize, options: TuiAltScreenOptions) -> Self {
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
}

impl Drop for Alt {
    fn drop(&mut self) {
        self.tui.stop(TuiStopOptions::default());
    }
}

#[test]
fn positions_single_line_input_cursor_on_press() {
    let input = Input::default();
    input.set_value("hello");
    input.render(20);
    assert!(input.handle_mouse(&mouse(MouseEventType::Press, 4, 0, 20, 1)).is_some_and(|r| r.handled));
    input.handle_input("X");
    assert_eq!(input.get_value(), "heXllo");
}

#[test]
fn selects_and_activates_list_rows() {
    let list = SelectList::new(items(&["A", "B", "C", "D", "E"]), 3, select_theme(), SelectListLayoutOptions::default());
    let selected = Arc::new(Mutex::new(None));
    let s = selected.clone();
    list.set_on_select(Some(Arc::new(move |item: SelectItem| *s.lock().unwrap() = Some(item.value))));
    assert!(list.handle_mouse(&mouse(MouseEventType::Press, 1, 2, 40, 3)).is_some_and(|r| r.handled));
    assert_eq!(list.get_selected_item().unwrap().value, "c");
    assert!(list.handle_mouse(&mouse(MouseEventType::Click, 1, 2, 40, 3)).is_some_and(|r| r.handled));
    assert_eq!(selected.lock().unwrap().as_deref(), Some("c"));
}

#[test]
fn activates_settings_rows() {
    let changes = Arc::new(Mutex::new(Vec::new()));
    let c = changes.clone();
    let list = SettingsList::new(
        vec![
            setting("mode", "Mode", "one", &["one", "two"]),
            setting("other", "Other", "off", &["off", "on"]),
            setting("third", "Third", "low", &["low", "high"]),
            setting("fourth", "Fourth", "x", &["x", "y"]),
        ],
        3,
        settings_theme(),
        Arc::new(move |id, value| c.lock().unwrap().push((id, value))),
        Arc::new(|| {}),
        SettingsListOptions::default(),
    );
    list.handle_mouse(&mouse(MouseEventType::Press, 1, 2, 40, 5));
    list.handle_mouse(&mouse(MouseEventType::Click, 1, 2, 40, 5));
    assert_eq!(*changes.lock().unwrap(), vec![("third".to_string(), "high".to_string())]);
}

#[test]
fn ignores_hover_and_clicks_visible_rows_after_scrolling() {
    for row in [0i64, 4] {
        // Select list.
        let list = SelectList::new(
            (0..12).map(|i| SelectItem::new(format!("item-{i}"), format!("Item {i}"), None)).collect(),
            5,
            select_theme(),
            SelectListLayoutOptions::default(),
        );
        let changes = Arc::new(Mutex::new(Vec::<String>::new()));
        let selected = Arc::new(Mutex::new(None::<String>));
        let (c, s) = (changes.clone(), selected.clone());
        list.set_on_selection_change(Some(Arc::new(move |item: SelectItem| c.lock().unwrap().push(item.value))));
        list.set_on_select(Some(Arc::new(move |item: SelectItem| *s.lock().unwrap() = Some(item.value))));
        list.set_selected_index(5);
        list.handle_mouse(&MouseEvent { wheel_delta: Some(1), ..mouse(MouseEventType::Wheel, 1, row, 80, 10) });
        assert_eq!(list.get_selected_item().unwrap().value, "item-6");
        assert_eq!(*changes.lock().unwrap(), vec!["item-6"]);
        let before = list.render(80);
        assert!(before[row as usize].ends_with(&format!("Item {}", 4 + row)), "{before:?}");
        for y in [0, 1, 2, 3, 4, row] {
            let r = list.handle_mouse(&MouseEvent { button: MouseButton::None, ..mouse(MouseEventType::Move, 1, y, 80, 10) });
            assert!(r.is_none());
            assert_eq!(list.render(80), before);
        }
        assert!(selected.lock().unwrap().is_none());
        list.handle_mouse(&mouse(MouseEventType::Press, 1, row, 80, 10));
        list.render(80);
        list.handle_mouse(&mouse(MouseEventType::Click, 1, row, 80, 10));
        assert_eq!(selected.lock().unwrap().clone(), Some(format!("item-{}", 4 + row)));
        assert_eq!(*changes.lock().unwrap(), vec!["item-6".to_string(), format!("item-{}", 4 + row)]);

        // Settings list.
        let changes = Arc::new(Mutex::new(Vec::new()));
        let c = changes.clone();
        let list = SettingsList::new(
            (0..12)
                .map(|i| SettingItem {
                    description: Some(format!("Description {i}")),
                    ..setting(&format!("item-{i}"), &format!("Item {i}"), "off", &["off", "on"])
                })
                .collect(),
            5,
            settings_theme(),
            Arc::new(move |id, value| c.lock().unwrap().push((id, value))),
            Arc::new(|| {}),
            SettingsListOptions { enable_search: true },
        );
        list.select_item("item-5");
        list.handle_mouse(&MouseEvent { wheel_delta: Some(1), ..mouse(MouseEventType::Wheel, 1, row + 2, 80, 10) });
        let before = list.render(80);
        assert!(before[4].starts_with("> Item 6"), "{before:?}");
        assert!(before[(row + 2) as usize].contains(&format!("Item {} ", 4 + row)));
        for y in [0, 1, 2, 3, 4, row] {
            let r = list.handle_mouse(&MouseEvent { button: MouseButton::None, ..mouse(MouseEventType::Move, 1, y + 2, 80, 10) });
            assert!(r.is_none());
            assert_eq!(list.render(80), before);
        }
        assert!(changes.lock().unwrap().is_empty());
        list.handle_mouse(&mouse(MouseEventType::Press, 1, row + 2, 80, 10));
        list.render(80);
        list.handle_mouse(&mouse(MouseEventType::Click, 1, row + 2, 80, 10));
        assert_eq!(*changes.lock().unwrap(), vec![(format!("item-{}", 4 + row), "on".to_string())]);
    }
}

/// A container that delegates keyboard input to a nested input.
struct InputOverlay {
    me: Weak<InputOverlay>,
    container: Container,
    input: Arc<Input>,
}

impl InputOverlay {
    fn new() -> Arc<Self> {
        Arc::new_cyclic(|me| {
            let input = Arc::new(Input::default());
            let container = Container::new();
            container.add_child(input.clone());
            InputOverlay { me: me.clone(), container, input }
        })
    }
}

impl Component for InputOverlay {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render_children(width)
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        self.input.handle_input(data);
    }
    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        let owner: Option<ComponentRef> = self.me.upgrade().map(|a| a as ComponentRef);
        self.container.handle_mouse_with_owner(event, owner)
    }
    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        Some(self.container.children())
    }
}

#[test]
fn keeps_delegating_overlay_focused_when_nested_input_clicked() {
    let h = Alt::new(20, 4, TuiAltScreenOptions::default());
    let overlay = InputOverlay::new();
    overlay.input.set_value("hi");
    h.tui.start();
    h.tui.show_overlay(
        overlay.clone(),
        Some(OverlayOptions { anchor: Some(OverlayAnchor::TopLeft), width: Some(SizeValue::Abs(20)), ..Default::default() }),
    );
    h.wait();
    h.terminal.send_input("\x1b[<0;5;1M");
    h.terminal.send_input("\x1b[<0;5;1m");
    h.terminal.send_input("!");
    h.wait();
    assert_eq!(overlay.input.get_value(), "hi!");
    let focused = h.tui.get_focused_component().unwrap();
    assert!(same_component(&focused, &(overlay.clone() as ComponentRef)));
}

/// A settings submenu that routes keys to its nested list.
struct SubmenuHost {
    container: Container,
    list: Arc<SelectList>,
}

impl Component for SubmenuHost {
    fn render(&self, width: usize) -> Vec<String> {
        self.container.render_children(width)
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        self.list.handle_input(data);
    }
    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        self.container.handle_mouse(event)
    }
    fn child_components(&self) -> Option<Vec<ComponentRef>> {
        Some(self.container.children())
    }
}

#[test]
fn keeps_settings_list_focused_when_submenu_click_closes_submenu() {
    let h = Alt::new(30, 6, TuiAltScreenOptions::default());
    let changes = Arc::new(Mutex::new(Vec::new()));
    let c = changes.clone();
    let list = SettingsList::new(
        vec![
            SettingItem {
                id: "theme".into(),
                label: "Theme".into(),
                current_value: "first".into(),
                submenu: Some(Arc::new(|_value: String, done: SubmenuDone| {
                    let list = Arc::new(SelectList::new(
                        vec![SelectItem::new("first", "First", None), SelectItem::new("second", "Second", None)],
                        5,
                        SelectListTheme::plain(),
                        SelectListLayoutOptions::default(),
                    ));
                    let d = done.clone();
                    list.set_on_select(Some(Arc::new(move |item: SelectItem| d(Some(item.value), None))));
                    let d = done.clone();
                    list.set_on_cancel(Some(Arc::new(move || d(None, None))));
                    let container = Container::new();
                    container.add_child(list.clone());
                    Arc::new(SubmenuHost { container, list }) as ComponentRef
                })),
                ..Default::default()
            },
            setting("other", "Other", "off", &["off", "on"]),
        ],
        5,
        settings_theme(),
        Arc::new(move |id, value| c.lock().unwrap().push((id, value))),
        Arc::new(|| {}),
        SettingsListOptions::default(),
    );
    h.tui.add_child(list.clone());
    h.tui.set_focus(Some(list.clone()));
    h.tui.start();
    h.wait();
    h.terminal.send_input("\r");
    h.wait();
    h.terminal.send_input("\x1b[<0;3;2M");
    h.terminal.send_input("\x1b[<0;3;2m");
    h.wait();
    assert_eq!(*changes.lock().unwrap(), vec![("theme".to_string(), "second".to_string())]);
    assert!(same_component(&h.tui.get_focused_component().unwrap(), &(list.clone() as ComponentRef)));
    h.terminal.send_input("\x1b[B");
    h.terminal.send_input("\r");
    h.wait();
    assert_eq!(
        *changes.lock().unwrap(),
        vec![("theme".to_string(), "second".to_string()), ("other".to_string(), "on".to_string())]
    );
}

fn editor_theme() -> EditorTheme {
    EditorTheme { border_color: Arc::new(|t: &str| t.to_string()), select_list: SelectListTheme::plain() }
}

#[test]
fn positions_and_focuses_multiline_editor_through_alt_screen_dispatch() {
    let h = Alt::new(20, 6, TuiAltScreenOptions::default());
    let editor = Editor::new(&h.tui, editor_theme(), EditorOptions::default());
    editor.set_text("hello");
    h.tui.add_child(editor.clone());
    h.tui.start();
    h.wait();
    h.terminal.send_input("\x1b[<0;3;2M");
    h.terminal.send_input("\x1b[<0;3;2m");
    h.terminal.send_input("X");
    h.wait();
    assert_eq!(editor.get_text(), "heXllo");
    assert!(same_component(&h.tui.get_focused_component().unwrap(), &(editor.clone() as ComponentRef)));
}

#[test]
fn selects_and_copies_editor_text_on_drag() {
    let copied = Arc::new(Mutex::new(Vec::new()));
    let c = copied.clone();
    let h = Alt::new(
        20,
        6,
        TuiAltScreenOptions {
            copy_selection: Some(Arc::new(move |text: &str| {
                c.lock().unwrap().push(text.to_string());
                pi_tui::event_loop::ready(CopySelectionResult::Copied)
            })),
            ..Default::default()
        },
    );
    let editor = Editor::new(&h.tui, editor_theme(), EditorOptions::default());
    editor.set_text("hello world");
    h.tui.add_child(editor.clone());
    h.tui.start();
    h.wait();
    let before = editor.get_cursor();
    h.terminal.send_input("\x1b[<0;1;2M");
    h.terminal.send_input("\x1b[<32;5;2M");
    h.terminal.send_input("\x1b[<0;5;2m");
    h.wait();
    assert_eq!(*copied.lock().unwrap(), vec!["hello"]);
    assert_eq!(editor.get_cursor(), before);
}
