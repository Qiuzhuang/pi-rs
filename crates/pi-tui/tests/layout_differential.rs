//! Differential test: fullscreen layout engine versus the TypeScript reference for random
//! trees of vertical/horizontal stacks (basis, grow, shrink, min/max, gap, align), scroll
//! views (follow-end, scrollbars, scrolling) and wrapped text.

use std::sync::Arc;

use pi_tui::components::scroll_view::{ScrollFollow, ScrollView, ScrollViewOptions, ScrollViewScrollbar};
use pi_tui::components::stack::{HStack, StackChild, StackEntryOptions, StackOptions, VStack};
use pi_tui::components::text::Text;
use pi_tui::event_loop::EventLoop;
use pi_tui::layout::render_layout_frame;
use pi_tui::layout_node::{StackAlign, StackBasis};
use pi_tui::tui::ComponentRef;
use serde_json::{Value, json};

fn build(s: &Value, scrolls: &mut Vec<(Arc<ScrollView>, i64)>, lp: &EventLoop) -> ComponentRef {
    match s["t"].as_str().unwrap() {
        "text" => Arc::new(Text::new(
            s["text"].as_str().unwrap(),
            s["px"].as_u64().unwrap() as usize,
            s["py"].as_u64().unwrap() as usize,
            None,
        )),
        "scroll" => {
            let child = build(&s["child"], scrolls, lp);
            let sv = ScrollView::new(
                child,
                ScrollViewOptions {
                    follow: Some(if s["follow"] == "end" { ScrollFollow::End } else { ScrollFollow::None }),
                    primary: Some(s["primary"].as_bool().unwrap()),
                    scrollbar: s["scrollbar"].as_str().map(|v| match v {
                        "auto" => ScrollViewScrollbar::Auto,
                        "always" => ScrollViewScrollbar::Always,
                        _ => ScrollViewScrollbar::Hidden,
                    }),
                    event_loop: Some(lp.clone()),
                    ..Default::default()
                },
            );
            scrolls.push((sv.clone(), s["scroll"].as_i64().unwrap()));
            sv
        }
        t => {
            let entries: Vec<StackChild> = s["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| StackChild {
                    component: build(&e["child"], scrolls, lp),
                    options: StackEntryOptions {
                        basis: match &e["basis"] {
                            Value::String(_) => Some(StackBasis::Auto),
                            Value::Number(n) => Some(StackBasis::Size(n.as_u64().unwrap() as usize)),
                            _ => None,
                        },
                        grow: e["grow"].as_u64().map(|n| n as usize),
                        shrink: e["shrink"].as_u64().map(|n| n as usize),
                        min_size: e["minSize"].as_u64().map(|n| n as usize),
                        max_size: e["maxSize"].as_u64().map(|n| n as usize),
                        visible: None,
                    },
                })
                .collect();
            let options = StackOptions {
                gap: s["gap"].as_u64().map(|n| n as usize),
                align: s["align"].as_str().map(|a| match a {
                    "start" => StackAlign::Start,
                    "center" => StackAlign::Center,
                    "end" => StackAlign::End,
                    _ => StackAlign::Stretch,
                }),
            };
            if t == "v" { Arc::new(VStack::new(entries, options)) } else { Arc::new(HStack::new(entries, options)) }
        }
    }
}

#[test]
fn layout_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/layout_cases.json")).unwrap();
    let lp = EventLoop::new_manual();
    let _guard = lp.make_current();
    let mut failures = Vec::new();
    for (i, case) in data.as_array().unwrap().iter().enumerate() {
        let mut scrolls = Vec::new();
        let root = build(&case["spec"], &mut scrolls, &lp);
        let (w, h) = (case["w"].as_u64().unwrap() as usize, case["h"].as_u64().unwrap() as usize);
        let first = render_layout_frame(&root, w, h, Arc::new(|| {}));
        for (sv, d) in &scrolls {
            sv.scroll_by(*d);
        }
        let second = render_layout_frame(&root, w, h, Arc::new(|| {}));
        let tops: Vec<Value> = scrolls.iter().map(|(sv, _)| json!([sv.scroll_top(), sv.is_following_end()])).collect();
        for (name, got, want) in [
            ("first", json!(first.lines), &case["first"]),
            ("second", json!(second.lines), &case["second"]),
            ("scroll state", json!(tops), &case["tops"]),
        ] {
            if &got != want {
                failures.push(format!("case {i} {name} ({w}x{h}) spec {}\n  got  {got}\n  want {want}", case["spec"]));
                break;
            }
        }
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(4);
        panic!("{n} cases diverged:\n{}", failures.join("\n"));
    }
}
