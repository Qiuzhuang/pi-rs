//! Source text to diagram model (grok-mermaid `parse.ts`).
//!
//! Every `parse_x` returns `None` when the source is not that kind of diagram,
//! or when it exceeds a cap.

use super::graph::{
    ClassInfo, Edge, Graph, Head, LineKind, MAX_EDGES, MAX_GROUP_DEPTH, MAX_GROUPS, MAX_MEMBERS, MAX_NODES, Shape,
    parse_dir,
};
use super::labels::{ascii_lower, clean_label, decode_html_entities, is_id_char, split_ws, src_lines};
use pi_tui::utils::{is_whitespace_char, js_trim, js_trim_end, js_trim_start};
use std::collections::HashMap;

// ---------------------------------------------------------------- statements

/// Split one source line into statements on `;`, stopping at a `%%` comment.
/// Quoted spans are opaque, so a label may contain `;` and `%%`.
fn split_statements(line: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut cur = String::new();
    let mut in_quotes = false;
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let trimmed = js_trim(cur);
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        cur.clear();
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                in_quotes = false;
            }
            cur.push(c);
        } else if c == '"' {
            in_quotes = true;
            cur.push(c);
        } else if c == '%' && chars.get(i + 1) == Some(&'%') {
            break;
        } else if c == ';' {
            flush(&mut cur, out);
        } else {
            cur.push(c);
        }
        i += 1;
    }
    flush(&mut cur, out);
}

/// All statements in a source block, in order.
pub(crate) fn statements_of(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src_lines(src) {
        split_statements(&line, &mut out);
    }
    out
}

fn first_word(s: &str) -> &str {
    split_ws(s).first().copied().unwrap_or("")
}

/// Split on the first occurrence of `sep`, Rust's `split_once`.
fn split_once<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    s.split_once(sep)
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// Diagram kind from the header statement, lowercased.
fn header_kind(statements: &[String]) -> Option<String> {
    let header = statements.first()?;
    let kind = first_word(header);
    (!kind.is_empty()).then(|| ascii_lower(kind))
}

/// A diagram type this renderer draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagramKind {
    Flowchart,
    State,
    Class,
    Er,
    Sequence,
}

/// The kind of diagram `src` declares, or `None` if its header names no type this renderer draws.
pub(crate) fn diagram_kind(src: &str) -> Option<DiagramKind> {
    let kind = header_kind(&statements_of(src))?;
    if kind == "graph" || kind == "flowchart" {
        return Some(DiagramKind::Flowchart);
    }
    if kind.starts_with("statediagram") {
        return Some(DiagramKind::State);
    }
    if kind.starts_with("classdiagram") {
        return Some(DiagramKind::Class);
    }
    if kind == "erdiagram" {
        return Some(DiagramKind::Er);
    }
    if kind == "sequencediagram" {
        return Some(DiagramKind::Sequence);
    }
    None
}

// ----------------------------------------------------------------- flowchart

pub(crate) fn parse_graph(src: &str) -> Option<Graph> {
    let statements = statements_of(src);
    let kind = header_kind(&statements);
    if kind.as_deref() != Some("graph") && kind.as_deref() != Some("flowchart") {
        return None;
    }

    let mut graph = Graph::new(parse_dir(split_ws(&statements[0]).get(1).copied().unwrap_or("TB")));
    let mut stack: Vec<usize> = Vec::new();

    for st in &statements[1..] {
        match ascii_lower(first_word(st)).as_str() {
            "subgraph" => {
                if graph.groups.len() >= MAX_GROUPS || stack.len() >= MAX_GROUP_DEPTH {
                    return None;
                }
                let (id, label) = parse_subgraph_decl(js_trim(&st["subgraph".len()..]));
                graph.groups.push(super::graph::Group { id, label, parent: stack.last().copied() });
                stack.push(graph.groups.len() - 1);
                graph.cur_group = stack.last().copied();
                continue;
            }
            "end" => {
                stack.pop();
                graph.cur_group = stack.last().copied();
                continue;
            }
            "classdef" | "class" | "style" | "linkstyle" | "click" | "direction" => continue,
            _ => {}
        }
        parse_statement(st, &mut graph);
        if graph.over_cap {
            return None;
        }
    }

    if graph.nodes.is_empty() { None } else { Some(graph) }
}

/// `subgraph id[Title]`, `subgraph "Title"`, or a bare title.
fn parse_subgraph_decl(rest: &str) -> (String, String) {
    if rest.starts_with('"')
        && let Some(close) = rest[1..].find('"').map(|i| i + 1)
    {
        let label = &rest[1..close];
        return (label.to_string(), decode_html_entities(label));
    }
    if let Some(open) = rest.find('[') {
        let id = js_trim(&rest[..open]);
        let label = clean_label(js_trim(rest[open + 1..].trim_end_matches(']')));
        if !id.is_empty() && !label.is_empty() {
            return (id.to_string(), label);
        }
    }
    (rest.to_string(), rest.to_string())
}

fn collect(chars: &[char], from: usize, to: usize) -> String {
    chars[from.min(chars.len())..to.min(chars.len())].iter().collect()
}

/// A chain of `node link node link node ...`, each link fanning out over `&`.
fn parse_statement(st: &str, graph: &mut Graph) {
    let chars: Vec<char> = st.chars().collect();

    let Some((head_group, head_next)) = parse_node_group(&chars, 0, graph) else {
        graph.warnings.push(format!("dropped, does not start with a node: \"{st}\""));
        return;
    };
    let mut prev = head_group;
    let mut i = head_next;

    loop {
        i = skip_spaces(&chars, i);
        if i >= chars.len() {
            break;
        }
        let Some(link) = parse_link(&chars, i) else {
            graph.warnings.push(format!("dropped, expected a link: \"{}\"", collect(&chars, i, chars.len())));
            break;
        };
        i = skip_spaces(&chars, link.next);
        let Some((target_group, target_next)) = parse_node_group(&chars, i, graph) else {
            graph.warnings.push(format!("dropped, link has no target: \"{st}\""));
            break;
        };
        i = target_next;
        for &f in &prev {
            for &t in &target_group {
                // `A <-- B` reads right-to-left: swap the endpoints.
                let reversed = link.left == Head::Arrow && link.right != Head::Arrow;
                let pushed = graph.push_edge(Edge {
                    from: if reversed { t } else { f },
                    to: if reversed { f } else { t },
                    label: link.label.clone(),
                    head_to: if reversed { Head::Arrow } else { link.right },
                    head_from: if reversed { link.right } else { link.left },
                    line: link.line,
                });
                if !pushed {
                    return;
                }
            }
        }
        prev = target_group;
    }
}

/// One or more nodes joined by `&`, which fan out into a cross product.
fn parse_node_group(chars: &[char], start: usize, graph: &mut Graph) -> Option<(Vec<usize>, usize)> {
    let (first, first_next) = parse_node(chars, start, graph)?;
    let mut group = vec![first];
    let mut i = first_next;
    loop {
        let j = skip_spaces(chars, i);
        if chars.get(j) != Some(&'&') {
            break;
        }
        let (next, next_next) = parse_node(chars, j + 1, graph)?;
        group.push(next);
        i = next_next;
    }
    Some((group, i))
}

fn skip_spaces(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    i
}

fn parse_node(chars: &[char], start: usize, graph: &mut Graph) -> Option<(usize, usize)> {
    let mut i = skip_spaces(chars, start);
    let id_start = i;
    while i < chars.len() && is_id_char(chars[i]) {
        i += 1;
    }
    if i == id_start {
        return None;
    }
    let id = collect(chars, id_start, i);

    let shaped = read_shape_at(chars, i);
    if let Some(unclosed) = shaped.unclosed {
        graph.warnings.push(format!("node \"{id}\": label is missing its closing `{unclosed}`"));
    }
    let index = graph.node_index(&id, shaped.label, shaped.shape)?;

    // `id:::name` (after any shape) attaches a style class; swallow it.
    let mut next = shaped.after;
    if chars.get(next) == Some(&':') && chars.get(next + 1) == Some(&':') && chars.get(next + 2) == Some(&':') {
        let mut k = next + 3;
        while k < chars.len() && (is_id_char(chars[k]) || chars[k] == '-') {
            k += 1;
        }
        // A name never ends in `-`: back off so `A:::x-->B` keeps its link.
        while k > next + 3 && chars[k - 1] == '-' {
            k -= 1;
        }
        if k > next + 3 {
            next = k;
        }
    }
    Some((index, next))
}

/// What a shape bracket yielded.
struct Shaped {
    shape: Shape,
    label: Option<String>,
    after: usize,
    /// The closing token that was expected but never found.
    unclosed: Option<&'static str>,
}

/// Dispatch on the bracket following an id to pick shape and closing token.
fn read_shape_at(chars: &[char], i: usize) -> Shaped {
    let c = chars.get(i).copied();
    let n = chars.get(i + 1).copied();
    match c {
        Some('[') => match n {
            Some('[') => read_shape(chars, i + 2, "]]", Shape::Rect),
            Some('(') => read_shape(chars, i + 2, ")]", Shape::Round),
            _ => read_shape(chars, i + 1, "]", Shape::Rect),
        },
        Some('(') => match n {
            Some('(') => read_shape(chars, i + 2, "))", Shape::Round),
            Some('[') => read_shape(chars, i + 2, "])", Shape::Round),
            _ => read_shape(chars, i + 1, ")", Shape::Round),
        },
        Some('{') => match n {
            Some('{') => read_shape(chars, i + 2, "}}", Shape::Diamond),
            _ => read_shape(chars, i + 1, "}", Shape::Diamond),
        },
        Some('>') => read_shape(chars, i + 1, "]", Shape::Rect),
        _ => Shaped { shape: Shape::Rect, label: None, after: i, unclosed: None },
    }
}

/// Read label text up to `closer`.
fn read_shape(chars: &[char], start: usize, closer: &'static str, shape: Shape) -> Shaped {
    let closer_chars: Vec<char> = closer.chars().collect();
    let mut j = start;
    while chars.get(j) == Some(&' ') || chars.get(j) == Some(&'\t') {
        j += 1;
    }
    let quoted = chars.get(j) == Some(&'"');

    let mut i = start;
    let mut text = String::new();
    let mut in_quotes = false;
    while i < chars.len() {
        let c = chars[i];
        if quoted && c == '"' {
            in_quotes = !in_quotes;
            text.push(c);
            i += 1;
            continue;
        }
        if !in_quotes && chars[i..].starts_with(&closer_chars) {
            return Shaped { shape, label: Some(clean_label(&text)), after: i + closer_chars.len(), unclosed: None };
        }
        text.push(c);
        i += 1;
    }
    // Ran off the end still looking for the closer.
    Shaped { shape, label: Some(clean_label(&text)), after: chars.len(), unclosed: Some(closer) }
}

fn is_link_char(c: char) -> bool {
    matches!(c, '-' | '.' | '=' | '<' | '>')
}

struct Link {
    left: Head,
    right: Head,
    line: LineKind,
    label: Option<String>,
    next: usize,
}

/// Read a link operator and its label.
fn parse_link(chars: &[char], start: usize) -> Option<Link> {
    let mut i = skip_spaces(chars, start);
    let mut left = Head::None;
    // A leading `o`/`x` decorates the tail, but only directly before an operator.
    if matches!(chars.get(i), Some('o') | Some('x')) && matches!(chars.get(i + 1), Some('-') | Some('.') | Some('=')) {
        left = if chars[i] == 'o' { Head::Circle } else { Head::Cross };
        i += 1;
    }

    let op_start = i;
    while i < chars.len() && is_link_char(chars[i]) {
        i += 1;
    }
    if i == op_start {
        return None;
    }
    let op1 = collect(chars, op_start, i);
    if left == Head::None && op1.starts_with('<') {
        left = Head::Arrow;
    }

    let mut line = line_kind(&op1);
    let mut right = if op1.contains('>') { Head::Arrow } else { Head::None };
    if right == Head::None
        && let Some((head, next)) = trailing_head(chars, i)
    {
        right = head;
        i = next;
    }

    if chars.get(i) == Some(&'|') {
        i += 1;
        let l_start = i;
        while i < chars.len() && chars[i] != '|' {
            i += 1;
        }
        let label = clean_label(&collect(chars, l_start, i));
        if chars.get(i) == Some(&'|') {
            i += 1;
        }
        return Some(Link { left, right, line, label: non_empty(label), next: i });
    }

    if right == Head::None {
        let text_start = skip_spaces(chars, i);
        let mut j = text_start;
        while j < chars.len() && !is_link_char(chars[j]) {
            j += 1;
        }
        if j < chars.len() && j > text_start && chars[j] != '<' {
            let text = collect(chars, text_start, j);
            let op2_start = j;
            while j < chars.len() && is_link_char(chars[j]) {
                j += 1;
            }
            let op2 = collect(chars, op2_start, j);
            if op2.contains('>') {
                right = Head::Arrow;
            } else if let Some((head, next)) = trailing_head(chars, j) {
                right = head;
                j = next;
            }
            if line == LineKind::Solid {
                line = line_kind(&op2);
            }
            return Some(Link { left, right, line, label: non_empty(clean_label(&text)), next: j });
        }
    }

    Some(Link { left, right, line, label: None, next: i })
}

fn line_kind(op: &str) -> LineKind {
    if op.contains('=') {
        LineKind::Thick
    } else if op.contains('.') {
        LineKind::Dotted
    } else {
        LineKind::Solid
    }
}

/// A trailing `o`/`x` head, only when followed by a statement boundary.
fn trailing_head(chars: &[char], i: usize) -> Option<(Head, usize)> {
    let head = match chars.get(i) {
        Some('o') => Head::Circle,
        Some('x') => Head::Cross,
        _ => return None,
    };
    let boundary = matches!(chars.get(i + 1), None | Some(' ') | Some('\t') | Some('|') | Some('&') | Some(';'));
    boundary.then_some((head, i + 1))
}

// --------------------------------------------------------------------- state

pub(crate) fn parse_state(src: &str) -> Option<Graph> {
    let statements = statements_of(src);
    let kind = header_kind(&statements)?;
    if !kind.starts_with("statediagram") {
        return None;
    }

    let mut graph = Graph::new(super::graph::Dir::Down);
    let mut in_note = false;

    for st in &statements[1..] {
        if in_note {
            if ascii_lower(st) == "end note" {
                in_note = false;
            }
            continue;
        }
        let st = drop_style_tags(st);
        let first = ascii_lower(first_word(&st));
        if first == "direction" {
            graph.dir = parse_dir(split_ws(&st).get(1).copied().unwrap_or(""));
        } else if first == "note" {
            // A single-line `note ... : text` needs no terminator.
            if !st.contains(':') {
                in_note = true;
            }
        } else if first == "state" {
            parse_state_decl(&st, &mut graph)?;
        } else if ["classdef", "class", "hide", "scale", "}", "--"].contains(&first.as_str()) {
            // Styling and composite-state punctuation carry no layout meaning.
        } else if st.contains("-->") {
            parse_transition(&st, &mut graph)?;
        } else {
            parse_state_desc(&st, &mut graph)?;
        }
        if graph.over_cap {
            return None;
        }
    }

    if graph.nodes.is_empty() { None } else { Some(graph) }
}

/// `state "Label" as id`, `state id <<choice>>`, or `state id {`.
fn parse_state_decl(st: &str, graph: &mut Graph) -> Option<()> {
    let rest = js_trim(&st["state".len()..]);
    let rest = js_trim(rest.strip_suffix('{').unwrap_or(rest));
    if rest.is_empty() {
        return Some(());
    }

    if rest.starts_with('"') {
        let close = rest[1..].find('"')? + 1;
        let label = &rest[1..close];
        let after = js_trim(&rest[close + 1..]);
        let id = if let Some(stripped) = after.strip_prefix("as") { js_trim(stripped) } else { label };
        return graph.node_label(id, decode_html_entities(label)).map(|_| ());
    }

    let mut shape = Shape::Round;
    let mut id = rest;
    let mut stereotyped = false;
    if let Some(pos) = rest.find("<<") {
        let after = &rest[pos + 2..];
        let stereo = js_trim(after.strip_suffix(">>").unwrap_or(after));
        if stereo == "choice" {
            shape = Shape::Diamond;
        }
        id = js_trim(&rest[..pos]);
        stereotyped = true;
    }
    if id.is_empty() || is_whitespace_char(id) {
        return None;
    }
    graph.node_index(id, stereotyped.then(|| id.to_string()), shape).map(|_| ())
}

/// `A --> B: label`, including chains `A --> B --> C`.
fn parse_transition(st: &str, graph: &mut Graph) -> Option<()> {
    let mut rest = st.to_string();
    let mut prev: Option<usize> = None;

    loop {
        let Some((lhs, rhs)) = split_once(&rest, "-->") else { break };
        let (lhs, rhs) = (lhs.to_string(), rhs.to_string());

        let from_id = js_trim(js_trim_end(&lhs).trim_end_matches('-'));
        let from = match prev {
            // Mid-chain: the source is the previous target, so nothing may precede.
            Some(prev) => {
                if !from_id.is_empty() {
                    return None;
                }
                prev
            }
            None => {
                if from_id.is_empty() {
                    return None;
                }
                state_endpoint(graph, from_id, true)?
            }
        };

        let next_arrow = rhs.find("-->");
        let to_part_raw = match next_arrow {
            Some(p) => &rhs[..p],
            None => &rhs[..],
        };
        let tail = match next_arrow {
            Some(p) => rhs[p..].to_string(),
            None => String::new(),
        };

        let colon = split_once(to_part_raw, ":");
        let to_part = colon.map_or(to_part_raw, |c| c.0);
        let label = colon.and_then(|c| non_empty(decode_html_entities(js_trim(c.1))));

        let to_id = js_trim(js_trim_end(js_trim_start(to_part).trim_start_matches('>')).trim_end_matches('-'));
        if to_id.is_empty() {
            return None;
        }
        let to = state_endpoint(graph, to_id, false)?;

        if !graph.push_edge(Edge {
            from,
            to,
            label,
            head_to: Head::Arrow,
            head_from: Head::None,
            line: LineKind::Solid,
        }) {
            return Some(());
        }
        prev = Some(to);
        rest = tail;
    }
    Some(())
}

/// Remove every `:::name` style tag from a statement.
fn drop_style_tags(st: &str) -> String {
    let chars: Vec<char> = st.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ':' && chars.get(i + 1) == Some(&':') && chars.get(i + 2) == Some(&':') {
            let mut k = i + 3;
            while k < chars.len() && (is_id_char(chars[k]) || chars[k] == '-') {
                k += 1;
            }
            while k > i + 3 && chars[k - 1] == '-' {
                k -= 1;
            }
            if k > i + 3 {
                i = k;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `[*]` is start or end depending on which side of the arrow it sits.
fn state_endpoint(graph: &mut Graph, id: &str, is_source: bool) -> Option<usize> {
    if id == "[*]" {
        return graph.node_index(if is_source { "[*]start" } else { "[*]end" }, Some("●".to_string()), Shape::Round);
    }
    graph.node_index(id, None, Shape::Round)
}

/// `id: description`, or a bare state name.
fn parse_state_desc(st: &str, graph: &mut Graph) -> Option<()> {
    if let Some((id, desc)) = split_once(st, ":") {
        let id = js_trim(id);
        let desc = js_trim(desc);
        if id.is_empty() || is_whitespace_char(id) || desc.is_empty() {
            return None;
        }
        return graph.node_label(id, decode_html_entities(desc)).map(|_| ());
    }
    if is_whitespace_char(st) {
        return None;
    }
    graph.node_index(st, None, Shape::Round).map(|_| ())
}

// --------------------------------------------------------------------- class

/// Relation operators, longest-first so `--|>` wins over `--`.
const CLASS_OPS: [(&str, Head, Head, LineKind); 14] = [
    ("<|--", Head::Triangle, Head::None, LineKind::Solid),
    ("--|>", Head::None, Head::Triangle, LineKind::Solid),
    ("<|..", Head::Triangle, Head::None, LineKind::Dotted),
    ("..|>", Head::None, Head::Triangle, LineKind::Dotted),
    ("*--", Head::DiamondFill, Head::None, LineKind::Solid),
    ("--*", Head::None, Head::DiamondFill, LineKind::Solid),
    ("o--", Head::DiamondOpen, Head::None, LineKind::Solid),
    ("--o", Head::None, Head::DiamondOpen, LineKind::Solid),
    ("<--", Head::Arrow, Head::None, LineKind::Solid),
    ("-->", Head::None, Head::Arrow, LineKind::Solid),
    ("<..", Head::Arrow, Head::None, LineKind::Dotted),
    ("..>", Head::None, Head::Arrow, LineKind::Dotted),
    ("--", Head::None, Head::None, LineKind::Solid),
    ("..", Head::None, Head::None, LineKind::Dotted),
];

const MAX_CLASS_OP: usize = 4;

pub(crate) struct ClassDiagram {
    pub graph: Graph,
    pub infos: Vec<ClassInfo>,
}

pub(crate) fn parse_class(src: &str) -> Option<ClassDiagram> {
    let statements = statements_of(src);
    let kind = header_kind(&statements)?;
    if !kind.starts_with("classdiagram") {
        return None;
    }

    let mut graph = Graph::new(super::graph::Dir::Down);
    let mut infos: Vec<ClassInfo> = Vec::new();
    /// Declare a class, keeping `infos` aligned with `graph.nodes`.
    fn declare(graph: &mut Graph, infos: &mut Vec<ClassInfo>, name: &str) -> Option<usize> {
        let idx = graph.node_index(name, None, Shape::Rect);
        while infos.len() < graph.nodes.len() {
            infos.push(ClassInfo::default());
        }
        idx
    }
    let mut cur_class: Option<usize> = None;

    for st in &statements[1..] {
        if let Some(c) = cur_class {
            if st == "}" {
                cur_class = None;
            } else {
                push_member(&mut infos[c], st);
            }
            continue;
        }
        let st = drop_style_tags(st);

        let first = ascii_lower(first_word(&st));
        if first == "direction" {
            graph.dir = parse_dir(split_ws(&st).get(1).copied().unwrap_or(""));
            continue;
        }
        if ["note", "callback", "click", "link", "style", "cssclass", "classdef", "namespace", "}"]
            .contains(&first.as_str())
        {
            continue;
        }
        if first == "class" {
            let rest = js_trim(&st["class".len()..]);
            let open = rest.ends_with('{');
            let name = if open { js_trim(&rest[..rest.len() - 1]) } else { rest };
            if name.is_empty() || is_whitespace_char(name) {
                return None;
            }
            let idx = declare(&mut graph, &mut infos, name)?;
            if open {
                cur_class = Some(idx);
            }
            continue;
        }

        if let Some(after) = st.strip_prefix("<<") {
            let (annotation, name) = split_once(after, ">>")?;
            let name = js_trim(name);
            if name.is_empty() || is_whitespace_char(name) {
                return None;
            }
            let idx = declare(&mut graph, &mut infos, name)?;
            infos[idx].annotation = Some(js_trim(annotation).to_string());
            continue;
        }

        if let Some(rel) = parse_class_relation(&st) {
            let f = declare(&mut graph, &mut infos, &rel.from)?;
            let t = declare(&mut graph, &mut infos, &rel.to)?;
            if graph.edges.len() >= MAX_EDGES {
                return None;
            }
            graph.edges.push(Edge {
                from: f,
                to: t,
                label: rel.label,
                head_to: rel.head_to,
                head_from: rel.head_from,
                line: rel.line,
            });
            continue;
        }

        if let Some((id, text)) = split_once(&st, ":") {
            let id = js_trim(id);
            let text = js_trim(text);
            if id.is_empty() || is_whitespace_char(id) || text.is_empty() {
                return None;
            }
            let idx = declare(&mut graph, &mut infos, id)?;
            push_member(&mut infos[idx], text);
            continue;
        }
        return None;
    }

    if graph.nodes.is_empty() {
        return None;
    }
    while infos.len() < graph.nodes.len() {
        infos.push(ClassInfo::default());
    }
    Some(ClassDiagram { graph, infos })
}

/// Add a member to the attribute or method compartment, eliding past the cap.
fn push_member(info: &mut ClassInfo, raw: &str) {
    if let Some(after) = raw.strip_prefix("<<") {
        if let Some((annotation, _)) = split_once(after, ">>") {
            info.annotation = Some(js_trim(annotation).to_string());
        }
        return;
    }
    let member = decode_html_entities(&display_generics(js_trim(raw)));
    let list = if member.contains('(') { &mut info.methods } else { &mut info.attrs };
    if list.len() < MAX_MEMBERS {
        list.push(member);
    } else if list.len() == MAX_MEMBERS {
        list.push("…".to_string());
    }
}

struct ClassRelation {
    from: String,
    to: String,
    head_from: Head,
    head_to: Head,
    line: LineKind,
    label: Option<String>,
}

fn parse_class_relation(st: &str) -> Option<ClassRelation> {
    let chars: Vec<char> = st.chars().collect();
    let mut found: Option<(usize, &str, Head, Head, LineKind)> = None;

    'outer: for pos in 0..chars.len() {
        let tail = collect(&chars, pos, pos + MAX_CLASS_OP);
        for (op, head_from, head_to, line) in CLASS_OPS {
            if !tail.starts_with(op) {
                continue;
            }
            // `o` is also an identifier character: skip a match glued to a name.
            if op.starts_with('o') && pos > 0 && is_id_char(chars[pos - 1]) {
                continue;
            }
            let after = chars.get(pos + op.chars().count());
            if op.ends_with('o') && after.is_some_and(|&c| is_id_char(c)) {
                continue;
            }
            found = Some((pos, op, head_from, head_to, line));
            break 'outer;
        }
    }
    let (pos, op, head_from, head_to, line) = found?;

    let lhs_raw = collect(&chars, 0, pos);
    let lhs_raw = js_trim(&lhs_raw);
    let rhs_raw = collect(&chars, pos + op.chars().count(), chars.len());
    let rhs_raw = js_trim(&rhs_raw);

    let (lhs, card_from) = strip_cardinality_suffix(lhs_raw);
    let (rhs, card_to) = strip_cardinality_prefix(rhs_raw);

    let split = split_once(rhs, ":");
    let to_id = js_trim(split.map_or(rhs, |s| s.0));
    let rel_label = split.and_then(|s| non_empty(decode_html_entities(js_trim(s.1))));

    if lhs.is_empty() || to_id.is_empty() || is_whitespace_char(lhs) || is_whitespace_char(to_id) {
        return None;
    }

    let label = non_empty(
        [card_from, rel_label.as_deref().unwrap_or(""), card_to]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    );
    Some(ClassRelation { from: lhs.to_string(), to: to_id.to_string(), head_from, head_to, line, label })
}

/// `Class "1"` — a quoted cardinality trailing the left-hand name.
fn strip_cardinality_suffix(s: &str) -> (&str, &str) {
    let t = js_trim_end(s);
    if let Some(rest) = t.strip_suffix('"')
        && let Some(q) = rest.rfind('"')
    {
        return (js_trim_end(&rest[..q]), &rest[q + 1..]);
    }
    (t, "")
}

/// `"0..*" Class` — a quoted cardinality leading the right-hand name.
fn strip_cardinality_prefix(s: &str) -> (&str, &str) {
    let t = js_trim_start(s);
    if let Some(rest) = t.strip_prefix('"')
        && let Some(q) = rest.find('"')
    {
        return (js_trim_start(&rest[q + 1..]), &rest[..q]);
    }
    (t, "")
}

/// Mermaid writes generics as `List~T~`; show them as `List<T>`.
pub(crate) fn display_generics(s: &str) -> String {
    let mut out = String::new();
    let mut open = false;
    for c in s.chars() {
        if c == '~' {
            out.push(if open { '>' } else { '<' });
            open = !open;
        } else {
            out.push(c);
        }
    }
    out
}

// ------------------------------------------------------------------------ ER

pub(crate) fn parse_er(src: &str) -> Option<ClassDiagram> {
    let statements = statements_of(src);
    if header_kind(&statements).as_deref() != Some("erdiagram") {
        return None;
    }

    let mut graph = Graph::new(super::graph::Dir::Down);
    let mut infos: Vec<ClassInfo> = Vec::new();
    let mut cur_entity: Option<usize> = None;

    for st in &statements[1..] {
        if let Some(e) = cur_entity {
            if st == "}" {
                cur_entity = None;
            } else {
                push_er_attribute(&mut infos[e], st);
            }
            continue;
        }

        if let Some((rel, label)) = split_er_relationship(st) {
            let tokens = split_ws(rel);
            if tokens.len() != 3 {
                return None;
            }
            let (card_l, card_r, line) = parse_er_op(tokens[1])?;
            let f = er_entity(&mut graph, &mut infos, tokens[0])?;
            let t = er_entity(&mut graph, &mut infos, tokens[2])?;
            if graph.edges.len() >= MAX_EDGES {
                return None;
            }
            let rel_label = label.map(clean_label).unwrap_or_default();
            graph.edges.push(Edge {
                from: f,
                to: t,
                label: non_empty(
                    [card_l, rel_label.as_str(), card_r]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                head_to: Head::None,
                head_from: Head::None,
                line,
            });
            continue;
        }

        let open = st.ends_with('{');
        let decl = if open { js_trim(&st[..st.len() - 1]) } else { st.as_str() };
        if decl.is_empty() || split_ws(decl).len() != 1 {
            return None;
        }
        let idx = er_entity(&mut graph, &mut infos, decl)?;
        if open {
            cur_entity = Some(idx);
        }
    }

    if graph.nodes.is_empty() {
        return None;
    }
    while infos.len() < graph.nodes.len() {
        infos.push(ClassInfo::default());
    }
    Some(ClassDiagram { graph, infos })
}

fn er_entity(graph: &mut Graph, infos: &mut Vec<ClassInfo>, token: &str) -> Option<usize> {
    let idx = if let Some(open) = token.find('[') {
        let id = &token[..open];
        let label = clean_label(token[open + 1..].trim_end_matches(']'));
        if id.is_empty() || label.is_empty() {
            return None;
        }
        graph.node_label(id, label)
    } else {
        graph.node_index(token, None, Shape::Rect)
    }?;
    while infos.len() < graph.nodes.len() {
        infos.push(ClassInfo::default());
    }
    Some(idx)
}

fn split_er_relationship(st: &str) -> Option<(&str, Option<&str>)> {
    let split = split_once(st, ":");
    let rel = split.map_or(st, |s| s.0);
    let label = split.map(|s| js_trim(s.1));
    split_ws(rel).iter().any(|t| parse_er_op(t).is_some()).then_some((rel, label))
}

/// A crow's-foot operator: two cardinality glyphs around `--` or `..`.
fn parse_er_op(tok: &str) -> Option<(&'static str, &'static str, LineKind)> {
    if tok.len() != 6 || !tok.is_ascii() {
        return None;
    }
    let line = match &tok[2..4] {
        "--" => LineKind::Solid,
        ".." => LineKind::Dotted,
        _ => return None,
    };
    let card_l = er_card(&tok[0..2])?;
    let card_r = er_card(&tok[4..6])?;
    Some((card_l, card_r, line))
}

fn er_card(tok: &str) -> Option<&'static str> {
    match tok {
        "|o" | "o|" => Some("0..1"),
        "||" => Some("1"),
        "}o" | "o{" => Some("0..*"),
        "}|" | "|{" => Some("1..*"),
        _ => None,
    }
}

/// ER attributes are `type name`; a trailing quoted comment is dropped.
fn push_er_attribute(info: &mut ClassInfo, raw: &str) {
    let mut parts: Vec<&str> = Vec::new();
    for tok in split_ws(raw) {
        if tok.starts_with('"') {
            break;
        }
        parts.push(tok);
    }
    if parts.is_empty() {
        return;
    }
    let line = decode_html_entities(&parts.join(" "));
    if info.attrs.len() < MAX_MEMBERS {
        info.attrs.push(line);
    } else if info.attrs.len() == MAX_MEMBERS {
        info.attrs.push("…".to_string());
    }
}

// ------------------------------------------------------------------ sequence

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeqHead {
    Arrow,
    Cross,
}

/// Message operators, longest-first so `-->>` wins over `-->`.
const SEQ_OPS: [(&str, bool, SeqHead); 8] = [
    ("-->>", true, SeqHead::Arrow),
    ("->>", false, SeqHead::Arrow),
    ("--x", true, SeqHead::Cross),
    ("-x", false, SeqHead::Cross),
    ("--)", true, SeqHead::Arrow),
    ("-)", false, SeqHead::Arrow),
    ("-->", true, SeqHead::Arrow),
    ("->", false, SeqHead::Arrow),
];

const MAX_SEQ_OP: usize = 4;

#[derive(Debug, Clone, Copy)]
pub(crate) enum NoteAnchor {
    Over { from: usize, to: usize },
    Left { at: usize },
    Right { at: usize },
}

#[derive(Debug, Clone)]
pub(crate) enum SeqItem {
    Message { from: usize, to: usize, text: Option<String>, dashed: bool, head: SeqHead },
    Note { anchor: NoteAnchor, text: String },
    Divider { text: String },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Sequence {
    pub labels: Vec<String>,
    pub index: HashMap<String, usize>,
    pub items: Vec<SeqItem>,
}

impl Sequence {
    fn participant(&mut self, id: &str, label: Option<String>) -> Option<usize> {
        if let Some(&existing) = self.index.get(id) {
            if let Some(label) = label {
                self.labels[existing] = label;
            }
            return Some(existing);
        }
        if self.labels.len() >= MAX_NODES {
            return None;
        }
        self.index.insert(id.to_string(), self.labels.len());
        self.labels.push(label.unwrap_or_else(|| id.to_string()));
        Some(self.labels.len() - 1)
    }
}

pub(crate) fn parse_sequence(src: &str) -> Option<Sequence> {
    let statements = statements_of(src);
    if header_kind(&statements).as_deref() != Some("sequencediagram") {
        return None;
    }

    let mut seq = Sequence::default();
    let mut autonumber = false;
    let mut msg_count = 0;
    // One entry per open block; `true` when it draws a divider on `end`.
    let mut blocks: Vec<bool> = Vec::new();

    for st in &statements[1..] {
        let first = first_word(st);
        let lower = ascii_lower(first);

        if lower == "participant" || lower == "actor" {
            let rest = js_trim(&st[first.len()..]);
            if rest.is_empty() {
                return None;
            }
            let as_split = split_once(rest, " as ");
            let (id, label) = match as_split {
                Some((id, label)) => (js_trim(id), Some(clean_label(label))),
                None => (rest, None),
            };
            seq.participant(id, label)?;
            continue;
        }
        if lower == "autonumber" {
            autonumber = true;
            continue;
        }
        if [
            "activate", "deactivate", "create", "destroy", "title", "acctitle", "accdescr", "links", "link",
            "properties",
        ]
        .contains(&lower.as_str())
        {
            continue;
        }
        if lower == "note" {
            let (text, anchor) = parse_note_anchor(js_trim(&st[first.len()..]), &mut seq)?;
            if seq.items.len() >= MAX_EDGES {
                return None;
            }
            seq.items.push(SeqItem::Note { anchor, text });
            continue;
        }
        if ["loop", "alt", "opt", "par", "critical", "break", "else", "and", "option"].contains(&lower.as_str()) {
            if ["else", "and", "option"].contains(&lower.as_str()) {
                // A continuation only divides a block that opened one.
                if blocks.last() != Some(&true) {
                    continue;
                }
            } else {
                blocks.push(true);
            }
            if seq.items.len() >= MAX_EDGES {
                return None;
            }
            seq.items.push(SeqItem::Divider { text: decode_html_entities(st) });
            continue;
        }
        if lower == "rect" || lower == "box" {
            blocks.push(false);
            continue;
        }
        if lower == "end" {
            if blocks.pop() == Some(true) {
                if seq.items.len() >= MAX_EDGES {
                    return None;
                }
                seq.items.push(SeqItem::Divider { text: "end".to_string() });
            }
            continue;
        }

        let (from, to, mut text, dashed, head) = parse_seq_message(st, &mut seq)?;
        if autonumber {
            msg_count += 1;
            text = Some(match text {
                None => format!("{msg_count}."),
                Some(t) => format!("{msg_count}. {t}"),
            });
        }
        if seq.items.len() >= MAX_EDGES {
            return None;
        }
        seq.items.push(SeqItem::Message { from, to, text, dashed, head });
    }

    if seq.labels.is_empty() { None } else { Some(seq) }
}

fn parse_note_anchor(rest: &str, seq: &mut Sequence) -> Option<(String, NoteAnchor)> {
    let lower = ascii_lower(rest);
    #[derive(PartialEq)]
    enum Kind {
        Over,
        Left,
        Right,
    }
    let (kind, ids_and_text) = if lower.starts_with("over ") {
        (Kind::Over, &rest["over ".len()..])
    } else if lower.starts_with("left of ") {
        (Kind::Left, &rest["left of ".len()..])
    } else if lower.starts_with("right of ") {
        (Kind::Right, &rest["right of ".len()..])
    } else {
        return None;
    };

    let (ids, text) = split_once(ids_and_text, ":")?;
    let text = decode_html_entities(js_trim(text));
    let parts: Vec<&str> = ids.split(',').map(js_trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let a = seq.participant(parts[0], None)?;

    match kind {
        Kind::Left => return Some((text, NoteAnchor::Left { at: a })),
        Kind::Right => return Some((text, NoteAnchor::Right { at: a })),
        Kind::Over => {}
    }
    let mut b = a;
    if let Some(second) = parts.get(1) {
        b = seq.participant(second, None)?;
    }
    Some((text, NoteAnchor::Over { from: a.min(b), to: a.max(b) }))
}

fn parse_seq_message(st: &str, seq: &mut Sequence) -> Option<(usize, usize, Option<String>, bool, SeqHead)> {
    let chars: Vec<char> = st.chars().collect();
    let mut found: Option<(usize, &str, bool, SeqHead)> = None;
    'outer: for pos in 0..chars.len() {
        let tail = collect(&chars, pos, pos + MAX_SEQ_OP);
        for (op, dashed, head) in SEQ_OPS {
            if tail.starts_with(op) {
                found = Some((pos, op, dashed, head));
                break 'outer;
            }
        }
    }
    let (pos, op, dashed, head) = found?;

    let from_id = collect(&chars, 0, pos);
    let from_id = js_trim(&from_id);
    if from_id.is_empty() {
        return None;
    }
    // `+`/`-` activate and deactivate the target; they carry no layout meaning.
    let rest = collect(&chars, pos + op.chars().count(), chars.len());
    let rest = js_trim_start(&rest).trim_start_matches(['+', '-']);

    let split = split_once(rest, ":");
    let to_id = js_trim(split.map_or(rest, |s| s.0));
    let text = split.and_then(|s| non_empty(decode_html_entities(js_trim(s.1))));
    if to_id.is_empty() {
        return None;
    }

    let from = seq.participant(from_id, None)?;
    let to = seq.participant(to_id, None)?;
    Some((from, to, text, dashed, head))
}
