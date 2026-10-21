//! Differential test: replays pseudo-random input sequences recorded from the
//! reference TypeScript Editor and compares text, cursor, render output and events.

mod common;

use std::sync::{Arc, Mutex};

use common::{Harness, chalk};
use pi_tui::components::editor::{Editor, EditorOptions};
use pi_tui::tui::Component;
use serde_json::Value;

#[test]
fn editor_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/editor_cases.json")).unwrap();
    let mut failures = Vec::new();
    for (ci, case) in data["cases"].as_array().unwrap().iter().enumerate() {
        let cols = case["cols"].as_u64().unwrap() as usize;
        let rows = case["rows"].as_u64().unwrap() as usize;
        let h = Harness::new(cols, rows);
        let padding_x = case["paddingX"].as_u64().unwrap() as usize;
        let editor = Editor::new(
            &h.tui,
            chalk::editor_theme(),
            EditorOptions { padding_x: Some(padding_x), autocomplete_max_visible: None },
        );
        editor.set_focused(case["focused"].as_bool().unwrap());
        for entry in case["history"].as_array().unwrap() {
            editor.add_to_history(entry.as_str().unwrap());
        }
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let ev = events.clone();
        editor.set_on_submit(Some(Arc::new(move |t: String| ev.lock().unwrap().push(format!("submit:{t}")))));
        let ev = events.clone();
        editor.set_on_change(Some(Arc::new(move |t: String| ev.lock().unwrap().push(format!("change:{t}")))));
        let mut history = Vec::new();
        for (si, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            events.lock().unwrap().clear();
            if let Some(t) = step.get("setText") {
                editor.set_text(t.as_str().unwrap());
                history.push(format!("setText({t})"));
            } else {
                let input = step["input"].as_str().unwrap();
                editor.handle_input(input);
                history.push(format!("{input:?}"));
            }
            let (line, col) = editor.get_cursor();
            let got = serde_json::json!({
                "text": editor.get_text(),
                "expanded": editor.get_expanded_text(),
                "cursor": [line, col],
                "render": editor.render(cols),
                "events": *events.lock().unwrap(),
            });
            let mut mismatches = Vec::new();
            for key in ["text", "expanded", "cursor", "render", "events"] {
                if got[key] != step[key] {
                    mismatches.push(format!("  {key}: got {}\n  {key}: want {}", got[key], step[key]));
                }
            }
            if !mismatches.is_empty() {
                failures.push(format!(
                    "case {ci} (cols {cols}, rows {rows}, padding {padding_x}) step {si}\n  inputs: {}\n{}",
                    history.join(" "),
                    mismatches.join("\n")
                ));
                break;
            }
        }
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(5);
        panic!("{n} cases diverged:\n{}", failures.join("\n\n"));
    }
}

#[test]
fn word_wrap_line_matches_typescript_reference() {
    use pi_tui::components::editor::word_wrap_line;
    let data: Value = serde_json::from_str(include_str!("fixtures/word_wrap_cases.json")).unwrap();
    let mut failures = Vec::new();
    for case in data.as_array().unwrap() {
        let text = case[0].as_str().unwrap();
        let width = case[1].as_u64().unwrap() as usize;
        let got: Vec<Value> = word_wrap_line(text, width, None)
            .into_iter()
            .map(|c| serde_json::json!([c.text, c.start_index, c.end_index]))
            .collect();
        if Value::from(got.clone()) != case[2] {
            failures.push(format!("wordWrapLine({text:?}, {width})\n  got  {got:?}\n  want {}", case[2]));
        }
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(8);
        panic!("{n} cases diverged:\n{}", failures.join("\n"));
    }
}
