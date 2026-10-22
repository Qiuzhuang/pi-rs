//! Graph layout: rank, order, place, route, draw (grok-mermaid `layout.ts`).
//!
//! Follows the Sugiyama outline — assign ranks along the flow axis, reorder
//! within ranks to cut crossings, then relax positions on the cross axis so
//! chains stay straight. `BT` and `RL` reuse the `TD`/`LR` layouts and flip the
//! finished canvas, so text never ends up mirrored.

use super::Cls;
use super::canvas::{CONT, Canvas, D, L, R, STY_DOT, STY_SOLID, STY_THICK, U, draw_text, draw_text_over_edges};
use super::graph::{ClassInfo, Dir, Edge, Graph, Head, LineKind, Node, Shape};
use super::labels::{MAX_LABEL, MAX_LINES, WRAP_WIDTH, fit_label, wrap_label};
use super::parse::display_generics;
use super::width::{measured, string_width};
use std::collections::HashMap;

/// Cells of padding between a box border and its text.
const PAD: i64 = 1;
/// Minimum horizontal / vertical space between boxes.
const GAP_X: i64 = 3;
const GAP_Y: i64 = 2;
/// Refuse to allocate a canvas larger than this many cells.
const MAX_CANVAS_CELLS: i64 = 1 << 21;

/// Saturating subtraction; Rust's `usize` arithmetic never goes negative.
pub(crate) fn sat(a: i64, b: i64) -> i64 {
    (a - b).max(0)
}

pub(crate) fn half(n: i64) -> i64 {
    n.div_euclid(2)
}

fn sw(s: &str) -> i64 {
    string_width(s) as i64
}

/// `Math.round`.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Placed {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub cx: i64,
    pub cy: i64,
    pub rank: i64,
}

/// Per-node dimensions. `lay*` include room for self-edge loops and labels.
struct NodeSizes {
    box_w: Vec<i64>,
    box_h: Vec<i64>,
    lay_w: Vec<i64>,
    lay_h: Vec<i64>,
    extra_h: Vec<i64>,
    self_label_w: Vec<i64>,
}

/// What to draw inside a node box.
pub(crate) enum NodeExtra {
    Plain,
    Frame(Canvas),
    Compartments(Vec<Vec<String>>),
}

struct RoutePlan {
    canvas_w: i64,
    canvas_h: i64,
    /// Coordinate just past each rank's boxes, where its bus rows begin.
    band_end: Vec<i64>,
    /// Bus track offset per edge.
    edge_bus: Vec<i64>,
    /// Coordinate of the first lane track.
    lane_base: i64,
    /// Lane track offset per edge.
    edge_lane: Vec<i64>,
}

// ------------------------------------------------------------------ ranking

/// Longest-path ranking over the graph's DAG (back edges excluded by a DFS colouring pass).
fn compute_ranks(graph: &Graph) -> Vec<i64> {
    let n = graph.nodes.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for e in &graph.edges {
        if e.from != e.to {
            children[e.from].push(e.to);
            indeg[e.to] += 1;
        }
    }

    let mut color = vec![0u8; n];
    let mut dag: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut order: Vec<usize> = Vec::new();

    // Roots first so ranks grow from natural entry points, then any leftovers.
    let roots: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    for start in roots.into_iter().chain(0..n) {
        if color[start] == 0 {
            dfs_dag(start, &children, &mut color, &mut dag, &mut order);
        }
    }

    let mut rank = vec![0i64; n];
    for &u in order.iter().rev() {
        for &v in &dag[u] {
            rank[v] = rank[v].max(rank[u] + 1);
        }
    }
    rank
}

/// Iterative DFS recording postorder and skipping edges back into the stack.
fn dfs_dag(start: usize, children: &[Vec<usize>], color: &mut [u8], dag: &mut [Vec<usize>], order: &mut Vec<usize>) {
    let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
    color[start] = 1;
    while let Some(frame) = stack.last_mut() {
        let u = frame.0;
        if frame.1 < children[u].len() {
            let v = children[u][frame.1];
            frame.1 += 1;
            if color[v] == 1 {
                continue; // grey: a back edge, ignore it
            }
            dag[u].push(v);
            if color[v] == 0 {
                color[v] = 1;
                stack.push((v, 0));
            }
        } else {
            color[u] = 2;
            order.push(u);
            stack.pop();
        }
    }
}

fn parents_children(edges: &[Edge], ranks: &[i64], n: usize) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        if e.from != e.to && ranks[e.to] > ranks[e.from] {
            parents[e.to].push(e.from);
            children[e.from].push(e.to);
        }
    }
    (parents, children)
}

/// Reorder nodes within each rank to minimise edge crossings (barycenter sweeps).
fn order_ranks(by_rank: &mut [Vec<usize>], edges: &[Edge], ranks: &[i64]) {
    let n = ranks.len();
    if by_rank.len() < 2 || n < 3 {
        return;
    }
    let (parents, children) = parents_children(edges, ranks, n);

    let mut pos = vec![0f64; n];
    let reindex = |row: &[usize], pos: &mut [f64]| {
        for (i, &v) in row.iter().enumerate() {
            pos[v] = i as f64;
        }
    };
    for row in by_rank.iter() {
        reindex(row, &mut pos);
    }

    let mut best: Vec<Vec<usize>> = by_rank.to_vec();
    let mut best_crossings = count_crossings(edges, ranks, &pos);
    if best_crossings == 0 {
        return;
    }

    let len = by_rank.len();
    for it in 0..8 {
        // Alternate sweeping down (sort by parents) and up (sort by children).
        let rows: Vec<usize> = if it % 2 == 0 { (1..len).collect() } else { (0..len - 1).rev().collect() };
        let neigh = if it % 2 == 0 { &parents } else { &children };
        for r in rows {
            sort_by_barycenter(&mut by_rank[r], neigh, &pos);
            reindex(&by_rank[r], &mut pos);
        }
        let crossings = count_crossings(edges, ranks, &pos);
        if crossings < best_crossings {
            best_crossings = crossings;
            best = by_rank.to_vec();
        }
        if best_crossings == 0 {
            break;
        }
    }

    for (row, best_row) in by_rank.iter_mut().zip(best) {
        *row = best_row;
    }
}

fn barycenter(v: usize, neigh: &[Vec<usize>], pos: &[f64]) -> f64 {
    if neigh[v].is_empty() {
        pos[v]
    } else {
        neigh[v].iter().map(|&u| pos[u]).sum::<f64>() / neigh[v].len() as f64
    }
}

fn sort_by_barycenter(row: &mut [usize], neigh: &[Vec<usize>], pos: &[f64]) {
    let mut keyed: Vec<(f64, usize)> = row.iter().map(|&v| (barycenter(v, neigh, pos), v)).collect();
    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (i, (_, v)) in keyed.into_iter().enumerate() {
        row[i] = v;
    }
}

fn count_crossings(edges: &[Edge], ranks: &[i64], pos: &[f64]) -> usize {
    let adjacent: Vec<(i64, f64, f64)> = edges
        .iter()
        .filter(|e| e.from != e.to && ranks[e.to] == ranks[e.from] + 1)
        .map(|e| (ranks[e.from], pos[e.from], pos[e.to]))
        .collect();
    let mut crossings = 0;
    for i in 0..adjacent.len() {
        let a = adjacent[i];
        for b in &adjacent[i + 1..] {
            if a.0 == b.0 && ((a.1 < b.1 && a.2 > b.2) || (a.1 > b.1 && a.2 < b.2)) {
                crossings += 1;
            }
        }
    }
    crossings
}

/// Assign a cross-axis centre to every node so nodes line up under their neighbours.
fn assign_positions(by_rank: &[Vec<usize>], size: &[i64], sep: i64, edges: &[Edge], ranks: &[i64]) -> Vec<i64> {
    let n = size.len();
    let (parents, children) = parents_children(edges, ranks, n);
    let sep = sep as f64;

    let mut pos = vec![0f64; n];
    for row in by_rank {
        let mut x = 0f64;
        for &v in row {
            let h = size[v] as f64 / 2.0;
            x += h;
            pos[v] = x;
            x += h + sep;
        }
    }

    for it in 0..10 {
        let neigh = if it % 2 == 0 { &parents } else { &children };
        if it % 2 == 0 {
            for row in by_rank {
                relax_rank(row, neigh, &mut pos, size, sep);
            }
        } else {
            for row in by_rank.iter().rev() {
                relax_rank(row, neigh, &mut pos, size, sep);
            }
        }
    }

    let mut min_left = f64::INFINITY;
    for v in 0..n {
        min_left = min_left.min(pos[v] - size[v] as f64 / 2.0);
    }
    if !min_left.is_finite() {
        min_left = 0.0;
    }
    (0..n).map(|v| js_round(pos[v] - min_left).max(0.0) as i64).collect()
}

fn relax_rank(nodes: &[usize], neigh: &[Vec<usize>], pos: &mut [f64], size: &[i64], sep: f64) {
    let n = nodes.len();
    if n == 0 {
        return;
    }
    let desired: Vec<f64> = nodes.iter().map(|&v| barycenter(v, neigh, pos)).collect();
    let half_of = |i: usize| size[nodes[i]] as f64 / 2.0;

    // Sweep right then left, then take the midpoint.
    let mut left = vec![0f64; n];
    for i in 0..n {
        left[i] = if i == 0 { desired[i] } else { desired[i].max(left[i - 1] + half_of(i - 1) + sep + half_of(i)) };
    }
    let mut right = vec![0f64; n];
    for i in (0..n).rev() {
        right[i] =
            if i == n - 1 { desired[i] } else { desired[i].min(right[i + 1] - half_of(i + 1) - sep - half_of(i)) };
    }
    for i in 0..n {
        pos[nodes[i]] = (left[i] + right[i]) / 2.0;
    }
    for i in 1..n {
        let min_p = pos[nodes[i - 1]] + half_of(i - 1) + sep + half_of(i);
        if pos[nodes[i]] < min_p {
            pos[nodes[i]] = min_p;
        }
    }
}

// ------------------------------------------------------------------- tracks

/// A span competing for a track: `(start, end, from, to, edge_index)`.
type Span5 = (i64, i64, i64, i64, i64);

/// Pack spans into as few parallel tracks as possible.
fn assign_tracks(spans: &[Span5]) -> (Vec<(usize, i64)>, i64) {
    let mut sorted = spans.to_vec();
    sorted.sort();
    let mut tracks: Vec<Vec<(i64, i64, i64, i64)>> = Vec::new();
    let mut assigned: Vec<(usize, i64)> = Vec::new();
    for (s, e, f, t, idx) in sorted {
        let slot = tracks.iter().position(|members| {
            members.iter().all(|&(s2, e2, f2, t2)| e2 + 2 <= s || e + 2 <= s2 || f2 == f || t2 == t)
        });
        let slot = match slot {
            Some(slot) => slot,
            None => {
                tracks.push(Vec::new());
                tracks.len() - 1
            }
        };
        tracks[slot].push((s, e, f, t));
        assigned.push((idx as usize, slot as i64));
    }
    (assigned, tracks.len() as i64)
}

/// Edges from rank `r` to `r + 1` that must jog sideways, so need a bus row.
fn bus_spans(graph: &Graph, ranks: &[i64], centers: &[i64], r: i64, exact: bool) -> Vec<Span5> {
    let mut out = Vec::new();
    for (i, e) in graph.edges.iter().enumerate() {
        let jogs = if exact { centers[e.from] != centers[e.to] } else { (centers[e.from] - centers[e.to]).abs() > 1 };
        if e.from != e.to && ranks[e.from] == r && ranks[e.to] == r + 1 && jogs {
            out.push((
                centers[e.from].min(centers[e.to]),
                centers[e.from].max(centers[e.to]),
                e.from as i64,
                e.to as i64,
                i as i64,
            ));
        }
    }
    out
}

/// Edges skipping a rank or running backwards; these go around in a lane.
fn lane_spans(graph: &Graph, ranks: &[i64], placed: &[Placed], vertical: bool) -> Vec<Span5> {
    let mut out = Vec::new();
    for (i, e) in graph.edges.iter().enumerate() {
        if e.from == e.to || ranks[e.to] == ranks[e.from] + 1 {
            continue;
        }
        let pf = placed[e.from];
        let pt = placed[e.to];
        let a = if vertical { pf.cy.min(pt.cy) } else { pf.cx.min(pt.cx) };
        let b = if vertical { pf.cy.max(pt.cy) } else { pf.cx.max(pt.cx) };
        out.push((a, b, e.from as i64, e.to as i64, i as i64));
    }
    out
}

// ----------------------------------------------------------------- placement

fn place_td(
    ranks: &[i64],
    max_rank: i64,
    by_rank: &[Vec<usize>],
    sizes: &NodeSizes,
    graph: &Graph,
    placed: &mut [Placed],
) -> RoutePlan {
    let centers = assign_positions(by_rank, &sizes.lay_w, GAP_X, &graph.edges, ranks);
    let nr = (max_rank + 1) as usize;

    let mut edge_bus = vec![0i64; graph.edges.len()];
    let mut bus_tracks = vec![0i64; nr];
    for r in 0..max_rank {
        let spans = bus_spans(graph, ranks, &centers, r, false);
        if spans.is_empty() {
            continue;
        }
        let (assigned, count) = assign_tracks(&spans);
        for (idx, slot) in assigned {
            edge_bus[idx] = slot;
        }
        bus_tracks[r as usize] = count;
    }

    let rank_h: Vec<i64> = by_rank
        .iter()
        .map(|row| {
            if row.is_empty() { 3 } else { row.iter().map(|&i| sizes.box_h[i] + sizes.extra_h[i]).max().unwrap_or(0) }
        })
        .collect();
    let mut rank_y = vec![0i64; nr];
    for r in 1..nr {
        rank_y[r] = rank_y[r - 1] + rank_h[r - 1] + GAP_Y.max(bus_tracks[r - 1] + 1);
    }
    let canvas_h = rank_y[nr - 1] + rank_h[nr - 1];
    let band_end: Vec<i64> = (0..nr).map(|r| rank_y[r] + rank_h[r]).collect();

    let mut diagram_w = 1;
    for (r, row) in by_rank.iter().enumerate() {
        for &idx in row {
            let w = sizes.box_w[idx];
            let h = sizes.box_h[idx];
            let cx = centers[idx];
            let x = sat(cx, half(w));
            let y = rank_y[r] + half(rank_h[r] - h - sizes.extra_h[idx]);
            placed[idx] = Placed { x, y, w, h, cx, cy: y + half(h), rank: r as i64 };
            diagram_w = diagram_w.max(x + w);
            if sizes.extra_h[idx] > 0 && sizes.self_label_w[idx] > 0 {
                diagram_w = diagram_w.max(x + w + 2 + sizes.self_label_w[idx]);
            }
        }
    }

    let mut content_w = diagram_w;
    for e in &graph.edges {
        let Some(label) = &e.label else { continue };
        if e.from == e.to {
            continue;
        }
        let lw = sw(label).min(MAX_LABEL as i64);
        content_w = if ranks[e.to] == ranks[e.from] + 1 {
            content_w.max(placed[e.to].cx + 2 + lw)
        } else {
            content_w.max(diagram_w + lw + 1)
        };
    }

    let mut edge_lane = vec![0i64; graph.edges.len()];
    let lanes = lane_spans(graph, ranks, placed, true);
    let mut canvas_w = content_w;
    let mut lane_base = 0;
    if !lanes.is_empty() {
        let (assigned, count) = assign_tracks(&lanes);
        for (idx, slot) in assigned {
            edge_lane[idx] = slot;
        }
        canvas_w = content_w + 1 + count;
        lane_base = content_w + 1;
    }

    RoutePlan { canvas_w, canvas_h, band_end, edge_bus, lane_base, edge_lane }
}

fn place_lr(
    ranks: &[i64],
    max_rank: i64,
    by_rank: &[Vec<usize>],
    sizes: &NodeSizes,
    graph: &Graph,
    placed: &mut [Placed],
) -> RoutePlan {
    let nr = (max_rank + 1) as usize;
    let col_w: Vec<i64> = by_rank.iter().map(|row| row.iter().map(|&i| sizes.box_w[i]).max().unwrap_or(0)).collect();

    // Left-to-right edge labels sit in the gap between columns.
    let max_label = graph
        .edges
        .iter()
        .filter(|e| e.from == e.to || ranks[e.to] == ranks[e.from] + 1)
        .filter_map(|e| e.label.as_deref())
        .map(|l| sw(l).min(MAX_LABEL as i64))
        .max()
        .unwrap_or(0);
    let base_gap = (GAP_X + 1).max(max_label + 3);

    let centers = assign_positions(by_rank, &sizes.lay_h, 1, &graph.edges, ranks);

    let mut edge_bus = vec![0i64; graph.edges.len()];
    let mut bus_tracks = vec![0i64; nr];
    for r in 0..max_rank {
        let spans = bus_spans(graph, ranks, &centers, r, true);
        if spans.is_empty() {
            continue;
        }
        let (assigned, count) = assign_tracks(&spans);
        for (idx, slot) in assigned {
            edge_bus[idx] = slot;
        }
        bus_tracks[r as usize] = count;
    }

    let mut rank_x = vec![0i64; nr];
    for r in 1..nr {
        rank_x[r] = rank_x[r - 1] + col_w[r - 1] + base_gap.max(bus_tracks[r - 1] + 1);
    }
    let self_tail = by_rank[nr - 1]
        .iter()
        .filter(|&&i| sizes.extra_h[i] > 0 && sizes.self_label_w[i] > 0)
        .map(|&i| 2 + sizes.self_label_w[i])
        .max()
        .unwrap_or(0);
    let canvas_w = rank_x[nr - 1] + col_w[nr - 1] + self_tail;
    let band_end: Vec<i64> = (0..nr).map(|r| rank_x[r] + col_w[r]).collect();

    let mut diagram_h = 1;
    for (r, row) in by_rank.iter().enumerate() {
        let x = rank_x[r];
        for &idx in row {
            let w = sizes.box_w[idx];
            let h = sizes.box_h[idx];
            let cy = centers[idx];
            let y = sat(cy, half(h + sizes.extra_h[idx]));
            placed[idx] = Placed { x, y, w, h, cx: x + half(w), cy: y + half(h), rank: r as i64 };
            diagram_h = diagram_h.max(y + h + sizes.extra_h[idx]);
        }
    }

    let mut edge_lane = vec![0i64; graph.edges.len()];
    let lanes = lane_spans(graph, ranks, placed, false);
    let mut canvas_h = diagram_h;
    let mut lane_base = 0;
    if !lanes.is_empty() {
        let (assigned, count) = assign_tracks(&lanes);
        for (idx, slot) in assigned {
            edge_lane[idx] = slot;
        }
        canvas_h = diagram_h + 1 + count;
        lane_base = diagram_h + 1;
    }

    RoutePlan { canvas_w, canvas_h, band_end, edge_bus, lane_base, edge_lane }
}

// -------------------------------------------------------------------- canvas

/// Rank, place, draw and route a graph onto a fresh canvas.
fn layout_canvas(graph: &Graph, extras: &[NodeExtra]) -> Option<Canvas> {
    let n = graph.nodes.len();
    if n == 0 {
        return None;
    }

    let ranks = compute_ranks(graph);
    let max_rank = ranks.iter().copied().max().unwrap_or(0).max(0);

    let mut by_rank: Vec<Vec<usize>> = vec![Vec::new(); (max_rank + 1) as usize];
    for (idx, &r) in ranks.iter().enumerate() {
        by_rank[r as usize].push(idx);
    }
    order_ranks(&mut by_rank, &graph.edges, &ranks);

    let wrapped: Vec<Vec<String>> = graph.nodes.iter().map(|node| wrap_label(&node.label, WRAP_WIDTH, MAX_LINES)).collect();
    let widest = |lines: &[String]| -> i64 {
        if lines.is_empty() { 1 } else { lines.iter().map(|l| sw(l)).max().unwrap_or(0).max(1) }
    };

    let mut box_w: Vec<i64> = extras
        .iter()
        .enumerate()
        .map(|(i, extra)| match extra {
            NodeExtra::Frame(sub) => (sub.w + 2).max(sw(&fit_label(&graph.nodes[i].label, WRAP_WIDTH)) + 4),
            NodeExtra::Compartments(sections) => {
                let flat: Vec<String> = sections.iter().flatten().cloned().collect();
                widest(&flat) + 2 * PAD + 2
            }
            NodeExtra::Plain => widest(&wrapped[i]) + 2 * PAD + 2,
        })
        .collect();
    let box_h: Vec<i64> = extras
        .iter()
        .enumerate()
        .map(|(i, extra)| match extra {
            NodeExtra::Frame(sub) => sub.h + 2,
            NodeExtra::Compartments(sections) => {
                let filled = sections.iter().filter(|s| !s.is_empty()).count() as i64;
                sections.iter().map(|s| s.len() as i64).sum::<i64>() + sat(filled, 1) + 2
            }
            NodeExtra::Plain => wrapped[i].len() as i64 + 2,
        })
        .collect();

    // A self-edge needs two rows below its box, and room beside it for a label.
    let mut extra_h = vec![0i64; n];
    let mut self_label_w = vec![0i64; n];
    for e in &graph.edges {
        if e.from != e.to {
            continue;
        }
        extra_h[e.from] = 2;
        if let Some(label) = &e.label {
            self_label_w[e.from] = self_label_w[e.from].max(sw(label).min(MAX_LABEL as i64));
        }
    }
    for i in 0..n {
        if extra_h[i] > 0 {
            box_w[i] = box_w[i].max(7);
        }
    }

    let sizes = NodeSizes {
        lay_w: box_w
            .iter()
            .enumerate()
            .map(|(i, &w)| w + if self_label_w[i] > 0 { 2 * (self_label_w[i] + 3) } else { 0 })
            .collect(),
        lay_h: box_h.iter().enumerate().map(|(i, &h)| h + extra_h[i]).collect(),
        box_w,
        box_h,
        extra_h,
        self_label_w,
    };

    let mut placed = vec![Placed::default(); n];

    let vertical = graph.dir == Dir::Down || graph.dir == Dir::Up;
    let plan = if vertical {
        place_td(&ranks, max_rank, &by_rank, &sizes, graph, &mut placed)
    } else {
        place_lr(&ranks, max_rank, &by_rank, &sizes, graph, &mut placed)
    };

    if plan.canvas_w * plan.canvas_h > MAX_CANVAS_CELLS {
        return None;
    }

    let mut canvas = Canvas::new(plan.canvas_w, plan.canvas_h);
    for idx in 0..n {
        match &extras[idx] {
            NodeExtra::Frame(sub) => draw_frame(&mut canvas, &placed[idx], &graph.nodes[idx].label, sub),
            NodeExtra::Compartments(sections) => draw_class_box(&mut canvas, &placed[idx], sections),
            NodeExtra::Plain => draw_box(&mut canvas, &placed[idx], &wrapped[idx], graph.nodes[idx].shape),
        }
    }

    for (i, edge) in graph.edges.iter().enumerate() {
        canvas.cur_style = match edge.line {
            LineKind::Dotted => STY_DOT,
            LineKind::Thick => STY_THICK,
            LineKind::Solid => STY_SOLID,
        };
        if edge.from == edge.to {
            route_self(&mut canvas, &placed[edge.from], edge);
            continue;
        }
        let from = placed[edge.from];
        let to = placed[edge.to];
        let adjacent = to.rank == from.rank + 1;
        let bus = plan.band_end[from.rank as usize] + plan.edge_bus[i];
        let lane = plan.lane_base + plan.edge_lane[i];
        if vertical {
            if adjacent {
                route_forward(&mut canvas, &from, &to, edge, bus);
            } else {
                route_back(&mut canvas, &from, &to, edge, lane);
            }
        } else if adjacent {
            route_forward_lr(&mut canvas, &from, &to, edge, bus);
        } else {
            route_back_lr(&mut canvas, &from, &to, edge, lane);
        }
    }

    canvas.finalize_mask();
    Some(canvas)
}

/// Apply the direction flip a finished canvas needs for `BT` / `RL`.
fn orient(mut canvas: Canvas, dir: Dir) -> Canvas {
    if dir == Dir::Up {
        canvas.flip_vertical();
    } else if dir == Dir::Left {
        canvas.flip_horizontal();
    }
    canvas
}

/// Flowchart and state diagrams: plain boxes, no extra content.
pub(crate) fn layout_flowchart(graph: &Graph) -> Option<Canvas> {
    let extras: Vec<NodeExtra> = graph.nodes.iter().map(|_| NodeExtra::Plain).collect();
    layout_canvas(graph, &extras).map(|c| orient(c, graph.dir))
}

/// Class and ER diagrams: boxes divided into title / attribute / method rows.
pub(crate) fn layout_class(graph: &Graph, infos: &[ClassInfo]) -> Option<Canvas> {
    let extras: Vec<NodeExtra> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let mut title: Vec<String> = Vec::new();
            if let Some(annotation) = &infos[i].annotation {
                title.push(format!("«{annotation}»"));
            }
            title.push(display_generics(&node.label));
            NodeExtra::Compartments(vec![title, infos[i].attrs.clone(), infos[i].methods.clone()])
        })
        .collect();
    layout_canvas(graph, &extras).map(|c| orient(c, graph.dir))
}

// -------------------------------------------------------------------- groups

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ItemKey {
    Node(usize),
    Group(usize),
}

type ScopeEdges = HashMap<Option<usize>, Vec<(ItemKey, ItemKey, usize)>>;

/// Lay out a flowchart that uses `subgraph`.
pub(crate) fn layout_grouped(graph: &Graph) -> Option<Canvas> {
    // A node whose id matches a subgraph id stands in for that subgraph.
    let mut proxy: HashMap<usize, usize> = HashMap::new();
    for (gi, g) in graph.groups.iter().enumerate() {
        if let Some(&ni) = graph.index.get(&g.id) {
            proxy.insert(ni, gi);
        }
    }

    let group_chain = |g: Option<usize>| -> Vec<usize> {
        let mut chain = Vec::new();
        let mut cur = g;
        while let Some(c) = cur {
            chain.push(c);
            cur = graph.groups[c].parent;
        }
        chain.reverse();
        chain
    };
    let endpoint = |n: usize| -> (ItemKey, Vec<usize>) {
        match proxy.get(&n) {
            None => (ItemKey::Node(n), group_chain(graph.node_group[n])),
            Some(&gi) => (ItemKey::Group(gi), group_chain(graph.groups[gi].parent)),
        }
    };

    // Edges bucketed by the scope that draws them; `None` is the top level.
    let mut scope_edges: ScopeEdges = HashMap::new();
    let mut referenced = vec![false; graph.groups.len()];
    for (ei, e) in graph.edges.iter().enumerate() {
        let (f_key, f_chain) = endpoint(e.from);
        let (t_key, t_chain) = endpoint(e.to);
        let mut k = 0;
        while k < f_chain.len() && k < t_chain.len() && f_chain[k] == t_chain[k] {
            k += 1;
        }
        let scope = if k == 0 { None } else { Some(f_chain[k - 1]) };
        let f_key = if f_chain.len() > k { ItemKey::Group(f_chain[k]) } else { f_key };
        let t_key = if t_chain.len() > k { ItemKey::Group(t_chain[k]) } else { t_key };
        for key in [f_key, t_key] {
            if let ItemKey::Group(g) = key {
                referenced[g] = true;
            }
        }
        scope_edges.entry(scope).or_default().push((f_key, t_key, ei));
    }

    let mut direct_nodes: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for (ni, &g) in graph.node_group.iter().enumerate() {
        if proxy.contains_key(&ni) {
            continue;
        }
        direct_nodes.entry(g).or_default().push(ni);
    }

    // Drop empty subgraphs, but keep any that an edge attaches to.
    let mut keep = vec![false; graph.groups.len()];
    for gi in (0..graph.groups.len()).rev() {
        let has_nodes = direct_nodes.get(&Some(gi)).is_some_and(|l| !l.is_empty());
        let has_children = graph.groups.iter().enumerate().any(|(c, g)| g.parent == Some(gi) && keep[c]);
        keep[gi] = has_nodes || has_children || referenced[gi];
    }

    build_scope(graph, None, &scope_edges, &direct_nodes, &keep).map(|c| orient(c, graph.dir))
}

fn build_scope(
    graph: &Graph,
    scope: Option<usize>,
    scope_edges: &ScopeEdges,
    direct_nodes: &HashMap<Option<usize>, Vec<usize>>,
    keep: &[bool],
) -> Option<Canvas> {
    let mut items: Vec<ItemKey> =
        direct_nodes.get(&scope).map(|l| l.iter().map(|&n| ItemKey::Node(n)).collect()).unwrap_or_default();
    items.extend((0..graph.groups.len()).filter(|&gi| graph.groups[gi].parent == scope && keep[gi]).map(ItemKey::Group));

    if items.is_empty() {
        return Some(Canvas::new(1, 1));
    }

    let mut index_of: HashMap<ItemKey, usize> = HashMap::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut extras: Vec<NodeExtra> = Vec::new();
    for item in items {
        index_of.insert(item, nodes.len());
        match item {
            ItemKey::Node(i) => {
                nodes.push(Node { label: graph.nodes[i].label.clone(), shape: graph.nodes[i].shape });
                extras.push(NodeExtra::Plain);
            }
            ItemKey::Group(i) => {
                let sub = build_scope(graph, Some(i), scope_edges, direct_nodes, keep)?;
                nodes.push(Node { label: graph.groups[i].label.clone(), shape: Shape::Rect });
                extras.push(NodeExtra::Frame(sub));
            }
        }
    }

    let mut edges: Vec<Edge> = Vec::new();
    for &(f, t, ei) in scope_edges.get(&scope).map(Vec::as_slice).unwrap_or(&[]) {
        let (Some(&fi), Some(&ti)) = (index_of.get(&f), index_of.get(&t)) else { continue };
        let e = &graph.edges[ei];
        edges.push(Edge { from: fi, to: ti, ..e.clone() });
    }

    // Layout only reads nodes/edges/dir, so a bare Graph carrying those is enough.
    let mut synth = Graph::new(graph.dir);
    synth.nodes = nodes;
    synth.edges = edges;
    layout_canvas(&synth, &extras)
}

// ------------------------------------------------------------------- drawing

pub(crate) fn draw_box(canvas: &mut Canvas, p: &Placed, lines: &[String], shape: Shape) {
    let Placed { x, y, w, h, .. } = *p;
    let right = x + w - 1;
    let bottom = y + h - 1;

    let rounded = shape == Shape::Round || shape == Shape::Diamond;
    canvas.set(x, y, if rounded { "╭" } else { "┌" }, Cls::Border);
    canvas.set(right, y, if rounded { "╮" } else { "┐" }, Cls::Border);
    canvas.set(x, bottom, if rounded { "╰" } else { "└" }, Cls::Border);
    canvas.set(right, bottom, if rounded { "╯" } else { "┘" }, Cls::Border);

    // The perimeter is drawn as bits so edges can tee into it, but it is the box outline.
    for cx in x + 1..right {
        canvas.add_bits(cx, y, L | R, Cls::Border);
        canvas.add_bits(cx, bottom, L | R, Cls::Border);
    }
    for cy in y + 1..bottom {
        canvas.add_bits(x, cy, U | D, Cls::Border);
        canvas.add_bits(right, cy, U | D, Cls::Border);
    }

    for cy in y..=bottom {
        for cx in x..=right {
            canvas.set_occupied(cx, cy);
        }
    }

    let inner = sat(w, 2 * PAD + 2).max(1);
    for (li, line) in lines.iter().enumerate() {
        let text = fit_label(line, inner as usize);
        let text_x = x + 1 + PAD + half(sat(inner, sw(&text)));
        draw_text(canvas, &text, text_x, y + 1 + li as i64, Cls::Text);
    }
}

/// A class or ER box: sections separated by horizontal rules, title centred.
fn draw_class_box(canvas: &mut Canvas, p: &Placed, sections: &[Vec<String>]) {
    draw_box(canvas, p, &[], Shape::Rect);
    let inner = sat(p.w, 2 * PAD + 2).max(1);
    let mut row = p.y + 1;
    let mut first = true;
    for (si, section) in sections.iter().enumerate() {
        if section.is_empty() {
            continue;
        }
        if !first {
            canvas.set(p.x, row, "├", Cls::Border);
            for x in p.x + 1..p.x + p.w - 1 {
                canvas.set(x, row, "─", Cls::Border);
            }
            canvas.set(p.x + p.w - 1, row, "┤", Cls::Border);
            row += 1;
        }
        first = false;
        for line in section {
            let text = fit_label(line, inner as usize);
            let tx = if si == 0 { p.x + 1 + PAD + half(sat(inner, sw(&text))) } else { p.x + 1 + PAD };
            draw_text_over_edges(canvas, &text, tx, row, Cls::Text);
            row += 1;
        }
    }
}

/// A subgraph frame: a titled box with a finished sub-canvas centred inside.
fn draw_frame(canvas: &mut Canvas, p: &Placed, title: &str, sub: &Canvas) {
    draw_box(canvas, p, &[], Shape::Rect);
    let t = fit_label(title, sat(p.w, 4) as usize);
    draw_text_over_edges(canvas, &format!(" {t} "), p.x + 1, p.y, Cls::Text);
    canvas.blit(sub, p.x + 1 + half(p.w - 2 - sub.w), p.y + 1 + half(p.h - 2 - sub.h));
}

// ------------------------------------------------------------------- routing

fn head_glyph(head: Head, arrow: &'static str) -> &'static str {
    match head {
        Head::Circle => "o",
        Head::Cross => "×",
        Head::DiamondFill => "◆",
        Head::DiamondOpen => "◇",
        Head::Triangle => match arrow {
            "▼" => "▽",
            "▲" => "△",
            "◄" => "◁",
            "▶" => "▷",
            _ => arrow,
        },
        Head::None | Head::Arrow => arrow,
    }
}

/// Adjacent ranks, top-down: drop, jog along the bus row, drop into the head.
fn route_forward(canvas: &mut Canvas, from: &Placed, to: &Placed, edge: &Edge, bus: i64) {
    let tx = to.cx;
    // A jog of one column reads as a kink; snap straight instead.
    let bx = if (from.cx - tx).abs() <= 1 { tx } else { from.cx };
    let by = from.y + from.h - 1;
    let head_row = to.y - 1;

    canvas.junction(bx, by, D);
    canvas.seg_v(bx, by, bus);
    if bx == tx {
        canvas.seg_v(bx, bus, head_row);
    } else {
        canvas.seg_h(bus, bx, tx);
        canvas.seg_v(tx, bus, head_row);
    }

    if edge.head_to == Head::None {
        canvas.add_bits(tx, head_row, U, Cls::Edge);
    } else {
        canvas.set(tx, head_row, head_glyph(edge.head_to, "▼"), Cls::Edge);
    }
    if edge.head_from != Head::None {
        canvas.set(bx, by, head_glyph(edge.head_from, "▲"), Cls::Edge);
    }

    if let Some(label) = &edge.label {
        place_label(canvas, label, head_row, tx + 1);
    }
}

/// A self-edge: a stub loop hanging below the box.
fn route_self(canvas: &mut Canvas, p: &Placed, edge: &Edge) {
    let bottom = p.y + p.h - 1;
    let exit_x = p.cx + 1;
    let ret_x = p.x + p.w - 2;
    if ret_x <= exit_x || bottom + 2 >= canvas.h {
        return;
    }

    let (v, h, bl, br) = match edge.line {
        LineKind::Dotted => ("╎", "╌", "╰", "╯"),
        LineKind::Thick => ("┃", "━", "┗", "┛"),
        LineKind::Solid => ("│", "─", "╰", "╯"),
    };

    canvas.junction(exit_x, bottom, D);
    canvas.set(exit_x, bottom + 1, v, Cls::Edge);
    canvas.set(exit_x, bottom + 2, bl, Cls::Edge);
    for x in exit_x + 1..ret_x {
        canvas.set(x, bottom + 2, h, Cls::Edge);
    }
    canvas.set(ret_x, bottom + 2, br, Cls::Edge);
    canvas.set(ret_x, bottom + 1, head_glyph(edge.head_to, "▲"), Cls::Edge);
    if let Some(label) = &edge.label {
        place_label(canvas, label, bottom + 1, p.x + p.w + 1);
    }
}

/// Skip or back edge, top-down: out the right side, up a lane, back in.
fn route_back(canvas: &mut Canvas, from: &Placed, to: &Placed, edge: &Edge, lane_x: i64) {
    let sx = from.x + from.w - 1;
    let sy = from.cy;
    let tx = to.x + to.w - 1;
    let tyc = to.cy;

    canvas.junction(sx, sy, R);
    canvas.seg_h(sy, sx, lane_x);
    canvas.seg_v(lane_x, sy, tyc);
    canvas.seg_h(tyc, tx + 1, lane_x);

    if edge.head_to == Head::None {
        canvas.add_bits(tx + 1, tyc, R, Cls::Edge);
    } else {
        canvas.set(tx + 1, tyc, head_glyph(edge.head_to, "◄"), Cls::Edge);
    }
    if edge.head_from != Head::None {
        canvas.set(sx, sy, head_glyph(edge.head_from, "◄"), Cls::Edge);
    }

    if let Some(label) = &edge.label {
        place_label(canvas, label, sat(tyc, 1), sat(lane_x, sw(label) + 1));
    }
}

/// Adjacent ranks, left-to-right: out the right side, jog on the bus column.
fn route_forward_lr(canvas: &mut Canvas, from: &Placed, to: &Placed, edge: &Edge, bus: i64) {
    let rx = from.x + from.w - 1;
    let ry = from.cy;
    let ly = to.cy;
    let head_col = to.x - 1;

    canvas.junction(rx, ry, R);
    canvas.seg_h(ry, rx, bus);
    if ry == ly {
        canvas.seg_h(ry, bus, head_col);
    } else {
        canvas.seg_v(bus, ry, ly);
        canvas.seg_h(ly, bus, head_col);
    }

    if edge.head_to == Head::None {
        canvas.add_bits(head_col, ly, R, Cls::Edge);
    } else {
        canvas.set(head_col, ly, head_glyph(edge.head_to, "▶"), Cls::Edge);
    }
    if edge.head_from != Head::None {
        canvas.set(rx, ry, head_glyph(edge.head_from, "◄"), Cls::Edge);
    }

    if let Some(label) = &edge.label {
        place_label(canvas, label, sat(ly, 1), bus + 1);
    }
}

/// Skip or back edge, left-to-right: down out the bottom, along a lane, back up.
fn route_back_lr(canvas: &mut Canvas, from: &Placed, to: &Placed, edge: &Edge, lane_y: i64) {
    let sx = from.cx;
    let sy = from.y + from.h - 1;
    let tx = to.cx;
    let ty = to.y + to.h - 1;

    canvas.junction(sx, sy, D);
    canvas.seg_v(sx, sy, lane_y);
    canvas.seg_h(lane_y, sx, tx);
    canvas.seg_v(tx, lane_y, ty + 1);

    if edge.head_to == Head::None {
        canvas.add_bits(tx, ty + 1, D, Cls::Edge);
    } else {
        canvas.set(tx, ty + 1, head_glyph(edge.head_to, "▲"), Cls::Edge);
    }
    if edge.head_from != Head::None {
        canvas.set(sx, sy, head_glyph(edge.head_from, "▲"), Cls::Edge);
    }

    if let Some(label) = &edge.label {
        place_label(canvas, label, sat(lane_y, 1), half(sx + tx));
    }
}

/// Write an edge label, stopping at the first cell already occupied.
fn place_label(canvas: &mut Canvas, label: &str, row: i64, start_x: i64) {
    if row >= canvas.h {
        return;
    }
    let text = fit_label(label, MAX_LABEL);
    let mut x = start_x;
    for (c, cw) in measured(&text) {
        if cw == 0 {
            continue;
        }
        let cw = cw as i64;
        if x + cw > canvas.w {
            break;
        }
        let mut blocked = false;
        for k in 0..cw {
            match canvas.cell(x + k, row) {
                Some(i) => {
                    if canvas.ch[i] != " " || canvas.mask[i] != 0 || canvas.occupied[i] != 0 {
                        blocked = true;
                    }
                }
                // Reading outside the grid yields `undefined`, which is not a blank cell.
                None => blocked = true,
            }
        }
        if blocked {
            break;
        }
        canvas.set(x, row, c, Cls::EdgeLabel);
        for k in 1..cw {
            canvas.set(x + k, row, CONT, Cls::EdgeLabel);
        }
        x += cw;
    }
}
