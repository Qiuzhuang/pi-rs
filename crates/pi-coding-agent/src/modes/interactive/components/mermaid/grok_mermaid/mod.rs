//! Rust port of the `grok-mermaid` npm package (0.2.3), which renders Mermaid diagrams as
//! Unicode box-drawing art for terminals. Only `render` is ported; pi does not use `toAnsi` or
//! `sourceBox`.
//!
//! Supported: `graph`/`flowchart` (including `subgraph`), `stateDiagram`, `classDiagram`,
//! `erDiagram` and `sequenceDiagram`.

mod canvas;
mod graph;
mod labels;
mod layout;
mod layout_seq;
mod parse;
mod width;
mod width_data;

use canvas::Canvas;
use labels::strip_controls;
use parse::DiagramKind;

/// Semantic class of a run of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cls {
    /// Box outlines, subgraph frames, compartment rules.
    Border,
    /// Node / participant / compartment labels.
    Text,
    /// Connector lines and arrowheads.
    Edge,
    /// Text sitting on an edge.
    EdgeLabel,
    /// The `mermaid: <kind>` header of a source box.
    Title,
    /// Blank filler.
    None,
}

/// A run of adjacent cells sharing one semantic class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Span {
    pub text: String,
    pub cls: Cls,
}

/// A rendered diagram. `plain[i]` and `styled[i]` describe the same row.
#[derive(Debug, Clone)]
pub(crate) struct MermaidArt {
    pub plain: Vec<String>,
    pub styled: Vec<Vec<Span>>,
    /// The display columns the widest row needs.
    pub width: usize,
    /// Source the grammar could not read and dropped. Advisory only.
    pub warnings: Vec<String>,
}

/// Render a Mermaid source block as Unicode box-drawing art.
///
/// `None` means there is no art to show: blank input, a syntax error, a diagram
/// type this renderer does not draw, or one large enough that laying it out is refused.
pub(crate) fn render(src: &str) -> Option<MermaidArt> {
    let src = strip_controls(src);
    if pi_tui::utils::js_trim(&src).is_empty() {
        return None;
    }
    let (canvas, warnings) = attempt(&src)?;
    let lines = canvas.to_lines();
    Some(MermaidArt { plain: lines.plain, styled: lines.styled, width: lines.width, warnings })
}

/// Draw `src`, retrying once without its last line if the grammar rejects it.
fn attempt(src: &str) -> Option<(Canvas, Vec<String>)> {
    if let Some(drawn) = draw(src) {
        return Some(drawn);
    }

    let body = src.trim_end_matches(pi_tui::utils::is_whitespace_ch);
    let cut = body.rfind('\n')?;
    let (canvas, mut warnings) = draw(&body[..cut])?;

    let dropped = pi_tui::utils::js_trim(&body[cut + 1..]);
    warnings.push(format!("dropped, unreadable final line: \"{dropped}\""));
    Some((canvas, warnings))
}

/// Dispatch on the declared diagram type; `None` means nothing was drawn.
fn draw(src: &str) -> Option<(Canvas, Vec<String>)> {
    match parse::diagram_kind(src)? {
        DiagramKind::Flowchart => {
            let graph = parse::parse_graph(src)?;
            let canvas =
                if graph.groups.is_empty() { layout::layout_flowchart(&graph) } else { layout::layout_grouped(&graph) }?;
            Some((canvas, graph.warnings))
        }
        DiagramKind::State => {
            let state = parse::parse_state(src)?;
            Some((layout::layout_flowchart(&state)?, Vec::new()))
        }
        DiagramKind::Class => {
            let cls = parse::parse_class(src)?;
            Some((layout::layout_class(&cls.graph, &cls.infos)?, Vec::new()))
        }
        DiagramKind::Er => {
            let er = parse::parse_er(src)?;
            Some((layout::layout_class(&er.graph, &er.infos)?, Vec::new()))
        }
        DiagramKind::Sequence => {
            let seq = parse::parse_sequence(src)?;
            Some((layout_seq::layout_sequence(&seq)?, Vec::new()))
        }
    }
}
