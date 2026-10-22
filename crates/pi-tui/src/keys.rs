//! Keyboard input handling for terminal applications. Port of `packages/tui/src/keys.ts`.
//!
//! Supports both legacy terminal sequences and the Kitty keyboard protocol
//! (<https://sw.kovidgoyal.net/kitty/keyboard-protocol/>).
//!
//! - [`matches_key`] checks if input matches a key identifier such as `"ctrl+c"`.
//! - [`parse_key`] parses input and returns the key identifier.
//! - [`Key`] helpers build key identifiers.
//! - [`set_kitty_protocol_active`] / [`is_kitty_protocol_active`] manage the global Kitty state.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

// =============================================================================
// Global Kitty Protocol State
// =============================================================================

static KITTY_PROTOCOL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set the global Kitty keyboard protocol state.
pub fn set_kitty_protocol_active(active: bool) {
    KITTY_PROTOCOL_ACTIVE.store(active, Ordering::SeqCst);
}

/// Query whether the Kitty keyboard protocol is currently active.
pub fn is_kitty_protocol_active() -> bool {
    KITTY_PROTOCOL_ACTIVE.load(Ordering::SeqCst)
}

fn kitty() -> bool {
    is_kitty_protocol_active()
}

/// A key identifier such as `"ctrl+c"`, `"escape"` or `"shift+tab"`.
pub type KeyId = String;

/// Helpers for building key identifiers.
pub struct Key;

#[allow(non_upper_case_globals)]
impl Key {
    pub const escape: &'static str = "escape";
    pub const esc: &'static str = "esc";
    pub const enter: &'static str = "enter";
    pub const return_: &'static str = "return";
    pub const tab: &'static str = "tab";
    pub const space: &'static str = "space";
    pub const backspace: &'static str = "backspace";
    pub const delete: &'static str = "delete";
    pub const insert: &'static str = "insert";
    pub const clear: &'static str = "clear";
    pub const home: &'static str = "home";
    pub const end: &'static str = "end";
    pub const page_up: &'static str = "pageUp";
    pub const page_down: &'static str = "pageDown";
    pub const up: &'static str = "up";
    pub const down: &'static str = "down";
    pub const left: &'static str = "left";
    pub const right: &'static str = "right";
    pub const f1: &'static str = "f1";
    pub const f2: &'static str = "f2";
    pub const f3: &'static str = "f3";
    pub const f4: &'static str = "f4";
    pub const f5: &'static str = "f5";
    pub const f6: &'static str = "f6";
    pub const f7: &'static str = "f7";
    pub const f8: &'static str = "f8";
    pub const f9: &'static str = "f9";
    pub const f10: &'static str = "f10";
    pub const f11: &'static str = "f11";
    pub const f12: &'static str = "f12";

    pub const backtick: &'static str = "`";
    pub const hyphen: &'static str = "-";
    pub const equals: &'static str = "=";
    pub const leftbracket: &'static str = "[";
    pub const rightbracket: &'static str = "]";
    pub const backslash: &'static str = "\\";
    pub const semicolon: &'static str = ";";
    pub const quote: &'static str = "'";
    pub const comma: &'static str = ",";
    pub const period: &'static str = ".";
    pub const slash: &'static str = "/";
    pub const exclamation: &'static str = "!";
    pub const at: &'static str = "@";
    pub const hash: &'static str = "#";
    pub const dollar: &'static str = "$";
    pub const percent: &'static str = "%";
    pub const caret: &'static str = "^";
    pub const ampersand: &'static str = "&";
    pub const asterisk: &'static str = "*";
    pub const leftparen: &'static str = "(";
    pub const rightparen: &'static str = ")";
    pub const underscore: &'static str = "_";
    pub const plus: &'static str = "+";
    pub const pipe: &'static str = "|";
    pub const tilde: &'static str = "~";
    pub const leftbrace: &'static str = "{";
    pub const rightbrace: &'static str = "}";
    pub const colon: &'static str = ":";
    pub const lessthan: &'static str = "<";
    pub const greaterthan: &'static str = ">";
    pub const question: &'static str = "?";

    pub fn ctrl(key: &str) -> String {
        format!("ctrl+{key}")
    }
    pub fn shift(key: &str) -> String {
        format!("shift+{key}")
    }
    pub fn alt(key: &str) -> String {
        format!("alt+{key}")
    }
    pub fn super_(key: &str) -> String {
        format!("super+{key}")
    }
    pub fn ctrl_shift(key: &str) -> String {
        format!("ctrl+shift+{key}")
    }
    pub fn shift_ctrl(key: &str) -> String {
        format!("shift+ctrl+{key}")
    }
    pub fn ctrl_alt(key: &str) -> String {
        format!("ctrl+alt+{key}")
    }
    pub fn alt_ctrl(key: &str) -> String {
        format!("alt+ctrl+{key}")
    }
    pub fn shift_alt(key: &str) -> String {
        format!("shift+alt+{key}")
    }
    pub fn alt_shift(key: &str) -> String {
        format!("alt+shift+{key}")
    }
    pub fn ctrl_super(key: &str) -> String {
        format!("ctrl+super+{key}")
    }
    pub fn super_ctrl(key: &str) -> String {
        format!("super+ctrl+{key}")
    }
    pub fn shift_super(key: &str) -> String {
        format!("shift+super+{key}")
    }
    pub fn super_shift(key: &str) -> String {
        format!("super+shift+{key}")
    }
    pub fn alt_super(key: &str) -> String {
        format!("alt+super+{key}")
    }
    pub fn super_alt(key: &str) -> String {
        format!("super+alt+{key}")
    }
    pub fn ctrl_shift_alt(key: &str) -> String {
        format!("ctrl+shift+alt+{key}")
    }
    pub fn ctrl_shift_super(key: &str) -> String {
        format!("ctrl+shift+super+{key}")
    }
}

// =============================================================================
// Constants
// =============================================================================

const SYMBOL_KEYS: &str = "`-=[]\\;',./!@#$%^&*()_+|~{}:<>?";
const SHIFTED_SYMBOL_KEYS: &str = "~!@#$%^&*()_+|{}:<>?";

const MOD_SHIFT: i64 = 1;
const MOD_ALT: i64 = 2;
const MOD_CTRL: i64 = 4;
const MOD_SUPER: i64 = 8;
const LOCK_MASK: i64 = 64 + 128;

const CP_ESCAPE: i64 = 27;
const CP_TAB: i64 = 9;
const CP_ENTER: i64 = 13;
const CP_SPACE: i64 = 32;
const CP_BACKSPACE: i64 = 127;
const CP_KP_ENTER: i64 = 57414;

const ARROW_UP: i64 = -1;
const ARROW_DOWN: i64 = -2;
const ARROW_RIGHT: i64 = -3;
const ARROW_LEFT: i64 = -4;

const FN_DELETE: i64 = -10;
const FN_INSERT: i64 = -11;
const FN_PAGE_UP: i64 = -12;
const FN_PAGE_DOWN: i64 = -13;
const FN_HOME: i64 = -14;
const FN_END: i64 = -15;

/// `String.fromCharCode(cp)` as a single UTF-16 unit, returned as a char when it is ASCII.
fn char_code_ascii(cp: i64) -> Option<char> {
    let unit = cp.rem_euclid(65536) as u32;
    if unit < 128 { char::from_u32(unit) } else { None }
}

fn is_in_set(set: &str, cp: i64) -> bool {
    char_code_ascii(cp).is_some_and(|c| set.contains(c))
}

fn is_symbol_key_str(key: &str) -> bool {
    key.len() == 1 && SYMBOL_KEYS.contains(key)
}

fn normalize_kitty_functional_codepoint(cp: i64) -> i64 {
    match cp {
        57399..=57408 => cp - 57399 + 48,
        57409 => 46,
        57410 => 47,
        57411 => 42,
        57412 => 45,
        57413 => 43,
        57415 => 61,
        57416 => 44,
        57417 => ARROW_LEFT,
        57418 => ARROW_RIGHT,
        57419 => ARROW_UP,
        57420 => ARROW_DOWN,
        57421 => FN_PAGE_UP,
        57422 => FN_PAGE_DOWN,
        57423 => FN_HOME,
        57424 => FN_END,
        57425 => FN_INSERT,
        57426 => FN_DELETE,
        _ => cp,
    }
}

fn normalize_shifted_letter_identity_codepoint(cp: i64, modifier: i64) -> i64 {
    let effective = modifier & !LOCK_MASK;
    if (effective & MOD_SHIFT) != 0 && (65..=90).contains(&cp) {
        return cp + 32;
    }
    cp
}

fn get_logical_shifted_symbol_candidate(cp: Option<i64>, modifier: i64, allowed: &str) -> Option<(i64, i64)> {
    let cp = cp?;
    if (modifier & MOD_SHIFT) == 0 {
        return None;
    }
    let normalized = normalize_kitty_functional_codepoint(cp);
    if !is_in_set(allowed, normalized) {
        return None;
    }
    Some((normalized, modifier & !MOD_SHIFT))
}

fn legacy_key_sequences(key: &str) -> &'static [&'static str] {
    match key {
        "up" => &["\x1b[A", "\x1bOA"],
        "down" => &["\x1b[B", "\x1bOB"],
        "right" => &["\x1b[C", "\x1bOC"],
        "left" => &["\x1b[D", "\x1bOD"],
        "home" => &["\x1b[H", "\x1bOH", "\x1b[1~", "\x1b[7~"],
        "end" => &["\x1b[F", "\x1bOF", "\x1b[4~", "\x1b[8~"],
        "insert" => &["\x1b[2~"],
        "delete" => &["\x1b[3~"],
        "pageUp" => &["\x1b[5~", "\x1b[[5~"],
        "pageDown" => &["\x1b[6~", "\x1b[[6~"],
        "clear" => &["\x1b[E", "\x1bOE"],
        "f1" => &["\x1bOP", "\x1b[11~", "\x1b[[A"],
        "f2" => &["\x1bOQ", "\x1b[12~", "\x1b[[B"],
        "f3" => &["\x1bOR", "\x1b[13~", "\x1b[[C"],
        "f4" => &["\x1bOS", "\x1b[14~", "\x1b[[D"],
        "f5" => &["\x1b[15~", "\x1b[[E"],
        "f6" => &["\x1b[17~"],
        "f7" => &["\x1b[18~"],
        "f8" => &["\x1b[19~"],
        "f9" => &["\x1b[20~"],
        "f10" => &["\x1b[21~"],
        "f11" => &["\x1b[23~"],
        "f12" => &["\x1b[24~"],
        _ => &[],
    }
}

fn legacy_shift_sequences(key: &str) -> &'static [&'static str] {
    match key {
        "up" => &["\x1b[a"],
        "down" => &["\x1b[b"],
        "right" => &["\x1b[c"],
        "left" => &["\x1b[d"],
        "clear" => &["\x1b[e"],
        "insert" => &["\x1b[2$"],
        "delete" => &["\x1b[3$"],
        "pageUp" => &["\x1b[5$"],
        "pageDown" => &["\x1b[6$"],
        "home" => &["\x1b[7$"],
        "end" => &["\x1b[8$"],
        _ => &[],
    }
}

fn legacy_ctrl_sequences(key: &str) -> &'static [&'static str] {
    match key {
        "up" => &["\x1bOa"],
        "down" => &["\x1bOb"],
        "right" => &["\x1bOc"],
        "left" => &["\x1bOd"],
        "clear" => &["\x1bOe"],
        "insert" => &["\x1b[2^"],
        "delete" => &["\x1b[3^"],
        "pageUp" => &["\x1b[5^"],
        "pageDown" => &["\x1b[6^"],
        "home" => &["\x1b[7^"],
        "end" => &["\x1b[8^"],
        _ => &[],
    }
}

fn legacy_sequence_key_id(data: &str) -> Option<&'static str> {
    Some(match data {
        "\x1bOA" => "up",
        "\x1bOB" => "down",
        "\x1bOC" => "right",
        "\x1bOD" => "left",
        "\x1bOH" => "home",
        "\x1bOF" => "end",
        "\x1b[E" => "clear",
        "\x1bOE" => "clear",
        "\x1bOe" => "ctrl+clear",
        "\x1b[e" => "shift+clear",
        "\x1b[2~" => "insert",
        "\x1b[2$" => "shift+insert",
        "\x1b[2^" => "ctrl+insert",
        "\x1b[3$" => "shift+delete",
        "\x1b[3^" => "ctrl+delete",
        "\x1b[[5~" => "pageUp",
        "\x1b[[6~" => "pageDown",
        "\x1b[a" => "shift+up",
        "\x1b[b" => "shift+down",
        "\x1b[c" => "shift+right",
        "\x1b[d" => "shift+left",
        "\x1bOa" => "ctrl+up",
        "\x1bOb" => "ctrl+down",
        "\x1bOc" => "ctrl+right",
        "\x1bOd" => "ctrl+left",
        "\x1b[5$" => "shift+pageUp",
        "\x1b[6$" => "shift+pageDown",
        "\x1b[7$" => "shift+home",
        "\x1b[8$" => "shift+end",
        "\x1b[5^" => "ctrl+pageUp",
        "\x1b[6^" => "ctrl+pageDown",
        "\x1b[7^" => "ctrl+home",
        "\x1b[8^" => "ctrl+end",
        "\x1bOP" => "f1",
        "\x1bOQ" => "f2",
        "\x1bOR" => "f3",
        "\x1bOS" => "f4",
        "\x1b[11~" => "f1",
        "\x1b[12~" => "f2",
        "\x1b[13~" => "f3",
        "\x1b[14~" => "f4",
        "\x1b[[A" => "f1",
        "\x1b[[B" => "f2",
        "\x1b[[C" => "f3",
        "\x1b[[D" => "f4",
        "\x1b[[E" => "f5",
        "\x1b[15~" => "f5",
        "\x1b[17~" => "f6",
        "\x1b[18~" => "f7",
        "\x1b[19~" => "f8",
        "\x1b[20~" => "f9",
        "\x1b[21~" => "f10",
        "\x1b[23~" => "f11",
        "\x1b[24~" => "f12",
        "\x1bb" => "alt+left",
        "\x1bf" => "alt+right",
        "\x1bp" => "alt+up",
        "\x1bn" => "alt+down",
        _ => return None,
    })
}

fn matches_legacy_sequence(data: &str, sequences: &[&str]) -> bool {
    sequences.contains(&data)
}

fn matches_legacy_modifier_sequence(data: &str, key: &str, modifier: i64) -> bool {
    if modifier == MOD_SHIFT {
        return matches_legacy_sequence(data, legacy_shift_sequences(key));
    }
    if modifier == MOD_CTRL {
        return matches_legacy_sequence(data, legacy_ctrl_sequences(key));
    }
    false
}

// =============================================================================
// Kitty Protocol Parsing
// =============================================================================

/// Event types from Kitty keyboard protocol (flag 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventType {
    Press,
    Repeat,
    Release,
}

static LAST_EVENT_TYPE: AtomicU8 = AtomicU8::new(0);

fn set_last_event_type(t: KeyEventType) {
    LAST_EVENT_TYPE.store(t as u8, Ordering::Relaxed);
}

/// The event type of the last parsed Kitty sequence.
pub fn last_event_type() -> KeyEventType {
    match LAST_EVENT_TYPE.load(Ordering::Relaxed) {
        1 => KeyEventType::Repeat,
        2 => KeyEventType::Release,
        _ => KeyEventType::Press,
    }
}

#[derive(Debug, Clone, Copy)]
struct ParsedKittySequence {
    codepoint: i64,
    shifted_key: Option<i64>,
    base_layout_key: Option<i64>,
    modifier: i64,
}

const RELEASE_MARKERS: [&str; 8] = [":3u", ":3~", ":3A", ":3B", ":3C", ":3D", ":3H", ":3F"];
const REPEAT_MARKERS: [&str; 8] = [":2u", ":2~", ":2A", ":2B", ":2C", ":2D", ":2H", ":2F"];

/// Check if the input is a key release event (Kitty protocol flag 2).
pub fn is_key_release(data: &str) -> bool {
    if data.contains("\x1b[200~") {
        return false;
    }
    RELEASE_MARKERS.iter().any(|m| data.contains(m))
}

/// Check if the input is a key repeat event (Kitty protocol flag 2).
pub fn is_key_repeat(data: &str) -> bool {
    if data.contains("\x1b[200~") {
        return false;
    }
    REPEAT_MARKERS.iter().any(|m| data.contains(m))
}

fn parse_event_type(s: Option<&str>) -> KeyEventType {
    match s.filter(|s| !s.is_empty()).and_then(parse_int) {
        Some(2) => KeyEventType::Repeat,
        Some(3) => KeyEventType::Release,
        _ => KeyEventType::Press,
    }
}

fn parse_int(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    Some(s.parse::<i64>().unwrap_or(i64::MAX))
}

/// Small cursor over ASCII input used to hand-parse CSI sequences.
struct Cur<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn new(s: &'a str) -> Self {
        Self { s: s.as_bytes(), i: 0 }
    }
    fn eat(&mut self, b: u8) -> bool {
        if self.s.get(self.i) == Some(&b) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn digits(&mut self) -> &'a str {
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).unwrap()
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn at_end(&self) -> bool {
        self.i >= self.s.len()
    }
}

/// Groups of `^\x1b\[(\d+)(?::(\d*))?(?::(\d+))?(?:;(\d+))?(?::(\d+))?u$`.
struct CsiU<'a> {
    codepoint: &'a str,
    shifted: Option<&'a str>,
    base: Option<&'a str>,
    modifier: Option<&'a str>,
    event: Option<&'a str>,
}

fn match_csi_u(data: &str) -> Option<CsiU<'_>> {
    // Backtracking regex emulation: try the possible group assignments.
    let rest = data.strip_prefix("\x1b[")?.strip_suffix('u')?;
    // Split on ';' (at most one allowed).
    let (left, right) = match rest.find(';') {
        Some(p) => (&rest[..p], Some(&rest[p + 1..])),
        None => (rest, None),
    };
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mut codepoint = "";
    let mut shifted = None;
    let mut base = None;
    let mut event = None;
    let modifier;
    let left_parts: Vec<&str> = left.split(':').collect();
    if let Some(right) = right {
        let right_parts: Vec<&str> = right.split(':').collect();
        if right_parts.len() > 2 || right_parts[0].is_empty() || !all_digits(right_parts[0]) {
            return None;
        }
        modifier = Some(right_parts[0]);
        if right_parts.len() == 2 {
            if right_parts[1].is_empty() || !all_digits(right_parts[1]) {
                return None;
            }
            event = Some(right_parts[1]);
        }
        if left_parts.len() > 3 {
            return None;
        }
        for (idx, p) in left_parts.iter().enumerate() {
            if !all_digits(p) {
                return None;
            }
            match idx {
                0 => {
                    if p.is_empty() {
                        return None;
                    }
                    codepoint = p;
                }
                1 => shifted = Some(*p),
                _ => {
                    if p.is_empty() {
                        return None;
                    }
                    base = Some(*p);
                }
            }
        }
    } else {
        modifier = None;
        // Without ';' the trailing (?::(\d+))? event group can't appear without a modifier,
        // but `\x1b[97:65u` etc. can. Up to three colon groups.
        // Up to three colon groups (codepoint, shifted, base) plus an optional trailing
        // event group, which the regex allows even without a modifier.
        if left_parts.len() > 4 {
            return None;
        }
        for (idx, p) in left_parts.iter().enumerate() {
            if !all_digits(p) {
                return None;
            }
            match idx {
                0 => {
                    if p.is_empty() {
                        return None;
                    }
                    codepoint = p;
                }
                1 => shifted = Some(*p),
                2 => {
                    if p.is_empty() {
                        return None;
                    }
                    base = Some(*p);
                }
                _ => {
                    if p.is_empty() {
                        return None;
                    }
                    event = Some(*p);
                }
            }
        }
    }
    Some(CsiU { codepoint, shifted, base, modifier, event })
}

fn parse_kitty_sequence(data: &str) -> Option<ParsedKittySequence> {
    if let Some(m) = match_csi_u(data) {
        let codepoint = parse_int(m.codepoint).unwrap_or(0);
        let shifted_key = m.shifted.filter(|s| !s.is_empty()).and_then(parse_int);
        let base_layout_key = m.base.filter(|s| !s.is_empty()).and_then(parse_int);
        let mod_value = m.modifier.filter(|s| !s.is_empty()).and_then(parse_int).unwrap_or(1);
        let event_type = parse_event_type(m.event);
        set_last_event_type(event_type);
        return Some(ParsedKittySequence {
            codepoint,
            shifted_key,
            base_layout_key,
            modifier: mod_value - 1,
        });
    }

    // Arrow keys / Home / End with modifier: \x1b[1;<mod>(:<event>)?[ABCDHF]
    {
        let mut c = Cur::new(data);
        if c.eat(0x1b) && c.eat(b'[') && c.eat(b'1') && c.eat(b';') {
            let m = c.digits();
            if !m.is_empty() {
                let mut ev = None;
                let save = c.i;
                if c.eat(b':') {
                    let e = c.digits();
                    if e.is_empty() {
                        c.i = save;
                    } else {
                        ev = Some(e);
                    }
                }
                if let Some(last) = c.peek() {
                    c.i += 1;
                    if c.at_end() {
                        let mod_value = parse_int(m).unwrap_or(1);
                        let event_type = parse_event_type(ev);
                        let codepoint = match last {
                            b'A' => Some(ARROW_UP),
                            b'B' => Some(ARROW_DOWN),
                            b'C' => Some(ARROW_RIGHT),
                            b'D' => Some(ARROW_LEFT),
                            _ => None,
                        };
                        if let Some(codepoint) = codepoint {
                            set_last_event_type(event_type);
                            return Some(ParsedKittySequence {
                                codepoint,
                                shifted_key: None,
                                base_layout_key: None,
                                modifier: mod_value - 1,
                            });
                        }
                    }
                }
            }
        }
    }

    // Functional keys: \x1b[<num>(;<mod>)?(:<event>)?~
    {
        let mut c = Cur::new(data);
        if c.eat(0x1b) && c.eat(b'[') {
            let num = c.digits();
            if !num.is_empty() {
                let mut mod_s = None;
                let mut ev = None;
                let mut ok = true;
                if c.eat(b';') {
                    let m = c.digits();
                    if m.is_empty() {
                        ok = false;
                    } else {
                        mod_s = Some(m);
                    }
                }
                if ok && c.eat(b':') {
                    let e = c.digits();
                    if e.is_empty() {
                        ok = false;
                    } else {
                        ev = Some(e);
                    }
                }
                if ok && c.eat(b'~') && c.at_end() {
                    let key_num = parse_int(num).unwrap_or(-1);
                    let mod_value = mod_s.and_then(parse_int).unwrap_or(1);
                    let event_type = parse_event_type(ev);
                    let codepoint = match key_num {
                        2 => Some(FN_INSERT),
                        3 => Some(FN_DELETE),
                        5 => Some(FN_PAGE_UP),
                        6 => Some(FN_PAGE_DOWN),
                        7 => Some(FN_HOME),
                        8 => Some(FN_END),
                        _ => None,
                    };
                    if let Some(codepoint) = codepoint {
                        set_last_event_type(event_type);
                        return Some(ParsedKittySequence {
                            codepoint,
                            shifted_key: None,
                            base_layout_key: None,
                            modifier: mod_value - 1,
                        });
                    }
                }
            }
        }
    }

    // Home/End with modifier: \x1b[1;<mod>(:<event>)?[HF]
    {
        let mut c = Cur::new(data);
        if c.eat(0x1b) && c.eat(b'[') && c.eat(b'1') && c.eat(b';') {
            let m = c.digits();
            if !m.is_empty() {
                let mut ev = None;
                let save = c.i;
                if c.eat(b':') {
                    let e = c.digits();
                    if e.is_empty() {
                        c.i = save;
                    } else {
                        ev = Some(e);
                    }
                }
                let last = c.peek();
                if matches!(last, Some(b'H') | Some(b'F')) {
                    c.i += 1;
                    if c.at_end() {
                        let mod_value = parse_int(m).unwrap_or(1);
                        let event_type = parse_event_type(ev);
                        let codepoint = if last == Some(b'H') { FN_HOME } else { FN_END };
                        set_last_event_type(event_type);
                        return Some(ParsedKittySequence {
                            codepoint,
                            shifted_key: None,
                            base_layout_key: None,
                            modifier: mod_value - 1,
                        });
                    }
                }
            }
        }
    }

    None
}

fn matches_kitty_sequence(data: &str, expected_codepoint: i64, expected_modifier: i64) -> bool {
    let Some(parsed) = parse_kitty_sequence(data) else {
        return false;
    };
    let actual_mod = parsed.modifier & !LOCK_MASK;
    let expected_mod = expected_modifier & !LOCK_MASK;

    let normalized_codepoint = normalize_shifted_letter_identity_codepoint(
        normalize_kitty_functional_codepoint(parsed.codepoint),
        parsed.modifier,
    );
    let normalized_expected = normalize_shifted_letter_identity_codepoint(
        normalize_kitty_functional_codepoint(expected_codepoint),
        expected_modifier,
    );

    if actual_mod == expected_mod && normalized_codepoint == normalized_expected {
        return true;
    }

    if let Some((cp, m)) = get_logical_shifted_symbol_candidate(parsed.shifted_key, actual_mod, SYMBOL_KEYS)
        && (m & !LOCK_MASK) == expected_mod && cp == normalize_kitty_functional_codepoint(expected_codepoint)
    {
        return true;
    }

    if actual_mod != expected_mod {
        return false;
    }

    if parsed.base_layout_key == Some(expected_codepoint) {
        let cp = normalized_codepoint;
        let is_latin_letter = (97..=122).contains(&cp);
        let is_known_symbol = is_in_set(SYMBOL_KEYS, cp);
        if !is_latin_letter && !is_known_symbol {
            return true;
        }
    }
    false
}

fn parse_modify_other_keys_sequence(data: &str) -> Option<(i64, i64)> {
    // /^\x1b\[27;(\d+);(\d+)~$/
    let mut c = Cur::new(data);
    if !(c.eat(0x1b) && c.eat(b'[') && c.eat(b'2') && c.eat(b'7') && c.eat(b';')) {
        return None;
    }
    let m = c.digits();
    if m.is_empty() || !c.eat(b';') {
        return None;
    }
    let k = c.digits();
    if k.is_empty() || !c.eat(b'~') || !c.at_end() {
        return None;
    }
    Some((parse_int(k)?, parse_int(m)? - 1))
}

fn matches_modify_other_keys(data: &str, expected_keycode: i64, expected_modifier: i64) -> bool {
    match parse_modify_other_keys_sequence(data) {
        Some((cp, m)) => cp == expected_keycode && m == expected_modifier,
        None => false,
    }
}

/// Whether we are running inside Windows Terminal (and not over SSH).
pub fn is_windows_terminal_session() -> bool {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    set("WT_SESSION") && !set("SSH_CONNECTION") && !set("SSH_CLIENT") && !set("SSH_TTY")
}

fn matches_raw_backspace(data: &str, expected_modifier: i64) -> bool {
    if data == "\x7f" {
        return expected_modifier == 0;
    }
    if data != "\x08" {
        return false;
    }
    if is_windows_terminal_session() { expected_modifier == MOD_CTRL } else { expected_modifier == 0 }
}

// =============================================================================
// Generic Key Matching
// =============================================================================

fn raw_ctrl_char(key: &str) -> Option<String> {
    let lower = key.to_lowercase();
    let c = lower.chars().next()?;
    let code = c as u32;
    if (97..=122).contains(&code) || c == '[' || c == '\\' || c == ']' || c == '_' {
        return Some(char::from_u32(code & 0x1f).unwrap().to_string());
    }
    if c == '-' {
        return Some("\x1f".to_string());
    }
    None
}

fn is_digit_key(key: &str) -> bool {
    key.len() == 1 && key.as_bytes()[0].is_ascii_digit()
}

fn matches_printable_modify_other_keys(data: &str, expected_keycode: i64, expected_modifier: i64) -> bool {
    let Some((cp, m)) = parse_modify_other_keys_sequence(data) else {
        return false;
    };
    let physical = m == expected_modifier
        && normalize_shifted_letter_identity_codepoint(cp, m)
            == normalize_shifted_letter_identity_codepoint(expected_keycode, expected_modifier);
    if physical {
        return true;
    }
    match get_logical_shifted_symbol_candidate(Some(cp), m, SHIFTED_SYMBOL_KEYS) {
        Some((lcp, lm)) => lm == expected_modifier && lcp == expected_keycode,
        None => false,
    }
}

fn format_key_name_with_modifiers(key_name: &str, modifier: i64) -> Option<String> {
    let effective = modifier & !LOCK_MASK;
    let supported = MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_SUPER;
    if (effective & !supported) != 0 {
        return None;
    }
    let mut mods = Vec::new();
    if effective & MOD_SHIFT != 0 {
        mods.push("shift");
    }
    if effective & MOD_CTRL != 0 {
        mods.push("ctrl");
    }
    if effective & MOD_ALT != 0 {
        mods.push("alt");
    }
    if effective & MOD_SUPER != 0 {
        mods.push("super");
    }
    Some(if mods.is_empty() { key_name.to_string() } else { format!("{}+{key_name}", mods.join("+")) })
}

struct ParsedKeyId {
    key: String,
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_: bool,
}

fn parse_key_id(key_id: &str) -> Option<ParsedKeyId> {
    let mut key = key_id.to_lowercase();
    let mut seen: Vec<&str> = Vec::new();
    loop {
        let modifier = ["shift", "alt", "ctrl", "super"].into_iter().find(|name| key.starts_with(&format!("{name}+")));
        let Some(modifier) = modifier else { break };
        if seen.contains(&modifier) {
            return None;
        }
        seen.push(modifier);
        key = key[modifier.len() + 1..].to_string();
    }
    if key.is_empty() || (key.contains('+') && key != "+") {
        return None;
    }
    Some(ParsedKeyId {
        key,
        ctrl: seen.contains(&"ctrl"),
        shift: seen.contains(&"shift"),
        alt: seen.contains(&"alt"),
        super_: seen.contains(&"super"),
    })
}

/// Match input data against a key identifier string such as `"ctrl+c"`, `"escape"`,
/// `"shift+tab"`, `"alt+enter"`, `"ctrl+shift+p"`.
pub fn matches_key(data: &str, key_id: &str) -> bool {
    let Some(parsed) = parse_key_id(key_id) else {
        return false;
    };
    let key = parsed.key.as_str();
    let mut modifier = 0;
    if parsed.shift {
        modifier |= MOD_SHIFT;
    }
    if parsed.alt {
        modifier |= MOD_ALT;
    }
    if parsed.ctrl {
        modifier |= MOD_CTRL;
    }
    if parsed.super_ {
        modifier |= MOD_SUPER;
    }
    let kitty_active = kitty();

    match key {
        "escape" | "esc" => {
            if modifier != 0 {
                return false;
            }
            return data == "\x1b"
                || matches_kitty_sequence(data, CP_ESCAPE, 0)
                || matches_modify_other_keys(data, CP_ESCAPE, 0);
        }
        "space" => {
            if !kitty_active {
                if modifier == MOD_CTRL && data == "\x00" {
                    return true;
                }
                if modifier == MOD_ALT && data == "\x1b " {
                    return true;
                }
            }
            if modifier == 0 {
                return data == " "
                    || matches_kitty_sequence(data, CP_SPACE, 0)
                    || matches_modify_other_keys(data, CP_SPACE, 0);
            }
            return matches_kitty_sequence(data, CP_SPACE, modifier)
                || matches_modify_other_keys(data, CP_SPACE, modifier);
        }
        "tab" => {
            if modifier == MOD_SHIFT {
                return data == "\x1b[Z"
                    || matches_kitty_sequence(data, CP_TAB, MOD_SHIFT)
                    || matches_modify_other_keys(data, CP_TAB, MOD_SHIFT);
            }
            if modifier == 0 {
                return data == "\t" || matches_kitty_sequence(data, CP_TAB, 0);
            }
            return matches_kitty_sequence(data, CP_TAB, modifier) || matches_modify_other_keys(data, CP_TAB, modifier);
        }
        "enter" | "return" => {
            if modifier == MOD_SHIFT {
                if matches_kitty_sequence(data, CP_ENTER, MOD_SHIFT) || matches_kitty_sequence(data, CP_KP_ENTER, MOD_SHIFT)
                {
                    return true;
                }
                if matches_modify_other_keys(data, CP_ENTER, MOD_SHIFT) {
                    return true;
                }
                if kitty_active {
                    return data == "\x1b\r" || data == "\n";
                }
                return false;
            }
            if modifier == MOD_ALT {
                if matches_kitty_sequence(data, CP_ENTER, MOD_ALT) || matches_kitty_sequence(data, CP_KP_ENTER, MOD_ALT) {
                    return true;
                }
                if matches_modify_other_keys(data, CP_ENTER, MOD_ALT) {
                    return true;
                }
                if !kitty_active {
                    return data == "\x1b\r";
                }
                return false;
            }
            if modifier == 0 {
                return data == "\r"
                    || (!kitty_active && data == "\n")
                    || data == "\x1bOM"
                    || matches_kitty_sequence(data, CP_ENTER, 0)
                    || matches_kitty_sequence(data, CP_KP_ENTER, 0);
            }
            return matches_kitty_sequence(data, CP_ENTER, modifier)
                || matches_kitty_sequence(data, CP_KP_ENTER, modifier)
                || matches_modify_other_keys(data, CP_ENTER, modifier);
        }
        "backspace" => {
            if modifier == MOD_ALT {
                if data == "\x1b\x7f" || data == "\x1b\x08" {
                    return true;
                }
                return matches_kitty_sequence(data, CP_BACKSPACE, MOD_ALT)
                    || matches_modify_other_keys(data, CP_BACKSPACE, MOD_ALT);
            }
            if modifier == MOD_CTRL {
                if matches_raw_backspace(data, MOD_CTRL) {
                    return true;
                }
                return matches_kitty_sequence(data, CP_BACKSPACE, MOD_CTRL)
                    || matches_modify_other_keys(data, CP_BACKSPACE, MOD_CTRL);
            }
            if modifier == 0 {
                return matches_raw_backspace(data, 0)
                    || matches_kitty_sequence(data, CP_BACKSPACE, 0)
                    || matches_modify_other_keys(data, CP_BACKSPACE, 0);
            }
            return matches_kitty_sequence(data, CP_BACKSPACE, modifier)
                || matches_modify_other_keys(data, CP_BACKSPACE, modifier);
        }
        "insert" | "delete" | "home" | "end" | "pageup" | "pagedown" => {
            let (name, cp) = match key {
                "insert" => ("insert", FN_INSERT),
                "delete" => ("delete", FN_DELETE),
                "home" => ("home", FN_HOME),
                "end" => ("end", FN_END),
                "pageup" => ("pageUp", FN_PAGE_UP),
                _ => ("pageDown", FN_PAGE_DOWN),
            };
            if modifier == 0 {
                return matches_legacy_sequence(data, legacy_key_sequences(name)) || matches_kitty_sequence(data, cp, 0);
            }
            if matches_legacy_modifier_sequence(data, name, modifier) {
                return true;
            }
            return matches_kitty_sequence(data, cp, modifier);
        }
        "clear" => {
            if modifier == 0 {
                return matches_legacy_sequence(data, legacy_key_sequences("clear"));
            }
            return matches_legacy_modifier_sequence(data, "clear", modifier);
        }
        "up" | "down" => {
            let (cp, alt_seq) = if key == "up" { (ARROW_UP, "\x1bp") } else { (ARROW_DOWN, "\x1bn") };
            if modifier == MOD_ALT {
                return data == alt_seq || matches_kitty_sequence(data, cp, MOD_ALT);
            }
            if modifier == 0 {
                return matches_legacy_sequence(data, legacy_key_sequences(key)) || matches_kitty_sequence(data, cp, 0);
            }
            if matches_legacy_modifier_sequence(data, key, modifier) {
                return true;
            }
            return matches_kitty_sequence(data, cp, modifier);
        }
        "left" | "right" => {
            let left = key == "left";
            let cp = if left { ARROW_LEFT } else { ARROW_RIGHT };
            if modifier == MOD_ALT {
                let (csi, legacy_upper, legacy_lower) =
                    if left { ("\x1b[1;3D", "\x1bB", "\x1bb") } else { ("\x1b[1;3C", "\x1bF", "\x1bf") };
                return data == csi
                    || (!kitty_active && data == legacy_upper)
                    || data == legacy_lower
                    || matches_kitty_sequence(data, cp, MOD_ALT);
            }
            if modifier == MOD_CTRL {
                let csi = if left { "\x1b[1;5D" } else { "\x1b[1;5C" };
                return data == csi
                    || matches_legacy_modifier_sequence(data, key, MOD_CTRL)
                    || matches_kitty_sequence(data, cp, MOD_CTRL);
            }
            if modifier == 0 {
                return matches_legacy_sequence(data, legacy_key_sequences(key)) || matches_kitty_sequence(data, cp, 0);
            }
            if matches_legacy_modifier_sequence(data, key, modifier) {
                return true;
            }
            return matches_kitty_sequence(data, cp, modifier);
        }
        "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" => {
            if modifier != 0 {
                return false;
            }
            return matches_legacy_sequence(data, legacy_key_sequences(key));
        }
        _ => {}
    }

    let is_letter = key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase();
    let is_digit = is_digit_key(key);
    if key.chars().count() == 1 && (is_letter || is_digit || is_symbol_key_str(key)) {
        let codepoint = key.chars().next().unwrap() as i64;
        let raw_ctrl = raw_ctrl_char(key);

        if modifier == MOD_CTRL + MOD_ALT && !kitty_active
            && let Some(rc) = &raw_ctrl
            && data == format!("\x1b{rc}")
        {
            return true;
        }
        if modifier == MOD_ALT && !kitty_active && data == format!("\x1b{key}") {
            return true;
        }
        if modifier == MOD_CTRL {
            if let Some(rc) = &raw_ctrl
                && data == rc
            {
                return true;
            }
            return matches_kitty_sequence(data, codepoint, MOD_CTRL)
                || matches_printable_modify_other_keys(data, codepoint, MOD_CTRL);
        }
        if modifier == MOD_SHIFT + MOD_CTRL {
            return matches_kitty_sequence(data, codepoint, MOD_SHIFT + MOD_CTRL)
                || matches_printable_modify_other_keys(data, codepoint, MOD_SHIFT + MOD_CTRL);
        }
        if modifier == MOD_SHIFT {
            if is_letter && data == key.to_uppercase() {
                return true;
            }
            return matches_kitty_sequence(data, codepoint, MOD_SHIFT)
                || matches_printable_modify_other_keys(data, codepoint, MOD_SHIFT);
        }
        if modifier != 0 {
            return matches_kitty_sequence(data, codepoint, modifier)
                || matches_printable_modify_other_keys(data, codepoint, modifier);
        }
        return data == key
            || matches_kitty_sequence(data, codepoint, 0)
            || matches_printable_modify_other_keys(data, codepoint, 0);
    }
    false
}

fn format_parsed_key(codepoint: i64, modifier: i64, base_layout_key: Option<i64>) -> Option<String> {
    let normalized = normalize_kitty_functional_codepoint(codepoint);
    let identity = normalize_shifted_letter_identity_codepoint(normalized, modifier);
    let is_latin_letter = (97..=122).contains(&identity);
    let is_digit = (48..=57).contains(&identity);
    let is_known_symbol = is_in_set(SYMBOL_KEYS, identity);
    let effective =
        if is_latin_letter || is_digit || is_known_symbol { identity } else { base_layout_key.unwrap_or(identity) };

    let key_name: String = match effective {
        CP_ESCAPE => "escape".into(),
        CP_TAB => "tab".into(),
        CP_ENTER | CP_KP_ENTER => "enter".into(),
        CP_SPACE => "space".into(),
        CP_BACKSPACE => "backspace".into(),
        FN_DELETE => "delete".into(),
        FN_INSERT => "insert".into(),
        FN_HOME => "home".into(),
        FN_END => "end".into(),
        FN_PAGE_UP => "pageUp".into(),
        FN_PAGE_DOWN => "pageDown".into(),
        ARROW_UP => "up".into(),
        ARROW_DOWN => "down".into(),
        ARROW_LEFT => "left".into(),
        ARROW_RIGHT => "right".into(),
        48..=57 | 97..=122 => char::from_u32(effective as u32).unwrap().to_string(),
        _ if is_in_set(SYMBOL_KEYS, effective) => char_code_ascii(effective).unwrap().to_string(),
        _ => return None,
    };
    format_key_name_with_modifiers(&key_name, modifier)
}

/// Parse input data and return the key identifier if recognized.
pub fn parse_key(data: &str) -> Option<String> {
    if let Some(k) = parse_kitty_sequence(data) {
        let logical = get_logical_shifted_symbol_candidate(k.shifted_key, k.modifier, SYMBOL_KEYS);
        return format_parsed_key(
            logical.map_or(k.codepoint, |l| l.0),
            logical.map_or(k.modifier, |l| l.1),
            k.base_layout_key,
        );
    }
    if let Some((cp, m)) = parse_modify_other_keys_sequence(data) {
        let logical = get_logical_shifted_symbol_candidate(Some(cp), m, SHIFTED_SYMBOL_KEYS);
        return format_parsed_key(logical.map_or(cp, |l| l.0), logical.map_or(m, |l| l.1), None);
    }

    let kitty_active = kitty();
    if kitty_active && (data == "\x1b\r" || data == "\n") {
        return Some("shift+enter".into());
    }
    if let Some(id) = legacy_sequence_key_id(data) {
        return Some(id.into());
    }

    let s = |v: &str| Some(v.to_string());
    match data {
        "\x1b" => return s("escape"),
        "\x1c" => return s("ctrl+\\"),
        "\x1d" => return s("ctrl+]"),
        "\x1f" => return s("ctrl+-"),
        "\x1b\x1b" => return s("ctrl+alt+["),
        "\x1b\x1c" => return s("ctrl+alt+\\"),
        "\x1b\x1d" => return s("ctrl+alt+]"),
        "\x1b\x1f" => return s("ctrl+alt+-"),
        "\t" => return s("tab"),
        _ => {}
    }
    if data == "\r" || (!kitty_active && data == "\n") || data == "\x1bOM" {
        return s("enter");
    }
    match data {
        "\x00" => return s("ctrl+space"),
        " " => return s("space"),
        "\x7f" => return s("backspace"),
        "\x08" => return s(if is_windows_terminal_session() { "ctrl+backspace" } else { "backspace" }),
        "\x1b[Z" => return s("shift+tab"),
        _ => {}
    }
    if !kitty_active && data == "\x1b\r" {
        return s("alt+enter");
    }
    if !kitty_active && data == "\x1b " {
        return s("alt+space");
    }
    if data == "\x1b\x7f" || data == "\x1b\x08" {
        return s("alt+backspace");
    }
    if !kitty_active && data == "\x1bB" {
        return s("alt+left");
    }
    if !kitty_active && data == "\x1bF" {
        return s("alt+right");
    }
    let units: Vec<u16> = data.encode_utf16().collect();
    if !kitty_active && units.len() == 2 && units[0] == 0x1b {
        let code = units[1] as u32;
        if (1..=26).contains(&code) {
            return Some(format!("ctrl+alt+{}", char::from_u32(code + 96).unwrap()));
        }
        if let Some(c) = char::from_u32(code)
            && ((97..=122).contains(&code) || (48..=57).contains(&code) || (code < 128 && SYMBOL_KEYS.contains(c)))
        {
            return Some(format!("alt+{c}"));
        }
    }
    match data {
        "\x1b[A" => return s("up"),
        "\x1b[B" => return s("down"),
        "\x1b[C" => return s("right"),
        "\x1b[D" => return s("left"),
        "\x1b[H" | "\x1bOH" => return s("home"),
        "\x1b[F" | "\x1bOF" => return s("end"),
        "\x1b[3~" => return s("delete"),
        "\x1b[5~" => return s("pageUp"),
        "\x1b[6~" => return s("pageDown"),
        _ => {}
    }
    if units.len() == 1 {
        let code = units[0] as u32;
        if (1..=26).contains(&code) {
            return Some(format!("ctrl+{}", char::from_u32(code + 96).unwrap()));
        }
        if (32..=126).contains(&code) {
            return Some(data.to_string());
        }
    }
    None
}

// =============================================================================
// Kitty CSI-u Printable Decoding
// =============================================================================

const KITTY_PRINTABLE_ALLOWED_MODIFIERS: i64 = MOD_SHIFT | LOCK_MASK;

/// Decode a Kitty CSI-u sequence into a printable character, if applicable.
///
/// Only accepts plain or Shift-modified keys and prefers the shifted keycode when Shift
/// is held.
pub fn decode_kitty_printable(data: &str) -> Option<String> {
    let m = match_csi_u(data)?;
    let codepoint = parse_int(m.codepoint)?;
    let shifted_key = m.shifted.filter(|s| !s.is_empty()).and_then(parse_int);
    let mod_value = m.modifier.filter(|s| !s.is_empty()).and_then(parse_int).unwrap_or(1);
    let modifier = mod_value - 1;
    if (modifier & !KITTY_PRINTABLE_ALLOWED_MODIFIERS) != 0 {
        return None;
    }
    if modifier & (MOD_ALT | MOD_CTRL) != 0 {
        return None;
    }
    let mut effective = codepoint;
    if modifier & MOD_SHIFT != 0
        && let Some(sk) = shifted_key
    {
        effective = sk;
    }
    effective = normalize_kitty_functional_codepoint(effective);
    if effective < 32 {
        return None;
    }
    u32::try_from(effective).ok().and_then(char::from_u32).map(|c| c.to_string())
}

fn decode_modify_other_keys_printable(data: &str) -> Option<String> {
    let (cp, m) = parse_modify_other_keys_sequence(data)?;
    let modifier = m & !LOCK_MASK;
    if (modifier & !MOD_SHIFT) != 0 {
        return None;
    }
    if cp < 32 {
        return None;
    }
    u32::try_from(cp).ok().and_then(char::from_u32).map(|c| c.to_string())
}

/// Decode a Kitty CSI-u or modifyOtherKeys sequence into a printable character.
pub fn decode_printable_key(data: &str) -> Option<String> {
    decode_kitty_printable(data).or_else(|| decode_modify_other_keys_printable(data))
}
