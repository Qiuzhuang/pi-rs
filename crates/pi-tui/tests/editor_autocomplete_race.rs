//! Autocomplete must never apply a stale suggestion list.
//!
//! TS resolves synchronous suggestions (slash commands) in microtasks, so the list always matches
//! the text before the next key is handled. Spawning the suggestion future made the list lag
//! behind fast typing: every newer result was discarded as stale (the text had moved on), the list
//! kept the items and prefix of an older request, and accepting it spliced the old completion over
//! the wrong prefix length ("/thinking" + Tab became "/thinkitree"). Escape right after "/" also
//! reached the app (interrupting the agent) instead of closing the list.

mod common;

use std::sync::Arc;

use common::{Harness, chalk};
use pi_tui::autocomplete::{CombinedAutocompleteProvider, CommandEntry, SlashCommand};
use pi_tui::components::editor::{Editor, EditorOptions};
use pi_tui::tui::Component;

fn command(name: &str) -> CommandEntry {
    CommandEntry::Command(SlashCommand {
        name: name.to_string(),
        description: None,
        argument_hint: None,
        get_argument_completions: None,
    })
}

fn editor() -> (Harness, Arc<Editor>) {
    let h = Harness::new(80, 24);
    let editor = Editor::new(&h.tui, chalk::editor_theme(), EditorOptions::default());
    editor.set_autocomplete_provider(Arc::new(CombinedAutocompleteProvider::new(
        vec![command("tree"), command("thinking"), command("settings")],
        std::env::temp_dir().to_string_lossy().to_string(),
        None,
    )));
    (h, editor)
}

#[test]
fn slash_command_list_follows_fast_typing_without_running_the_loop() {
    let (_h, editor) = editor();
    // No event-loop turn between keys: the list must still track the text.
    for c in "/t".chars() {
        editor.handle_input(&c.to_string());
    }
    assert!(editor.is_showing_autocomplete(), "the list shows right after typing '/t'");
    for c in "hinking".chars() {
        editor.handle_input(&c.to_string());
    }
    assert!(editor.is_showing_autocomplete());
    editor.handle_input("\t");
    assert_eq!(editor.get_text(), "/thinking ");
}

#[test]
fn escape_right_after_slash_closes_the_list() {
    let (_h, editor) = editor();
    editor.handle_input("/");
    assert!(editor.is_showing_autocomplete(), "the list shows before the next key is handled");
    editor.handle_input("\x1b");
    assert!(!editor.is_showing_autocomplete());
    assert_eq!(editor.get_text(), "/");
}

#[test]
fn stale_prefix_never_slices_inside_a_multibyte_character() {
    // A completion computed for an older, longer prefix applied to text ending in multi-byte
    // characters must not panic.
    let provider = CombinedAutocompleteProvider::new(vec![command("tree")], ".", None);
    use pi_tui::autocomplete::{AutocompleteItem, AutocompleteProvider};
    let lines = vec!["/xü".to_string()];
    // "/xü".len() - "/".len() lands inside 'ü'.
    let result = provider.apply_completion(&lines, 0, "/xü".len(), &AutocompleteItem::new("tree", "tree"), "/");
    assert!(result.lines[0].contains("tree"));
}

/// A provider that reads the editor back from `getSuggestions` / `shouldTriggerFileCompletion`,
/// like an extension provider or the app's slash-command argument completions (which read session
/// state that an agent thread may hold while it updates the editor). Providers must be called
/// without the editor's state lock.
struct ReadBackProvider {
    editor: std::sync::Mutex<std::sync::Weak<Editor>>,
    inner: CombinedAutocompleteProvider,
}

impl pi_tui::autocomplete::AutocompleteProvider for ReadBackProvider {
    fn get_suggestions(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        options: pi_tui::autocomplete::SuggestOptions,
    ) -> pi_tui::event_loop::BoxFuture<Option<pi_tui::autocomplete::AutocompleteSuggestions>> {
        if let Some(editor) = self.editor.lock().unwrap().upgrade() {
            let _ = editor.get_text();
        }
        self.inner.get_suggestions(lines, cursor_line, cursor_col, options)
    }
    fn apply_completion(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        item: &pi_tui::autocomplete::AutocompleteItem,
        prefix: &str,
    ) -> pi_tui::autocomplete::CompletionResult {
        self.inner.apply_completion(lines, cursor_line, cursor_col, item, prefix)
    }
    fn should_trigger_file_completion(&self, _lines: &[String], _cursor_line: usize, _cursor_col: usize) -> bool {
        if let Some(editor) = self.editor.lock().unwrap().upgrade() {
            let _ = editor.get_text();
        }
        true
    }
    fn has_should_trigger_file_completion(&self) -> bool {
        true
    }
}

#[test]
fn providers_are_called_without_the_editor_lock() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (_h, editor) = editor();
        let provider = Arc::new(ReadBackProvider {
            editor: std::sync::Mutex::new(Arc::downgrade(&editor)),
            inner: CombinedAutocompleteProvider::new(
                vec![command("tree"), command("thinking")],
                std::env::temp_dir().to_string_lossy().to_string(),
                None,
            ),
        });
        editor.set_autocomplete_provider(provider);
        for c in "/th".chars() {
            editor.handle_input(&c.to_string());
        }
        // Tab: forced request (shouldTriggerFileCompletion is consulted) and a single match.
        editor.handle_input("\t");
        let completed = editor.get_text();
        // Tab outside a slash command: a forced request asks `shouldTriggerFileCompletion`.
        editor.set_text("hello");
        editor.handle_input("\t");
        let _ = tx.send(completed);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(text) => assert_eq!(text, "/thinking "),
        Err(_) => {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "deadlock: a provider was called under the editor lock");
            std::process::exit(101);
        }
    }
}
