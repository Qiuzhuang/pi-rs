//! Differential test: marked lexer tokens versus the TypeScript reference
//! (marked + marked-cjk-friendly + pi's strict strikethrough and LaTeX extensions).

use pi_tui::marked::{Align, MarkedOptions, Token, TokenType as T, lexer};
use serde_json::{Map, Value, json};

fn norm(t: &Token) -> Value {
    let mut o = Map::new();
    o.insert("type".into(), json!(t.kind.as_str()));
    o.insert("raw".into(), json!(t.raw));
    let has_text = matches!(
        t.kind,
        T::Code
            | T::Heading
            | T::Blockquote
            | T::ListItem
            | T::Html
            | T::Paragraph
            | T::Text
            | T::Escape
            | T::Link
            | T::Image
            | T::Strong
            | T::Em
            | T::Codespan
            | T::Del
            | T::Latex
            | T::LatexBlock
    );
    if has_text {
        o.insert("text".into(), json!(t.text));
    }
    if t.kind == T::Heading {
        o.insert("depth".into(), json!(t.depth));
    }
    if t.kind == T::List {
        o.insert("ordered".into(), json!(t.ordered));
    }
    if matches!(t.kind, T::List | T::ListItem) {
        o.insert("loose".into(), json!(t.loose));
    }
    if t.kind == T::ListItem {
        o.insert("task".into(), json!(t.task));
    }
    if let Some(c) = t.checked {
        o.insert("checked".into(), json!(c));
    }
    if t.pending {
        o.insert("pending".into(), json!(true));
    }
    if t.kind == T::Def {
        o.insert("tag".into(), json!(t.tag));
    }
    if matches!(t.kind, T::Link | T::Image | T::Def) {
        o.insert("href".into(), json!(t.href));
    }
    if let Some(title) = t.title.as_ref().filter(|s| !s.is_empty()) {
        o.insert("title".into(), json!(title));
    }
    if t.kind == T::Code
        && !t.indented
        && let Some(lang) = &t.lang
    {
        o.insert("lang".into(), json!(lang));
    }
    if t.kind == T::List {
        o.insert("start".into(), t.start.map_or(Value::Null, |s| json!(s)));
    }
    if !t.tokens.is_empty() {
        o.insert("tokens".into(), Value::Array(t.tokens.iter().map(norm).collect()));
    }
    if t.kind == T::List {
        o.insert("items".into(), Value::Array(t.items.iter().map(norm).collect()));
    }
    if t.kind == T::Table {
        let cell = |c: &pi_tui::marked::TableCell| json!({"text": c.text, "tokens": c.tokens.iter().map(norm).collect::<Vec<_>>()});
        o.insert("header".into(), Value::Array(t.header.iter().map(cell).collect()));
        o.insert("rows".into(), Value::Array(t.rows.iter().map(|r| Value::Array(r.iter().map(cell).collect())).collect()));
        o.insert(
            "align".into(),
            Value::Array(
                t.align
                    .iter()
                    .map(|a| match a {
                        Some(Align::Left) => json!("left"),
                        Some(Align::Center) => json!("center"),
                        Some(Align::Right) => json!("right"),
                        None => Value::Null,
                    })
                    .collect(),
            ),
        );
    }
    Value::Object(o)
}

/// First differing path between two JSON values.
fn diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                let (va, vb) = (x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null));
                if let Some(d) = diff(va, vb, &format!("{path}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            for i in 0..x.len().max(y.len()) {
                let (va, vb) = (x.get(i).unwrap_or(&Value::Null), y.get(i).unwrap_or(&Value::Null));
                if let Some(d) = diff(va, vb, &format!("{path}[{i}]")) {
                    return Some(d);
                }
            }
            None
        }
        _ if a == b => None,
        _ => Some(format!("{path}: got {a}, want {b}")),
    }
}

#[test]
fn lexer_matches_typescript_reference() {
    let data: Value = serde_json::from_str(include_str!("fixtures/marked_cases.json")).unwrap();
    let cases = data.as_array().unwrap();
    let mut failures = Vec::new();
    for case in cases {
        let src = case[0].as_str().unwrap().replace('\t', "   ");
        let got = Value::Array(lexer(&src, MarkedOptions::pi()).iter().map(norm).collect());
        if let Some(d) = diff(&got, &case[1], "") {
            failures.push(format!("{src:?}\n  {d}"));
        }
    }
    if !failures.is_empty() {
        let n = failures.len();
        failures.truncate(12);
        panic!("{n} of {} cases diverged:\n{}", cases.len(), failures.join("\n"));
    }
}

#[test]
fn sanity_tokens() {
    let toks = lexer("# Hi *there*\n\n- [ ] a\n- b\n\n| x | y |\n|---|---|\n| 1 | 2 |\n", MarkedOptions::pi());
    assert!(!toks.iter().map(norm).collect::<Vec<Value>>().is_empty());
    assert_eq!(toks[0].kind, T::Heading);
    assert_eq!(toks[0].tokens[1].kind, T::Em);
}
