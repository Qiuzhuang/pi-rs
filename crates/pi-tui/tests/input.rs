//! Port of input.test.ts.

use std::sync::{Arc, Mutex};

use pi_tui::components::input::{Input, InputOptions};
use pi_tui::tui::Component;
use pi_tui::utils::{strip_terminal_sequences, visible_width};

fn keys(input: &Input, keys: &[&str]) {
    for k in keys {
        input.handle_input(k);
    }
}

fn typ(input: &Input, text: &str) {
    for c in text.chars() {
        input.handle_input(&c.to_string());
    }
}

const UNDO: &str = "\x1b[45;5u";
const RIGHT: &str = "\x1b[C";

#[test]
fn submits_value_including_backslash() {
    let input = Input::default();
    let submitted = Arc::new(Mutex::new(None));
    let s2 = submitted.clone();
    input.set_on_submit(Some(Arc::new(move |v| *s2.lock().unwrap() = Some(v))));
    typ(&input, "hello\\");
    input.handle_input("\r");
    assert_eq!(submitted.lock().unwrap().as_deref(), Some("hello\\"));
}

#[test]
fn inserts_backslash_as_regular_character() {
    let input = Input::default();
    keys(&input, &["\\", "x"]);
    assert_eq!(input.get_value(), "\\x");
}

#[test]
fn custom_prompt_and_styled_placeholder() {
    let input = Input::new(InputOptions {
        prompt: Some(String::new()),
        placeholder: Some("Find transcript".into()),
        placeholder_style: Some(Arc::new(|t: &str| format!("\x1b[2m{t}\x1b[22m"))),
    });
    input.set_focused(true);
    let empty = &input.render(20)[0];
    assert!(empty.contains("\x1b[2m"));
    assert_eq!(strip_terminal_sequences(empty).trim_end(), "Find transcript");
    input.handle_input("n");
    assert_eq!(strip_terminal_sequences(&input.render(20)[0]).trim_end(), "n");
}

#[test]
fn does_not_overflow_with_wide_text() {
    let width = 93;
    let cases = [
        "가나다라마바사아자차카타파하 한글 텍스트가 터미널 너비를 초과하면 크래시가 발생합니다 이것은 재현용 테스트입니다",
        "これはテスト文章です。日本語のテキストが正しく表示されるかどうかを確認するためのサンプルテキストです。あいうえお",
        "这是一段测试文本，用于验证中文字符在终端中的显示宽度是否被正确计算，如果不正确就会导致用户界面崩溃的问题",
        "ＡＢＣＤＥＦＧＨＩＪＫＬＭＮＯＰＱＲＳＴＵＶＷＸＹＺ０１２３４５６７８９ａｂｃｄｅｆｇｈｉｊｋｌｍ",
    ];
    for text in cases {
        for mv in 0..3 {
            let input = Input::default();
            input.set_value(text);
            input.set_focused(true);
            match mv {
                1 => {
                    for _ in 0..10 {
                        input.handle_input(RIGHT);
                    }
                }
                2 => input.handle_input("\x05"),
                _ => {}
            }
            let line = &input.render(width)[0];
            assert!(visible_width(line) <= width, "{text} {mv}");
        }
    }
}

#[test]
fn keeps_cursor_visible_when_scrolling_wide_text() {
    let input = Input::default();
    input.set_value("가나다라마바사아자차카타파하");
    input.set_focused(true);
    input.handle_input("\x01");
    for _ in 0..5 {
        input.handle_input(RIGHT);
    }
    assert!(visible_width(&input.render(20)[0]) <= 20);
}

#[test]
fn ctrl_w_saves_to_kill_ring_and_ctrl_y_yanks() {
    let input = Input::default();
    input.set_value("foo bar baz");
    keys(&input, &["\x05", "\x17"]);
    assert_eq!(input.get_value(), "foo bar ");
    keys(&input, &["\x01", "\x19"]);
    assert_eq!(input.get_value(), "bazfoo bar ");
}

#[test]
fn ctrl_w_preserves_ascii_punctuation_boundaries() {
    let input = Input::default();
    input.set_value("foo.bar");
    keys(&input, &["\x05", "\x17"]);
    assert_eq!(input.get_value(), "foo.");
    input.set_value("foo:bar");
    keys(&input, &["\x05", "\x17"]);
    assert_eq!(input.get_value(), "foo:");
}

#[test]
fn ctrl_w_handles_unicode_word_boundaries() {
    // "你好世界。你好，世界" segments as: 你好|世界|。|你好|，|世界
    let input = Input::default();
    input.set_value("你好世界。你好，世界");
    input.handle_input("\x05");
    for expected in ["你好世界。你好，", "你好世界。你好", "你好世界。", "你好世界", "你好", ""] {
        input.handle_input("\x17");
        assert_eq!(input.get_value(), expected);
    }
}

#[test]
fn ctrl_u_and_ctrl_k_save_to_kill_ring() {
    let input = Input::default();
    input.set_value("hello world");
    input.handle_input("\x01");
    for _ in 0..6 {
        input.handle_input(RIGHT);
    }
    input.handle_input("\x15");
    assert_eq!(input.get_value(), "world");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "hello world");

    let input = Input::default();
    input.set_value("hello world");
    keys(&input, &["\x01", "\x0b"]);
    assert_eq!(input.get_value(), "");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "hello world");
}

#[test]
fn ctrl_y_noop_when_empty() {
    let input = Input::default();
    input.set_value("test");
    keys(&input, &["\x05", "\x19"]);
    assert_eq!(input.get_value(), "test");
}

fn kill_words(input: &Input, words: &[&str]) {
    for w in words {
        input.set_value(*w);
        keys(input, &["\x05", "\x17"]);
    }
}

#[test]
fn alt_y_cycles_kill_ring() {
    let input = Input::default();
    kill_words(&input, &["first", "second", "third"]);
    assert_eq!(input.get_value(), "");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "third");
    for expected in ["second", "first", "third"] {
        input.handle_input("\x1by");
        assert_eq!(input.get_value(), expected);
    }
}

#[test]
fn alt_y_noop_without_yank_or_single_entry() {
    let input = Input::default();
    kill_words(&input, &["test"]);
    input.set_value("other");
    keys(&input, &["\x05", "x"]);
    assert_eq!(input.get_value(), "otherx");
    input.handle_input("\x1by");
    assert_eq!(input.get_value(), "otherx");

    let input = Input::default();
    kill_words(&input, &["only"]);
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "only");
    input.handle_input("\x1by");
    assert_eq!(input.get_value(), "only");
}

#[test]
fn consecutive_kills_accumulate() {
    let input = Input::default();
    input.set_value("one two three");
    keys(&input, &["\x05", "\x17", "\x17", "\x17"]);
    assert_eq!(input.get_value(), "");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "one two three");
}

#[test]
fn non_delete_actions_break_accumulation() {
    let input = Input::default();
    input.set_value("foo bar baz");
    keys(&input, &["\x05", "\x17"]);
    assert_eq!(input.get_value(), "foo bar ");
    input.handle_input("x");
    assert_eq!(input.get_value(), "foo bar x");
    input.handle_input("\x17");
    assert_eq!(input.get_value(), "foo bar ");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "foo bar x");
    input.handle_input("\x1by");
    assert_eq!(input.get_value(), "foo bar baz");
}

#[test]
fn non_yank_actions_break_yank_chain_and_rotation_persists() {
    let input = Input::default();
    kill_words(&input, &["first", "second"]);
    input.set_value("");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "second");
    input.handle_input("x");
    assert_eq!(input.get_value(), "secondx");
    input.handle_input("\x1by");
    assert_eq!(input.get_value(), "secondx");

    let input = Input::default();
    kill_words(&input, &["first", "second", "third"]);
    input.set_value("");
    keys(&input, &["\x19", "\x1by"]);
    assert_eq!(input.get_value(), "second");
    input.handle_input("x");
    input.set_value("");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "second");
}

#[test]
fn forward_deletions_append() {
    let input = Input::default();
    input.set_value("prefix|suffix");
    input.handle_input("\x01");
    for _ in 0..6 {
        input.handle_input(RIGHT);
    }
    input.handle_input("\x0b");
    assert_eq!(input.get_value(), "prefix");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "prefix|suffix");
}

#[test]
fn alt_d_deletes_word_forward() {
    let input = Input::default();
    input.set_value("hello world test");
    input.handle_input("\x01");
    input.handle_input("\x1bd");
    assert_eq!(input.get_value(), " world test");
    input.handle_input("\x1bd");
    assert_eq!(input.get_value(), " test");
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "hello world test");

    let input = Input::default();
    input.set_value("foo.bar baz");
    input.handle_input("\x01");
    for expected in [".bar baz", "bar baz", " baz"] {
        input.handle_input("\x1bd");
        assert_eq!(input.get_value(), expected);
    }
}

#[test]
fn alt_d_handles_unicode_word_boundaries() {
    let input = Input::default();
    input.set_value("你好世界。你好，世界");
    input.handle_input("\x01");
    for expected in ["世界。你好，世界", "。你好，世界", "你好，世界", "，世界", "世界"] {
        input.handle_input("\x1bd");
        assert_eq!(input.get_value(), expected);
    }
}

#[test]
fn yank_and_yank_pop_in_middle() {
    let input = Input::default();
    kill_words(&input, &["word"]);
    input.set_value("hello world");
    input.handle_input("\x01");
    for _ in 0..6 {
        input.handle_input(RIGHT);
    }
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "hello wordworld");

    let input = Input::default();
    kill_words(&input, &["FIRST", "SECOND"]);
    input.set_value("hello world");
    input.handle_input("\x01");
    for _ in 0..6 {
        input.handle_input(RIGHT);
    }
    input.handle_input("\x19");
    assert_eq!(input.get_value(), "hello SECONDworld");
    input.handle_input("\x1by");
    assert_eq!(input.get_value(), "hello FIRSTworld");
}

#[test]
fn undo_behaviors() {
    let input = Input::default();
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "");

    let input = Input::default();
    typ(&input, "hello world");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "hello");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "");

    let input = Input::default();
    typ(&input, "hello  ");
    for expected in ["hello ", "hello", ""] {
        input.handle_input(UNDO);
        assert_eq!(input.get_value(), expected);
    }

    let input = Input::default();
    typ(&input, "hello");
    input.handle_input("\x7f");
    assert_eq!(input.get_value(), "hell");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "hello");

    let input = Input::default();
    typ(&input, "hello");
    keys(&input, &["\x01", RIGHT, "\x1b[3~"]);
    assert_eq!(input.get_value(), "hllo");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "hello");

    let input = Input::default();
    typ(&input, "hello world");
    input.handle_input("\x17");
    assert_eq!(input.get_value(), "hello ");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "hello world");

    for (key, expected) in [("\x0b", "hello "), ("\x15", "world")] {
        let input = Input::default();
        typ(&input, "hello world");
        input.handle_input("\x01");
        for _ in 0..6 {
            input.handle_input(RIGHT);
        }
        input.handle_input(key);
        assert_eq!(input.get_value(), expected);
        input.handle_input(UNDO);
        assert_eq!(input.get_value(), "hello world");
    }

    let input = Input::default();
    typ(&input, "hello ");
    keys(&input, &["\x17", "\x19"]);
    assert_eq!(input.get_value(), "hello ");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "");

    let input = Input::default();
    input.set_value("hello world");
    input.handle_input("\x01");
    for _ in 0..5 {
        input.handle_input(RIGHT);
    }
    input.handle_input("\x1b[200~beep boop\x1b[201~");
    assert_eq!(input.get_value(), "hellobeep boop world");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "hello world");

    let input = Input::default();
    input.set_value("hello world");
    keys(&input, &["\x01", "\x1bd"]);
    assert_eq!(input.get_value(), " world");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "hello world");

    let input = Input::default();
    typ(&input, "abc");
    keys(&input, &["\x01", "\x05", "d", "e"]);
    assert_eq!(input.get_value(), "abcde");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "abc");
    input.handle_input(UNDO);
    assert_eq!(input.get_value(), "");
}
