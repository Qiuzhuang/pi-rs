//! Port of `packages/coding-agent/src/modes/interactive/components/armin.ts`.
//!
//! Armin says hi! A fun easter egg with animated XBM art.

use crate::modes::interactive::theme::theme::{ThemeColor, theme};
use parking_lot::Mutex;
use pi_tui::tui::Component;
use pi_tui::tui::Tui;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

// XBM image: 31x36 pixels, LSB first, 1=background, 0=foreground
pub const ARMIN_WIDTH: i64 = 31;

pub const ARMIN_HEIGHT: i64 = 36;

const WIDTH: usize = ARMIN_WIDTH as usize;
const HEIGHT: usize = ARMIN_HEIGHT as usize;
const BITS: [u8; 144] = [
    0xff, 0xff, 0xff, 0x7f, 0xff, 0xf0, 0xff, 0x7f, 0xff, 0xed, 0xff, 0x7f, 0xff, 0xdb, 0xff, 0x7f, 0xff, 0xb7, 0xff,
    0x7f, 0xff, 0x77, 0xfe, 0x7f, 0x3f, 0xf8, 0xfe, 0x7f, 0xdf, 0xff, 0xfe, 0x7f, 0xdf, 0x3f, 0xfc, 0x7f, 0x9f, 0xc3,
    0xfb, 0x7f, 0x6f, 0xfc, 0xf4, 0x7f, 0xf7, 0x0f, 0xf7, 0x7f, 0xf7, 0xff, 0xf7, 0x7f, 0xf7, 0xff, 0xe3, 0x7f, 0xf7,
    0x07, 0xe8, 0x7f, 0xef, 0xf8, 0x67, 0x70, 0x0f, 0xff, 0xbb, 0x6f, 0xf1, 0x00, 0xd0, 0x5b, 0xfd, 0x3f, 0xec, 0x53,
    0xc1, 0xff, 0xef, 0x57, 0x9f, 0xfd, 0xee, 0x5f, 0x9f, 0xfc, 0xae, 0x5f, 0x1f, 0x78, 0xac, 0x5f, 0x3f, 0x00, 0x50,
    0x6c, 0x7f, 0x00, 0xdc, 0x77, 0xff, 0xc0, 0x3f, 0x78, 0xff, 0x01, 0xf8, 0x7f, 0xff, 0x03, 0x9c, 0x78, 0xff, 0x07,
    0x8c, 0x7c, 0xff, 0x0f, 0xce, 0x78, 0xff, 0xff, 0xcf, 0x7f, 0xff, 0xff, 0xcf, 0x78, 0xff, 0xff, 0xdf, 0x78, 0xff,
    0xff, 0xdf, 0x7d, 0xff, 0xff, 0x3f, 0x7e, 0xff, 0xff, 0xff, 0x7f,
];

const BYTES_PER_ROW: usize = WIDTH.div_ceil(8);
const DISPLAY_HEIGHT: usize = HEIGHT.div_ceil(2); // Half-block rendering

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    #[serde(rename = "typewriter")]
    Typewriter,
    #[serde(rename = "scanline")]
    Scanline,
    #[serde(rename = "rain")]
    Rain,
    #[serde(rename = "fade")]
    Fade,
    #[serde(rename = "crt")]
    Crt,
    #[serde(rename = "glitch")]
    Glitch,
    #[serde(rename = "dissolve")]
    Dissolve,
}

impl Effect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Typewriter => "typewriter",
            Self::Scanline => "scanline",
            Self::Rain => "rain",
            Self::Fade => "fade",
            Self::Crt => "crt",
            Self::Glitch => "glitch",
            Self::Dissolve => "dissolve",
        }
    }
}

const EFFECTS: [Effect; 7] = [
    Effect::Typewriter,
    Effect::Scanline,
    Effect::Rain,
    Effect::Fade,
    Effect::Crt,
    Effect::Glitch,
    Effect::Dissolve,
];

/// `Math.random()`: a small xorshift generator seeded from the clock.
pub(crate) fn random() -> f64 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new({
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x2545F4914F6CDD1D);
            (nanos ^ 0x9E3779B97F4A7C15) | 1
        });
    }
    STATE.with(|state| {
        let mut x = state.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        state.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

fn random_index(len: usize) -> usize {
    ((random() * len as f64).floor() as usize).min(len.saturating_sub(1))
}

// Get pixel at (x, y): true = foreground, false = background
pub fn is_armin_pixel(x: i64, y: i64) -> bool {
    if y >= ARMIN_HEIGHT || x < 0 || y < 0 {
        return false;
    }
    let byte_index = y as usize * BYTES_PER_ROW + (x as usize) / 8;
    let bit_index = x as usize % 8;
    BITS.get(byte_index).is_some_and(|b| (b >> bit_index) & 1 == 0)
}

// Get the character for a cell (2 vertical pixels packed)
fn get_char(x: usize, row: usize) -> &'static str {
    let upper = is_armin_pixel(x as i64, (row * 2) as i64);
    let lower = is_armin_pixel(x as i64, (row * 2 + 1) as i64);
    match (upper, lower) {
        (true, true) => "█",
        (true, false) => "▀",
        (false, true) => "▄",
        (false, false) => " ",
    }
}

type Grid = Vec<Vec<&'static str>>;

// Build the final image grid
fn build_final_grid() -> Grid {
    (0..DISPLAY_HEIGHT).map(|row| (0..WIDTH).map(|x| get_char(x, row)).collect()).collect()
}

fn create_empty_grid() -> Grid {
    vec![vec![" "; WIDTH]; DISPLAY_HEIGHT]
}

fn shuffled_positions() -> Vec<(usize, usize)> {
    let mut positions: Vec<(usize, usize)> = Vec::new();
    for row in 0..DISPLAY_HEIGHT {
        for x in 0..WIDTH {
            positions.push((row, x));
        }
    }
    // Fisher-Yates shuffle
    for i in (1..positions.len()).rev() {
        let j = random_index(i + 1);
        positions.swap(i, j);
    }
    positions
}

struct Drop {
    y: i64,
    settled: usize,
}

enum EffectState {
    Typewriter { pos: usize },
    Scanline { row: usize },
    Rain { drops: Vec<Drop> },
    Fade { positions: Vec<(usize, usize)>, idx: usize },
    Crt { expansion: i64 },
    Glitch { phase: usize, glitch_frames: usize },
    Dissolve { positions: Vec<(usize, usize)>, idx: usize },
}

fn resolve_positions(
    current_grid: &mut Grid,
    final_grid: &Grid,
    positions: &[(usize, usize)],
    idx: &mut usize,
    pixels_per_frame: usize,
) -> bool {
    for _ in 0..pixels_per_frame {
        if *idx >= positions.len() {
            return true;
        }
        let (row, x) = positions[*idx];
        current_grid[row][x] = final_grid[row][x];
        *idx += 1;
    }
    false
}

struct ArminState {
    final_grid: Grid,
    current_grid: Grid,
    effect_state: EffectState,
    cached_lines: Vec<String>,
    cached_width: usize,
    grid_version: u64,
    cached_version: Option<u64>,
}

impl ArminState {
    fn tick_effect(&mut self) -> bool {
        let final_grid = &self.final_grid;
        match &mut self.effect_state {
            EffectState::Typewriter { pos } => {
                let pixels_per_frame = 3;
                for _ in 0..pixels_per_frame {
                    let row = *pos / WIDTH;
                    let x = *pos % WIDTH;
                    if row >= DISPLAY_HEIGHT {
                        return true;
                    }
                    self.current_grid[row][x] = final_grid[row][x];
                    *pos += 1;
                }
                false
            }
            EffectState::Scanline { row } => {
                if *row >= DISPLAY_HEIGHT {
                    return true;
                }
                // Copy row
                self.current_grid[*row] = final_grid[*row].clone();
                *row += 1;
                false
            }
            EffectState::Rain { drops } => {
                let mut all_settled = true;
                self.current_grid = create_empty_grid();
                for (x, drop) in drops.iter_mut().enumerate() {
                    // Draw settled pixels
                    let settled_from = DISPLAY_HEIGHT - drop.settled.min(DISPLAY_HEIGHT);
                    for (current, target) in self.current_grid[settled_from..].iter_mut().zip(&final_grid[settled_from..]) {
                        current[x] = target[x];
                    }

                    // Check if this column is done
                    if drop.settled >= DISPLAY_HEIGHT {
                        continue;
                    }
                    all_settled = false;

                    // Find the target row for this column (lowest non-space pixel)
                    let mut target_row: i64 = -1;
                    for row in (0..DISPLAY_HEIGHT - drop.settled).rev() {
                        if final_grid[row][x] != " " {
                            target_row = row as i64;
                            break;
                        }
                    }

                    // Move drop down
                    drop.y += 1;

                    // Draw falling drop
                    if drop.y >= 0 && (drop.y as usize) < DISPLAY_HEIGHT {
                        if target_row >= 0 && drop.y >= target_row {
                            // Settle
                            drop.settled = DISPLAY_HEIGHT - target_row as usize;
                            drop.y = -((random() * 5.0).floor() as i64) - 1;
                        } else {
                            // Still falling
                            self.current_grid[drop.y as usize][x] = "▓";
                        }
                    }
                }
                all_settled
            }
            EffectState::Fade { positions, idx } => resolve_positions(&mut self.current_grid, final_grid, positions, idx, 15),
            EffectState::Dissolve { positions, idx } => {
                resolve_positions(&mut self.current_grid, final_grid, positions, idx, 20)
            }
            EffectState::Crt { expansion } => {
                let mid_row = (DISPLAY_HEIGHT / 2) as i64;
                self.current_grid = create_empty_grid();
                // Draw from middle expanding outward
                let top = mid_row - *expansion;
                let bottom = mid_row + *expansion;
                let from = top.max(0);
                let to = bottom.min(DISPLAY_HEIGHT as i64 - 1);
                if from <= to {
                    let rows = from as usize..=to as usize;
                    self.current_grid[rows.clone()].clone_from_slice(&final_grid[rows]);
                }
                *expansion += 1;
                *expansion > DISPLAY_HEIGHT as i64
            }
            EffectState::Glitch { phase, glitch_frames } => {
                if *phase < *glitch_frames {
                    // Glitch phase: show corrupted version
                    self.current_grid = final_grid
                        .iter()
                        .map(|row| {
                            let offset = (random() * 7.0).floor() as i64 - 3;
                            // Random horizontal offset
                            if random() < 0.3 {
                                // JS `slice(offset).concat(slice(0, offset))` with negative offsets counting from the end.
                                let len = row.len() as i64;
                                let k = if offset < 0 { (len + offset).max(0) } else { offset.min(len) } as usize;
                                let mut shifted: Vec<&'static str> = row[k..].to_vec();
                                shifted.extend_from_slice(&row[..k]);
                                shifted.truncate(WIDTH);
                                return shifted;
                            }
                            // Random vertical swap
                            if random() < 0.2 {
                                let swap_row = random_index(DISPLAY_HEIGHT);
                                return final_grid[swap_row].clone();
                            }
                            row.clone()
                        })
                        .collect();
                    *phase += 1;
                    return false;
                }
                // Final frame: show clean image
                self.current_grid = final_grid.clone();
                true
            }
        }
    }
}

/// TS: `implements Component`
pub struct ArminComponent {
    state: Arc<Mutex<ArminState>>,
    stopped: Arc<AtomicBool>,
}

impl ArminComponent {
    pub fn new(ui: Arc<Tui>) -> Self {
        let effect = EFFECTS[random_index(EFFECTS.len())];
        let mut current_grid = create_empty_grid();
        let effect_state = match effect {
            Effect::Typewriter => EffectState::Typewriter { pos: 0 },
            Effect::Scanline => EffectState::Scanline { row: 0 },
            // Track falling position for each column
            Effect::Rain => EffectState::Rain {
                drops: (0..WIDTH)
                    .map(|_| Drop { y: -((random() * (DISPLAY_HEIGHT * 2) as f64).floor() as i64), settled: 0 })
                    .collect(),
            },
            Effect::Fade => EffectState::Fade { positions: shuffled_positions(), idx: 0 },
            Effect::Crt => EffectState::Crt { expansion: 0 },
            Effect::Glitch => EffectState::Glitch { phase: 0, glitch_frames: 8 },
            Effect::Dissolve => {
                // Start with random noise
                const CHARS: [&str; 7] = [" ", "░", "▒", "▓", "█", "▀", "▄"];
                current_grid = (0..DISPLAY_HEIGHT)
                    .map(|_| (0..WIDTH).map(|_| CHARS[random_index(CHARS.len())]).collect())
                    .collect();
                // Shuffle positions for gradual resolve
                EffectState::Dissolve { positions: shuffled_positions(), idx: 0 }
            }
        };
        let state = Arc::new(Mutex::new(ArminState {
            final_grid: build_final_grid(),
            current_grid,
            effect_state,
            cached_lines: Vec::new(),
            cached_width: 0,
            grid_version: 0,
            cached_version: None,
        }));
        let stopped = Arc::new(AtomicBool::new(false));

        // Start animation
        let fps = if effect == Effect::Glitch { 60 } else { 30 };
        {
            let state = Arc::downgrade(&state);
            let stopped = stopped.clone();
            let ui = Arc::downgrade(&ui);
            std::thread::Builder::new()
                .name("armin".into())
                .spawn(move || {
                    let frame = Duration::from_micros(1_000_000 / fps);
                    loop {
                        std::thread::sleep(frame);
                        if stopped.load(Ordering::SeqCst) {
                            break;
                        }
                        let Some(state) = state.upgrade() else { break };
                        let done = {
                            let mut s = state.lock();
                            let done = s.tick_effect();
                            s.grid_version += 1;
                            done
                        };
                        if let Some(ui) = ui.upgrade() {
                            ui.request_render(false);
                        }
                        if done {
                            break;
                        }
                    }
                }))
                .ok();
        }

        Self { state, stopped }
    }

    pub fn dispose(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

impl std::ops::Drop for ArminComponent {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl Component for ArminComponent {
    fn render(&self, width: usize) -> Vec<String> {
        let mut s = self.state.lock();
        if width == s.cached_width && s.cached_version == Some(s.grid_version) {
            return s.cached_lines.clone();
        }

        let t = theme();
        let padding = 1;
        let available_width = width.saturating_sub(padding);

        let mut lines: Vec<String> = s
            .current_grid
            .iter()
            .map(|row| {
                // Clip row to available width before applying color
                let clipped: String = row.iter().take(available_width).copied().collect();
                let pad_right = width.saturating_sub(padding + clipped.chars().count());
                format!(" {}{}", t.fg(&ThemeColor::Accent, &clipped), " ".repeat(pad_right))
            })
            .collect();

        // Add "ARMIN SAYS HI" at the end
        let message = "ARMIN SAYS HI";
        let msg_pad_right = width.saturating_sub(padding + message.len());
        lines.push(format!(" {}{}", t.fg(&ThemeColor::Accent, message), " ".repeat(msg_pad_right)));

        s.cached_lines = lines.clone();
        s.cached_width = width;
        s.cached_version = Some(s.grid_version);
        lines
    }

    fn invalidate(&self) {
        self.state.lock().cached_width = 0;
    }
}
