//! Differential test: Markdown component output versus the TypeScript reference.

mod common;

use common::chalk;
use pi_tui::components::markdown::{DefaultTextStyle, Markdown, MarkdownOptions};
use pi_tui::terminal_image::{TerminalCapabilities, set_capabilities};
use pi_tui::tui::Component;
use serde_json::Value;

#[test]
fn markdown_render_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/markdown_render_cases.json")).unwrap();
    let mut failures = Vec::new();
    for case in data.as_array().unwrap() {
        let src = case["src"].as_str().unwrap();
        let width = case["width"].as_u64().unwrap() as usize;
        let style = match case["style"].as_str().unwrap() {
            "grayItalic" => Some(DefaultTextStyle { color: Some(chalk::style(&[chalk::GRAY])), italic: true, ..Default::default() }),
            "bg" => Some(DefaultTextStyle { bg_color: Some(chalk::style(&[chalk::BG_BLUE])), bold: true, ..Default::default() }),
            _ => None,
        };
        let preserve = case["preserve"].as_bool().unwrap();
        set_capabilities(TerminalCapabilities {
            images: None,
            true_color: true,
            hyperlinks: case["hyperlinks"].as_bool().unwrap(),
        });
        let md = Markdown::new(
            src,
            case["paddingX"].as_u64().unwrap() as usize,
            case["paddingY"].as_u64().unwrap() as usize,
            chalk::markdown_theme(),
            style,
            Some(MarkdownOptions {
                preserve_ordered_list_markers: preserve,
                preserve_backslash_escapes: preserve,
                render_latex: Some(case["renderLatex"].as_bool().unwrap()),
                transform: None,
            }),
        );
        let got = Value::from(md.render(width));
        if got != case["lines"] {
            let want = case["lines"].as_array().unwrap();
            let got = got.as_array().unwrap();
            let idx = (0..got.len().max(want.len())).find(|&i| got.get(i) != want.get(i)).unwrap();
            failures.push(format!(
                "{src:?} (width {width}, style {})\n  line {idx}: got  {:?}\n  line {idx}: want {:?}",
                case["style"],
                got.get(idx),
                want.get(idx)
            ));
        }
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(10);
        panic!("{n} cases diverged:\n{}", failures.join("\n"));
    }
}
