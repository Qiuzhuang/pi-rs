//! Differential test: fullscreen renderer output versus the TypeScript reference for random
//! sequences of content changes, wheel scrolling, keyboard navigation, mouse selection,
//! word selection, transcript search, focus loss and resizes.

use std::sync::Arc;

use pi_tui::components::scroll_view::{ScrollFollow, ScrollView, ScrollViewOptions};
use pi_tui::components::stack::{StackChild, StackEntryOptions, StackOptions, VStack};
use pi_tui::components::text::Text;
use pi_tui::event_loop::EventLoop;
use pi_tui::layout_node::StackBasis;
use pi_tui::testing::VirtualTerminal;
use pi_tui::tui::{ComponentRef, Tui, TuiOptions};
use pi_tui::tui_alt_screen::TuiAltScreenOptions;
use serde_json::Value;

fn show(s: &str) -> String {
    s.chars().take(300).collect()
}

fn first_diff(got: &str, want: &str) -> String {
    let g: Vec<char> = got.chars().collect();
    let w: Vec<char> = want.chars().collect();
    let pos = g.iter().zip(&w).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
    let ctx = |v: &[char]| v[pos.saturating_sub(40)..(pos + 40).min(v.len())].iter().collect::<String>();
    format!("at char {pos}\n  got  {:?}\n  want {:?}", ctx(&g), ctx(&w))
}

fn content(v: &Value) -> String {
    v.as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n")
}

#[test]
fn alt_screen_rendering_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/alt_screen_cases.json")).unwrap();
    let mut failures = Vec::new();
    let only: Option<usize> = std::env::var("PI_CASE").ok().and_then(|v| v.parse().ok());
    'cases: for (ci, case) in data.as_array().unwrap().iter().enumerate() {
        if only.is_some_and(|o| o != ci) {
            continue;
        }
        let lp = EventLoop::new_manual();
        let _guard = lp.make_current();
        // Each reference step waits ~40ms, enough for a throttled follow-up render.
        let wait = || {
            lp.run_pending();
            lp.advance_ms(40);
            lp.run_pending();
        };
        let terminal = VirtualTerminal::new(case["cols"].as_u64().unwrap() as usize, case["rows"].as_u64().unwrap() as usize);
        let tui = Tui::alt_screen_with(
            Arc::new(terminal.clone()),
            TuiOptions { event_loop: Some(lp.clone()), ..Default::default() },
            TuiAltScreenOptions::default(),
        );
        let text = Arc::new(Text::new(content(&case["content"]), 0, 0, None));
        if case["explicit"].as_bool().unwrap() {
            let sv = ScrollView::new(
                text.clone(),
                ScrollViewOptions { follow: Some(ScrollFollow::End), primary: Some(true), ..Default::default() },
            );
            let footer: ComponentRef = Arc::new(Text::new("footer", 0, 0, None));
            tui.set_layout_root(Some(Arc::new(VStack::new(
                vec![
                    StackChild {
                        component: sv,
                        options: StackEntryOptions {
                            basis: Some(StackBasis::Size(0)),
                            grow: Some(1),
                            min_size: Some(1),
                            ..Default::default()
                        },
                    },
                    StackChild {
                        component: footer,
                        options: StackEntryOptions { basis: Some(StackBasis::Auto), min_size: Some(1), ..Default::default() },
                    },
                ],
                StackOptions::default(),
            ))));
        } else {
            tui.add_child(text.clone());
        }
        tui.start();
        wait();
        let init = terminal.get_writes();
        if init != case["initWrites"].as_str().unwrap() {
            failures.push(format!("case {ci} start {}", first_diff(&init, case["initWrites"].as_str().unwrap())));
            tui.stop(Default::default());
            continue;
        }
        for (si, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            terminal.clear_writes();
            match step["op"].as_str().unwrap() {
                "set" => {
                    text.set_text(content(&step["content"]));
                    tui.request_render(false);
                }
                "input" => {
                    // The reference terminal delivers input synchronously, so every chunk is
                    // handled before the coalesced render runs.
                    for d in step["data"].as_array().unwrap() {
                        terminal.send_input(d.as_str().unwrap());
                    }
                }
                "resize" => terminal.resize(step["cols"].as_u64().unwrap() as usize, step["rows"].as_u64().unwrap() as usize),
                other => panic!("op {other}"),
            }
            wait();
            let got = terminal.get_writes();
            let want = step["writes"].as_str().unwrap();
            if got != want {
                failures.push(format!("case {ci} step {si} {} {}", step["op"], step.get("data").map_or(String::new(), |d| d.to_string()))
                    + "\n"
                    + &first_diff(&got, want)
                    + &format!("\n  full got  {:?}\n  full want {:?}", show(&got), show(want)));
                tui.stop(Default::default());
                continue 'cases;
            }
        }
        terminal.clear_writes();
        tui.stop(Default::default());
        let stop = terminal.get_writes();
        if stop != case["stopWrites"].as_str().unwrap() {
            failures.push(format!("case {ci} stop {}", first_diff(&stop, case["stopWrites"].as_str().unwrap())));
        }
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(4);
        panic!("{n} cases diverged:\n{}", failures.join("\n\n"));
    }
}
