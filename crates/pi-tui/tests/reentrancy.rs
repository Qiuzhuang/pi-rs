//! Concurrency regressions: user code (component methods, overlay `visible` callbacks, input
//! listeners) re-entering the TUI must never deadlock. The TUI may not hold its internal locks
//! while calling into components or callbacks.

mod common;

use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use common::{Harness, TestComponent, chalk};
use pi_tui::autocomplete::{AutocompleteItem, AutocompleteProvider, AutocompleteSuggestions, CompletionResult, SuggestOptions};
use pi_tui::components::editor::{Editor, EditorOptions};
use pi_tui::event_loop::{self, BoxFuture};
use pi_tui::tui::{Component, ComponentRef, InputListenerResult, OverlayOptions, Tui};

/// Run `f` on its own thread and fail (instead of hanging) when it does not finish.
fn run_with_timeout(name: &str, f: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => handle.join().expect("test thread panicked"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            handle.join().expect("test thread panicked");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // The deadlocked thread would also block process exit (exit hooks take TUI locks).
            eprintln!("{name}: deadlock (did not finish within 10s)");
            std::process::exit(101);
        }
    }
}

/// A focusable component whose render and input handling call back into the TUI.
struct ReentrantComponent {
    tui: Mutex<Weak<Tui>>,
    renders: Mutex<usize>,
    inputs: Mutex<Vec<String>>,
    focused: Mutex<bool>,
}

impl ReentrantComponent {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            tui: Mutex::new(Weak::new()),
            renders: Mutex::new(0),
            inputs: Mutex::new(Vec::new()),
            focused: Mutex::new(false),
        })
    }

    fn tui(&self) -> Option<Arc<Tui>> {
        self.tui.lock().unwrap().upgrade()
    }
}

impl Component for ReentrantComponent {
    fn render(&self, _width: usize) -> Vec<String> {
        let renders = {
            let mut renders = self.renders.lock().unwrap();
            *renders += 1;
            *renders
        };
        if let Some(tui) = self.tui() {
            let _ = tui.get_focused_component();
            let _ = tui.has_overlay();
            let _ = tui.children();
            // A forced render resets the render state, which the renderer must not hold.
            tui.request_render(renders == 1);
        }
        vec![format!("reentrant {}", self.inputs.lock().unwrap().len())]
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_input(&self, data: &str) {
        self.inputs.lock().unwrap().push(data.to_string());
        if let Some(tui) = self.tui() {
            // Focus changes and overlay toggles from inside an input handler.
            let me = tui.get_focused_component();
            let overlay = tui.show_overlay(Arc::new(TestComponent::default()) as ComponentRef, None);
            overlay.hide();
            tui.set_focus(me);
            tui.request_render(true);
        }
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn set_focused(&self, focused: bool) {
        *self.focused.lock().unwrap() = focused;
        if let Some(tui) = self.tui() {
            let _ = tui.get_focused_component();
        }
    }

    fn focused(&self) -> bool {
        *self.focused.lock().unwrap()
    }
}

#[test]
fn overlay_visible_callback_can_reenter_the_tui() {
    run_with_timeout("overlay visible callback", || {
        let h = Harness::new(40, 10);
        let base = ReentrantComponent::new();
        *base.tui.lock().unwrap() = Arc::downgrade(&h.tui);
        let base_ref = h.add(base.clone());
        h.tui.set_focus(Some(base_ref.clone()));
        h.tui.start();

        let calls = Arc::new(Mutex::new(0usize));
        let weak_tui = Arc::downgrade(&h.tui);
        let counter = calls.clone();
        let visible = Arc::new(move |_cols: usize, _rows: usize| {
            *counter.lock().unwrap() += 1;
            // Re-enter the TUI from the callback (it runs during focus changes).
            if let Some(tui) = weak_tui.upgrade() {
                let _ = tui.get_focused_component();
                let _ = tui.children();
            }
            true
        });
        let overlay_component = TestComponent::new(&["overlay"]);
        let handle = h.tui.show_overlay(
            overlay_component.clone() as ComponentRef,
            Some(OverlayOptions { visible: Some(visible), ..Default::default() }),
        );
        h.wait_for_render();
        handle.unfocus(None);
        handle.focus();
        h.tui.set_focus(Some(base_ref.clone()));
        h.tui.set_focus(None);
        handle.set_hidden(true);
        handle.set_hidden(false);
        h.terminal.send_input("x");
        h.wait_for_render();
        handle.hide();
        h.wait_for_render();
        assert!(*calls.lock().unwrap() > 0, "visible callback was consulted");
        h.tui.stop(Default::default());
    });
}

#[test]
fn component_render_and_input_can_reenter_the_tui() {
    run_with_timeout("re-entrant component", || {
        let h = Harness::new(40, 10);
        let c = ReentrantComponent::new();
        *c.tui.lock().unwrap() = Arc::downgrade(&h.tui);
        let c_ref = h.add(c.clone());
        h.tui.set_focus(Some(c_ref));
        h.tui.start();
        h.wait_for_render();
        h.terminal.send_input("a");
        h.wait_for_render();
        h.terminal.send_input("b");
        h.wait_for_render();
        assert_eq!(*c.inputs.lock().unwrap(), vec!["a".to_string(), "b".to_string()]);
        assert!(*c.renders.lock().unwrap() > 0);
        assert!(h.terminal.get_viewport().iter().any(|l| l.contains("reentrant 2")));
        h.tui.stop(Default::default());
    });
}

#[test]
fn input_listener_can_unsubscribe_itself_and_subscribe_another_during_dispatch() {
    run_with_timeout("input listener unsubscribe", || {
        let h = Harness::new(40, 10);
        let base = TestComponent::new(&["base"]);
        h.add(base);
        h.tui.start();

        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let own_id: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
        let weak_tui = Arc::downgrade(&h.tui);
        let (seen2, own_id2) = (seen.clone(), own_id.clone());
        let id = h.tui.add_input_listener(Arc::new(move |data: &str| {
            seen2.lock().unwrap().push(format!("first:{data}"));
            if let Some(tui) = weak_tui.upgrade() {
                // Unsubscribe while being dispatched, and subscribe a replacement.
                if let Some(id) = own_id2.lock().unwrap().take() {
                    tui.remove_input_listener(id);
                }
                let seen3 = seen2.clone();
                tui.add_input_listener(Arc::new(move |data: &str| {
                    seen3.lock().unwrap().push(format!("second:{data}"));
                    None::<InputListenerResult>
                }));
            }
            None
        }));
        *own_id.lock().unwrap() = Some(id);
        h.terminal.send_input("a");
        h.wait_for_render();
        h.terminal.send_input("b");
        h.wait_for_render();
        assert_eq!(*seen.lock().unwrap(), vec!["first:a".to_string(), "second:b".to_string()]);
        h.tui.stop(Default::default());
    });
}

/// An autocomplete provider that reads the editor back while being consulted, like a JS provider
/// whose `applyCompletion` calls `ctx.ui.getEditorText()`.
struct ReentrantProvider {
    editor: Mutex<Weak<Editor>>,
    reads: Mutex<usize>,
}

impl ReentrantProvider {
    fn read_editor(&self) {
        if let Some(editor) = self.editor.lock().unwrap().upgrade() {
            let _ = editor.get_text();
            *self.reads.lock().unwrap() += 1;
        }
    }
}

impl AutocompleteProvider for ReentrantProvider {
    fn trigger_characters(&self) -> Vec<String> {
        self.read_editor();
        Vec::new()
    }

    fn get_suggestions(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        options: SuggestOptions,
    ) -> BoxFuture<Option<AutocompleteSuggestions>> {
        let before = lines[cursor_line][..cursor_col].to_string();
        let items: Vec<AutocompleteItem> = match before.as_str() {
            "one" if options.force => vec![AutocompleteItem::new("one-only", "one-only")],
            "two" if options.force => {
                vec![AutocompleteItem::new("two-a", "two-a"), AutocompleteItem::new("two-b", "two-b")]
            }
            _ => Vec::new(),
        };
        event_loop::ready((!items.is_empty()).then(|| AutocompleteSuggestions { prefix: before, items }))
    }

    fn apply_completion(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> CompletionResult {
        self.read_editor();
        let line = &lines[cursor_line];
        let start = cursor_col - prefix.len();
        let mut new_lines = lines.to_vec();
        new_lines[cursor_line] = format!("{}{}{}", &line[..start], item.value, &line[cursor_col..]);
        CompletionResult { lines: new_lines, cursor_line, cursor_col: start + item.value.len() }
    }
}

#[test]
fn autocomplete_provider_can_read_the_editor_while_applying_a_completion() {
    run_with_timeout("re-entrant autocomplete provider", || {
        let h = Harness::new(80, 24);
        let editor = Editor::new(&h.tui, chalk::editor_theme(), EditorOptions::default());
        let provider = Arc::new(ReentrantProvider { editor: Mutex::new(Arc::downgrade(&editor)), reads: Mutex::new(0) });
        editor.set_autocomplete_provider(provider.clone());
        let flush = || {
            for _ in 0..3 {
                h.lp.run_pending();
            }
        };

        // A single forced suggestion is applied directly (async completion path).
        editor.set_text("one");
        editor.handle_input("\t");
        flush();
        assert_eq!(editor.get_text(), "one-only");

        // Several suggestions: the menu opens and Tab accepts the selection (input path).
        editor.set_text("two");
        editor.handle_input("\t");
        flush();
        editor.handle_input("\t");
        assert_eq!(editor.get_text(), "two-a");
        assert!(*provider.reads.lock().unwrap() >= 2, "the provider read the editor back");
    });
}
