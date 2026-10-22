//! Program Status Protocol (OSC 7501). Port of `packages/tui/src/program-status.ts`.
//!
//! A program tells the terminal whether it is idle, working, blocked on the user, done, or
//! failed. Only the root record is supported.

use base64::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Blocked,
    Done,
    Error,
    /// Removes the status instead of reporting one.
    Clear,
}

impl ProgramState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Error => "error",
            Self::Clear => "clear",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

impl BlockedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatus {
    pub state: ProgramState,
    /// Stable program name, `[A-Za-z0-9_.+-]{1,32}`. Other values are omitted.
    pub app: Option<String>,
    /// What a blocked program waits for. Omitted for other states.
    pub kind: Option<BlockedKind>,
    /// One human-readable line.
    pub message: Option<String>,
}

impl ProgramStatus {
    pub fn new(state: ProgramState) -> Self {
        Self { state, app: None, kind: None, message: None }
    }
}

/// Feature detection query. A supporting terminal replies with the same body.
pub const PROGRAM_STATUS_QUERY: &str = "\x1b]7501;?\x1b\\";

/// The reply to [`PROGRAM_STATUS_QUERY`].
pub fn is_program_status_reply(sequence: &str) -> bool {
    let Some(body) = sequence.strip_prefix("\x1b]7501;?") else {
        return false;
    };
    let body = if let Some(b) = body.strip_suffix('\x07') {
        b
    } else if let Some(b) = body.strip_suffix("\x1b\\") {
        b
    } else {
        return false;
    };
    !body.contains(['\x07', '\x1b'])
}

const MAX_MESSAGE_BYTES: usize = 2048;

fn is_valid_app(app: &str) -> bool {
    (1..=32).contains(&app.len())
        && app.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
}

fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = 0;
    for (i, c) in text.char_indices() {
        if i + c.len_utf8() > max_bytes {
            break;
        }
        end = i + c.len_utf8();
    }
    &text[..end]
}

/// Encode a status report. Control characters in the message are replaced by spaces.
pub fn format_program_status(status: &ProgramStatus) -> String {
    let mut pairs = vec![format!("state={}", status.state.as_str())];
    if let Some(app) = status.app.as_deref().filter(|a| is_valid_app(a)) {
        pairs.push(format!("app={app}"));
    }
    if status.state == ProgramState::Blocked
        && let Some(kind) = status.kind
    {
        pairs.push(format!("kind={}", kind.as_str()));
    }
    let raw = status.message.as_deref().unwrap_or("");
    let mut replaced = String::with_capacity(raw.len());
    let mut in_run = false;
    for c in raw.chars() {
        let control = (c as u32) <= 0x1f || (0x7f..=0x9f).contains(&(c as u32));
        if control {
            if !in_run {
                replaced.push(' ');
            }
            in_run = true;
        } else {
            replaced.push(c);
            in_run = false;
        }
    }
    let message = truncate_utf8(crate::utils::js_trim(&replaced), MAX_MESSAGE_BYTES);
    if !message.is_empty() {
        pairs.push(format!("msg={}", base64::engine::general_purpose::STANDARD.encode(message.as_bytes())));
    }
    format!("\x1b]7501;{}\x1b\\", pairs.join(":"))
}
