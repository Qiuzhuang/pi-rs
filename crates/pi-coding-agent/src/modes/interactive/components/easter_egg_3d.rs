//! Port of `packages/coding-agent/src/modes/interactive/components/easter-egg-3d.ts` (and the entry points of
//! `easter-egg-3d.lazy.ts`, which only defers loading in TS).
//!
//! Fullscreen 3D easter eggs: the pi logo (header logo click) and Armin (/arminsayshi). Both are bitmaps built from
//! one block per pixel. The current screen dissolves into braille dust while the model spins in the center and its
//! blocks play a sliding puzzle. Leaving plays the same timeline backwards, so the screen reassembles.
//!
//! The pi logo lifts off the header, flies to the center, and grows; the dust spreads out from the header logo.
//! Armin grows out of a speck at the center; the dust spreads out from the center.
//!
//! The blocks are ray cast per braille dot. A braille cell holds 2x4 roughly square dots, so one pixel of a
//! half-block bitmap is exactly 2x2 dots, and the header logo (4x2 cells) is 8x8 dots when it lifts off.

use crate::modes::interactive::components::armin::{ARMIN_HEIGHT, ARMIN_WIDTH, is_armin_pixel};
use crate::modes::interactive::components::keybinding_hints::format_key_text;
use crate::modes::interactive::theme::theme::{TerminalTheme, ThemeToken, theme};
use futures::FutureExt;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use pi_tui::colors::{Color, background_ansi, color_to_rgb, foreground_ansi, indexed_color, rgb_color};
use pi_tui::keybindings::get_keybindings;
use pi_tui::tui::{
    Component, MouseEvent, MouseEventType, MouseResult, OverlayAnchor, OverlayHandle, OverlayOptions, Tui, TuiMode,
};
use pi_tui::utils::{graphemes, visible_width};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Weak};
use std::time::{Duration, Instant};

pub type Rgb = [f64; 3];

#[derive(Debug, Clone)]
pub struct ScreenCell {
    pub text: String,
    /// 1 or 2 for a grapheme, 0 for the second column of a wide grapheme.
    pub width: usize,
    pub fg: Rgb,
    /// None when the cell uses the terminal's default background.
    pub bg: Option<Rgb>,
    /// Seconds until the cell turns into braille dust.
    pub delay: f64,
    /// Number of braille dots the glyph turns into.
    pub ink: i64,
    pub seed: f64,
}

#[derive(Debug, Clone)]
pub struct Star {
    /// Braille character with the single dot the star occupies.
    pub glyph: char,
    /// Color the star leans toward; it only ever shows a small step from the background toward it.
    pub tint: Rgb,
    /// Largest step from the background toward the tint, from 0 to 1.
    pub strength: f64,
    pub phase: f64,
    /// Flicker speed in radians per second.
    pub speed: f64,
}

#[derive(Debug, Clone)]
pub struct Box {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub color: Rgb,
}

#[derive(Debug, Clone)]
pub struct Pose {
    pub center_x: f64,
    pub center_y: f64,
    /// Braille dots per model pixel at depth 0.
    pub scale: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
}

const DEPTH: f64 = 0.7;
const FLY_START: f64 = 0.1;
const FLY_DURATION: f64 = 1.3;
/// Braille dots per pixel when a model without an origin appears, growing from a speck at the center.
const START_SCALE: f64 = 0.1;

/// Grid position of a block: column and row in the bitmap, and layer (-1, 0, 1) in depth.
pub type Cell3 = [i64; 3];

#[derive(Debug, Clone)]
pub struct ModelBlocks {
    pub home: Cell3,
    pub color: Rgb,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOrigin {
    pub column: i64,
    pub row: i64,
}

/// A bitmap built from one block per foreground pixel. Each is a block that the puzzle slides around.
#[derive(Clone)]
pub struct Model {
    /// Bitmap size in pixels.
    pub columns: i64,
    pub rows: i64,
    pub blocks: Vec<ModelBlocks>,
    /// Camera distance from the model's center, in pixels.
    pub camera_distance: f64,
    /// Farthest distance of any block corner from the center, with blocks on the outer depth layers.
    pub radius: f64,
    /// Largest share of the screen width the spinning model covers.
    pub width_share: f64,
    /// Number of blocks the puzzle moves per step, given a random number from 0 to 1.
    pub puzzle_moves: Arc<dyn Fn(f64) -> i64 + Send + Sync>,
    /// Top-left cell of the model's half-block rendering on screen. The model lifts off from there and replaces it.
    /// None to grow out of the center.
    pub origin: Option<ModelOrigin>,
}

fn create_model(
    columns: i64,
    rows: i64,
    pixel: impl Fn(i64, i64) -> Option<Rgb>,
    camera_distance: f64,
    width_share: f64,
    origin: Option<ModelOrigin>,
    puzzle_moves: Arc<dyn Fn(f64, usize) -> i64 + Send + Sync>,
) -> Model {
    let mut blocks = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            if let Some(color) = pixel(column, row) {
                blocks.push(ModelBlocks { home: [column, row, 0], color });
            }
        }
    }
    let block_count = blocks.len();
    let half_x = columns as f64 / 2.0;
    let half_y = rows as f64 / 2.0;
    let half_z = 1.0 + DEPTH / 2.0;
    Model {
        columns,
        rows,
        blocks,
        camera_distance,
        radius: (half_x * half_x + half_y * half_y + half_z * half_z).sqrt(),
        width_share,
        puzzle_moves: Arc::new(move |random| puzzle_moves(random, block_count)),
        origin,
    }
}

const CORAL: Rgb = [228.0, 138.0, 122.0];
const BLUE: Rgb = [79.0, 142.0, 179.0];
const YELLOW: Rgb = [234.0, 182.0, 93.0];
const PI_LOGO_PIXELS: [&str; 4] = ["ccc.", "b.c.", "bb.y", "b..y"];

fn pi_logo_model(origin: ModelOrigin) -> Model {
    create_model(
        4,
        4,
        |column, row| match PI_LOGO_PIXELS[row as usize].as_bytes()[column as usize] {
            b'c' => Some(CORAL),
            b'b' => Some(BLUE),
            b'y' => Some(YELLOW),
            _ => None,
        },
        10.0,
        0.35,
        Some(origin),
        Arc::new(|random, _| if random < 0.4 { 2 } else { 1 }),
    )
}

fn armin_model(color: Rgb) -> Model {
    create_model(
        ARMIN_WIDTH,
        ARMIN_HEIGHT,
        move |column, row| if is_armin_pixel(column, row) { Some(color) } else { None },
        // Far enough that the perspective stays mild for a figure about 36 blocks tall.
        80.0,
        0.45,
        None,
        Arc::new(|random, block_count| js_round(block_count as f64 * (0.03 + random * 0.04)) as i64),
    )
}

/// `Math.round`.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

// The sliding puzzle: after the model spun for a while, blocks slide into free neighboring cells, a few at a time.
// Each cycle shuffles, flies every block back home, and holds the model briefly.
const PUZZLE_START: f64 = FLY_START + FLY_DURATION + 3.0;
const PUZZLE_STEP: f64 = 0.3;
const PUZZLE_STEPS: usize = 12;
const PUZZLE_RETURN: f64 = 1.0;
const PUZZLE_HOLD: f64 = 0.6;
const PUZZLE_CYCLE: f64 = PUZZLE_STEP * PUZZLE_STEPS as f64 + PUZZLE_RETURN + PUZZLE_HOLD;
const PUZZLE_MOVES: [Cell3; 10] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

/// Block positions after each step of one shuffle cycle, starting at home. Deterministic per cycle.
fn shuffle_steps(model: &Model, cycle: i64) -> Vec<Vec<Cell3>> {
    let columns = model.columns;
    let rows = model.rows;
    let mut positions: Vec<Cell3> = model.blocks.iter().map(|b| b.home).collect();
    let mut steps = vec![positions.clone()];
    let mut last_moved: HashSet<usize> = HashSet::new();
    let mut random = cycle as f64 * 7_919.0 + 1.0;
    let mut next = || {
        let value = hash(random);
        random += 1.0;
        value
    };
    let key = |[x, y, z]: Cell3| ((z + 1) * rows + y) * columns + x;
    for _ in 0..PUZZLE_STEPS {
        let mut occupied: HashSet<i64> = positions.iter().map(|p| key(*p)).collect();
        let mut next_positions = positions.clone();
        let mut moved: Vec<usize> = Vec::new();
        let move_count = (model.puzzle_moves)(next());
        for _ in 0..move_count {
            let mut candidates: Vec<(usize, Cell3)> = Vec::new();
            for (block, &[x, y, z]) in positions.iter().enumerate() {
                if moved.contains(&block) {
                    continue;
                }
                for [dx, dy, dz] in PUZZLE_MOVES {
                    let target: Cell3 = [x + dx, y + dy, z + dz];
                    if target[0] < 0 || target[0] >= columns || target[1] < 0 || target[1] >= rows {
                        continue;
                    }
                    if target[2] < -1 || target[2] > 1 || occupied.contains(&key(target)) {
                        continue;
                    }
                    candidates.push((block, target));
                }
            }
            // Prefer blocks that did not just move, so the puzzle does not look like one block jittering.
            let fresh: Vec<(usize, Cell3)> =
                candidates.iter().copied().filter(|(block, _)| !last_moved.contains(block)).collect();
            let pool = if !fresh.is_empty() { fresh } else { candidates };
            let pick = (next() * pool.len() as f64).floor() as usize;
            let Some(&(block, target)) = pool.get(pick) else { break };
            occupied.insert(key(target));
            next_positions[block] = target;
            moved.push(block);
        }
        last_moved.clear();
        last_moved.extend(moved.iter().copied());
        positions = next_positions;
        steps.push(positions.clone());
    }
    steps
}

const FRAME_MS: f64 = 1000.0 / 30.0;
const DUST_DURATION: f64 = 0.35;
const WAVE_SPREAD: f64 = 0.55;
const WAVE_JITTER: f64 = 0.12;
const DISSOLVE_END: f64 = FLY_START + WAVE_SPREAD + WAVE_JITTER + DUST_DURATION;
const BACKGROUND_FADE: f64 = 0.5;
const EXIT_DURATION: f64 = 1.1;
// A faint starfield fades in behind the logo once the screen has dissolved.
const STARS_START: f64 = 2.5;
const STARS_FADE: f64 = 2.0;
const STAR_DENSITY: f64 = 0.018;
// Star tints: neutral, pale blue, pale gold, and pale rose, each mixed half with the terminal's foreground.
const STAR_TINTS: [Rgb; 4] = [[255.0, 255.0, 255.0], [170.0, 195.0, 255.0], [255.0, 225.0, 170.0], [255.0, 190.0, 205.0]];
const LOGO_COLOR_STEP: f64 = 5.0;
// On light backgrounds most of each braille cell shows the bright background, which washes the logo out. There,
// the logo gets a halo: its colors and coverage blurred into the cell backgrounds, strongest under the logo and
// fading out over HALO_RADIUS_X columns and HALO_RADIUS_Y rows (cells are about twice as tall as wide).
const LIGHT_HALO_STRENGTH: f64 = 0.3;
const HALO_RADIUS_X: i64 = 6;
const HALO_RADIUS_Y: i64 = 3;
const HALO_LEVELS: f64 = 32.0;
const STAR_ALPHA_LEVELS: f64 = 8.0;
const STAR_FLICKER_LEVELS: f64 = 3.0;
const HINT_FADE: f64 = 0.5;

// Braille dot bits, indexed by row * 2 + column within the 2x4 cell.
const DOT_BITS: [u8; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];

fn braille(bits: u8) -> char {
    char::from_u32(0x2800 + bits as u32).unwrap_or(' ')
}

static LIGHT: LazyLock<[f64; 3]> = LazyLock::new(|| normalize([-0.45, -0.6, 0.75]));
static HALF_VECTOR: LazyLock<[f64; 3]> = LazyLock::new(|| normalize([LIGHT[0], LIGHT[1], LIGHT[2] + 1.0]));

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

fn clamp01(value: f64) -> f64 {
    if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

/// Quintic smoothstep.
fn smooth(value: f64) -> f64 {
    let t = clamp01(value);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn ease_out(value: f64) -> f64 {
    let t = 1.0 - clamp01(value);
    1.0 - t * t * t
}

fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    [a[0] + (b[0] - a[0]) * amount, a[1] + (b[1] - a[1]) * amount, a[2] + (b[2] - a[2]) * amount]
}

/// JS `ToInt32`.
fn to_int32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let m = value.trunc().rem_euclid(4_294_967_296.0);
    (m as u64 as u32) as i32
}

fn hash(value: f64) -> f64 {
    let mut x = (to_int32(value) ^ (0x9e3779b9u32 as i32)).wrapping_mul(0x85ebca6bu32 as i32);
    x ^= ((x as u32) >> 13) as i32;
    x = x.wrapping_mul(0xc2b2ae35u32 as i32);
    x ^= ((x as u32) >> 16) as i32;
    (x as u32) as f64 / 4_294_967_296.0
}

/// Whether a background is light, by relative luminance.
fn is_light([r, g, b]: Rgb) -> bool {
    0.2126 * r + 0.7152 * g + 0.0722 * b > 128.0
}

fn to_rgb(color: &Color) -> Rgb {
    let rgb = color_to_rgb(color);
    [rgb.r, rgb.g, rgb.b]
}

/// Braille dots a glyph turns into, a rough measure of how much ink it has.
fn glyph_ink(text: &str, seed: f64) -> i64 {
    if text.trim().is_empty() {
        return 0;
    }
    let mut chars = text.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if ".,:;'`-_·".contains(c) {
            return 2;
        }
        if ('\u{2500}'..='\u{257f}').contains(&c) {
            return 3;
        }
    }
    4 + (seed * 3.0).floor() as i64
}

/// Parse rendered lines into cells with resolved colors. Other escape sequences (OSC, APC, cursor) are skipped.
fn parse_screen(lines: &[String], width: usize, foreground: Rgb, background: Rgb) -> Vec<Vec<ScreenCell>> {
    let palette = |index: i64| -> Option<Rgb> { indexed_color(index).ok().map(|c| to_rgb(&c)) };
    let parse_int = |s: &str| -> Option<i64> {
        let t = s.trim_start();
        let end = t
            .char_indices()
            .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
            .map(|(i, _)| i)
            .unwrap_or(t.len());
        t[..end].parse().ok()
    };
    lines
        .iter()
        .map(|line| {
            let mut cells: Vec<ScreenCell> = Vec::new();
            let mut fg: Option<Rgb> = None;
            let mut bg: Option<Rgb> = None;
            let mut dim = false;
            let mut inverse = false;

            let mut apply_sgr = |params: &str, fg: &mut Option<Rgb>, bg: &mut Option<Rgb>| {
                let codes: Vec<&str> = if params.is_empty() { vec!["0"] } else { params.split(';').collect() };
                let mut i = 0;
                while i < codes.len() {
                    let parts: Vec<Option<i64>> = codes[i].split(':').map(parse_int).collect();
                    let Some(code) = parts.first().copied().unwrap_or(Some(0)) else {
                        i += 1;
                        continue;
                    };
                    if code == 38 || code == 48 {
                        // Extended color, either as `38;5;n` / `38;2;r;g;b` or colon sub-parameters.
                        let args: Vec<Option<i64>> = if parts.len() > 1 {
                            // `38:2::r:g:b` has an empty color space id, dropped here.
                            parts[1..].iter().copied().filter(|p| p.is_some()).collect()
                        } else {
                            let kind = codes.get(i + 1).and_then(|s| parse_int(s));
                            let count = match kind {
                                Some(5) => 2,
                                Some(2) => 4,
                                _ => 1,
                            };
                            let args = codes.iter().skip(i + 1).take(count).map(|s| parse_int(s)).collect();
                            i += count;
                            args
                        };
                        let color = match (args.first().copied().flatten(), args.get(1).copied().flatten()) {
                            (Some(5), Some(index)) => palette(index),
                            (Some(2), _) if args.len() >= 4 => match (args[1], args[2], args[3]) {
                                (Some(r), Some(g), Some(b)) => Some([r as f64, g as f64, b as f64]),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(color) = color {
                            if code == 38 {
                                *fg = Some(color);
                            } else {
                                *bg = Some(color);
                            }
                        }
                    } else if code == 0 {
                        *fg = None;
                        *bg = None;
                        dim = false;
                        inverse = false;
                    } else if code == 2 {
                        dim = true;
                    } else if code == 22 {
                        dim = false;
                    } else if code == 7 {
                        inverse = true;
                    } else if code == 27 {
                        inverse = false;
                    } else if (30..=37).contains(&code) {
                        *fg = palette(code - 30);
                    } else if (90..=97).contains(&code) {
                        *fg = palette(code - 90 + 8);
                    } else if code == 39 {
                        *fg = None;
                    } else if (40..=47).contains(&code) {
                        *bg = palette(code - 40);
                    } else if (100..=107).contains(&code) {
                        *bg = palette(code - 100 + 8);
                    } else if code == 49 {
                        *bg = None;
                    }
                    i += 1;
                }
                (dim, inverse)
            };

            let mut state_dim = false;
            let mut state_inverse = false;
            let push_text = |text: &str,
                             cells: &mut Vec<ScreenCell>,
                             fg: Option<Rgb>,
                             bg: Option<Rgb>,
                             dim: bool,
                             inverse: bool| {
                for segment in graphemes(text) {
                    let glyph_width = visible_width(segment);
                    if glyph_width == 0 || cells.len() + glyph_width > width {
                        continue;
                    }
                    let mut cell_fg = fg.unwrap_or(foreground);
                    let mut cell_bg = bg;
                    if inverse {
                        cell_fg = bg.unwrap_or(background);
                        cell_bg = Some(fg.unwrap_or(foreground));
                    }
                    if dim {
                        cell_fg = mix(cell_fg, cell_bg.unwrap_or(background), 0.4);
                    }
                    let cell = ScreenCell {
                        text: segment.to_string(),
                        width: glyph_width,
                        fg: cell_fg,
                        bg: cell_bg,
                        delay: 0.0,
                        ink: 0,
                        seed: 0.0,
                    };
                    let wide = glyph_width == 2;
                    cells.push(cell.clone());
                    if wide {
                        cells.push(ScreenCell { text: String::new(), width: 0, ..cell });
                    }
                }
            };

            let bytes = line.as_bytes();
            let mut i = 0;
            while i < line.len() {
                let Some(rel) = line[i..].find('\x1b') else {
                    push_text(&line[i..], &mut cells, fg, bg, state_dim, state_inverse);
                    break;
                };
                let sequence_start = i + rel;
                if sequence_start > i {
                    push_text(&line[i..sequence_start], &mut cells, fg, bg, state_dim, state_inverse);
                }
                let kind = bytes.get(sequence_start + 1).copied();
                match kind {
                    Some(b'[') => {
                        let mut end = sequence_start + 2;
                        while end < bytes.len() && (bytes[end] < 0x40 || bytes[end] > 0x7e) {
                            end += 1;
                        }
                        if bytes.get(end) == Some(&b'm') {
                            let (d, inv) = apply_sgr(&line[sequence_start + 2..end], &mut fg, &mut bg);
                            state_dim = d;
                            state_inverse = inv;
                        }
                        i = end + 1;
                    }
                    Some(b']') | Some(b'_') | Some(b'P') | Some(b'^') => {
                        // String sequences end at BEL or ST (ESC \).
                        let rest = &line[sequence_start + 2..];
                        let bell = rest.find('\x07').map(|p| p + sequence_start + 2);
                        let st = rest.find("\x1b\\").map(|p| p + sequence_start + 2);
                        match (bell, st) {
                            (None, None) => break,
                            (Some(b), None) => i = b + 1,
                            (Some(b), Some(s)) if b < s => i = b + 1,
                            (_, Some(s)) => i = s + 2,
                        }
                    }
                    _ => {
                        i = sequence_start + 2;
                    }
                }
                // Keep `i` on a char boundary.
                while i < line.len() && !line.is_char_boundary(i) {
                    i += 1;
                }
            }
            cells
        })
        .collect()
}

/// Row-major 3x3 rotation matrix for yaw (y), then pitch (x), then roll (z).
fn rotation(yaw: f64, pitch: f64, roll: f64) -> [f64; 9] {
    let (sy, cy, sx, cx, sz, cz) = (yaw.sin(), yaw.cos(), pitch.sin(), pitch.cos(), roll.sin(), roll.cos());
    // Rz * Rx * Ry
    let ry = [cy, 0.0, sy, 0.0, 1.0, 0.0, -sy, 0.0, cy];
    let rx = [1.0, 0.0, 0.0, 0.0, cx, -sx, 0.0, sx, cx];
    let rz = [cz, -sz, 0.0, sz, cz, 0.0, 0.0, 0.0, 1.0];
    multiply(&rz, &multiply(&rx, &ry))
}

fn multiply(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    let mut result = [0.0; 9];
    for row in 0..3 {
        for column in 0..3 {
            result[row * 3 + column] =
                a[row * 3] * b[column] + a[row * 3 + 1] * b[3 + column] + a[row * 3 + 2] * b[6 + column];
        }
    }
    result
}

// The spin is locked to the puzzle: one turn per cycle, facing the camera in the middle of each hold.
const SPIN_RAMP: f64 = 1.4;
const SPIN_SPEED: f64 = (PI * 2.0) / PUZZLE_CYCLE;
// How much the spin slows down while facing the camera (and speeds up while facing away), from 0 to 1.
const SPIN_LINGER: f64 = 0.6;
const FIRST_FRONT_VIEW: f64 = PUZZLE_START + PUZZLE_STEP * PUZZLE_STEPS as f64 + PUZZLE_RETURN + PUZZLE_HOLD / 2.0;

/// Uniform spin that accelerates from rest over SPIN_RAMP, then turns at SPIN_SPEED.
fn base_spin_phase(time: f64) -> f64 {
    let elapsed = time - FLY_START;
    if elapsed <= 0.0 {
        return 0.0;
    }
    let u = elapsed / SPIN_RAMP;
    // Integral of the quintic smoothstep ramp.
    if u < 1.0 {
        return SPIN_SPEED * SPIN_RAMP * (u.powi(6) - 3.0 * u.powi(5) + 2.5 * u.powi(4));
    }
    SPIN_SPEED * (SPIN_RAMP * 0.5 + elapsed - SPIN_RAMP)
}

// Extra rotation added during the flight, so the phase is a whole number of turns at every assembled front view.
static SPIN_ALIGNMENT: LazyLock<f64> = LazyLock::new(|| PI * 2.0 - (base_spin_phase(FIRST_FRONT_VIEW) % (PI * 2.0)));

/// Spin phase: whole turns exactly when the reassembled logo faces the camera.
fn spin_phase(time: f64) -> f64 {
    base_spin_phase(time) + *SPIN_ALIGNMENT * smooth((time - FLY_START) / FLY_DURATION)
}

/// Yaw of the logo. It matches the phase at whole turns but lingers there, so the logo reads from the front.
fn spin_angle(time: f64) -> f64 {
    let phase = spin_phase(time);
    phase - SPIN_LINGER * phase.sin()
}

#[derive(Debug, Clone)]
pub struct Face {
    pub axis: usize,
    /// Plane coordinate on `axis` in object space.
    pub plane: f64,
    pub u_min: f64,
    pub u_max: f64,
    pub v_min: f64,
    pub v_max: f64,
    /// Projected bounds in braille dots, inclusive.
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
}

/// Renders the model's blocks into braille cells. Only faces that point at the camera and are not covered by a
/// touching block are drawn. Each face is rasterized over its projected bounds by intersecting each dot's ray
/// with the face's plane, with a depth buffer resolving overlaps. Buffers are reused between frames.
pub struct BlockRaster {
    /// Braille dot bits per cell.
    pub bits: Vec<u8>,
    /// Lit dots per cell.
    pub counts: Vec<u8>,
    /// Summed RGB of the lit dots per cell.
    pub rgb: Vec<f32>,
    /// Halo tint strength per cell, from 0 to 1. Only set when a halo was requested.
    pub halo_amount: Vec<f32>,
    /// Halo color per cell.
    pub halo_rgb: Vec<f32>,
    width: usize,
    height: usize,
    /// Ray parameter of the nearest hit per dot; smaller is nearer.
    depth: Vec<f32>,
    /// Face index + 1 of the nearest hit per dot, 0 for none.
    face_ids: Vec<u16>,
    /// Cells written by the previous frame, cleared before the next one.
    dirty: Option<Rect>,
    /// Cells with a halo from the previous frame, cleared before the next one.
    halo_dirty: Option<Rect>,
    camera_distance: f64,
}

impl BlockRaster {
    pub fn new(camera_distance: f64) -> Self {
        Self {
            bits: Vec::new(),
            counts: Vec::new(),
            rgb: Vec::new(),
            halo_amount: Vec::new(),
            halo_rgb: Vec::new(),
            width: 0,
            height: 0,
            depth: Vec::new(),
            face_ids: Vec::new(),
            dirty: None,
            halo_dirty: None,
            camera_distance,
        }
    }

    pub fn render(&mut self, width: usize, height: usize, pose: &Pose, boxes: &[Box], background: Rgb, halo_strength: f64) {
        let dot_width = width * 2;
        let dot_height = height * 4;
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.bits = vec![0; width * height];
            self.counts = vec![0; width * height];
            self.rgb = vec![0.0; width * height * 3];
            self.halo_amount = vec![0.0; width * height];
            self.halo_rgb = vec![0.0; width * height * 3];
            self.halo_dirty = None;
            self.depth = vec![f32::INFINITY; dot_width * dot_height];
            self.face_ids = vec![0; dot_width * dot_height];
            self.dirty = None;
        } else if let Some(d) = self.dirty.take() {
            for row in d.min_y..=d.max_y {
                let start = row as usize * width + d.min_x as usize;
                let end = row as usize * width + d.max_x as usize + 1;
                self.bits[start..end].fill(0);
                self.counts[start..end].fill(0);
                self.rgb[start * 3..end * 3].fill(0.0);
            }
        }
        if let Some(d) = self.halo_dirty.take() {
            for row in d.min_y..=d.max_y {
                let start = row as usize * width + d.min_x as usize;
                let end = row as usize * width + d.max_x as usize + 1;
                self.halo_amount[start..end].fill(0.0);
                self.halo_rgb[start * 3..end * 3].fill(0.0);
            }
        }

        let m = rotation(pose.yaw, pose.pitch, pose.roll);
        let center_x = pose.center_x;
        let center_y = pose.center_y;
        let scale = pose.scale;
        let camera_distance = self.camera_distance;
        // The camera sits at (0, 0, cameraDistance) in camera space; object space is the transpose rotation.
        let origin = [m[6] * camera_distance, m[7] * camera_distance, m[8] * camera_distance];
        let light = is_light(background);
        let faces = self.visible_faces(&m, &origin, pose, boxes, dot_width as i64, dot_height as i64, light);
        if faces.is_empty() {
            return;
        }
        let mut min_x = dot_width as i64;
        let mut min_y = dot_height as i64;
        let mut max_x = -1;
        let mut max_y = -1;
        for face in &faces {
            min_x = min_x.min(face.min_x);
            min_y = min_y.min(face.min_y);
            max_x = max_x.max(face.max_x);
            max_y = max_y.max(face.max_y);
        }
        if max_x < min_x || max_y < min_y {
            return;
        }

        for (face_index, face) in faces.iter().enumerate() {
            let a = face.axis;
            let u = (a + 1) % 3;
            let v = (a + 2) % 3;
            let origin_a = origin[a];
            let origin_u = origin[u];
            let origin_v = origin[v];
            // The ray direction in object space is linear in the dot position, so it is stepped per dot.
            let step_a = m[a] / scale;
            let step_u = m[u] / scale;
            let step_v = m[v] / scale;
            let sx = (face.min_x as f64 + 0.5 - center_x) / scale;
            for dot_y in face.min_y..=face.max_y {
                let sy = (dot_y as f64 + 0.5 - center_y) / scale;
                let mut direction_a = m[a] * sx + m[3 + a] * sy - m[6 + a] * camera_distance;
                let mut direction_u = m[u] * sx + m[3 + u] * sy - m[6 + u] * camera_distance;
                let mut direction_v = m[v] * sx + m[3 + v] * sy - m[6 + v] * camera_distance;
                let mut index = dot_y as usize * dot_width + face.min_x as usize;
                for _dot_x in face.min_x..=face.max_x {
                    let t = (face.plane - origin_a) / direction_a;
                    if t > 0.0 && (t as f32) < self.depth[index] {
                        let hit_u = origin_u + t * direction_u;
                        let hit_v = origin_v + t * direction_v;
                        if hit_u >= face.u_min && hit_u <= face.u_max && hit_v >= face.v_min && hit_v <= face.v_max {
                            self.depth[index] = t as f32;
                            self.face_ids[index] = (face_index + 1) as u16;
                        }
                    }
                    direction_a += step_a;
                    direction_u += step_u;
                    direction_v += step_v;
                    index += 1;
                }
            }
        }

        // Shade the hit dots, pack them into braille cells, and reset the dot buffers for the next frame.
        let mut cell_min_x = width as i64;
        let mut cell_min_y = height as i64;
        let mut cell_max_x = -1;
        let mut cell_max_y = -1;
        for dot_y in min_y..=max_y {
            let mut index = dot_y as usize * dot_width + min_x as usize;
            for dot_x in min_x..=max_x {
                let id = self.face_ids[index];
                if id == 0 {
                    index += 1;
                    continue;
                }
                let face = &faces[id as usize - 1];
                // Points farther from the camera fade slightly toward the background for depth.
                let fog = clamp01(0.15 - camera_distance * (1.0 - self.depth[index] as f64) * 0.12)
                    * if light { 0.5 } else { 1.0 };
                self.face_ids[index] = 0;
                self.depth[index] = f32::INFINITY;
                let cell_x = dot_x >> 1;
                let cell_y = dot_y >> 2;
                let cell = cell_y as usize * width + cell_x as usize;
                self.bits[cell] |= DOT_BITS[((dot_y & 3) * 2 + (dot_x & 1)) as usize];
                self.counts[cell] = self.counts[cell].saturating_add(1);
                self.rgb[cell * 3] += (face.red + (background[0] - face.red) * fog) as f32;
                self.rgb[cell * 3 + 1] += (face.green + (background[1] - face.green) * fog) as f32;
                self.rgb[cell * 3 + 2] += (face.blue + (background[2] - face.blue) * fog) as f32;
                cell_min_x = cell_min_x.min(cell_x);
                cell_max_x = cell_max_x.max(cell_x);
                cell_min_y = cell_min_y.min(cell_y);
                cell_max_y = cell_max_y.max(cell_y);
                index += 1;
            }
        }
        if cell_max_x >= 0 {
            self.dirty = Some(Rect { min_x: cell_min_x, min_y: cell_min_y, max_x: cell_max_x, max_y: cell_max_y });
        }
        if halo_strength > 0.0 {
            self.render_halo(halo_strength);
        }
    }

    /// Blur the logo's coverage and color over neighboring cells with a separable tent filter, so the tint falls
    /// off smoothly past the logo's edges.
    fn render_halo(&mut self, strength: f64) {
        let Some(cells) = self.dirty else { return };
        let width = self.width as i64;
        let height = self.height as i64;
        let min_x = (cells.min_x - HALO_RADIUS_X).max(0);
        let max_x = (cells.max_x + HALO_RADIUS_X).min(width - 1);
        let min_y = (cells.min_y - HALO_RADIUS_Y).max(0);
        let max_y = (cells.max_y + HALO_RADIUS_Y).min(height - 1);
        let region_width = (max_x - min_x + 1) as usize;
        let tent = |radius: i64| -> Vec<f64> {
            let weights: Vec<f64> = (0..radius * 2 + 1).map(|i| (radius + 1 - (i - radius).abs()) as f64).collect();
            let total: f64 = weights.iter().sum();
            weights.into_iter().map(|w| w / total).collect()
        };
        let weights_x = tent(HALO_RADIUS_X);
        let weights_y = tent(HALO_RADIUS_Y);

        // Horizontal pass over the rows that contain the logo: coverage and premultiplied color per cell.
        let rows = (cells.max_y - cells.min_y + 1) as usize;
        let mut horizontal = vec![0.0f32; rows * region_width * 4];
        for y in cells.min_y..=cells.max_y {
            for x in min_x..=max_x {
                let (mut coverage, mut red, mut green, mut blue) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
                for dx in -HALO_RADIUS_X..=HALO_RADIUS_X {
                    let source_x = x + dx;
                    if source_x < cells.min_x || source_x > cells.max_x {
                        continue;
                    }
                    let source = (y * width + source_x) as usize;
                    let count = self.counts[source];
                    if count == 0 {
                        continue;
                    }
                    let weight = weights_x[(dx + HALO_RADIUS_X) as usize] / 8.0;
                    coverage += count as f64 * weight;
                    red += self.rgb[source * 3] as f64 * weight;
                    green += self.rgb[source * 3 + 1] as f64 * weight;
                    blue += self.rgb[source * 3 + 2] as f64 * weight;
                }
                let target = (((y - cells.min_y) as usize) * region_width + (x - min_x) as usize) * 4;
                horizontal[target] = coverage as f32;
                horizontal[target + 1] = red as f32;
                horizontal[target + 2] = green as f32;
                horizontal[target + 3] = blue as f32;
            }
        }

        // Vertical pass into the halo buffers.
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let (mut coverage, mut red, mut green, mut blue) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
                for dy in -HALO_RADIUS_Y..=HALO_RADIUS_Y {
                    let source_y = y + dy;
                    if source_y < cells.min_y || source_y > cells.max_y {
                        continue;
                    }
                    let weight = weights_y[(dy + HALO_RADIUS_Y) as usize];
                    let source = (((source_y - cells.min_y) as usize) * region_width + (x - min_x) as usize) * 4;
                    coverage += horizontal[source] as f64 * weight;
                    red += horizontal[source + 1] as f64 * weight;
                    green += horizontal[source + 2] as f64 * weight;
                    blue += horizontal[source + 3] as f64 * weight;
                }
                let target = (y * width + x) as usize;
                // Cells under the logo keep at least their own coverage, so faces stay solidly tinted.
                let count = self.counts[target] as f64;
                if count > 0.0 && count / 8.0 > coverage {
                    coverage = count / 8.0;
                    red = (self.rgb[target * 3] as f64 / count) * coverage;
                    green = (self.rgb[target * 3 + 1] as f64 / count) * coverage;
                    blue = (self.rgb[target * 3 + 2] as f64 / count) * coverage;
                }
                if coverage <= 0.0 {
                    continue;
                }
                // Quantized so neighboring cells usually share a background and its escape sequence.
                let amount = js_round(strength * coverage.min(1.0).powf(0.7) * HALO_LEVELS) / HALO_LEVELS;
                if amount <= 0.0 {
                    continue;
                }
                self.halo_amount[target] = amount as f32;
                self.halo_rgb[target * 3] = (js_round(red / coverage / LOGO_COLOR_STEP) * LOGO_COLOR_STEP) as f32;
                self.halo_rgb[target * 3 + 1] = (js_round(green / coverage / LOGO_COLOR_STEP) * LOGO_COLOR_STEP) as f32;
                self.halo_rgb[target * 3 + 2] = (js_round(blue / coverage / LOGO_COLOR_STEP) * LOGO_COLOR_STEP) as f32;
            }
        }
        self.halo_dirty = Some(Rect { min_x, min_y, max_x, max_y });
    }

    #[allow(clippy::too_many_arguments)]
    fn visible_faces(
        &self,
        m: &[f64; 9],
        origin: &[f64; 3],
        pose: &Pose,
        boxes: &[Box],
        dot_width: i64,
        dot_height: i64,
        light_background: bool,
    ) -> Vec<Face> {
        let mut faces = Vec::new();
        let camera_distance = self.camera_distance;
        for (box_index, b) in boxes.iter().enumerate() {
            for a in 0..3 {
                let u = (a + 1) % 3;
                let v = (a + 2) % 3;
                for side in [-1.0f64, 1.0] {
                    let plane = if side > 0.0 { b.max[a] } else { b.min[a] };
                    // Back faces point away from the camera.
                    if (origin[a] - plane) * side <= 0.0 {
                        continue;
                    }
                    // Faces pressed against a neighboring block are hidden. Positions are exact while blocks rest.
                    let covered = boxes.iter().enumerate().any(|(other_index, other)| {
                        other_index != box_index
                            && (if side > 0.0 { other.min[a] } else { other.max[a] }) == plane
                            && other.min[u] <= b.min[u]
                            && other.max[u] >= b.max[u]
                            && other.min[v] <= b.min[v]
                            && other.max[v] >= b.max[v]
                    });
                    if covered {
                        continue;
                    }

                    let mut min_x = f64::INFINITY;
                    let mut min_y = f64::INFINITY;
                    let mut max_x = f64::NEG_INFINITY;
                    let mut max_y = f64::NEG_INFINITY;
                    let mut corner = [0.0f64; 3];
                    for corner_u in [b.min[u], b.max[u]] {
                        for corner_v in [b.min[v], b.max[v]] {
                            corner[a] = plane;
                            corner[u] = corner_u;
                            corner[v] = corner_v;
                            let x = m[0] * corner[0] + m[1] * corner[1] + m[2] * corner[2];
                            let y = m[3] * corner[0] + m[4] * corner[1] + m[5] * corner[2];
                            let z = m[6] * corner[0] + m[7] * corner[1] + m[8] * corner[2];
                            let perspective = (pose.scale * camera_distance) / (camera_distance - z);
                            let screen_x = pose.center_x + x * perspective;
                            let screen_y = pose.center_y + y * perspective;
                            min_x = min_x.min(screen_x);
                            min_y = min_y.min(screen_y);
                            max_x = max_x.max(screen_x);
                            max_y = max_y.max(screen_y);
                        }
                    }
                    let f_min_x = (min_x.floor() as i64).max(0);
                    let f_min_y = (min_y.floor() as i64).max(0);
                    let f_max_x = (max_x.ceil() as i64).min(dot_width - 1);
                    let f_max_y = (max_y.ceil() as i64).min(dot_height - 1);
                    if f_max_x < f_min_x || f_max_y < f_min_y {
                        continue;
                    }
                    // Faces are flat, so lighting is computed once per face. The normal in camera space is a
                    // column of the rotation matrix.
                    let nx = m[a] * side;
                    let ny = m[3 + a] * side;
                    let nz = m[6 + a] * side;
                    let diffuse = (nx * LIGHT[0] + ny * LIGHT[1] + nz * LIGHT[2]).max(0.0);
                    let rim = (nx * 0.8 - nz * 0.3).max(0.0);
                    // On light backgrounds, highlights toward white would vanish, so faces only get darker than
                    // the brand colors there.
                    let specular = if light_background {
                        0.0
                    } else {
                        (nx * HALF_VECTOR[0] + ny * HALF_VECTOR[1] + nz * HALF_VECTOR[2]).max(0.0).powi(24) * 0.6 * 255.0
                    };
                    let light = if light_background {
                        (0.55 + diffuse * 0.45 + rim * 0.1).min(1.0)
                    } else {
                        0.45 + diffuse * 0.78 + rim * 0.25
                    };
                    faces.push(Face {
                        axis: a,
                        plane,
                        u_min: b.min[u],
                        u_max: b.max[u],
                        v_min: b.min[v],
                        v_max: b.max[v],
                        min_x: f_min_x,
                        min_y: f_min_y,
                        max_x: f_max_x,
                        max_y: f_max_y,
                        red: (b.color[0] * light + specular).min(255.0),
                        green: (b.color[1] * light + specular).min(255.0),
                        blue: (b.color[2] * light + specular).min(255.0),
                    });
                }
            }
        }
        faces
    }
}

/// Which easter egg to play. The pi logo lifts off the header logo, whose top-left cell is at `column`, `row`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum EasterEgg3d {
    #[serde(rename = "pi-logo")]
    PiLogo { column: i64, row: i64 },
    #[serde(rename = "armin")]
    Armin {},
}

/// Module-level `playing` flag: only one easter egg plays at a time.
static PLAYING: AtomicBool = AtomicBool::new(false);

/// Show the animation as a fullscreen overlay until it is dismissed. The overlay takes focus and mouse input and
/// returns focus when hidden, so the rest of the UI keeps running underneath untouched.
pub async fn play_easter_egg3d(tui: Arc<Tui>, screen: &[String], egg: &EasterEgg3d) -> anyhow::Result<()> {
    if PLAYING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    // Fading needs the terminal's actual default colors; the theme only knows its own.
    let reported = tui.query_terminal_colors(Duration::from_millis(100), None).await;
    let t = theme();
    let dark = t.appearance() == TerminalTheme::Dark;
    let theme_colors = t.colors();
    let theme_rgb = |token: ThemeToken, fallback: Rgb| theme_colors.get(&token).map(to_rgb).unwrap_or(fallback);
    let colors = EasterEgg3dAnimationColors {
        foreground: reported
            .foreground
            .map(|c| [c.r, c.g, c.b])
            .unwrap_or_else(|| theme_rgb(ThemeToken::Text, [255.0, 255.0, 255.0])),
        background: reported.background.map(|c| [c.r, c.g, c.b]).unwrap_or(if dark {
            [0.0, 0.0, 0.0]
        } else {
            [255.0, 255.0, 255.0]
        }),
    };
    if tui.has_overlay() {
        PLAYING.store(false, Ordering::SeqCst);
        return Ok(());
    }
    let model = match egg {
        EasterEgg3d::Armin {} => armin_model(theme_rgb(ThemeToken::Accent, CORAL)),
        EasterEgg3d::PiLogo { column, row } => pi_logo_model(ModelOrigin { column: *column, row: *row }),
    };
    let overlay: Arc<Mutex<Option<OverlayHandle>>> = Arc::new(Mutex::new(None));
    let overlay_for_done = overlay.clone();
    let animation = Arc::new(EasterEgg3dAnimation::new(
        tui.clone(),
        screen,
        &model,
        &colors,
        Arc::new(move || {
            PLAYING.store(false, Ordering::SeqCst);
            let handle_taken = overlay_for_done.lock().take();
            if let Some(handle) = handle_taken {
                handle.hide();
            }
        }),
    ));
    let handle = tui.show_overlay(
        animation,
        Some(OverlayOptions {
            anchor: Some(OverlayAnchor::TopLeft),
            width: Some("100%".into()),
            max_height: Some("100%".into()),
            ..Default::default()
        }),
    );
    *overlay.lock() = Some(handle);
    Ok(())
}

fn spawn_detached(future: BoxFuture<'static, ()>) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
    } else {
        std::thread::spawn(move || futures::executor::block_on(future));
    }
}

/// Plays a 3D easter egg. Only fullscreen mode can show it, because it dissolves the rendered screen. The screen is
/// captured before starting. Returns false when it cannot play. (TS: `easter-egg-3d.lazy.ts`.)
fn play_easter_egg_lazy(tui: Arc<Tui>, egg: EasterEgg3d) -> bool {
    if tui.mode() != TuiMode::Fullscreen {
        return false;
    }
    if tui.has_overlay() {
        return true;
    }
    let screen = tui.get_screen_lines();
    spawn_detached(
        async move {
            let _ = play_easter_egg3d(tui, &screen, &egg).await;
        }
        .boxed(),
    );
    true
}

/// Plays the 3D pi logo, lifting off the header logo whose top-left cell is at `column`, `row`.
pub fn play_pi_logo_3d(tui: Arc<Tui>, column: i64, row: i64) {
    play_easter_egg_lazy(tui, EasterEgg3d::PiLogo { column, row });
}

/// Plays the 3D Armin. Returns false when it cannot play, so the caller can fall back to the inline version.
pub fn play_armin_3d(tui: Arc<Tui>) -> bool {
    play_easter_egg_lazy(tui, EasterEgg3d::Armin {})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EasterEgg3dAnimationColors {
    pub foreground: Rgb,
    pub background: Rgb,
}

struct ExitState {
    start: Instant,
    time: f64,
    yaw: f64,
    target_yaw: f64,
    offsets: Vec<[f64; 3]>,
}

struct AnimationState {
    last_render: Instant,
    timer_running: bool,
    exit: Option<ExitState>,
    shuffle: Option<(i64, Vec<Vec<Cell3>>)>,
    screen_width: i64,
    screen_height: i64,
    /// Star per cell index, or None.
    stars: Vec<Option<Star>>,
    cells: Vec<Vec<ScreenCell>>,
    ansi_cache: HashMap<u64, String>,
    raster: BlockRaster,
}

struct AnimationInner {
    tui: Weak<Tui>,
    /// The screen to dissolve, as rendered lines.
    screen: Vec<String>,
    model: Model,
    foreground: Rgb,
    background: Rgb,
    on_done: Arc<dyn Fn() + Send + Sync>,
    start_time: Instant,
    state: Mutex<AnimationState>,
}

struct Hint {
    row: usize,
    start: usize,
    text: Vec<char>,
    key_length: usize,
    key_color: Rgb,
    color: Rgb,
}

impl AnimationInner {
    fn elapsed(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    fn exit_progress(exit: Option<&ExitState>) -> f64 {
        exit.map(|e| clamp01(e.start.elapsed().as_secs_f64() / EXIT_DURATION)).unwrap_or(0.0)
    }

    fn finish(&self) {
        {
            let mut s = self.state.lock();
            if !s.timer_running {
                return;
            }
            s.timer_running = false;
        }
        (self.on_done)();
    }

    fn rows(&self) -> usize {
        self.tui.upgrade().map(|t| t.terminal().rows()).unwrap_or(24).max(1)
    }

    /// Each block's displacement from its place in the model at `time`, in grid units.
    fn block_offsets(&self, s: &mut AnimationState, time: f64) -> Vec<[f64; 3]> {
        let blocks = &self.model.blocks;
        let home = || blocks.iter().map(|_| [0.0; 3]).collect::<Vec<_>>();
        if time < PUZZLE_START {
            return home();
        }
        let cycle = ((time - PUZZLE_START) / PUZZLE_CYCLE).floor() as i64;
        let local = time - PUZZLE_START - cycle as f64 * PUZZLE_CYCLE;
        if s.shuffle.as_ref().map(|(c, _)| *c) != Some(cycle) {
            s.shuffle = Some((cycle, shuffle_steps(&self.model, cycle)));
        }
        let steps = &s.shuffle.as_ref().map(|(_, steps)| steps).expect("shuffle set above");
        let offset = |position: &Cell3, index: usize| -> [f64; 3] {
            let block = blocks[index].home;
            [
                (position[0] - block[0]) as f64,
                (position[1] - block[1]) as f64,
                (position[2] - block[2]) as f64,
            ]
        };
        let shuffle_end = PUZZLE_STEP * PUZZLE_STEPS as f64;
        if local < shuffle_end {
            let step = (local / PUZZLE_STEP).floor() as usize;
            let progress = smooth((local - step as f64 * PUZZLE_STEP) / PUZZLE_STEP);
            return steps[step]
                .iter()
                .enumerate()
                .map(|(index, from)| {
                    let a = offset(from, index);
                    let b = offset(&steps[step + 1][index], index);
                    [a[0] + (b[0] - a[0]) * progress, a[1] + (b[1] - a[1]) * progress, a[2] + (b[2] - a[2]) * progress]
                })
                .collect();
        }
        if local < shuffle_end + PUZZLE_RETURN {
            // All blocks fly home at once. Alternating arcs in depth keep them from passing through each other.
            let u = (local - shuffle_end) / PUZZLE_RETURN;
            let remaining = 1.0 - smooth(u);
            let arc = (PI * clamp01(u)).sin() * 0.8;
            return steps[PUZZLE_STEPS]
                .iter()
                .enumerate()
                .map(|(index, position)| {
                    let [x, y, z] = offset(position, index);
                    [x * remaining, y * remaining, z * remaining + if index % 2 == 0 { arc } else { -arc }]
                })
                .collect();
        }
        home()
    }

    fn fly_progress(time: f64) -> f64 {
        smooth((time - FLY_START) / FLY_DURATION)
    }

    fn star_alpha(time: f64) -> f64 {
        smooth((time - STARS_START) / STARS_FADE)
    }

    fn hint_alpha(time: f64) -> f64 {
        smooth((time - FLY_START - FLY_DURATION) / HINT_FADE)
    }

    fn pose(&self, width: usize, height: usize, progress: f64, time: f64) -> Pose {
        let model = &self.model;
        let columns = model.columns as f64;
        let rows = model.rows as f64;
        let camera_distance = model.camera_distance;
        let radius = model.radius;
        let reach = radius * (camera_distance / (camera_distance - radius));
        // Lifting off, the front face must cover exactly the half-block cells (2x2 dots per pixel) despite the
        // perspective.
        let start_scale =
            if model.origin.is_some() { (2.0 * (camera_distance - DEPTH / 2.0)) / camera_distance } else { START_SCALE };
        let end_scale = start_scale
            .max((width as f64 * 2.0 * model.width_share).min((height as f64 * 4.0 - 8.0) * 0.48) / reach);
        let end_x = width as f64;
        let end_y = height as f64 * 2.0 - 2.0;
        let (start_x, start_y) = match model.origin {
            Some(origin) => (origin.column as f64 * 2.0 + columns, origin.row as f64 * 4.0 + rows),
            None => (end_x, end_y),
        };
        Pose {
            center_x: start_x + (end_x - start_x) * progress,
            center_y: start_y + (end_y - start_y) * progress,
            // Interpolate the zoom geometrically so it feels uniform.
            scale: start_scale * (end_scale / start_scale).powf(progress),
            yaw: spin_angle(time),
            // Tilts are zero at whole turns, so the camera looks straight at the front of the reassembled logo.
            pitch: 0.3 * spin_phase(time).sin() * progress,
            roll: 0.06 * (2.0 * spin_phase(time)).sin() * progress,
        }
    }

    fn hint(&self, width: usize, height: usize, alpha: f64, background: Rgb) -> Option<Hint> {
        if alpha <= 0.0 {
            return None;
        }
        let raw_key =
            get_keybindings().get_keys("tui.select.cancel").into_iter().next().unwrap_or_else(|| "escape".to_string());
        let key = format_key_text(&raw_key, None);
        let text: Vec<char> = format!("{key} to return").chars().collect();
        if text.len() > width {
            return None;
        }
        let t = theme();
        let theme_colors = t.colors();
        let token_rgb = |token: ThemeToken| theme_colors.get(&token).map(to_rgb).unwrap_or(self.foreground);
        Some(Hint {
            row: height.saturating_sub(2),
            start: (width - text.len()) / 2,
            key_length: key.chars().count(),
            text,
            key_color: mix(background, token_rgb(ThemeToken::Muted), alpha),
            color: mix(background, token_rgb(ThemeToken::Dim), alpha),
        })
    }

    /// A sparse, deterministic starfield: one braille dot in about STAR_DENSITY of all cells.
    fn prepare_stars(width: usize, height: usize) -> Vec<Option<Star>> {
        (0..width * height)
            .map(|index| {
                if hash((index * 3 + 0x51ed) as f64) >= STAR_DENSITY {
                    return None;
                }
                let random = |salt: f64| hash(index as f64 * 7.0 + salt * 0x9e37 as f64);
                Some(Star {
                    glyph: braille(DOT_BITS[(random(1.0) * 8.0).floor() as usize]),
                    tint: STAR_TINTS[(random(2.0) * STAR_TINTS.len() as f64).floor() as usize],
                    strength: 0.1 + random(3.0) * 0.12,
                    phase: random(4.0) * PI * 2.0,
                    speed: 0.4 + random(5.0) * 0.8,
                })
            })
            .collect()
    }

    fn prepare_cells(&self, width: usize, foreground: Rgb) -> Vec<Vec<ScreenCell>> {
        let mut cells = parse_screen(&self.screen, width, foreground, self.background);
        let height = self.rows() as f64;
        let model = &self.model;
        let columns = model.columns as f64;
        let rows = model.rows as f64;
        // The dust spreads out from where the model starts.
        let (center_x, center_y) = match model.origin {
            Some(origin) => (origin.column as f64 + columns / 2.0, origin.row as f64 + rows / 4.0),
            None => (width as f64 / 2.0, height / 2.0),
        };
        let far_x = center_x.max(width as f64 - center_x);
        let far_y = center_y.max(height - center_y) * 2.0;
        let farthest = (far_x * far_x + far_y * far_y).sqrt();
        for (row, line) in cells.iter_mut().enumerate() {
            for column in 0..line.len() {
                // Wide graphemes share their first column's timing so both halves change together.
                let (owner_text, owner_delay) = if line[column].width == 0 && column > 0 {
                    (line[column - 1].text.clone(), line[column - 1].delay)
                } else {
                    (line[column].text.clone(), line[column].delay)
                };
                let seed_value = row as f64 * 65_537.0 + column as f64;
                let seed = hash(seed_value);
                let cell = &mut line[column];
                if cell.width != 0 {
                    let dx = column as f64 - center_x;
                    let dy = (row as f64 - center_y) * 2.0;
                    let distance = (dx * dx + dy * dy).sqrt() / farthest;
                    cell.delay = FLY_START + distance * WAVE_SPREAD + seed * WAVE_JITTER;
                } else {
                    cell.delay = owner_delay;
                }
                cell.seed = seed_value;
                cell.ink = glyph_ink(&owner_text, seed);
            }
        }
        // The 3D model replaces its half-block rendering on screen.
        if let Some(origin) = model.origin {
            let end_row = origin.row + (model.rows + 1) / 2;
            for row in origin.row.max(0)..end_row {
                let Some(line) = cells.get_mut(row as usize) else { continue };
                for column in origin.column.max(0)..origin.column + model.columns {
                    if let Some(cell) = line.get_mut(column as usize) {
                        cell.text = " ".to_string();
                        cell.width = 1;
                        cell.ink = 0;
                        cell.bg = None;
                    }
                }
            }
        }
        cells
    }

    fn pack(color: Rgb) -> u64 {
        let c = |v: f64| (js_round(v).clamp(0.0, 255.0) as u64) & 0xff;
        (c(color[0]) << 16) | (c(color[1]) << 8) | c(color[2])
    }

    fn ansi(cache: &mut HashMap<u64, String>, key: u64, is_background: bool) -> String {
        let cache_key = key * 2 + if is_background { 1 } else { 0 };
        cache
            .entry(cache_key)
            .or_insert_with(|| {
                let color = rgb_color(((key >> 16) & 255) as f64, ((key >> 8) & 255) as f64, (key & 255) as f64)
                    .unwrap_or(Color::Rgb { r: 0.0, g: 0.0, b: 0.0 });
                let mode = theme().get_color_mode();
                if is_background { background_ansi(&color, mode) } else { foreground_ansi(&color, mode) }
            })
            .clone()
    }

    fn close(&self) {
        let skip = {
            let s = self.state.lock();
            s.exit.is_some()
        };
        if skip {
            self.finish();
            return;
        }
        let time = self.elapsed();
        let yaw = spin_angle(time);
        let turn = PI * 2.0;
        let mut s = self.state.lock();
        let offsets = self.block_offsets(&mut s, time);
        s.exit = Some(ExitState { start: Instant::now(), time, yaw, target_yaw: (yaw / turn).ceil() * turn, offsets });
    }

    fn render(&self, width: usize) -> Vec<String> {
        let height = self.rows();
        let foreground = self.foreground;
        let background = self.background;
        let mut guard = self.state.lock();
        let s = &mut *guard;
        s.last_render = Instant::now();
        if width as i64 != s.screen_width || height as i64 != s.screen_height {
            s.screen_width = width as i64;
            s.screen_height = height as i64;
            s.cells = self.prepare_cells(width, foreground);
            s.stars = Self::prepare_stars(width, height);
        }

        // Dissolve time runs forward on entry and backward on exit, so the screen reassembles in reverse order.
        let dissolve_time;
        let pose;
        let hint_alpha;
        let star_alpha;
        let offsets: Vec<[f64; 3]>;
        if let Some(exit) = &s.exit {
            let progress = Self::exit_progress(Some(exit));
            let landing = clamp01(progress / 0.8);
            let settle = 1.0 - smooth(landing);
            dissolve_time = exit.time.min(DISSOLVE_END) * (1.0 - progress);
            let mut p = self.pose(width, height, Self::fly_progress(exit.time) * settle, exit.time);
            p.yaw = exit.yaw + (exit.target_yaw - exit.yaw) * ease_out(landing);
            p.pitch *= settle;
            p.roll *= settle;
            pose = p;
            hint_alpha = Self::hint_alpha(exit.time) * (1.0 - smooth(progress / 0.2));
            star_alpha = Self::star_alpha(exit.time) * (1.0 - smooth(progress / 0.3));
            // Blocks are home well before the logo lands.
            let gather = 1.0 - smooth(progress / 0.5);
            offsets = exit.offsets.iter().map(|[x, y, z]| [x * gather, y * gather, z * gather]).collect();
        } else {
            let time = self.elapsed();
            dissolve_time = time;
            pose = self.pose(width, height, Self::fly_progress(time), time);
            hint_alpha = Self::hint_alpha(time);
            star_alpha = Self::star_alpha(time);
            offsets = self.block_offsets(s, time);
        }

        let columns = self.model.columns as f64;
        let rows = self.model.rows as f64;
        let boxes: Vec<Box> = self
            .model
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| {
                let [dx, dy, dz] = offsets[index];
                let x = block.home[0] as f64 - columns / 2.0 + dx;
                let y = block.home[1] as f64 - rows / 2.0 + dy;
                Box { min: [x, y, dz - DEPTH / 2.0], max: [x + 1.0, y + 1.0, dz + DEPTH / 2.0], color: block.color }
            })
            .collect();
        s.raster.render(
            width,
            height,
            &pose,
            &boxes,
            background,
            if is_light(background) { LIGHT_HALO_STRENGTH } else { 0.0 },
        );
        let raster = &s.raster;
        let halo = |index: usize, base: Option<Rgb>| -> Option<Rgb> {
            let amount = raster.halo_amount[index] as f64;
            if amount <= 0.0 {
                return base;
            }
            let color = &raster.halo_rgb;
            Some(mix(
                base.unwrap_or(background),
                [color[index * 3] as f64, color[index * 3 + 1] as f64, color[index * 3 + 2] as f64],
                amount,
            ))
        };
        let background_fade = smooth(dissolve_time / BACKGROUND_FADE);
        let text_fade = smooth(dissolve_time / 0.6) * 0.3;
        // Once the screen has fully dissolved, the text layer is empty and can be skipped.
        let text_active = dissolve_time < DISSOLVE_END || background_fade < 1.0;
        let hint = self.hint(width, height, hint_alpha, background);
        let frame = (dissolve_time * 14.0).floor();
        let now = self.elapsed();
        // Star brightness is quantized, so a row only changes when one of its stars steps to another level
        // instead of on every frame.
        let star_level = js_round(star_alpha * STAR_ALPHA_LEVELS) / STAR_ALPHA_LEVELS;
        let stars = &s.stars;
        let starfield = |index: usize| -> Option<(char, Rgb)> {
            if star_level <= 0.0 {
                return None;
            }
            let star = stars.get(index)?.as_ref()?;
            let flicker = js_round((0.5 + 0.5 * (now * star.speed + star.phase).sin()) * STAR_FLICKER_LEVELS)
                / STAR_FLICKER_LEVELS;
            Some((
                star.glyph,
                mix(background, mix(foreground, star.tint, 0.5), star.strength * (0.7 + 0.3 * flicker) * star_level),
            ))
        };
        let ansi_cache = &mut s.ansi_cache;
        let mut lines: Vec<String> = Vec::with_capacity(height);
        for row in 0..height {
            let row_cells: &[ScreenCell] = s.cells.get(row).map(|c| c.as_slice()).unwrap_or(&[]);
            let mut line = String::new();
            let mut current_fg: i64 = -1;
            let mut current_bg: i64 = -1;
            let mut emit = |text: &str, fg: Option<Rgb>, bg: Option<Rgb>, line: &mut String| {
                let fg_key = fg.map(|c| Self::pack(c) as i64).unwrap_or(-1);
                let bg_key = bg.map(|c| Self::pack(c) as i64).unwrap_or(-1);
                if fg_key != current_fg && text != " " {
                    line.push_str(&if fg.is_some() {
                        Self::ansi(ansi_cache, fg_key as u64, false)
                    } else {
                        "\x1b[39m".to_string()
                    });
                    current_fg = fg_key;
                }
                if bg_key != current_bg {
                    line.push_str(&if bg.is_some() {
                        Self::ansi(ansi_cache, bg_key as u64, true)
                    } else {
                        "\x1b[49m".to_string()
                    });
                    current_bg = bg_key;
                }
                line.push_str(text);
            };
            let mut column = 0;
            while column < width {
                let index = row * width + column;
                let cell = if text_active { row_cells.get(column) } else { None };
                let cell_bg = halo(
                    index,
                    match cell {
                        Some(c) if c.bg.is_some() && background_fade < 1.0 => {
                            Some(mix(c.bg.unwrap_or(background), background, background_fade))
                        }
                        _ => None,
                    },
                );
                if let Some(hint) = &hint {
                    if row == hint.row && column >= hint.start && column < hint.start + hint.text.len() {
                        let offset = column - hint.start;
                        let color = if offset < hint.key_length { hint.key_color } else { hint.color };
                        emit(&hint.text[offset].to_string(), Some(color), cell_bg, &mut line);
                        column += 1;
                        continue;
                    }
                }
                let dots = raster.bits[index];
                if dots != 0 {
                    // Quantized so neighboring cells on one face usually share a color and its escape sequence.
                    let step = LOGO_COLOR_STEP * raster.counts[index] as f64;
                    let rgb = &raster.rgb;
                    let color: Rgb = [
                        js_round(rgb[index * 3] as f64 / step) * LOGO_COLOR_STEP,
                        js_round(rgb[index * 3 + 1] as f64 / step) * LOGO_COLOR_STEP,
                        js_round(rgb[index * 3 + 2] as f64 / step) * LOGO_COLOR_STEP,
                    ];
                    emit(&braille(dots).to_string(), Some(color), cell_bg, &mut line);
                    column += 1;
                    continue;
                }
                let Some(cell) = cell else {
                    match starfield(index) {
                        Some((glyph, fg)) => emit(&glyph.to_string(), Some(fg), cell_bg, &mut line),
                        None => emit(" ", None, cell_bg, &mut line),
                    }
                    column += 1;
                    continue;
                };
                let dust = (dissolve_time - cell.delay) / DUST_DURATION;
                if dust < 0.0 && cell.width == 2 && raster.bits.get(index + 1).copied().unwrap_or(0) == 0 {
                    emit(&cell.text, Some(mix(cell.fg, background, text_fade)), cell_bg, &mut line);
                    column += 2;
                    continue;
                }
                if dust < 0.0 && cell.width == 1 {
                    emit(&cell.text, Some(mix(cell.fg, background, text_fade)), cell_bg, &mut line);
                    column += 1;
                    continue;
                }
                let dot_count =
                    if dust < 0.0 { cell.ink } else { js_round(cell.ink as f64 * (1.0 - clamp01(dust))) as i64 };
                if dot_count <= 0 {
                    match starfield(index) {
                        Some((glyph, fg)) => emit(&glyph.to_string(), Some(fg), cell_bg, &mut line),
                        None => emit(" ", None, cell_bg, &mut line),
                    }
                    column += 1;
                    continue;
                }
                // Pick `dotCount` distinct dots; the choice changes a few times per second so the dust shimmers.
                let mut bits: u8 = 0;
                let mut placed = 0;
                let mut attempt = 0;
                while placed < dot_count && attempt < 32 {
                    let bit = DOT_BITS
                        [(hash(cell.seed * 7919.0 + frame * 131.0 + attempt as f64) * 8.0).floor() as usize % 8];
                    attempt += 1;
                    if bits & bit != 0 {
                        continue;
                    }
                    bits |= bit;
                    placed += 1;
                }
                let fade = text_fade + (1.0 - text_fade) * clamp01(dust).powf(0.8);
                emit(&braille(bits).to_string(), Some(mix(cell.fg, background, fade)), cell_bg, &mut line);
                column += 1;
            }
            line.push_str("\x1b[0m");
            lines.push(line);
        }
        lines
    }
}

/// TS: `implements Component`
pub struct EasterEgg3dAnimation {
    inner: Arc<AnimationInner>,
}

impl EasterEgg3dAnimation {
    pub fn new(
        tui: Arc<Tui>,
        screen: &[String],
        model: &Model,
        colors: &EasterEgg3dAnimationColors,
        on_done: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let inner = Arc::new(AnimationInner {
            tui: Arc::downgrade(&tui),
            screen: screen.to_vec(),
            model: model.clone(),
            foreground: colors.foreground,
            background: colors.background,
            on_done,
            start_time: Instant::now(),
            state: Mutex::new(AnimationState {
                last_render: Instant::now(),
                timer_running: true,
                exit: None,
                shuffle: None,
                screen_width: -1,
                screen_height: -1,
                stars: Vec::new(),
                cells: Vec::new(),
                ansi_cache: HashMap::new(),
                raster: BlockRaster::new(model.camera_distance),
            }),
        });

        // Frame timer (TS: setInterval(..., FRAME_MS).unref()).
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("easter-egg-3d".into())
            .spawn(move || {
                let frame = Duration::from_micros((FRAME_MS * 1000.0) as u64);
                loop {
                    std::thread::sleep(frame);
                    let Some(inner) = weak.upgrade() else { break };
                    let (running, exiting_done, stale) = {
                        let s = inner.state.lock();
                        (
                            s.timer_running,
                            s.exit.is_some() && AnimationInner::exit_progress(s.exit.as_ref()) >= 1.0,
                            s.last_render.elapsed() > Duration::from_millis(1000),
                        )
                    };
                    if !running {
                        break;
                    }
                    // Also stop when no longer rendered, e.g. when pi hides all overlays on exit.
                    if exiting_done || stale {
                        inner.finish();
                        break;
                    }
                    if let Some(tui) = inner.tui.upgrade() {
                        tui.request_render(false);
                    }
                }
            })
            .ok();

        Self { inner }
    }

    /// Play the exit animation. A second call skips it.
    pub fn close(&self) {
        self.inner.close();
    }
}

impl Component for EasterEgg3dAnimation {
    fn render(&self, width: usize) -> Vec<String> {
        self.inner.render(width)
    }

    fn handle_input(&self, data: &str) {
        let keybindings = get_keybindings();
        if keybindings.matches(data, "tui.select.cancel") || keybindings.matches(data, "app.clear") {
            self.close();
        }
    }

    fn handles_input(&self) -> bool {
        true
    }

    fn handle_mouse(&self, event: &MouseEvent) -> Option<MouseResult> {
        if event.kind == MouseEventType::Click {
            self.close();
        }
        Some(MouseResult { handled: true, render: Some(false), ..Default::default() })
    }

    fn invalidate(&self) {
        let mut s = self.inner.state.lock();
        s.screen_width = -1;
        s.screen_height = -1;
        s.ansi_cache.clear();
    }
}
