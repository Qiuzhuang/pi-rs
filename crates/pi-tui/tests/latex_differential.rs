//! Differential test: LaTeX rendering versus the TypeScript reference.

use pi_tui::latex::{RenderLatexOptions, render_latex};
use serde_json::Value;

#[test]
fn render_latex_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/latex_cases.json")).unwrap();
    let mut failures = Vec::new();
    for case in data.as_array().unwrap() {
        let src = case[0].as_str().unwrap();
        for (display, want) in [(false, &case[1]), (true, &case[2])] {
            let got = render_latex(src, RenderLatexOptions { display }).map_or(Value::Null, Value::from);
            if &got != want {
                failures.push(format!("render_latex({src:?}, display={display})\n  got  {got}\n  want {want}"));
            }
        }
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(10);
        panic!("{n} cases diverged:\n{}", failures.join("\n"));
    }
}
