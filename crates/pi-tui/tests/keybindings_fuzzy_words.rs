//! Ports of keybindings.test.ts, fuzzy.test.ts and word-navigation.test.ts.

use pi_tui::fuzzy::{fuzzy_filter, fuzzy_match};
use pi_tui::keybindings::{KeySpec, KeybindingConflict, KeybindingsManager, TUI_KEYBINDINGS};
use pi_tui::word_navigation::{Segment, WordNavigationOptions, find_word_backward, find_word_forward};

fn mgr(user: Vec<(&str, KeySpec)>) -> KeybindingsManager {
    KeybindingsManager::new(TUI_KEYBINDINGS.clone(), user.into_iter().map(|(k, v)| (k.to_string(), Some(v))).collect())
}

#[test]
fn binds_ctrl_j_as_newline_alias() {
    let k = mgr(vec![]);
    assert_eq!(k.get_keys("tui.input.newLine"), ["shift+enter", "ctrl+j"]);
    assert!(k.matches("\n", "tui.input.newLine"));
    assert!(k.matches("\x1b[106;5u", "tui.input.newLine"));
}

#[test]
fn binds_editor_viewport_navigation() {
    let k = mgr(vec![]);
    assert_eq!(k.get_keys("tui.editor.cursorLineStart"), ["home", "ctrl+a"]);
    assert_eq!(k.get_keys("tui.editor.cursorLineEnd"), ["end", "ctrl+e"]);
    assert_eq!(k.get_keys("tui.editor.pageUp"), ["pageUp", "ctrl+pageUp"]);
    assert_eq!(k.get_keys("tui.editor.pageDown"), ["pageDown", "ctrl+pageDown"]);
    assert!(k.get_keys("tui.editor.historyPrevious").is_empty());
    assert!(k.get_keys("tui.editor.historyNext").is_empty());
}

#[test]
fn binds_alt_screen_navigation() {
    let k = mgr(vec![]);
    let cases: [(&str, &[&str]); 14] = [
        ("tui.altScreen.pageUp", &["pageUp"]),
        ("tui.altScreen.pageDown", &["pageDown"]),
        ("tui.altScreen.halfPageUp", &[]),
        ("tui.altScreen.halfPageDown", &[]),
        ("tui.altScreen.lineUp", &[]),
        ("tui.altScreen.lineDown", &[]),
        ("tui.altScreen.previousPrompt", &["ctrl+shift+up", "ctrl+up"]),
        ("tui.altScreen.nextPrompt", &["ctrl+shift+down", "ctrl+down"]),
        ("tui.altScreen.search", &["ctrl+shift+f"]),
        ("tui.altScreen.searchNext", &["enter", "ctrl+g"]),
        ("tui.altScreen.searchPrevious", &["shift+enter", "ctrl+shift+g"]),
        ("tui.altScreen.searchClose", &["escape"]),
        ("tui.altScreen.top", &["ctrl+home"]),
        ("tui.altScreen.bottom", &["ctrl+end"]),
    ];
    for (id, keys) in cases {
        assert_eq!(k.get_keys(id), keys, "{id}");
    }
}

#[test]
fn user_overrides_do_not_evict_other_actions() {
    let k = mgr(vec![("tui.input.submit", vec!["enter", "ctrl+enter"].into())]);
    assert_eq!(k.get_keys("tui.input.submit"), ["enter", "ctrl+enter"]);
    assert_eq!(k.get_keys("tui.select.confirm"), ["enter"]);
    let k = mgr(vec![("tui.select.up", vec!["up", "ctrl+p"].into())]);
    assert_eq!(k.get_keys("tui.select.up"), ["up", "ctrl+p"]);
    assert_eq!(k.get_keys("tui.editor.cursorUp"), ["up"]);
}

#[test]
fn reports_direct_user_conflicts() {
    let k = mgr(vec![("tui.input.submit", "ctrl+x".into()), ("tui.select.confirm", "ctrl+x".into())]);
    assert_eq!(
        k.get_conflicts(),
        vec![KeybindingConflict {
            key: "ctrl+x".into(),
            keybindings: vec!["tui.input.submit".into(), "tui.select.confirm".into()]
        }]
    );
    assert_eq!(k.get_keys("tui.editor.cursorLeft"), ["left", "ctrl+b"]);
}

#[test]
fn fuzzy_match_basics() {
    let r = fuzzy_match("", "anything");
    assert!(r.matches && r.score == 0.0);
    assert!(!fuzzy_match("longquery", "short").matches);
    let r = fuzzy_match("test", "test");
    assert!(r.matches && r.score < 0.0);
    assert!(fuzzy_match("abc", "aXbXc").matches);
    assert!(!fuzzy_match("abc", "cba").matches);
    assert!(fuzzy_match("ABC", "abc").matches);
    assert!(fuzzy_match("abc", "ABC").matches);
    assert!(fuzzy_match("foo", "foobar").score < fuzzy_match("foo", "f_o_o_bar").score);
    assert!(fuzzy_match("fb", "foo-bar").score < fuzzy_match("fb", "afbx").score);
    assert!(fuzzy_match("codex52", "gpt-5.2-codex").matches);
}

#[test]
fn fuzzy_filter_basics() {
    let items = vec!["apple", "banana", "cherry"];
    assert_eq!(fuzzy_filter(&items, "", |x| x.to_string()), items);
    let r = fuzzy_filter(&items, "an", |x| x.to_string());
    assert!(r.contains(&"banana") && !r.contains(&"apple") && !r.contains(&"cherry"));
    assert_eq!(fuzzy_filter(&["a_p_p", "app", "application"], "app", |x| x.to_string())[0], "app");
    assert_eq!(fuzzy_filter(&["clone", "cl"], "cl", |x| x.to_string()), ["cl", "clone"]);
    let items = vec![("foo", 1), ("bar", 2), ("foobar", 3)];
    let r = fuzzy_filter(&items, "foo", |x| x.0.to_string());
    assert_eq!(r.len(), 2);
    let item = ("gpt-5.5", "openai-codex");
    assert_eq!(fuzzy_filter(&[item], "openai-codex/gpt-5.5", |m| format!("{} {}", m.0, m.1)), [item]);
}

fn back(text: &str, c: usize) -> usize {
    find_word_backward(text, c, &WordNavigationOptions::default())
}
fn fwd(text: &str, c: usize) -> usize {
    find_word_forward(text, c, &WordNavigationOptions::default())
}

#[test]
fn word_backward() {
    assert_eq!(back("hello world", 11), 6);
    assert_eq!(back("hello world", 6), 0);
    for t in ["foo.bar", "foo:bar"] {
        assert_eq!(back(t, 7), 4);
        assert_eq!(back(t, 4), 3);
        assert_eq!(back(t, 3), 0);
    }
    let t = "path/to/file";
    assert_eq!(back(t, 12), 8);
    assert_eq!(back(t, 8), 7);
    assert_eq!(back(t, 7), 5);
    assert_eq!(back(t, 5), 4);
    assert_eq!(back(t, 4), 0);
    // CJK: dictionary segmentation gives 你好|世界 (byte offsets).
    let t = "你好世界 test";
    assert_eq!(back(t, t.len()), 13);
    assert_eq!(back(t, 13), 6);
    assert_eq!(back(t, 6), 0);
    assert_eq!(back("  hello  ", 9), 2);
    assert_eq!(back("  hello  ", 2), 0);
    assert_eq!(back("foo...bar", 9), 6);
    assert_eq!(back("foo...bar", 6), 3);
    assert_eq!(back("foo...bar", 3), 0);
    assert_eq!(back("hello", 0), 0);
}

#[test]
fn word_forward() {
    assert_eq!(fwd("hello world", 0), 5);
    assert_eq!(fwd("hello world", 5), 11);
    for t in ["foo.bar", "foo:bar"] {
        assert_eq!(fwd(t, 0), 3);
        assert_eq!(fwd(t, 3), 4);
        assert_eq!(fwd(t, 4), 7);
    }
    let t = "path/to/file";
    assert_eq!(fwd(t, 0), 4);
    assert_eq!(fwd(t, 4), 5);
    assert_eq!(fwd(t, 5), 7);
    assert_eq!(fwd(t, 7), 8);
    assert_eq!(fwd(t, 8), 12);
    let t = "你好世界 test";
    let first = fwd(t, 0);
    assert!(first > 0 && first <= 12);
    let mut pos = 0;
    while pos < t.len() {
        let next = fwd(t, pos);
        if next == pos {
            break;
        }
        pos = next;
    }
    assert_eq!(pos, t.len());
    assert_eq!(fwd("  hello  ", 0), 7);
    assert_eq!(fwd("  hello  ", 7), 9);
    assert_eq!(fwd("foo...bar", 0), 3);
    assert_eq!(fwd("foo...bar", 3), 6);
    assert_eq!(fwd("foo...bar", 6), 9);
    assert_eq!(fwd("hello", 5), 5);
}

#[test]
fn atomic_segments() {
    let marker = "[paste #1 +5 lines]";
    let text = format!("hello {marker} world");
    let seg = |s: &str, w: bool| Segment { segment: s.to_string(), is_word_like: w };
    let text2 = text.clone();
    let segment = move |input: &str| -> Vec<Segment> {
        if input == text2 {
            vec![seg("hello", true), seg(" ", false), seg(marker, true), seg(" ", false), seg("world", true)]
        } else if input == &text2[..26] {
            vec![seg("hello", true), seg(" ", false), seg(marker, true), seg(" ", false)]
        } else if input == &text2[6..] {
            vec![seg(marker, true), seg(" ", false), seg("world", true)]
        } else {
            vec![]
        }
    };
    let is_atomic = move |s: &str| s == marker;
    let opts = WordNavigationOptions { segment: Some(&segment), is_atomic_segment: Some(&is_atomic) };
    assert_eq!(find_word_backward(&text, text.len(), &opts), 26);
    assert_eq!(find_word_backward(&text, 26, &opts), 6);
    assert_eq!(find_word_forward(&text, 6, &opts), 6 + marker.len());
}
