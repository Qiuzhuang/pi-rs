//! Port of stdin-buffer.test.ts. Timers are simulated with a virtual clock.

use std::time::Duration;

use pi_tui::keys::matches_key;
use pi_tui::stdin_buffer::{StdinBuffer, StdinBufferOptions, StdinEvent};

struct Sim {
    buf: StdinBuffer,
    now: u64,
    deadline: Option<u64>,
    seqs: Vec<String>,
    pastes: Vec<String>,
}

impl Sim {
    fn new(opts: StdinBufferOptions) -> Self {
        Self { buf: StdinBuffer::new(opts), now: 0, deadline: None, seqs: vec![], pastes: vec![] }
    }
    fn with_timeout(ms: u64) -> Self {
        Self::new(StdinBufferOptions { timeout: Some(Duration::from_millis(ms)), escape_timeout: None })
    }
    fn absorb(&mut self, events: Vec<StdinEvent>) {
        for e in events {
            match e {
                StdinEvent::Data(s) => self.seqs.push(s),
                StdinEvent::Paste(s) => self.pastes.push(s),
            }
        }
        self.deadline = self.buf.pending_timeout().map(|d| self.now + d.as_millis() as u64);
    }
    fn process(&mut self, data: &str) {
        let ev = self.buf.process(data);
        self.absorb(ev);
    }
    fn tick(&mut self, ms: u64) {
        let target = self.now + ms;
        while let Some(d) = self.deadline {
            if d > target {
                break;
            }
            self.now = d;
            let ev = self.buf.handle_timeout();
            self.absorb(ev);
        }
        self.now = target;
    }
    fn destroy(&mut self) {
        self.buf.destroy();
        self.deadline = None;
    }
}

fn sim() -> Sim {
    Sim::with_timeout(10)
}

#[test]
fn regular_characters() {
    let mut s = sim();
    s.process("a");
    assert_eq!(s.seqs, ["a"]);
    let mut s = sim();
    s.process("abc");
    assert_eq!(s.seqs, ["a", "b", "c"]);
    let mut s = sim();
    s.process("hello 世界");
    assert_eq!(s.seqs, ["h", "e", "l", "l", "o", " ", "世", "界"]);
}

#[test]
fn complete_escape_sequences() {
    for seq in ["\x1b[<35;20;5m", "\x1b[A", "\x1b[11~", "\x1ba", "\x1bOA"] {
        let mut s = sim();
        s.process(seq);
        assert_eq!(s.seqs, [seq]);
    }
}

#[test]
fn buffers_incomplete_mouse_sgr() {
    let mut s = sim();
    s.process("\x1b");
    assert!(s.seqs.is_empty());
    assert_eq!(s.buf.get_buffer(), "\x1b");
    s.process("[<35");
    assert_eq!(s.buf.get_buffer(), "\x1b[<35");
    s.process(";20;5m");
    assert_eq!(s.seqs, ["\x1b[<35;20;5m"]);
    assert_eq!(s.buf.get_buffer(), "");
}

#[test]
fn buffers_incomplete_csi_and_many_chunks() {
    let mut s = sim();
    s.process("\x1b[");
    s.process("1;");
    assert!(s.seqs.is_empty());
    s.process("5H");
    assert_eq!(s.seqs, ["\x1b[1;5H"]);
    let mut s = sim();
    for c in ["\x1b", "[", "<", "3", "5", ";", "2", "0", ";", "5", "m"] {
        s.process(c);
    }
    assert_eq!(s.seqs, ["\x1b[<35;20;5m"]);
}

#[test]
fn flushes_incomplete_sequence_after_timeout() {
    let mut s = sim();
    s.process("\x1b[<35");
    assert!(s.seqs.is_empty());
    s.tick(15);
    assert_eq!(s.seqs, ["\x1b[<35"]);
}

#[test]
fn lone_esc_then_cr_after_timeout() {
    let mut s = sim();
    s.process("\x1b");
    s.tick(20);
    s.process("\r");
    assert_eq!(s.seqs, ["\x1b", "\r"]);
    assert!(matches_key(&s.seqs[0], "escape"));
}

#[test]
fn merges_esc_cr_within_larger_escape_timeout() {
    let mut s = Sim::new(StdinBufferOptions { timeout: None, escape_timeout: Some(Duration::from_millis(100)) });
    s.process("\x1b");
    s.tick(20);
    s.process("\r");
    assert_eq!(s.seqs, ["\x1b\r"]);
    assert!(matches_key(&s.seqs[0], "alt+enter"));
}

#[test]
fn sequence_timeout_not_applied_to_lone_esc() {
    let mut s = Sim::with_timeout(100);
    s.process("\x1b");
    s.tick(20);
    s.process("\r");
    assert_eq!(s.seqs, ["\x1b", "\r"]);
}

#[test]
fn keeps_fragmented_mouse_sequence_buffered_by_default() {
    let mut s = Sim::new(StdinBufferOptions::default());
    s.process("\x1b[");
    s.tick(20);
    assert!(s.seqs.is_empty());
    s.process("<65;48;39M");
    assert_eq!(s.seqs, ["\x1b[<65;48;39M"]);
}

const REPLIES: [(&str, &str); 7] = [
    ("\x1b]4;1;", "\x1b]4;1;rgb:eaea/6c6c/6d6d\x07"),
    ("\x1b]10;", "\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
    ("\x1b]11;", "\x1b]11;rgb:0000/0000/0000\x07"),
    ("\x1b]4;1;", "\x1b]4;1;rgb:6464/6464/4d4d\x07"),
    ("\x1bP>|", "\x1bP>|Ghostty 1.3.1\x1b\\"),
    ("\x1b_Gi=1;", "\x1b_Gi=1;OK\x1b\\"),
    ("\x1b]7501;?", "\x1b]7501;?\x1b\\"),
];

#[test]
fn keeps_delayed_replies_intact_at_every_split() {
    for (header, reply) in REPLIES {
        for split in header.len()..reply.len() {
            let mut s = Sim::new(StdinBufferOptions::default());
            s.process(&reply[..split]);
            s.tick(5000);
            s.process(&format!("{}x", &reply[split..]));
            assert!(!s.seqs.iter().any(|q| matches_key(q, "ctrl+g")), "{reply:?} split {split}");
            assert_eq!(s.seqs, [reply, "x"], "{reply:?} split {split}");
        }
    }
}

#[test]
fn keeps_osc_reply_buffered_across_expired_timeouts() {
    let mut s = sim();
    s.process("\x1b]4;1;rgb:eaea/6c");
    s.tick(100);
    assert!(s.seqs.is_empty());
    s.process("6c/6d6d");
    s.tick(100);
    assert!(s.seqs.is_empty());
    s.process("\x07");
    assert_eq!(s.seqs, [REPLIES[0].1]);
}

#[test]
fn bare_string_introducers_time_out_as_alt_keys() {
    let mut s = sim();
    for prefix in ["\x1b]", "\x1bP", "\x1b_"] {
        s.process(prefix);
        s.tick(10);
    }
    assert_eq!(s.seqs, ["\x1b]", "\x1bP", "\x1b_"]);
}

#[test]
fn recovers_batched_legacy_alt_input() {
    for ambiguous in
        ["\x1b]a", "\x1b]1", "\x1b]10", "\x1b]0;title", "\x1b]4;", "\x1b]4;a;", "\x1bPa", "\x1bP>", "\x1b_a", "\x1b_G", "\x1b_Gi=1"]
    {
        let mut s = Sim::new(StdinBufferOptions::default());
        s.process(ambiguous);
        s.tick(20);
        s.process("x\r\x03\x04");
        s.tick(49);
        assert!(s.seqs.is_empty(), "{ambiguous:?}");
        s.tick(1);
        let mut want: Vec<String> = vec![ambiguous[..2].to_string()];
        want.extend(ambiguous[2..].chars().map(|c| c.to_string()));
        want.extend(["x", "\r", "\x03", "\x04"].map(String::from));
        assert_eq!(s.seqs, want, "{ambiguous:?}");
        assert_eq!(s.buf.get_buffer(), "");
        s.process("y\r\x03\x04");
        assert_eq!(s.seqs[s.seqs.len() - 4..], ["y", "\r", "\x03", "\x04"]);
    }
}

#[test]
fn recognizes_reply_header_assembled_within_timeout() {
    let mut s = sim();
    s.process("\x1b]1");
    s.tick(5);
    s.process("0;");
    s.tick(5000);
    assert!(s.seqs.is_empty());
    s.process("rgb:ffff/ffff/ffff\x1b\\");
    assert_eq!(s.seqs, [REPLIES[1].1]);
}

#[test]
fn recovers_from_unterminated_reply_on_new_escape() {
    let mut s = sim();
    s.process("\x1b]4;1;rgb:eaea/6c");
    s.tick(100);
    s.process("\x1b");
    s.tick(100);
    s.process("[Ahello");
    assert_eq!(s.seqs, ["\x1b[A", "h", "e", "l", "l", "o"]);
}

#[test]
fn discards_canceled_reply() {
    let mut s = sim();
    for cancel in ["\x18", "\x1a"] {
        s.process(&format!("\x1b]11;rgb:0000{cancel}x"));
    }
    assert_eq!(s.seqs, ["x", "x"]);
}

#[test]
fn mixed_content() {
    let mut s = sim();
    s.process("abc\x1b[A");
    assert_eq!(s.seqs, ["a", "b", "c", "\x1b[A"]);
    let mut s = sim();
    s.process("\x1b[Aabc");
    assert_eq!(s.seqs, ["\x1b[A", "a", "b", "c"]);
    let mut s = sim();
    s.process("\x1b[A\x1b[B\x1b[C");
    assert_eq!(s.seqs, ["\x1b[A", "\x1b[B", "\x1b[C"]);
    let mut s = sim();
    s.process("abc\x1b[<35");
    assert_eq!(s.seqs, ["a", "b", "c"]);
    assert_eq!(s.buf.get_buffer(), "\x1b[<35");
    s.process(";20;5m");
    assert_eq!(s.seqs, ["a", "b", "c", "\x1b[<35;20;5m"]);
}

#[test]
fn kitty_sequences() {
    let cases: [(&str, &[&str]); 12] = [
        ("\x1b[97u", &["\x1b[97u"]),
        ("\x1b[97;1:3u", &["\x1b[97;1:3u"]),
        ("\x1b[97u\x1b[97;1:3u", &["\x1b[97u", "\x1b[97;1:3u"]),
        ("\x1b[97u\x1b[97;1:3u\x1b[98u\x1b[98;1:3u", &["\x1b[97u", "\x1b[97;1:3u", "\x1b[98u", "\x1b[98;1:3u"]),
        ("\x1b[1;1:1A", &["\x1b[1;1:1A"]),
        ("\x1b[3;1:3~", &["\x1b[3;1:3~"]),
        ("\x1b\x1b[27;129:3u", &["\x1b", "\x1b[27;129:3u"]),
        ("\x1b\x1b[27;1:3u", &["\x1b", "\x1b[27;1:3u"]),
        ("\x1b\x1b", &["\x1b\x1b"]),
        ("a\x1b[97;1:3u", &["a", "\x1b[97;1:3u"]),
        ("\x1b[224uà", &["\x1b[224u"]),
        ("\x1b[97ub", &["\x1b[97u", "b"]),
    ];
    for (input, want) in cases {
        let mut s = sim();
        s.process(input);
        assert_eq!(s.seqs, want, "{input:?}");
    }
    let mut s = sim();
    s.process("\x1b[64u");
    s.process("@");
    assert_eq!(s.seqs, ["\x1b[64u"]);
    let mut s = sim();
    s.process("\x1b[64;3u@");
    assert_eq!(s.seqs, ["\x1b[64;3u", "@"]);
    let mut s = sim();
    s.process("\x1b[104u\x1b[104;1:3u\x1b[105u\x1b[105;1:3u");
    assert_eq!(s.seqs, ["\x1b[104u", "\x1b[104;1:3u", "\x1b[105u", "\x1b[105;1:3u"]);
}

#[test]
fn mouse_events() {
    for seq in ["\x1b[<0;10;5M", "\x1b[<0;10;5m", "\x1b[<35;20;5m"] {
        let mut s = sim();
        s.process(seq);
        assert_eq!(s.seqs, [seq]);
    }
    let mut s = sim();
    for c in ["\x1b[<3", "5;1", "5;", "10m"] {
        s.process(c);
    }
    assert_eq!(s.seqs, ["\x1b[<35;15;10m"]);
    let mut s = sim();
    s.process("\x1b[<35;1;1m\x1b[<35;2;2m\x1b[<35;3;3m");
    assert_eq!(s.seqs, ["\x1b[<35;1;1m", "\x1b[<35;2;2m", "\x1b[<35;3;3m"]);
    let mut s = sim();
    s.process("\x1b[M abc");
    assert_eq!(s.seqs, ["\x1b[M ab", "c"]);
    let mut s = sim();
    s.process("\x1b[M");
    assert_eq!(s.buf.get_buffer(), "\x1b[M");
    s.process(" a");
    assert_eq!(s.buf.get_buffer(), "\x1b[M a");
    s.process("b");
    assert_eq!(s.seqs, ["\x1b[M ab"]);
}

#[test]
fn edge_cases() {
    let mut s = sim();
    s.process("");
    assert_eq!(s.seqs, [""]);

    let mut s = sim();
    s.process("\x1b");
    assert!(s.seqs.is_empty());
    s.tick(15);
    assert_eq!(s.seqs, ["\x1b"]);

    let mut s = Sim::new(StdinBufferOptions::default());
    s.process("\x1b");
    s.tick(20);
    assert_eq!(s.seqs, ["\x1b"]);

    let mut s = sim();
    s.process("\x1b");
    assert_eq!(s.buf.flush(), ["\x1b"]);

    let mut s = sim();
    let ev = s.buf.process_bytes(b"\x1b[A");
    s.absorb(ev);
    assert_eq!(s.seqs, ["\x1b[A"]);

    let long_seq = format!("\x1b[{}H", "1;".repeat(50));
    let mut s = sim();
    s.process(&long_seq);
    assert_eq!(s.seqs, [long_seq]);
}

#[test]
fn flush_and_clear() {
    let mut s = sim();
    s.process("\x1b[<35");
    assert_eq!(s.buf.flush(), ["\x1b[<35"]);
    assert_eq!(s.buf.get_buffer(), "");
    let mut s = sim();
    assert!(s.buf.flush().is_empty());
    let mut s = sim();
    s.process("\x1b[<35");
    s.tick(15);
    assert_eq!(s.seqs, ["\x1b[<35"]);
    let mut s = sim();
    s.process("\x1b[<35");
    s.buf.clear();
    assert_eq!(s.buf.get_buffer(), "");
    assert!(s.seqs.is_empty());
}

#[test]
fn bracketed_paste() {
    let mut s = sim();
    s.process("\x1b[200~hello world\x1b[201~");
    assert_eq!(s.pastes, ["hello world"]);
    assert!(s.seqs.is_empty());

    let mut s = sim();
    s.process("\x1b[200~");
    s.process("hello ");
    assert!(s.pastes.is_empty());
    s.process("world\x1b[201~");
    assert_eq!(s.pastes, ["hello world"]);

    let mut s = sim();
    s.process("a");
    s.process("\x1b[200~pasted\x1b[201~");
    s.process("b");
    assert_eq!(s.seqs, ["a", "b"]);
    assert_eq!(s.pastes, ["pasted"]);

    let mut s = sim();
    s.process("\x1b[200~line1\nline2\nline3\x1b[201~");
    assert_eq!(s.pastes, ["line1\nline2\nline3"]);

    let mut s = sim();
    s.process("\x1b[200~Hello 世界 🎉\x1b[201~");
    assert_eq!(s.pastes, ["Hello 世界 🎉"]);
    assert!(s.seqs.is_empty());
}

#[test]
fn destroy_clears_buffer_and_timeouts() {
    let mut s = sim();
    s.process("\x1b[<35");
    s.destroy();
    assert_eq!(s.buf.get_buffer(), "");
    s.tick(15);
    assert!(s.seqs.is_empty());
}
