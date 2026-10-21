//! Differential test: main-screen differential rendering versus the TypeScript reference.
//! Random sequences of content changes, resizes, forced redraws, cursor markers and
//! renderer settings; every step's terminal output must match byte for byte.

mod common;

use std::sync::{Arc, Mutex};

use common::Harness;
use pi_tui::tui::Component;
use pi_tui::utils::truncate_to_width;
use serde_json::Value;

#[derive(Default)]
struct Static {
    lines: Mutex<Vec<String>>,
}

impl Component for Static {
    fn render(&self, width: usize) -> Vec<String> {
        self.lines.lock().unwrap().iter().map(|l| truncate_to_width(l, width, "", false)).collect()
    }
}

fn show(s: &str) -> String {
    s.chars().take(400).collect()
}

#[test]
fn main_screen_rendering_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/render_cases.json")).unwrap();
    let mut failures = Vec::new();
    'cases: for (ci, case) in data.as_array().unwrap().iter().enumerate() {
        let h = Harness::new(case["cols"].as_u64().unwrap() as usize, case["rows"].as_u64().unwrap() as usize);
        let comp = Arc::new(Static::default());
        h.tui.add_child(comp.clone());
        h.tui.start();
        h.wait_for_render();
        let init = h.terminal.get_writes();
        if init != case["initWrites"].as_str().unwrap() {
            failures.push(format!("case {ci} start\n  got  {:?}\n  want {:?}", show(&init), show(case["initWrites"].as_str().unwrap())));
            continue;
        }
        for (si, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            h.terminal.clear_writes();
            match step["op"].as_str().unwrap() {
                "set" => {
                    *comp.lines.lock().unwrap() =
                        step["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
                    h.tui.request_render(false);
                }
                "resize" => {
                    h.terminal.resize(step["cols"].as_u64().unwrap() as usize, step["rows"].as_u64().unwrap() as usize)
                }
                "clearOnShrink" => {
                    h.tui.set_clear_on_shrink(step["value"].as_bool().unwrap());
                    h.tui.request_render(false);
                }
                "hwCursor" => h.tui.set_show_hardware_cursor(step["value"].as_bool().unwrap()),
                "force" => h.tui.request_render(true),
                other => panic!("op {other}"),
            }
            h.wait_for_render();
            let got = h.terminal.get_writes();
            let want = step["writes"].as_str().unwrap();
            if got != want {
                failures.push(format!("case {ci} step {si} ({})\n  got  {:?}\n  want {:?}", step["op"], show(&got), show(want)));
                continue 'cases;
            }
        }
        h.tui.stop(Default::default());
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(5);
        panic!("{n} cases diverged:\n{}", failures.join("\n"));
    }
}
