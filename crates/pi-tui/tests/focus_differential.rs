//! Differential test: overlay focus management versus the TypeScript reference. Random
//! sequences of capturing / non-capturing / conditionally visible overlays with focus,
//! unfocus (with targets), hide, set_hidden, set_focus, hide_overlay and keyboard input.
//! Each step compares focus flags, delivered input, handle state and rendered output.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use common::Harness;
use pi_tui::tui::{Component, ComponentRef, OverlayHandle, OverlayOptions, OverlayUnfocusOptions, SizeValue};
use serde_json::{Value, json};

struct F {
    label: String,
    focusable: bool,
    focused: AtomicBool,
    inputs: Mutex<String>,
}

impl Component for F {
    fn render(&self, _width: usize) -> Vec<String> {
        vec![self.label.clone()]
    }
    fn handles_input(&self) -> bool {
        true
    }
    fn handle_input(&self, data: &str) {
        self.inputs.lock().unwrap().push_str(data);
    }
    fn is_focusable(&self) -> bool {
        self.focusable
    }
    fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::SeqCst);
    }
    fn focused(&self) -> bool {
        self.focused.load(Ordering::SeqCst)
    }
}

struct Empty;
impl Component for Empty {
    fn render(&self, _width: usize) -> Vec<String> {
        Vec::new()
    }
}

fn comp(label: &str, focusable: bool) -> Arc<F> {
    Arc::new(F { label: label.into(), focusable, focused: AtomicBool::new(false), inputs: Mutex::new(String::new()) })
}

#[test]
fn overlay_focus_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/focus_cases.json")).unwrap();
    let mut failures = Vec::new();
    'cases: for (ci, case) in data.as_array().unwrap().iter().enumerate() {
        let h = Harness::new(30, 8);
        h.tui.add_child(Arc::new(Empty));
        let editor = comp("EDITOR", true);
        let mut comps: Vec<Arc<F>> = vec![editor.clone()];
        let mut handles: Vec<OverlayHandle> = Vec::new();
        let mut visible: Vec<Arc<AtomicBool>> = Vec::new();
        h.tui.set_focus(Some(editor.clone()));
        h.tui.start();
        h.wait_for_render();
        let target = |comps: &Vec<Arc<F>>, t: i64| -> Option<ComponentRef> {
            if t < 0 { None } else { Some(comps[t as usize].clone() as ComponentRef) }
        };
        for (si, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let hi = || step["h"].as_u64().unwrap() as usize;
            match step["op"].as_str().unwrap() {
                "show" => {
                    let k = handles.len();
                    let c = comp(&format!("OV{k}"), step["focusable"].as_bool().unwrap());
                    comps.push(c.clone());
                    let flag = Arc::new(AtomicBool::new(true));
                    visible.push(flag.clone());
                    let mut opts = OverlayOptions {
                        width: Some(SizeValue::Abs(8)),
                        row: Some(SizeValue::Abs(step["row"].as_i64().unwrap())),
                        col: Some(SizeValue::Abs(step["col"].as_i64().unwrap())),
                        non_capturing: step["nonCapturing"].as_bool().unwrap(),
                        ..Default::default()
                    };
                    if step["hasVisible"].as_bool().unwrap() {
                        opts.visible = Some(Arc::new(move |_, _| flag.load(Ordering::SeqCst)));
                    }
                    handles.push(h.tui.show_overlay(c, Some(opts)));
                }
                "input" => h.terminal.send_input(step["data"].as_str().unwrap()),
                "focus" => handles[hi()].focus(),
                "unfocus" => handles[hi()].unfocus(None),
                "unfocusTarget" => handles[hi()].unfocus(Some(OverlayUnfocusOptions { target: target(&comps, step["t"].as_i64().unwrap()) })),
                "hide" => handles[hi()].hide(),
                "setHidden" => handles[hi()].set_hidden(step["v"].as_bool().unwrap()),
                "setFocus" => h.tui.set_focus(target(&comps, step["t"].as_i64().unwrap())),
                "hideOverlay" => h.tui.hide_overlay(),
                "toggleVisible" => visible[hi()].store(step["v"].as_bool().unwrap(), Ordering::SeqCst),
                other => panic!("op {other}"),
            }
            h.terminal.clear_writes();
            h.tui.request_render(false);
            h.wait_for_render();
            let state = json!({
                "focused": comps.iter().map(|c| if c.focusable { json!(c.focused()) } else { Value::Null }).collect::<Vec<_>>(),
                "inputs": comps.iter().map(|c| c.inputs.lock().unwrap().clone()).collect::<Vec<_>>(),
                "handles": handles.iter().map(|hd| json!([hd.is_focused(), hd.is_hidden()])).collect::<Vec<_>>(),
            });
            let writes = h.terminal.get_writes();
            if state != step["state"] || writes != step["writes"].as_str().unwrap() {
                let ops: Vec<String> = case["steps"].as_array().unwrap()[..=si]
                    .iter()
                    .map(|s| {
                        let mut s = s.clone();
                        s.as_object_mut().unwrap().remove("writes");
                        s.as_object_mut().unwrap().remove("state");
                        s.to_string()
                    })
                    .collect();
                failures.push(format!(
                    "case {ci} step {si}\n  ops {}\n  state got  {state}\n  state want {}\n  writes got  {writes:?}\n  writes want {:?}",
                    ops.join(" "),
                    step["state"],
                    step["writes"]
                ));
                h.tui.stop(Default::default());
                continue 'cases;
            }
        }
        h.tui.stop(Default::default());
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(3);
        panic!("{n} cases diverged:\n{}", failures.join("\n\n"));
    }
}
