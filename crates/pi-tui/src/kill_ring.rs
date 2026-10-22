//! Ring buffer for Emacs-style kill/yank operations. Port of `packages/tui/src/kill-ring.ts`.

/// Tracks killed (deleted) text entries. Consecutive kills can accumulate into a single
/// entry. Supports yank (paste most recent) and yank-pop (cycle through older entries).
#[derive(Debug, Clone, Default)]
pub struct KillRing {
    ring: Vec<String>,
}

impl KillRing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add text to the kill ring. With `accumulate`, merge with the most recent entry,
    /// prepending (backward deletion) or appending (forward deletion).
    pub fn push(&mut self, text: &str, prepend: bool, accumulate: bool) {
        if text.is_empty() {
            return;
        }
        if accumulate
            && let Some(last) = self.ring.pop()
        {
            self.ring.push(if prepend { format!("{text}{last}") } else { format!("{last}{text}") });
            return;
        }
        self.ring.push(text.to_string());
    }

    /// Most recent entry without modifying the ring.
    pub fn peek(&self) -> Option<&str> {
        self.ring.last().map(String::as_str)
    }

    /// Move last entry to front (for yank-pop cycling).
    pub fn rotate(&mut self) {
        if self.ring.len() > 1 {
            let last = self.ring.pop().unwrap();
            self.ring.insert(0, last);
        }
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}
