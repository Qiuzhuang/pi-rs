//! Differential test: overlay positioning and compositing on the main screen versus the
//! TypeScript reference (random content, overlay sizes, anchors, offsets and margins).

mod common;

use std::sync::Arc;

use common::{Harness, TestComponent};
use pi_tui::tui::{OverlayAnchor, OverlayMargin, OverlayOptions, SizeValue};
use serde_json::Value;


fn size(v: &Value) -> Option<SizeValue> {
    match v {
        Value::Number(n) => Some(SizeValue::Abs(n.as_i64().unwrap())),
        Value::String(s) => Some(SizeValue::Percent(s.clone())),
        _ => None,
    }
}

fn options(o: &Value) -> OverlayOptions {
    let anchor = o["anchor"].as_str().map(|a| match a {
        "center" => OverlayAnchor::Center,
        "top-left" => OverlayAnchor::TopLeft,
        "top-right" => OverlayAnchor::TopRight,
        "bottom-left" => OverlayAnchor::BottomLeft,
        "bottom-right" => OverlayAnchor::BottomRight,
        "top-center" => OverlayAnchor::TopCenter,
        "bottom-center" => OverlayAnchor::BottomCenter,
        "left-center" => OverlayAnchor::LeftCenter,
        "right-center" => OverlayAnchor::RightCenter,
        other => panic!("anchor {other}"),
    });
    let margin = match &o["margin"] {
        Value::Number(n) => Some(OverlayMargin::all(n.as_i64().unwrap())),
        Value::Object(m) => Some(OverlayMargin {
            top: m.get("top").and_then(Value::as_i64),
            right: m.get("right").and_then(Value::as_i64),
            bottom: m.get("bottom").and_then(Value::as_i64),
            left: m.get("left").and_then(Value::as_i64),
        }),
        _ => None,
    };
    OverlayOptions {
        width: size(&o["width"]),
        min_width: o["minWidth"].as_i64(),
        max_height: size(&o["maxHeight"]),
        anchor,
        offset_x: o["offsetX"].as_i64(),
        offset_y: o["offsetY"].as_i64(),
        row: size(&o["row"]),
        col: size(&o["col"]),
        margin,
        ..Default::default()
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn overlay_compositing_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/overlay_cases.json")).unwrap();
    let mut failures = Vec::new();
    for (i, case) in data.as_array().unwrap().iter().enumerate() {
        let cols = case["cols"].as_u64().unwrap() as usize;
        let rows = case["rows"].as_u64().unwrap() as usize;
        let h = Harness::new(cols, rows);
        let base = TestComponent::new(&[]);
        base.set_owned(strings(&case["base"]));
        h.tui.add_child(base);
        for o in case["overlays"].as_array().unwrap() {
            let overlay = TestComponent::new(&[]);
            overlay.set_owned(strings(&o["lines"]));
            h.tui.show_overlay(overlay as Arc<_>, Some(options(&o["options"])));
        }
        h.tui.start();
        h.tui.request_render(true);
        h.wait_for_render();
        // Compare the exact terminal output. (Viewports are not compared: the reference
        // emulator, xterm.js, uses Unicode 6 widths for emoji while the renderer does not.)
        let got = h.terminal.get_writes();
        let want = case["writes"].as_str().unwrap();
        if got != want {
            let g: Vec<char> = got.chars().collect();
            let w: Vec<char> = want.chars().collect();
            let pos = g.iter().zip(&w).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
            let ctx = |v: &[char]| v[pos.saturating_sub(30)..(pos + 30).min(v.len())].iter().collect::<String>();
            failures.push(format!(
                "case {i} ({cols}x{rows}) overlays {}\n  got  {:?}\n  want {:?}",
                case["overlays"],
                ctx(&g),
                ctx(&w)
            ));
        }
        h.tui.stop(Default::default());
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(6);
        panic!("{n} cases diverged:\n{}", failures.join("\n"));
    }
}
