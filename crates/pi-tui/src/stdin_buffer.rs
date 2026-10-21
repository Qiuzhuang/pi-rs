//! Buffers stdin input and emits complete sequences. Port of `packages/tui/src/stdin-buffer.ts`.
//!
//! stdin data can arrive in partial chunks, especially for escape sequences like mouse
//! events. The buffer accumulates input until a complete sequence is detected.
//!
//! Unlike the TypeScript `EventEmitter` version, this type is a synchronous state machine:
//! [`StdinBuffer::process`] returns the emitted events, and when input is left pending the
//! caller must call [`StdinBuffer::handle_timeout`] once [`StdinBuffer::deadline`] passes.
//!
//! Based on code from OpenTUI (<https://github.com/anomalyco/opentui>), MIT License,
//! Copyright (c) 2025 opentui.

use std::time::{Duration, Instant};

const ESC: &str = "\x1b";
const DEFAULT_SEQUENCE_TIMEOUT_MS: u64 = 50;
const DEFAULT_ESCAPE_TIMEOUT_MS: u64 = 10;
const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

/// An event emitted by the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinEvent {
    /// A complete key/mouse/reply sequence or a single character.
    Data(String),
    /// The content of a bracketed paste (without the markers).
    Paste(String),
}

/// Options for [`StdinBuffer`].
#[derive(Debug, Clone, Copy, Default)]
pub struct StdinBufferOptions {
    /// Maximum time to wait for an incomplete keyboard or mouse sequence (default 50ms).
    pub timeout: Option<Duration>,
    /// Maximum time to wait after a lone ESC before treating it as Escape (default 10ms).
    pub escape_timeout: Option<Duration>,
}

/// `^\x1b(?:\](?:1[01];|4;\d{1,3};|7501;\?)|P>\||_G(?:[a-z]=\d+,)*[a-z]=\d+;)`
pub(crate) fn matches_terminal_reply_header(s: &str) -> bool {
    let Some(rest) = s.strip_prefix(ESC) else {
        return false;
    };
    if let Some(osc) = rest.strip_prefix(']') {
        if osc.starts_with("10;") || osc.starts_with("11;") || osc.starts_with("7501;?") {
            return true;
        }
        if let Some(p) = osc.strip_prefix("4;") {
            let digits = p.bytes().take_while(u8::is_ascii_digit).count();
            // \d{1,3}; — with backtracking, any 1..=3 leading digits followed by ';'.
            return (1..=3).any(|n| n <= digits && p.as_bytes().get(n) == Some(&b';'));
        }
        return false;
    }
    if rest.starts_with("P>|") {
        return true;
    }
    if let Some(apc) = rest.strip_prefix("_G") {
        // (?:[a-z]=\d+,)*[a-z]=\d+;
        let b = apc.as_bytes();
        let mut i = 0;
        loop {
            if i >= b.len() || !b[i].is_ascii_lowercase() {
                return false;
            }
            if b.get(i + 1) != Some(&b'=') {
                return false;
            }
            let mut j = i + 2;
            let start = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j == start {
                return false;
            }
            match b.get(j) {
                Some(b';') => return true,
                Some(b',') => i = j + 1,
                _ => return false,
            }
        }
    }
    false
}

fn is_control_string(data: &str) -> bool {
    data.starts_with("\x1b]") || data.starts_with("\x1bP") || data.starts_with("\x1b_")
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum SeqStatus {
    Complete,
    Incomplete,
    NotEscape,
}

fn is_complete_sequence(data: &str) -> SeqStatus {
    if !data.starts_with(ESC) {
        return SeqStatus::NotEscape;
    }
    if data.len() == 1 {
        return SeqStatus::Incomplete;
    }
    let after = &data[1..];
    if after.starts_with('[') {
        if after.starts_with("[M") {
            return if utf16_len(data) >= 6 { SeqStatus::Complete } else { SeqStatus::Incomplete };
        }
        return is_complete_csi_sequence(data);
    }
    if after.starts_with(']') {
        return if data.ends_with("\x1b\\") || data.ends_with('\x07') {
            SeqStatus::Complete
        } else {
            SeqStatus::Incomplete
        };
    }
    if after.starts_with('P') || after.starts_with('_') {
        return if data.ends_with("\x1b\\") { SeqStatus::Complete } else { SeqStatus::Incomplete };
    }
    if after.starts_with('O') {
        return if utf16_len(after) >= 2 { SeqStatus::Complete } else { SeqStatus::Incomplete };
    }
    SeqStatus::Complete
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

fn is_complete_csi_sequence(data: &str) -> SeqStatus {
    if utf16_len(data) < 3 {
        return SeqStatus::Incomplete;
    }
    let payload = &data[2..];
    let last = payload.chars().last().unwrap();
    // charCodeAt of the last UTF-16 unit; for astral chars it's a low surrogate (not in range).
    let last_code = if last.len_utf16() == 1 { last as u32 } else { 0xDC00 };
    if (0x40..=0x7e).contains(&last_code) {
        if let Some(inner) = payload.strip_prefix('<') {
            let body = &inner[..inner.len() - last.len_utf8()];
            let parts: Vec<&str> = body.split(';').collect();
            let digits_ok =
                parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
            if (last == 'M' || last == 'm') && digits_ok {
                return SeqStatus::Complete;
            }
            return SeqStatus::Incomplete;
        }
        return SeqStatus::Complete;
    }
    SeqStatus::Incomplete
}

fn parse_unmodified_kitty_printable_codepoint(sequence: &str) -> Option<u32> {
    // /^\x1b\[(\d+)(?::\d*)?(?::\d+)?u$/
    let rest = sequence.strip_prefix("\x1b[")?.strip_suffix('u')?;
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if parts[0].is_empty() || !all_digits(parts[0]) {
        return None;
    }
    if parts.len() >= 2 && !all_digits(parts[1]) {
        return None;
    }
    if parts.len() == 3 && (parts[2].is_empty() || !all_digits(parts[2])) {
        return None;
    }
    let cp: u64 = parts[0].parse().unwrap_or(u64::MAX);
    if cp >= 32 { u32::try_from(cp).ok().or(Some(u32::MAX)) } else { None }
}

/// Split accumulated buffer into complete sequences and an incomplete remainder.
fn extract_complete_sequences(buffer: &str) -> (Vec<String>, String) {
    let mut sequences = Vec::new();
    let mut pos = 0; // byte offset

    while pos < buffer.len() {
        let remaining = &buffer[pos..];
        if remaining.starts_with(ESC) {
            let terminal_reply = is_control_string(remaining) && matches_terminal_reply_header(remaining);
            // Char boundary offsets: bounds[k] = byte offset after k chars.
            let mut bounds: Vec<usize> = remaining.char_indices().map(|(i, _)| i).collect();
            bounds.push(remaining.len());
            let char_count = bounds.len() - 1;
            let chars: Vec<char> = remaining.chars().collect();
            let mut seq_end = 1usize; // in chars
            while seq_end <= char_count {
                if terminal_reply && seq_end > 2 {
                    let ch = chars[seq_end - 1];
                    if ch == '\x18' || ch == '\x1a' {
                        pos += bounds[seq_end];
                        break;
                    }
                    if ch == '\x1b' && seq_end < char_count && chars[seq_end] != '\\' {
                        pos += bounds[seq_end - 1];
                        break;
                    }
                }
                let candidate = &remaining[..bounds[seq_end]];
                match is_complete_sequence(candidate) {
                    SeqStatus::Complete => {
                        if candidate == "\x1b\x1b" {
                            let next = chars.get(seq_end).copied();
                            if matches!(next, Some('[') | Some(']') | Some('O') | Some('P') | Some('_')) {
                                sequences.push(ESC.to_string());
                                pos += 1;
                                break;
                            }
                        }
                        sequences.push(candidate.to_string());
                        pos += bounds[seq_end];
                        break;
                    }
                    SeqStatus::Incomplete => seq_end += 1,
                    SeqStatus::NotEscape => {
                        sequences.push(candidate.to_string());
                        pos += bounds[seq_end];
                        break;
                    }
                }
            }
            if seq_end > char_count {
                return (sequences, remaining.to_string());
            }
        } else {
            let ch = remaining.chars().next().unwrap();
            sequences.push(ch.to_string());
            pos += ch.len_utf8();
        }
    }
    (sequences, String::new())
}

/// Buffers stdin input and emits complete sequences.
#[derive(Debug)]
pub struct StdinBuffer {
    buffer: String,
    deadline: Option<Instant>,
    timeout: Duration,
    escape_timeout: Duration,
    paste_mode: bool,
    paste_buffer: String,
    pending_kitty_printable_codepoint: Option<u32>,
}

impl Default for StdinBuffer {
    fn default() -> Self {
        Self::new(StdinBufferOptions::default())
    }
}

impl StdinBuffer {
    pub fn new(options: StdinBufferOptions) -> Self {
        Self {
            buffer: String::new(),
            deadline: None,
            timeout: options.timeout.unwrap_or(Duration::from_millis(DEFAULT_SEQUENCE_TIMEOUT_MS)),
            escape_timeout: options.escape_timeout.unwrap_or(Duration::from_millis(DEFAULT_ESCAPE_TIMEOUT_MS)),
            paste_mode: false,
            paste_buffer: String::new(),
            pending_kitty_printable_codepoint: None,
        }
    }

    /// When pending input should be flushed via [`handle_timeout`](Self::handle_timeout).
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Duration of the currently armed timeout (escape or sequence timeout), if any.
    pub fn pending_timeout(&self) -> Option<Duration> {
        self.deadline.map(|_| if self.buffer == ESC { self.escape_timeout } else { self.timeout })
    }

    /// Feed raw bytes. A single byte > 127 is converted to ESC + (byte - 128).
    pub fn process_bytes(&mut self, data: &[u8]) -> Vec<StdinEvent> {
        if data.len() == 1 && data[0] > 127 {
            let s = format!("\x1b{}", char::from(data[0] - 128));
            return self.process(&s);
        }
        let s = String::from_utf8_lossy(data).into_owned();
        self.process(&s)
    }

    /// Feed input; returns the events emitted.
    pub fn process(&mut self, data: &str) -> Vec<StdinEvent> {
        let mut out = Vec::new();
        self.process_into(data, &mut out);
        out
    }

    fn process_into(&mut self, data: &str, out: &mut Vec<StdinEvent>) {
        self.deadline = None;

        if data.is_empty() && self.buffer.is_empty() {
            self.emit_data_sequence(String::new(), out);
            return;
        }
        self.buffer.push_str(data);

        if self.paste_mode {
            let buf = std::mem::take(&mut self.buffer);
            self.paste_buffer.push_str(&buf);
            self.finish_paste_if_complete(out);
            return;
        }

        if let Some(start) = self.buffer.find(BRACKETED_PASTE_START) {
            if start > 0 {
                let before = self.buffer[..start].to_string();
                let (sequences, _) = extract_complete_sequences(&before);
                for s in sequences {
                    self.emit_data_sequence(s, out);
                }
            }
            self.pending_kitty_printable_codepoint = None;
            let after = self.buffer[start + BRACKETED_PASTE_START.len()..].to_string();
            self.buffer.clear();
            self.paste_mode = true;
            self.paste_buffer = after;
            self.finish_paste_if_complete(out);
            return;
        }

        let buf = std::mem::take(&mut self.buffer);
        let (sequences, remainder) = extract_complete_sequences(&buf);
        self.buffer = remainder;
        for s in sequences {
            self.emit_data_sequence(s, out);
        }

        if !self.buffer.is_empty() {
            if matches_terminal_reply_header(&self.buffer) {
                return;
            }
            let timeout = if self.buffer == ESC { self.escape_timeout } else { self.timeout };
            self.deadline = Some(Instant::now() + timeout);
        }
    }

    fn finish_paste_if_complete(&mut self, out: &mut Vec<StdinEvent>) {
        if let Some(end) = self.paste_buffer.find(BRACKETED_PASTE_END) {
            let pasted = self.paste_buffer[..end].to_string();
            let remaining = self.paste_buffer[end + BRACKETED_PASTE_END.len()..].to_string();
            self.paste_mode = false;
            self.paste_buffer.clear();
            self.pending_kitty_printable_codepoint = None;
            out.push(StdinEvent::Paste(pasted));
            if !remaining.is_empty() {
                self.process_into(&remaining, out);
            }
        }
    }

    /// Flush pending input after the timeout elapsed. Returns the events emitted.
    pub fn handle_timeout(&mut self) -> Vec<StdinEvent> {
        let mut out = Vec::new();
        let flushed = self.flush();
        for sequence in flushed {
            if utf16_len(&sequence) > 2 && is_control_string(&sequence) {
                // Recover the Alt key and reparse its tail.
                self.emit_data_sequence(sequence[..2].to_string(), &mut out);
                self.process_into(&sequence[2..], &mut out);
            } else {
                self.emit_data_sequence(sequence, &mut out);
            }
        }
        out
    }

    /// Call [`handle_timeout`](Self::handle_timeout) if the deadline has passed.
    pub fn poll_timeout(&mut self, now: Instant) -> Vec<StdinEvent> {
        match self.deadline {
            Some(d) if now >= d => self.handle_timeout(),
            _ => Vec::new(),
        }
    }

    fn emit_data_sequence(&mut self, sequence: String, out: &mut Vec<StdinEvent>) {
        let mut chars = sequence.chars();
        let raw_cp = match (chars.next(), chars.next()) {
            (Some(c), None) if c.len_utf16() == 1 => Some(c as u32),
            _ => None,
        };
        if raw_cp.is_some() && raw_cp == self.pending_kitty_printable_codepoint {
            self.pending_kitty_printable_codepoint = None;
            return;
        }
        self.pending_kitty_printable_codepoint = parse_unmodified_kitty_printable_codepoint(&sequence);
        out.push(StdinEvent::Data(sequence));
    }

    /// Return and clear any buffered input.
    pub fn flush(&mut self) -> Vec<String> {
        self.deadline = None;
        if self.buffer.is_empty() {
            return Vec::new();
        }
        self.pending_kitty_printable_codepoint = None;
        vec![std::mem::take(&mut self.buffer)]
    }

    pub fn clear(&mut self) {
        self.deadline = None;
        self.buffer.clear();
        self.paste_mode = false;
        self.paste_buffer.clear();
        self.pending_kitty_printable_codepoint = None;
    }

    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }

    pub fn destroy(&mut self) {
        self.clear();
    }
}
