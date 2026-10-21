//! Prompt history keybindings (ported from editor-history-keybindings.test.ts).
//! Lives in its own test binary because it changes the global keybindings.

mod common;

use std::sync::Arc;

use common::{Harness, chalk};
use pi_tui::components::editor::{Editor, EditorOptions};
use pi_tui::keybindings::{KeySpec, KeybindingsManager, TUI_KEYBINDINGS, set_keybindings};
use pi_tui::tui::Component;

#[test]
fn browses_history_directly_without_first_moving_the_cursor() {
    set_keybindings(Arc::new(KeybindingsManager::new(
        TUI_KEYBINDINGS.clone(),
        vec![
            ("tui.editor.historyPrevious".to_string(), Some(KeySpec::One("ctrl+p".to_string()))),
            ("tui.editor.historyNext".to_string(), Some(KeySpec::One("ctrl+n".to_string()))),
        ],
    )));
    let h = Harness::new(80, 24);
    let editor = Editor::new(&h.tui, chalk::editor_theme(), EditorOptions::default());
    editor.add_to_history("older prompt");
    editor.add_to_history("newer\nmultiline prompt");
    editor.set_text("draft");
    editor.handle_input("\x1b[D");
    editor.handle_input("\x1b[D");

    editor.handle_input("\x10");
    assert_eq!(editor.get_text(), "newer\nmultiline prompt");
    assert_eq!(editor.get_cursor(), (0, 0));
    editor.handle_input("\x10");
    assert_eq!(editor.get_text(), "older prompt");
    editor.handle_input("\x0e");
    assert_eq!(editor.get_text(), "newer\nmultiline prompt");
    assert_eq!(editor.get_cursor(), (1, 16));
    editor.handle_input("\x0e");
    assert_eq!(editor.get_text(), "draft");
    assert_eq!(editor.get_cursor(), (0, 3));

    set_keybindings(Arc::new(KeybindingsManager::new(TUI_KEYBINDINGS.clone(), Vec::new())));
}
