//! Mouse-wheel line counts with optional acceleration. Port of `wheel-scroll.ts`.

/// Lines moved per mouse-wheel event, or [`WheelScrollLines::Auto`] to accelerate fast wheel spins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelScrollLines {
    Lines(f64),
    Auto,
}

impl From<u32> for WheelScrollLines {
    fn from(n: u32) -> Self {
        WheelScrollLines::Lines(n as f64)
    }
}

// Several events closer than this belong to one physical notch or a high-resolution source.
const BURST_GAP_MS: f64 = 5.0;
// A pause longer than this ends a scroll gesture.
const GESTURE_GAP_MS: f64 = 200.0;
// Average event gap that maps to one line per event.
const REFERENCE_GAP_MS: f64 = 100.0;
const MAX_AUTO_LINES: f64 = 6.0;

/// Local macOS terminals receive OS-accelerated wheel deltas (one event per line); other
/// platforms and SSH sessions usually send one event per wheel notch.
fn terminal_accelerates_wheel() -> bool {
    cfg!(target_os = "macos")
        && std::env::var_os("SSH_CONNECTION").is_none()
        && std::env::var_os("SSH_CLIENT").is_none()
        && std::env::var_os("SSH_TTY").is_none()
}

/// Converts wheel events into line counts.
///
/// In auto mode on terminals that do not accelerate wheel input, the count follows event
/// velocity: an isolated notch moves one line, a fast spin up to six lines per event.
#[derive(Debug, Clone)]
pub struct WheelScrollAccelerator {
    lines: WheelScrollLines,
    accelerate: bool,
    last_time: f64,
    last_direction: i32,
    average_gap: Option<f64>,
    carry: f64,
}

impl WheelScrollAccelerator {
    pub fn new(lines: WheelScrollLines) -> Self {
        Self::with_acceleration(lines, !terminal_accelerates_wheel())
    }

    pub fn with_acceleration(lines: WheelScrollLines, accelerate: bool) -> Self {
        Self { lines, accelerate, last_time: f64::NEG_INFINITY, last_direction: 0, average_gap: None, carry: 0.0 }
    }

    pub fn set_lines(&mut self, lines: WheelScrollLines) {
        self.lines = lines;
        self.reset();
    }

    /// The positive line count for a wheel event in `direction` (-1 or 1) at `now` (milliseconds).
    pub fn next(&mut self, direction: i32, now: f64) -> i64 {
        let WheelScrollLines::Auto = self.lines else {
            let WheelScrollLines::Lines(n) = self.lines else { unreachable!() };
            return if n.is_finite() { n.floor().max(1.0) as i64 } else { 1 };
        };
        if !self.accelerate {
            return 1;
        }
        let gap = now - self.last_time;
        let same_gesture = direction == self.last_direction && gap <= GESTURE_GAP_MS;
        self.last_time = now;
        self.last_direction = direction;
        if !same_gesture {
            self.average_gap = None;
            self.carry = 0.0;
            return 1;
        }
        if gap < BURST_GAP_MS {
            return 1;
        }
        let average = match self.average_gap {
            None => gap,
            Some(a) => (a + gap) / 2.0,
        };
        self.average_gap = Some(average);
        let lines = (REFERENCE_GAP_MS / average).clamp(1.0, MAX_AUTO_LINES) + self.carry;
        let whole = lines.floor();
        self.carry = lines - whole;
        whole as i64
    }

    fn reset(&mut self) {
        self.last_time = f64::NEG_INFINITY;
        self.last_direction = 0;
        self.average_gap = None;
        self.carry = 0.0;
    }
}

impl Default for WheelScrollAccelerator {
    fn default() -> Self {
        Self::new(WheelScrollLines::Auto)
    }
}
