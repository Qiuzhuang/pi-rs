//! Editor autocomplete behaviour (ported from editor.test.ts "Autocomplete").

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Harness, chalk};
use pi_tui::autocomplete::{
    AutocompleteItem, AutocompleteProvider, AutocompleteSuggestions, CombinedAutocompleteProvider, CommandEntry,
    CompletionResult, SlashCommand, SuggestOptions,
};
use pi_tui::components::editor::{Editor, EditorOptions};
use pi_tui::event_loop::{self, BoxFuture};
use pi_tui::tui::Component;
use pi_tui::utils::strip_terminal_sequences;

type GetFn = dyn Fn(&[String], usize, usize, SuggestOptions) -> BoxFuture<Option<AutocompleteSuggestions>> + Send + Sync;

/// Test provider built from a closure, with the standard prefix-replacing `apply_completion`.
struct FnProvider {
    triggers: Vec<String>,
    get: Box<GetFn>,
    apply_with: Option<Arc<dyn AutocompleteProvider>>,
}

impl FnProvider {
    fn new(
        get: impl Fn(&[String], usize, usize, SuggestOptions) -> BoxFuture<Option<AutocompleteSuggestions>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self { triggers: Vec::new(), get: Box::new(get), apply_with: None }
    }

    fn triggers(mut self, t: &[&str]) -> Self {
        self.triggers = t.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Synchronous convenience: suggestions computed from the text before the cursor.
    fn sync(f: impl Fn(&str, &SuggestOptions) -> Option<AutocompleteSuggestions> + Send + Sync + 'static) -> Self {
        Self::new(move |lines, line, col, opts| {
            let before = lines[line][..col].to_string();
            event_loop::ready(f(&before, &opts))
        })
    }
}

impl AutocompleteProvider for FnProvider {
    fn trigger_characters(&self) -> Vec<String> {
        self.triggers.clone()
    }

    fn get_suggestions(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        options: SuggestOptions,
    ) -> BoxFuture<Option<AutocompleteSuggestions>> {
        (self.get)(lines, cursor_line, cursor_col, options)
    }

    fn apply_completion(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> CompletionResult {
        if let Some(p) = &self.apply_with {
            return p.apply_completion(lines, cursor_line, cursor_col, item, prefix);
        }
        let line = &lines[cursor_line];
        let before = &line[..cursor_col - prefix.len()];
        let after = &line[cursor_col..];
        let mut new_lines = lines.to_vec();
        new_lines[cursor_line] = format!("{before}{}{after}", item.value);
        CompletionResult { lines: new_lines, cursor_line, cursor_col: cursor_col - prefix.len() + item.value.len() }
    }
}

fn suggestions(prefix: &str, items: &[(&str, &str)]) -> Option<AutocompleteSuggestions> {
    Some(AutocompleteSuggestions {
        prefix: prefix.to_string(),
        items: items.iter().map(|(v, l)| AutocompleteItem::new(*v, *l)).collect(),
    })
}

struct Fixture {
    h: Harness,
    editor: Arc<Editor>,
}

impl Fixture {
    fn new() -> Self {
        let h = Harness::new(80, 24);
        let editor = Editor::new(&h.tui, chalk::editor_theme(), EditorOptions::default());
        Self { h, editor }
    }

    fn provider(&self, p: impl AutocompleteProvider + 'static) {
        self.editor.set_autocomplete_provider(Arc::new(p));
    }

    fn type_str(&self, s: &str) {
        for c in s.chars() {
            self.editor.handle_input(&c.to_string());
        }
    }

    /// Equivalent of `flushAutocomplete()`: settle pending microtasks/immediates.
    fn flush(&self) {
        for _ in 0..3 {
            self.h.lp.run_pending();
        }
    }

    fn tick(&self, ms: u64) {
        self.h.lp.advance_ms(ms);
    }

    fn showing(&self) -> bool {
        self.editor.is_showing_autocomplete()
    }

    fn text(&self) -> String {
        self.editor.get_text()
    }
}

fn recording_provider(requests: &Arc<Mutex<Vec<String>>>, triggers: &[&str]) -> FnProvider {
    let r = requests.clone();
    FnProvider::sync(move |before, _| {
        r.lock().unwrap().push(before.to_string());
        None
    })
    .triggers(triggers)
}

#[test]
fn triggers_and_debounces_symbol_completion_after_cjk_punctuation() {
    let mut befores: Vec<String> = vec!["查看，".into(), "\u{3000}".into()];
    befores.extend("，．：；！？（）［］｛｝“”‘’…—。、「」『』《》【】".chars().map(|c| c.to_string()));
    befores.extend(["(", "see (", "`", "["].iter().map(|s| s.to_string()));
    for before in &befores {
        for trigger in ["@", "#", "$", "-"] {
            let f = Fixture::new();
            let requests = Arc::new(Mutex::new(Vec::new()));
            f.provider(recording_provider(&requests, &["$", "-"]));
            f.editor.set_text(before);
            f.editor.handle_input(trigger);
            f.tick(19);
            f.flush();
            assert!(requests.lock().unwrap().is_empty(), "{before}{trigger}");
            f.tick(1);
            f.flush();
            assert_eq!(*requests.lock().unwrap(), vec![format!("{before}{trigger}")]);

            f.editor.handle_input("r");
            f.editor.handle_input("e");
            f.tick(19);
            f.flush();
            assert_eq!(requests.lock().unwrap().len(), 1);
            f.tick(1);
            f.flush();
            assert_eq!(*requests.lock().unwrap(), vec![format!("{before}{trigger}"), format!("{before}{trigger}re")]);
        }
    }
}

#[test]
fn does_not_auto_trigger_after_cjk_letters_or_for_unprefixed_paths() {
    let f = Fixture::new();
    let requests = Arc::new(Mutex::new(Vec::new()));
    f.provider(recording_provider(&requests, &[]));
    for text in [
        "user@example.com",
        "张三@example.com",
        "查看@src",
        "あ@src",
        "カ@src",
        "한@src",
        "ㄅ@src",
        "𠮷@src",
        "か\u{3099}@src",
        "禰\u{e0100}@src",
        "々@src",
        "Ａ@src",
        "文档@备份",
        "prefix#123",
        "foo(@src",
        "问题#123",
        "查看，/path/",
        "查看，./文档/",
        "src/index.ts",
        "./文档/说明.md",
        "文档/说明.md",
        "查看src/index.ts",
    ] {
        f.editor.set_text("");
        f.type_str(text);
        f.tick(20);
        f.flush();
        assert!(requests.lock().unwrap().is_empty(), "{text}");
    }
}

#[test]
fn requests_path_completion_after_cjk_punctuation_only_on_tab() {
    let f = Fixture::new();
    let requests = Arc::new(Mutex::new(Vec::<(String, bool)>::new()));
    let r = requests.clone();
    f.provider(FnProvider::sync(move |before, opts| {
        r.lock().unwrap().push((before.to_string(), opts.force));
        None
    }));
    let text = "查看，/path/";
    f.type_str(text);
    f.tick(20);
    f.flush();
    assert!(requests.lock().unwrap().is_empty());
    f.editor.handle_input("\t");
    f.flush();
    assert_eq!(*requests.lock().unwrap(), vec![(text.to_string(), true)]);
}

#[test]
fn completes_chinese_path_prefixes_after_whitespace_or_cjk_punctuation_with_tab() {
    let dir = std::env::temp_dir().join(format!("pi-editor-autocomplete-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("文档")).unwrap();
    std::fs::write(dir.join("文档").join("说明.md"), "text").unwrap();
    let f = Fixture::new();
    f.editor.set_autocomplete_provider(Arc::new(CombinedAutocompleteProvider::new(
        Vec::new(),
        dir.to_string_lossy().to_string(),
        None,
    )));
    for separator in [" ", "\t", "\u{3000}", "\u{00a0}", "，", "。"] {
        f.editor.set_text(&format!("查看{separator}"));
        let before = f.text();
        f.editor.handle_input("文");
        f.editor.handle_input("\t");
        f.flush();
        assert_eq!(f.text(), format!("{before}文档/"), "separator {separator:?}");
        f.editor.handle_input("说");
        f.editor.handle_input("\t");
        f.flush();
        assert_eq!(f.text(), format!("{before}文档/说明.md"));
        assert_eq!(f.editor.get_cursor(), (0, f.text().len()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ends_unquoted_trigger_and_debounce_contexts_at_whitespace_or_cjk_punctuation() {
    for separator in [" ", "\u{3000}", "，", "。"] {
        for trigger in ["@", "#", "$"] {
            let f = Fixture::new();
            let requests = Arc::new(Mutex::new(Vec::<String>::new()));
            let prefix = format!("{trigger}src");
            let r = requests.clone();
            let p = prefix.clone();
            f.provider(
                FnProvider::sync(move |before, _| {
                    r.lock().unwrap().push(before.to_string());
                    if before == p { suggestions(&p, &[(&format!("{p}/"), "src/")]) } else { None }
                })
                .triggers(&["$"]),
            );
            f.editor.set_text(&format!("{trigger}sr"));
            f.editor.handle_input("c");
            f.tick(20);
            f.flush();
            assert!(f.showing());
            f.editor.handle_input(separator);
            f.flush();
            assert_eq!(*requests.lock().unwrap(), vec![prefix.clone(), format!("{prefix}{separator}")]);
            assert!(!f.showing());
            f.editor.handle_input("文");
            f.tick(20);
            f.flush();
            assert_eq!(*requests.lock().unwrap(), vec![prefix.clone(), format!("{prefix}{separator}")]);
        }
    }
}

#[test]
fn re_triggers_cjk_path_completion_after_accepting_directories_and_deleting() {
    let base: Arc<dyn AutocompleteProvider> = Arc::new(CombinedAutocompleteProvider::new(
        Vec::new(),
        std::env::current_dir().unwrap().to_string_lossy().to_string(),
        None,
    ));
    for directory in ["文档", "我的 文档", "资料，归档"] {
        let quoted = directory != "文档";
        let first_two: String = directory.chars().take(2).collect();
        let initial = if quoted { format!("@\"{first_two}") } else { "@文".to_string() };
        let directory_value = if quoted { format!("@\"{directory}/\"") } else { format!("@{directory}/") };
        let file_value = if quoted { format!("@\"{directory}/说明.md\"") } else { format!("@{directory}/说明.md") };
        let file_prefix = if quoted { format!("@\"{directory}/说") } else { format!("@{directory}/说") };
        let f = Fixture::new();
        let (i, dv, fv, fp, d) =
            (initial.clone(), directory_value.clone(), file_value.clone(), file_prefix.clone(), directory.to_string());
        let mut provider = FnProvider::sync(move |before, _| {
            let prefix = &before[before.find('@').unwrap_or(0)..];
            if prefix == i {
                return suggestions(prefix, &[(&dv, &format!("{d}/"))]);
            }
            if prefix == fp { suggestions(prefix, &[(&fv, "说明.md")]) } else { None }
        });
        provider.apply_with = Some(base.clone());
        f.provider(provider);
        f.editor.set_text(&format!("查看：{initial}"));
        f.editor.handle_input("\t");
        f.flush();
        assert_eq!(f.text(), format!("查看：{directory_value}"));
        assert!(!f.showing());

        f.editor.handle_input("说");
        f.tick(20);
        f.flush();
        assert!(f.showing(), "{directory}");

        for deletion in ["\x7f", "\x1b[3~"] {
            f.editor.handle_input("错");
            f.tick(20);
            f.flush();
            assert!(!f.showing());
            if deletion == "\x1b[3~" {
                f.editor.handle_input("\x1b[D");
            }
            f.editor.handle_input(deletion);
            f.tick(20);
            f.flush();
            assert!(f.showing(), "{directory} after {deletion:?}");
        }

        f.editor.handle_input("\t");
        assert_eq!(f.text(), format!("查看：{file_value} "));
        assert_eq!(f.editor.get_cursor(), (0, f.text().len()));
    }
}

#[test]
fn auto_applies_single_force_file_suggestion_without_showing_menu() {
    let f = Fixture::new();
    f.provider(FnProvider::sync(|before, opts| {
        if opts.force && before == "Work" { suggestions("Work", &[("Workspace/", "Workspace/")]) } else { None }
    }));
    f.type_str("Work");
    assert_eq!(f.text(), "Work");
    f.editor.handle_input("\t");
    f.flush();
    assert_eq!(f.text(), "Workspace/");
    assert!(!f.showing());
    f.editor.handle_input("\x1b[45;5u");
    assert_eq!(f.text(), "Work");
}

#[test]
fn shows_menu_when_force_file_has_multiple_suggestions() {
    let f = Fixture::new();
    f.provider(FnProvider::sync(|before, opts| {
        if opts.force && before == "src" { suggestions("src", &[("src/", "src/"), ("src.txt", "src.txt")]) } else { None }
    }));
    f.type_str("src");
    f.editor.handle_input("\t");
    f.flush();
    assert_eq!(f.text(), "src");
    assert!(f.showing());
    f.editor.handle_input("\t");
    assert_eq!(f.text(), "src/");
    assert!(!f.showing());
}

#[test]
fn keeps_suggestions_open_when_typing_in_force_mode() {
    let f = Fixture::new();
    let files = [("readme.md", "readme.md"), ("package.json", "package.json"), ("src/", "src/"), ("dist/", "dist/")];
    f.provider(FnProvider::sync(move |before, opts| {
        if !(opts.force || before.contains('/') || before.starts_with('.')) {
            return None;
        }
        let filtered: Vec<(&str, &str)> =
            files.iter().copied().filter(|(v, _)| v.to_lowercase().starts_with(&before.to_lowercase())).collect();
        if filtered.is_empty() { None } else { suggestions(before, &filtered) }
    }));
    f.editor.handle_input("\t");
    f.flush();
    assert!(f.showing());
    f.editor.handle_input("r");
    f.flush();
    assert_eq!(f.text(), "r");
    assert!(f.showing());
    f.editor.handle_input("e");
    f.flush();
    assert_eq!(f.text(), "re");
    assert!(f.showing());
    f.editor.handle_input("\t");
    assert_eq!(f.text(), "readme.md");
    assert!(!f.showing());
}

fn counting_provider(calls: &Arc<Mutex<usize>>, value: &'static str, label: &'static str) -> FnProvider {
    let c = calls.clone();
    FnProvider::sync(move |before, _| {
        *c.lock().unwrap() += 1;
        suggestions(before, &[(value, label)])
    })
}

#[test]
fn debounces_at_and_hash_autocomplete_while_typing() {
    for (input, value) in [("@mai", "@main.ts"), ("#298", "#2983")] {
        let f = Fixture::new();
        let calls = Arc::new(Mutex::new(0));
        f.provider(counting_provider(&calls, value, value));
        f.type_str(input);
        assert_eq!(*calls.lock().unwrap(), 0);
        assert!(!f.showing());
        f.tick(50);
        f.flush();
        assert_eq!(*calls.lock().unwrap(), 1);
        assert!(f.showing());
    }
}

#[test]
fn debounces_custom_trigger_characters_autocomplete_while_typing() {
    let f = Fixture::new();
    let calls = Arc::new(Mutex::new(0));
    f.provider(counting_provider(&calls, "$skill-name", "skill-name").triggers(&["$"]));
    f.type_str("$sk");
    assert_eq!(*calls.lock().unwrap(), 0);
    f.tick(50);
    f.flush();
    assert_eq!(*calls.lock().unwrap(), 1);
    assert!(f.showing());
}

#[test]
fn resets_custom_trigger_characters_when_provider_changes() {
    let f = Fixture::new();
    f.provider(FnProvider::sync(|_, _| suggestions("$", &[("$skill-name", "skill-name")])).triggers(&["$"]));
    let calls = Arc::new(Mutex::new(0));
    let c = calls.clone();
    f.provider(FnProvider::sync(move |_, _| {
        *c.lock().unwrap() += 1;
        suggestions("$", &[("$skill-name", "skill-name")])
    }));
    f.type_str("$s");
    f.tick(50);
    f.flush();
    assert_eq!(*calls.lock().unwrap(), 0);
    assert!(!f.showing());
}

#[test]
fn aborts_active_at_autocomplete_when_typing_continues() {
    let f = Fixture::new();
    let aborts = Arc::new(Mutex::new(0));
    let a = aborts.clone();
    f.provider(FnProvider::new(move |_, _, _, opts| {
        let (resolver, deferred) = event_loop::deferred::<Option<AutocompleteSuggestions>>();
        let lp = event_loop::current();
        let r1 = resolver.clone();
        let timer = lp.set_timeout(Duration::from_millis(500), move || {
            r1.resolve(suggestions("@main", &[("@main.ts", "main.ts")]));
        });
        let a = a.clone();
        opts.signal.on_abort(move || {
            *a.lock().unwrap() += 1;
            lp.clear_timer(timer);
            resolver.resolve(None);
        });
        Box::pin(deferred)
    }));
    f.type_str("@mai");
    f.tick(250);
    f.flush();
    f.editor.handle_input("n");
    f.tick(50);
    f.flush();
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[test]
fn hides_autocomplete_when_backspacing_slash_command_to_empty() {
    let f = Fixture::new();
    f.provider(FnProvider::sync(|before, _| {
        if !before.starts_with('/') {
            return None;
        }
        let query = &before[1..];
        let filtered: Vec<(&str, &str)> =
            [("/model", "model"), ("/help", "help")].into_iter().filter(|(v, _)| v.starts_with(query)).collect();
        if filtered.is_empty() { None } else { suggestions(before, &filtered) }
    }));
    f.editor.handle_input("/");
    f.flush();
    assert_eq!(f.text(), "/");
    assert!(f.showing());
    f.editor.handle_input("\x7f");
    f.flush();
    assert_eq!(f.text(), "");
    assert!(!f.showing());
}

#[test]
fn re_queries_picker_when_cursor_moves_back_into_command_name() {
    let f = Fixture::new();
    f.provider(FnProvider::sync(|before, _| {
        if !before.starts_with('/') {
            return None;
        }
        if let Some(space) = before.find(' ') {
            return suggestions(&before[space + 1..], &[("repo", "repo"), ("message", "message"), ("help", "help")]);
        }
        suggestions(before, &[("cmd", "cmd")])
    }));
    for c in "/cmd ".chars() {
        f.editor.handle_input(&c.to_string());
        f.flush();
    }
    assert_eq!(f.text(), "/cmd ");
    assert!(f.showing());
    let at_arg: String = f.editor.render(80).iter().map(|l| strip_terminal_sequences(l)).collect::<Vec<_>>().join("\n");
    assert!(at_arg.contains("repo"));
    f.editor.handle_input("\x1b[D");
    f.flush();
    let after: String = f.editor.render(80).iter().map(|l| strip_terminal_sequences(l)).collect::<Vec<_>>().join("\n");
    assert!(!after.contains("repo"));
    assert!(!after.contains("message"));
}

/// Provider for `/<command> <arg>` completion; `filter` restricts to prefix matches.
fn argument_provider(command: &'static str, args: &'static [&'static str], filter: bool) -> FnProvider {
    FnProvider::sync(move |before, _| {
        let rest = before.strip_prefix('/')?.strip_prefix(command)?;
        let arg = rest.trim_start();
        if arg.len() == rest.len() || arg.is_empty() || arg.contains(char::is_whitespace) {
            return None;
        }
        let items: Vec<(&str, &str)> =
            args.iter().copied().filter(|a| !filter || a.starts_with(arg)).map(|a| (a, a)).collect();
        if items.is_empty() { None } else { suggestions(arg, &items) }
    })
}

#[test]
fn enter_applies_exact_or_prefix_matched_slash_argument() {
    let cases: [(&'static [&'static str], bool, &str, &str); 5] = [
        (&["one", "two", "three"], true, "/argtest two", "/argtest two"),
        (&["two", "three", "twelve"], true, "/argtest t", "/argtest two"),
        (&["one", "two", "three"], false, "/argtest tw", "/argtest two"),
        (&["one", "two", "three"], false, "/argtest t", "/argtest two"),
        (&["gpt-4o", "gpt-4o-mini", "claude-sonnet"], true, "/model gpt-4o-mini", "/model gpt-4o-mini"),
    ];
    for (args, filter, typed, expected) in cases {
        let f = Fixture::new();
        let command = if typed.starts_with("/model") { "model" } else { "argtest" };
        f.provider(argument_provider(command, args, filter));
        f.type_str(typed);
        assert_eq!(f.text(), typed);
        f.flush();
        assert!(f.showing(), "{typed}");
        f.editor.handle_input("\r");
        assert_eq!(f.text(), expected);
    }
}

fn command(name: &str, completions: Option<Vec<&'static str>>) -> CommandEntry {
    CommandEntry::Command(SlashCommand {
        name: name.to_string(),
        description: Some(format!("{name} command")),
        argument_hint: None,
        get_argument_completions: completions.map(|values| {
            let f: pi_tui::autocomplete::ArgumentCompletions = Arc::new(move |prefix: &str| {
                let items: Vec<AutocompleteItem> = values
                    .iter()
                    .filter(|v| v.starts_with(&prefix[..prefix.len().min(1)]))
                    .map(|v| AutocompleteItem::new(*v, *v))
                    .collect();
                event_loop::ready(if items.is_empty() { None } else { Some(items) })
            });
            f
        }),
    })
}

#[test]
fn awaits_async_slash_command_argument_completions() {
    let f = Fixture::new();
    f.editor.set_autocomplete_provider(Arc::new(CombinedAutocompleteProvider::new(
        vec![command("load-skills", Some(vec!["skill-a"]))],
        std::env::current_dir().unwrap().to_string_lossy().to_string(),
        None,
    )));
    f.editor.set_text("/load-skills ");
    f.editor.handle_input("s");
    f.flush();
    assert!(f.showing());
    f.editor.handle_input("\t");
    assert_eq!(f.text(), "/load-skills skill-a");
    assert!(!f.showing());
}

#[test]
fn does_not_show_argument_completions_when_command_has_no_argument_completer() {
    let f = Fixture::new();
    f.editor.set_autocomplete_provider(Arc::new(CombinedAutocompleteProvider::new(
        vec![command("help", None), command("model", Some(vec!["claude-opus"]))],
        std::env::current_dir().unwrap().to_string_lossy().to_string(),
        None,
    )));
    f.type_str("/he");
    f.flush();
    assert!(f.showing());
    f.editor.handle_input("\t");
    assert_eq!(f.text(), "/help ");
    assert!(!f.showing());
}
