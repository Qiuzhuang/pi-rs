//! `CombinedAutocompleteProvider` behaviour (ported from autocomplete.test.ts and
//! autocomplete-skill-slash.test.ts). Cursor columns are UTF-8 byte offsets.
//! The `@` file-suggestion tests need `fd` (or Debian's `fdfind`) on `PATH` and
//! are skipped otherwise, like the TS suite.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pi_tui::autocomplete::{
    AutocompleteItem, AutocompleteProvider, AutocompleteSuggestions, CombinedAutocompleteProvider, CommandEntry,
    SlashCommand, SuggestOptions,
};
use pi_tui::event_loop::ready;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl std::task::Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

fn suggest(
    provider: &CombinedAutocompleteProvider,
    lines: &[String],
    line: usize,
    col: usize,
    force: bool,
) -> Option<AutocompleteSuggestions> {
    block_on(provider.get_suggestions(lines, line, col, SuggestOptions { signal: Default::default(), force }))
}

fn suggest_line(provider: &CombinedAutocompleteProvider, line: &str, force: bool) -> Option<AutocompleteSuggestions> {
    suggest(provider, &[line.to_string()], 0, line.len(), force)
}

fn values(result: &Option<AutocompleteSuggestions>) -> Vec<String> {
    result.as_ref().map(|r| r.items.iter().map(|i| i.value.clone()).collect()).unwrap_or_default()
}

fn apply_line(
    provider: &CombinedAutocompleteProvider,
    line: &str,
    col: usize,
    item: &AutocompleteItem,
    prefix: &str,
) -> (String, usize) {
    let r = provider.apply_completion(&[line.to_string()], 0, col, item, prefix);
    (r.lines[0].clone(), r.cursor_col)
}

/// A temporary directory removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn str(&self) -> String {
        self.0.to_string_lossy().to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn setup_folder(base: &Path, dirs: &[&str], files: &[(&str, &str)]) {
    for dir in dirs {
        std::fs::create_dir_all(base.join(dir)).unwrap();
    }
    for (path, contents) in files {
        let full = base.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
}

fn fd_path() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for name in ["fd", "fdfind"] {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Root/cwd/outside layout of the fd tests; `None` when fd is missing.
struct FdFixture {
    root: TempDir,
    base: PathBuf,
    outside: PathBuf,
    fd: String,
}

impl FdFixture {
    fn new() -> Option<Self> {
        let Some(fd) = fd_path() else {
            eprintln!("fd is not installed; skipping");
            return None;
        };
        let root = TempDir::new("pi-autocomplete-root-");
        let base = root.0.join("cwd");
        let outside = root.0.join("outside");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        Some(Self { root, base, outside, fd })
    }

    fn provider(&self) -> CombinedAutocompleteProvider {
        self.provider_at(&self.base)
    }

    fn provider_at(&self, dir: &Path) -> CombinedAutocompleteProvider {
        CombinedAutocompleteProvider::new(Vec::new(), dir.to_string_lossy().to_string(), Some(self.fd.clone()))
    }
}

fn plain_provider(dir: &TempDir) -> CombinedAutocompleteProvider {
    CombinedAutocompleteProvider::new(Vec::new(), dir.str(), None)
}

// ---------------------------------------------------------------------------
// extractPathPrefix
// ---------------------------------------------------------------------------

#[test]
fn extracts_root_from_hey_slash_when_forced() {
    let provider = CombinedAutocompleteProvider::new(Vec::new(), "/tmp", None);
    let result = suggest_line(&provider, "hey /", true).expect("suggestions for root directory");
    assert_eq!(result.prefix, "/");
}

#[test]
fn extracts_slash_a_when_forced() {
    let provider = CombinedAutocompleteProvider::new(Vec::new(), "/tmp", None);
    if let Some(result) = suggest_line(&provider, "/A", true) {
        assert_eq!(result.prefix, "/A");
    }
}

#[test]
fn does_not_trigger_for_slash_commands() {
    let provider = CombinedAutocompleteProvider::new(Vec::new(), "/tmp", None);
    assert_eq!(suggest_line(&provider, "/model", true), None);
}

#[test]
fn triggers_for_absolute_paths_after_slash_command_argument() {
    let provider = CombinedAutocompleteProvider::new(Vec::new(), "/tmp", None);
    let result = suggest_line(&provider, "/command /", true).expect("suggestions for absolute path");
    assert_eq!(result.prefix, "/");
}

// ---------------------------------------------------------------------------
// fd @ file suggestions
// ---------------------------------------------------------------------------

#[test]
fn fd_returns_all_files_and_folders_for_empty_query() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &["src"], &[("README.md", "readme")]);
    let mut v = values(&suggest_line(&f.provider(), "@", false));
    v.sort();
    assert_eq!(v, vec!["@README.md", "@src/"]);
}

#[test]
fn fd_recognizes_at_after_cjk_punctuation() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("README.md", "readme")]);
    let provider = f.provider();
    let mut befores: Vec<String> = vec!["查看，".into(), "\u{3000}".into()];
    befores.extend("，．：；！？（）［］｛｝“”‘’…—。、「」『』《》【】".chars().map(String::from));
    for before in &befores {
        for force in [false, true] {
            let line = format!("{before}@REA");
            let result = suggest_line(&provider, &line, force).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(result.prefix, "@REA");
            assert_eq!(values(&Some(result.clone())), vec!["@README.md"]);
            let (applied, col) = apply_line(&provider, &line, line.len(), &result.items[0], &result.prefix);
            assert_eq!(applied, format!("{before}@README.md "));
            assert_eq!(col, applied.len());
        }
    }
}

#[test]
fn fd_recognizes_at_after_opening_wrappers() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("README.md", "readme")]);
    let provider = f.provider();
    for before in ["(", "see (", "[", "`", "<", "{"] {
        let line = format!("{before}@REA");
        let result = suggest_line(&provider, &line, false).unwrap_or_else(|| panic!("{line}"));
        assert_eq!(result.prefix, "@REA");
        let (applied, _) = apply_line(&provider, &line, line.len(), &result.items[0], &result.prefix);
        assert_eq!(applied, format!("{before}@README.md "));
    }
    assert_eq!(suggest_line(&provider, "foo(@REA", false), None);
}

#[test]
fn fd_preserves_cjk_and_embedded_at_in_attachment_paths() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("文档/说明.md", "text"), ("文档@备份/说明.md", "backup")]);
    let provider = f.provider();
    for before in ["", "查看，"] {
        for directory in ["文档", "文档@备份"] {
            let prefix = format!("@{directory}/说");
            let line = format!("{before}{prefix}");
            let result = suggest_line(&provider, &line, false);
            assert_eq!(result.as_ref().map(|r| r.prefix.clone()), Some(prefix.clone()), "{line}");
            assert_eq!(values(&result), vec![format!("@{directory}/说明.md")]);
        }
    }
}

#[test]
fn fd_completes_quoted_cjk_attachments_after_prose() {
    let Some(f) = FdFixture::new() else { return };
    let provider = f.provider();
    for separator in [" ", "\u{3000}", "，", "。"] {
        let directory = format!("我的{separator}文档");
        setup_folder(
            &f.base,
            &[],
            &[(&format!("{directory}/说明.md"), "text"), ("文档/说明.md", "not the quoted path")],
        );
        let line = format!("查看：@\"{directory}/说\"后文");
        let cursor = line.find("\"后文").unwrap();
        let result = suggest(&provider, &[line.clone()], 0, cursor, false).expect("suggestions");
        assert_eq!(result.prefix, format!("@\"{directory}/说"));
        assert_eq!(values(&Some(result.clone())), vec![format!("@\"{directory}/说明.md\"")]);
        let (applied, col) = apply_line(&provider, &line, cursor, &result.items[0], &result.prefix);
        assert_eq!(applied, format!("查看：@\"{directory}/说明.md\" 后文"));
        assert_eq!(col, format!("查看：@\"{directory}/说明.md\" ").len());
    }
}

#[test]
fn fd_ignores_email_addresses_and_at_after_letters() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("README.md", "readme"), ("example.com", "text")]);
    let provider = f.provider();
    for before in ["user", "查看", "あ", "カ", "한", "ㄅ", "𠮷", "か\u{3099}", "禰\u{e0100}", "々", "Ａ"] {
        for name in ["REA", "example.com"] {
            let line = format!("{before}@{name}");
            assert_eq!(suggest_line(&provider, &line, false), None, "{line}");
        }
    }
}

#[test]
fn fd_matches_file_with_extension_in_query() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("file.txt", "content")]);
    assert!(values(&suggest_line(&f.provider(), "@file.txt", false)).contains(&"@file.txt".to_string()));
}

#[test]
fn fd_matches_regex_metacharacters_literally() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("pages/[id].tsx", "x"), ("pages/index.tsx", "x"), ("pages/(group).ts", "x")]);
    let provider = f.provider();
    for (line, expected) in
        [("@pages/[", "@pages/[id].tsx"), ("@pages/(", "@pages/(group).ts"), ("@[id", "@pages/[id].tsx")]
    {
        assert_eq!(values(&suggest_line(&provider, line, false)), vec![expected], "{line}");
    }
}

#[test]
fn fd_filters_are_case_insensitive() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &["src"], &[("README.md", "readme")]);
    assert_eq!(values(&suggest_line(&f.provider(), "@re", false)), vec!["@README.md"]);
}

#[test]
fn fd_ranks_directories_before_files() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &["src"], &[("src.txt", "text")]);
    let v = values(&suggest_line(&f.provider(), "@src", false));
    assert_eq!(v.first().map(String::as_str), Some("@src/"));
    assert!(v.contains(&"@src.txt".to_string()));
}

#[test]
fn fd_returns_nested_file_paths() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("src/index.ts", "export {};\n")]);
    assert!(values(&suggest_line(&f.provider(), "@index", false)).contains(&"@src/index.ts".to_string()));
}

#[test]
fn fd_matches_deeply_nested_paths() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(
        &f.base,
        &[],
        &[("packages/tui/src/autocomplete.ts", "export {};"), ("packages/ai/src/autocomplete.ts", "export {};")],
    );
    let v = values(&suggest_line(&f.provider(), "@tui/src/auto", false));
    assert!(v.contains(&"@packages/tui/src/autocomplete.ts".to_string()));
    assert!(!v.contains(&"@packages/ai/src/autocomplete.ts".to_string()));
}

#[test]
fn fd_matches_directory_in_middle_of_path_with_full_path() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(
        &f.base,
        &[],
        &[("src/components/Button.tsx", "export {};"), ("src/utils/helpers.ts", "export {};")],
    );
    let v = values(&suggest_line(&f.provider(), "@components/", false));
    assert!(v.contains(&"@src/components/Button.tsx".to_string()));
    assert!(!v.contains(&"@src/utils/helpers.ts".to_string()));
}

#[test]
fn fd_scopes_fuzzy_search_to_relative_directories_recursively() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(
        &f.outside,
        &[],
        &[
            ("nested/alpha.ts", "export {};"),
            ("nested/deeper/also-alpha.ts", "export {};"),
            ("nested/deeper/zzz.ts", "export {};"),
        ],
    );
    let v = values(&suggest_line(&f.provider(), "@../outside/a", false));
    assert!(v.contains(&"@../outside/nested/alpha.ts".to_string()), "{v:?}");
    assert!(v.contains(&"@../outside/nested/deeper/also-alpha.ts".to_string()));
    assert!(!v.contains(&"@../outside/nested/deeper/zzz.ts".to_string()));
}

#[test]
fn fd_ranks_shallower_same_score_matches_first() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &["scope/aaa/venv/lib/python3.12/site-packages/pkg/core/profile", "scope/projects"], &[]);
    let v = values(&suggest_line(&f.provider(), "@scope/pro", false));
    assert_eq!(v.first().map(String::as_str), Some("@scope/projects/"));
    assert!(v.contains(&"@scope/aaa/venv/lib/python3.12/site-packages/pkg/core/profile/".to_string()));
}

#[test]
fn fd_includes_scoped_direct_children_when_flooded() {
    let Some(f) = FdFixture::new() else { return };
    let mut dirs: Vec<String> = vec!["scope/projects".into()];
    dirs.extend((1..=250).map(|i| format!("scope/a{i:03}/venv/lib/python3.12/site-packages/pkg/core/profile")));
    let dir_refs: Vec<&str> = dirs.iter().map(String::as_str).collect();
    setup_folder(&f.base, &dir_refs, &[]);
    let v = values(&suggest_line(&f.provider(), "@scope/pro", false));
    assert_eq!(v.first().map(String::as_str), Some("@scope/projects/"));
    assert!(v.iter().any(|value| value.contains("/profile/")), "should keep deep fuzzy matches");
}

#[test]
fn fd_quotes_paths_with_whitespace_or_cjk_punctuation() {
    let Some(f) = FdFixture::new() else { return };
    let provider = f.provider();
    for separator in [" ", "\u{3000}", "，", "。"] {
        let directory = format!("my{separator}folder");
        setup_folder(&f.base, &[], &[(&format!("{directory}/test.txt"), "content")]);
        let result = suggest_line(&provider, "@my", false).expect("suggestions");
        let item = result
            .items
            .iter()
            .find(|e| e.value == format!("@\"{directory}/\""))
            .unwrap_or_else(|| panic!("{directory}"));
        let applied = provider.apply_completion(&["@my".to_string()], 0, 3, item, &result.prefix);
        let continued = suggest(&provider, &applied.lines, 0, applied.cursor_col, false);
        assert_eq!(continued.as_ref().map(|c| c.prefix.clone()), Some(format!("@\"{directory}/")));
        assert!(values(&continued).contains(&format!("@\"{directory}/test.txt\"")));
    }
}

#[test]
fn fd_includes_hidden_paths_but_excludes_git() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(
        &f.base,
        &[".pi", ".github", ".git"],
        &[(".pi/config.json", "{}"), (".github/workflows/ci.yml", "name: ci"), (".git/config", "[core]")],
    );
    let v = values(&suggest_line(&f.provider(), "@", false));
    assert!(v.contains(&"@.pi/".to_string()));
    assert!(v.contains(&"@.github/".to_string()));
    assert!(!v.iter().any(|value| value == "@.git" || value.starts_with("@.git/")));
}

#[cfg(unix)]
#[test]
fn fd_follows_symlinked_directories() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("dir/some_file.txt", "real")]);
    setup_folder(&f.outside, &[], &[("some_file.txt", "symlinked")]);
    std::os::unix::fs::symlink("../outside", f.base.join("symlinked_dir")).unwrap();
    let v = values(&suggest_line(&f.provider(), "@some", false));
    assert!(v.contains(&"@dir/some_file.txt".to_string()));
    assert!(v.contains(&"@symlinked_dir/some_file.txt".to_string()));
}

#[cfg(unix)]
#[test]
fn fd_returns_symlinked_directories_by_name() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.outside, &[], &[("nested/file.txt", "symlinked")]);
    std::os::unix::fs::symlink("../outside", f.base.join("symlinked_dir")).unwrap();
    assert!(values(&suggest_line(&f.provider(), "@symlinked", false)).contains(&"@symlinked_dir/".to_string()));
}

#[cfg(unix)]
#[test]
fn fd_returns_symlinked_files() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("original.txt", "content")]);
    std::os::unix::fs::symlink("original.txt", f.base.join("link.txt")).unwrap();
    assert!(values(&suggest_line(&f.provider(), "@link", false)).contains(&"@link.txt".to_string()));
}

#[test]
fn fd_same_suggestions_when_cwd_contains_query() {
    let Some(f) = FdFixture::new() else { return };
    let normal = f.root.0.join("cwd-normal");
    let query_in_path = f.root.0.join("cwd-plan-repro");
    for dir in [&normal, &query_in_path] {
        setup_folder(
            dir,
            &["packages/coding-agent/examples/extensions/plan-mode"],
            &[
                ("packages/coding-agent/examples/extensions/plan-mode/README.md", "readme"),
                ("packages/tui/docs/plan.md", "plan"),
            ],
        );
    }
    let normalize = |r: Option<AutocompleteSuggestions>| {
        let mut v: Vec<String> = r
            .map(|r| r.items)
            .unwrap_or_default()
            .iter()
            .map(|i| format!("{} :: {}", i.label, i.description.clone().unwrap_or_default()))
            .collect();
        v.sort();
        v
    };
    let a = normalize(suggest_line(&f.provider_at(&normal), "@plan", false));
    let b = normalize(suggest_line(&f.provider_at(&query_in_path), "@plan", false));
    assert_eq!(b, a);
    assert!(a.contains(&"plan-mode/ :: packages/coding-agent/examples/extensions/plan-mode".to_string()));
    assert!(a.contains(&"plan.md :: packages/tui/docs/plan.md".to_string()));
}

#[test]
fn fd_continues_inside_quoted_paths() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("my folder/test.txt", "content"), ("my folder/other.txt", "content")]);
    let line = "@\"my folder/\"";
    let v = values(&suggest(&f.provider(), &[line.to_string()], 0, line.len() - 1, false));
    assert!(v.contains(&"@\"my folder/test.txt\"".to_string()));
    assert!(v.contains(&"@\"my folder/other.txt\"".to_string()));
}

#[test]
fn fd_applies_quoted_completion_without_duplicating_quote() {
    let Some(f) = FdFixture::new() else { return };
    setup_folder(&f.base, &[], &[("my folder/test.txt", "content")]);
    let provider = f.provider();
    let line = "@\"my folder/te\"";
    let cursor = line.len() - 1;
    let result = suggest(&provider, &[line.to_string()], 0, cursor, false).expect("suggestions");
    let item = result.items.iter().find(|e| e.value == "@\"my folder/test.txt\"").expect("test.txt");
    let (applied, _) = apply_line(&provider, line, cursor, item, &result.prefix);
    assert_eq!(applied, "@\"my folder/test.txt\" ");
}

// ---------------------------------------------------------------------------
// dot-slash path completion
// ---------------------------------------------------------------------------

#[test]
fn completes_chinese_path_prefixes_after_separators() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("说明.md", "file"), ("文档/说明.md", "nested file")]);
    let provider = plain_provider(&dir);
    let mut completions: Vec<(String, String)> = vec![
        ("说".into(), "说明.md".into()),
        ("文".into(), "文档/".into()),
        ("文档/说".into(), "文档/说明.md".into()),
        ("./文档/说".into(), "./文档/说明.md".into()),
    ];
    completions.push((format!("{}/文档/说", dir.str()), format!("{}/文档/说明.md", dir.str())));
    for separator in " \t\u{3000}\u{00a0}，：；。！？（「《".chars() {
        for (prefix, value) in &completions {
            let before = format!("查看𠮷{separator}");
            let line = format!("{before}{prefix} 后文");
            let cursor = before.len() + prefix.len();
            let result = suggest(&provider, &[line.clone()], 0, cursor, true).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(&result.prefix, prefix);
            assert_eq!(values(&Some(result.clone())), vec![value.clone()]);
            let (applied, col) = apply_line(&provider, &line, cursor, &result.items[0], &result.prefix);
            assert_eq!(applied, format!("{before}{value} 后文"));
            assert_eq!(col, before.len() + value.len());
        }
    }
}

#[test]
fn treats_unquoted_separators_as_boundaries() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("归档/说明.md", "other")]);
    let provider = plain_provider(&dir);
    for separator in [" ", "\u{3000}", "，", "。"] {
        let directory = format!("资料{separator}归档");
        setup_folder(&dir.0, &[], &[(&format!("{directory}/说明.md"), "archive")]);
        for marker in ["", "@"] {
            let line = format!("{marker}{directory}/说");
            let result = suggest_line(&provider, &line, true);
            assert_eq!(result.as_ref().map(|r| r.prefix.as_str()), Some("归档/说"), "{line}");
            assert_eq!(values(&result), vec!["归档/说明.md"]);
        }
        let quoted = format!("查看，\"{directory}/说\"后文");
        let cursor = quoted.find("\"后文").unwrap();
        let result = suggest(&provider, &[quoted.clone()], 0, cursor, true).expect("suggestions");
        assert_eq!(result.prefix, format!("\"{directory}/说"));
        assert_eq!(values(&Some(result.clone())), vec![format!("\"{directory}/说明.md\"")]);
        let (applied, _) = apply_line(&provider, &quoted, cursor, &result.items[0], &result.prefix);
        assert_eq!(applied, format!("查看，\"{directory}/说明.md\"后文"));
        let missing = format!("查看，\"不存在{separator}归档/说");
        assert_eq!(suggest_line(&provider, &missing, true), None);
    }
}

#[test]
fn handles_empty_prefix_after_separators_consistently() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("说明.md", "text")]);
    let provider = plain_provider(&dir);
    for separator in [" ", "\t", "\u{3000}", "，", "。"] {
        for force in [false, true] {
            let line = format!("查看{separator}");
            let result = suggest_line(&provider, &line, force);
            assert_eq!(result.as_ref().map(|r| r.prefix.as_str()), Some(""), "{line:?}");
            assert_eq!(values(&result), vec!["说明.md"]);
        }
    }
    assert_eq!(suggest(&provider, &[String::new()], 0, 0, false), None);
}

#[test]
fn completes_paths_after_opening_wrappers() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("src/main.ts", "x")]);
    let provider = plain_provider(&dir);
    for wrapper in ["(", "[", "{", "<", "`", "((", "(`"] {
        for prefix in ["src/ma", "./src/ma"] {
            let before = format!("see {wrapper}");
            let line = format!("{before}{prefix}");
            let result = suggest_line(&provider, &line, true).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(result.prefix, prefix);
            let value = prefix.replacen("src/ma", "src/main.ts", 1);
            assert_eq!(values(&Some(result.clone())), vec![value.clone()]);
            let (applied, _) = apply_line(&provider, &line, line.len(), &result.items[0], &result.prefix);
            assert_eq!(applied, format!("{before}{value}"));
        }
    }
}

#[test]
fn completes_quoted_paths_after_opening_wrappers() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("my dir/main.ts", "x")]);
    let result = suggest_line(&plain_provider(&dir), "see (\"my dir/ma", true);
    assert_eq!(result.as_ref().map(|r| r.prefix.as_str()), Some("\"my dir/ma"));
    assert_eq!(values(&result), vec!["\"my dir/main.ts\""]);
}

#[test]
fn keeps_wrappers_closed_inside_the_path() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("[slug]/page.tsx", "x"), ("(group)/layout.tsx", "x")]);
    let provider = plain_provider(&dir);
    for (prefix, value) in
        [("[slug]/pa", "[slug]/page.tsx"), ("(group)/la", "(group)/layout.tsx"), ("./[slug]/pa", "./[slug]/page.tsx")]
    {
        let line = format!("see {prefix}");
        let result = suggest_line(&provider, &line, true);
        assert_eq!(result.as_ref().map(|r| r.prefix.as_str()), Some(prefix), "{line}");
        assert_eq!(values(&result), vec![value]);
    }
}

#[test]
fn preserves_cjk_in_unprefixed_tab_completions() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("文档/说明.md", "text")]);
    let result = suggest_line(&plain_provider(&dir), "文档/说", true);
    assert_eq!(result.as_ref().map(|r| r.prefix.as_str()), Some("文档/说"));
    assert_eq!(values(&result), vec!["文档/说明.md"]);
}

#[test]
fn preserves_dot_slash_prefix_for_files() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("update.sh", "#!/bin/bash"), ("utils.ts", "export {};")]);
    let v = values(&suggest_line(&plain_provider(&dir), "./up", true));
    assert!(v.contains(&"./update.sh".to_string()), "{v:?}");
}

#[test]
fn preserves_dot_slash_prefix_for_directories() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &["src"], &[("src/index.ts", "export {};")]);
    let v = values(&suggest_line(&plain_provider(&dir), "./sr", true));
    assert!(v.contains(&"./src/".to_string()), "{v:?}");
}

// ---------------------------------------------------------------------------
// quoted path completion
// ---------------------------------------------------------------------------

#[test]
fn quotes_paths_with_whitespace_or_cjk_punctuation_for_direct_completion() {
    let dir = TempDir::new("pi-autocomplete-");
    let provider = plain_provider(&dir);
    for separator in [" ", "\u{3000}", "，", "。"] {
        let directory = format!("my{separator}folder");
        setup_folder(&dir.0, &[], &[(&format!("{directory}/test.txt"), "content")]);
        let result = suggest_line(&provider, "my", true).expect("suggestions");
        let item = result
            .items
            .iter()
            .find(|e| e.value == format!("\"{directory}/\""))
            .unwrap_or_else(|| panic!("{directory}"));
        let applied = provider.apply_completion(&["my".to_string()], 0, 2, item, &result.prefix);
        let continued = suggest(&provider, &applied.lines, 0, applied.cursor_col, true);
        assert_eq!(continued.as_ref().map(|c| c.prefix.clone()), Some(format!("\"{directory}/")));
        assert_eq!(values(&continued), vec![format!("\"{directory}/test.txt\"")]);
    }
}

#[test]
fn keeps_quoted_directories_before_files() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &["z folder", "z，folder"], &[("a.txt", "text")]);
    let result = suggest(&plain_provider(&dir), &[String::new()], 0, 0, true).expect("suggestions");
    let dirs: Vec<bool> = result.items.iter().map(|i| i.label.ends_with('/')).collect();
    assert_eq!(dirs, vec![true, true, false]);
}

#[test]
fn continues_completion_inside_quoted_paths() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("my folder/test.txt", "content"), ("my folder/other.txt", "content")]);
    let line = "\"my folder/\"";
    let v = values(&suggest(&plain_provider(&dir), &[line.to_string()], 0, line.len() - 1, true));
    assert!(v.contains(&"\"my folder/test.txt\"".to_string()));
    assert!(v.contains(&"\"my folder/other.txt\"".to_string()));
}

#[test]
fn applies_quoted_completion_without_duplicating_closing_quote() {
    let dir = TempDir::new("pi-autocomplete-");
    setup_folder(&dir.0, &[], &[("my folder/test.txt", "content")]);
    let provider = plain_provider(&dir);
    let line = "\"my folder/te\"";
    let cursor = line.len() - 1;
    let result = suggest(&provider, &[line.to_string()], 0, cursor, true).expect("suggestions");
    let item = result.items.iter().find(|e| e.value == "\"my folder/test.txt\"").expect("test.txt");
    let (applied, _) = apply_line(&provider, line, cursor, item, &result.prefix);
    assert_eq!(applied, "\"my folder/test.txt\"");
}

// ---------------------------------------------------------------------------
// slash-command filter (autocomplete-skill-slash.test.ts)
// ---------------------------------------------------------------------------

fn command(name: &str, description: &str) -> CommandEntry {
    CommandEntry::Command(SlashCommand {
        name: name.into(),
        description: Some(description.into()),
        ..Default::default()
    })
}

fn skill_commands() -> Vec<CommandEntry> {
    vec![
        command("skill:deep-research", "Multi-agent deep research"),
        command("skill:research-idea", "Refine a raw idea into a falsifiable seed"),
        command("skill:to-sidecar", "Route work to a sidecar"),
        command("skill:brainstorm", "Generate ideas"),
        command("model", "Select the active model"),
    ]
}

fn suggestions_for(prefix: &str) -> Vec<String> {
    let provider = CombinedAutocompleteProvider::new(skill_commands(), ".", None);
    let line = format!("/{prefix}");
    let result = suggest_line(&provider, &line, false);
    assert!(result.is_some(), "expected suggestions for {line:?}");
    values(&result)
}

#[test]
fn ranks_research_idea_first_for_idea() {
    let items = suggestions_for("idea");
    assert_eq!(items[0], "skill:research-idea");
    let pos = |name: &str| items.iter().position(|i| i == name);
    assert!(pos("skill:deep-research") > pos("skill:research-idea"));
}

#[test]
fn keeps_ordinary_slash_commands_matching() {
    assert!(suggestions_for("mod").contains(&"model".to_string()));
}

#[test]
fn completes_commands_after_leading_whitespace() {
    let provider = CombinedAutocompleteProvider::new(
        vec![CommandEntry::Command(SlashCommand { name: "model".into(), ..Default::default() })],
        ".",
        None,
    );
    for (line, expected) in [(" /", " /model "), ("  /mod", "  /model "), ("\t/mod", "\t/model ")] {
        let result = suggest_line(&provider, line, false).expect("suggestions");
        assert_eq!(result.prefix, line.trim_start());
        assert_eq!(values(&Some(result.clone())), vec!["model"]);
        let (applied, col) = apply_line(&provider, line, line.len(), &result.items[0], &result.prefix);
        assert_eq!(applied, expected);
        assert_eq!(col, expected.len());
    }
}

#[test]
fn completes_command_arguments_after_leading_whitespace() {
    let provider = CombinedAutocompleteProvider::new(
        vec![CommandEntry::Command(SlashCommand {
            name: "model".into(),
            get_argument_completions: Some(Arc::new(|prefix: &str| {
                assert_eq!(prefix, "son");
                ready(Some(vec![AutocompleteItem::new("sonnet", "sonnet")]))
            })),
            ..Default::default()
        })],
        ".",
        None,
    );
    let line = "  /model son";
    let result = suggest_line(&provider, line, false).expect("suggestions");
    assert_eq!(result.prefix, "son");
    let (applied, _) = apply_line(&provider, line, line.len(), &result.items[0], &result.prefix);
    assert_eq!(applied, "  /model sonnet");
}

#[test]
fn keeps_explicit_skill_queries_working() {
    assert!(suggestions_for("skill:side").contains(&"skill:to-sidecar".to_string()));
}

#[test]
fn lists_skills_while_typing_the_skill_prefix() {
    let items: Vec<String> = suggestions_for("skill").into_iter().filter(|i| i.starts_with("skill:")).collect();
    assert_eq!(items, vec!["skill:deep-research", "skill:research-idea", "skill:to-sidecar", "skill:brainstorm"]);
}

#[test]
fn keeps_fuzzy_skill_prefix_shorthand_working() {
    assert!(suggestions_for("skbra").contains(&"skill:brainstorm".to_string()));
}
