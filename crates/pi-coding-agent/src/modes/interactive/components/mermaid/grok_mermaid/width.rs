//! Display width, measured in grapheme clusters (grok-mermaid `width.ts`).
//!
//! A cluster is the unit both of measuring and of painting, so a box is always
//! sized for exactly what gets drawn into it. Clustering is UAX #29 (pi-tui's
//! grapheme segmentation, like `Intl.Segmenter`); per-code-point widths come
//! from the generated table.

use super::width_data::WIDTHS;

const VS16: u32 = 0xfe0f;

fn is_regional_indicator(cp: u32) -> bool {
    (0x1f1e6..=0x1f1ff).contains(&cp)
}

/// Width of one code point; the table covers the whole code point space.
fn code_point_width(cp: u32) -> usize {
    let (mut lo, mut hi) = (0i64, WIDTHS.len() as i64 - 1);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let run = WIDTHS[mid as usize];
        if cp < run.0 {
            hi = mid - 1;
        } else if cp > run.1 {
            lo = mid + 1;
        } else {
            return run.2 as usize;
        }
    }
    1
}

/// Columns occupied by one grapheme cluster.
///
/// The widest code point wins, so a base plus its combining marks measures as
/// the base. A variation selector requesting emoji presentation forces two
/// columns, as does a regional indicator pair (a flag). Zero is a real answer.
pub(crate) fn cluster_width(cluster: &str) -> usize {
    let mut w = 0;
    let mut vs16 = false;
    let mut regional = 0;
    for ch in cluster.chars() {
        let cp = ch as u32;
        if cp == VS16 {
            vs16 = true;
        }
        if is_regional_indicator(cp) {
            regional += 1;
        }
        w = w.max(code_point_width(cp));
    }
    if vs16 || regional >= 2 { 2 } else { w }
}

/// Grapheme clusters paired with their display width.
pub(crate) fn measured(s: &str) -> impl Iterator<Item = (&str, usize)> {
    pi_tui::utils::graphemes(s).map(|g| (g, cluster_width(g)))
}

/// Display columns of a string.
pub(crate) fn string_width(s: &str) -> usize {
    pi_tui::utils::graphemes(s).map(cluster_width).sum()
}
