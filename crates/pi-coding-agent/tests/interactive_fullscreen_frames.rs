//! Fullscreen-mode frame output while a response streams and tools run, through the real
//! interactive mode (agent events on runtime threads, frames on the TUI event loop).
//!
//! Regression for flicker in fullscreen mode: agent events rebuilt the streaming tool row
//! (`content_box.clear()` + re-add) on a runtime thread while the TUI loop rendered, so frames
//! timed by the spinner showed the transcript without the tool row and the next frame put it back:
//! the whole viewport jumped up and down. Frames must be differential (no `\x1b[2J`, one
//! synchronized write each) and the screen must never bounce back to a state it just left.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use pi_ai::providers::faux::{FauxMessageOptions, faux_assistant_message, faux_text, faux_thinking, faux_tool_call};
use pi_ai::types::StopReason;
use pi_coding_agent::core::agent_session_runtime::{AgentSessionRuntime, CreateAgentSessionRuntimeFactory};
use pi_coding_agent::core::agent_session_services::AgentSessionServices;
use pi_coding_agent::core::settings_schema::TuiMode;
use pi_coding_agent::modes::interactive::interactive_mode::{InteractiveMode, InteractiveModeOptions};
use pi_tui::terminal::Terminal;
use pi_tui::testing::VirtualTerminal;
use serde_json::json;
use support::*;

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const BSU: &str = "\x1b[?2026h";
const ESU: &str = "\x1b[?2026l";
const ROWS: usize = 60;

struct Fixture {
    harness: Harness,
    mode: InteractiveMode,
    terminal: VirtualTerminal,
    agent_dir: std::path::PathBuf,
    run: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
}

impl Fixture {
    async fn new() -> Fixture {
        let agent_dir = std::env::temp_dir().join(format!("pi-fullscreen-frames-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&agent_dir).unwrap();
        // SAFETY: tests in this binary are serialized by LOCK.
        unsafe {
            std::env::set_var("PI_CODING_AGENT_DIR", &agent_dir);
            std::env::set_var("PI_OFFLINE", "1");
            std::env::set_var("PI_SKIP_VERSION_CHECK", "1");
        }
        let harness = create_harness(HarnessOptions {
            settings: Some(json!({ "quietStartup": true, "tuiMode": "fullscreen" })),
            faux_tokens_per_second: Some(300.0),
            ..Default::default()
        })
        .await
        .unwrap();
        let session = harness.session.clone();
        let services = AgentSessionServices {
            cwd: harness.session_manager.get_cwd(),
            agent_dir: agent_dir.to_string_lossy().into_owned(),
            model_runtime: harness.model_runtime.clone(),
            settings_manager: harness.settings_manager.clone(),
            resource_loader: session.resource_loader(),
            diagnostics: Vec::new(),
        };
        let factory: CreateAgentSessionRuntimeFactory =
            Arc::new(|_| Box::pin(async { Err(anyhow::anyhow!("session replacement is not supported in this test")) }));
        let runtime = Arc::new(AgentSessionRuntime::new(session, &services, &factory, None, None));
        let terminal = VirtualTerminal::new(100, ROWS);
        let mode = InteractiveMode::new(
            runtime,
            Some(&InteractiveModeOptions {
                terminal: Some(Arc::new(terminal.clone()) as Arc<dyn Terminal>),
                tui_mode: Some(TuiMode::Fullscreen),
                ..Default::default()
            }),
        );
        mode.init().await.unwrap();
        let m = mode.clone();
        let run = Some(tokio::spawn(async move { m.run().await }));
        let fx = Fixture { harness, mode, terminal, agent_dir, run };
        fx.wait_for("the editor", |s| s.contains('─')).await;
        fx
    }

    fn screen(&self) -> String {
        self.terminal.get_viewport().join("\n")
    }

    async fn wait_for(&self, what: &str, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let screen = self.screen();
            if predicate(&screen) {
                return screen;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}; screen:\n{screen}");
            }
        }
    }

    fn cleanup(mut self) {
        if let Some(run) = self.run.take() {
            run.abort();
        }
        self.mode.stop(None);
        self.harness.cleanup();
        let _ = std::fs::remove_dir_all(&self.agent_dir);
    }
}

/// Strip escape sequences (CSI, OSC, APC/DCS strings) from one rendered row.
fn plain(row: &str) -> String {
    let mut out = String::new();
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') | Some('_') | Some('P') | Some('^') => {
                while let Some(n) = chars.next() {
                    if n == '\x07' {
                        break;
                    }
                    if n == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out.trim_end().to_string()
}

/// Rows written by one frame: `(row, text)` for each `CSI row;1H CSI 2K text`.
fn frame_rows(frame: &str) -> Vec<(usize, String)> {
    let mut rows = Vec::new();
    let mut parts = frame.split("\x1b[").peekable();
    parts.next();
    let mut current: Option<(usize, String)> = None;
    for part in parts {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        let is_row_start = !digits.is_empty() && part[digits.len()..].starts_with(";1H");
        if is_row_start {
            if let Some(row) = current.take() {
                rows.push(row);
            }
            current = Some((digits.parse().unwrap(), String::new()));
            continue;
        }
        if let Some((_, text)) = current.as_mut()
            && let Some(rest) = part.strip_prefix("2K")
        {
            text.push_str(rest);
        } else if let Some((_, text)) = current.as_mut() {
            text.push_str("\x1b[");
            text.push_str(part);
        }
    }
    if let Some(row) = current {
        rows.push(row);
    }
    rows.into_iter().map(|(r, t)| (r, plain(&t))).collect()
}

/// Mask what legitimately animates (spinner glyphs, elapsed timers) so only layout changes count.
fn normalize(row: &str) -> String {
    let mut out = String::new();
    for c in row.chars() {
        out.push(if ('\u{2800}'..='\u{28ff}').contains(&c) { '#' } else { c });
    }
    let re = regex::Regex::new(r"(Elapsed|Took) [0-9.]+s").unwrap();
    re.replace_all(&out, "$1 #").into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streaming_tool_calls_render_stable_differential_frames() {
    let _lock = LOCK.lock().await;
    let fx = Fixture::new().await;
    let body: String = (0..80).map(|i| format!("def function_{i}(x):\n    return x * {i}\n\n")).collect();
    let mut write_args = serde_json::Map::new();
    write_args.insert("path".into(), json!("big.py"));
    write_args.insert("content".into(), json!(body));
    let mut read_args = serde_json::Map::new();
    read_args.insert("path".into(), json!("big.py"));
    fx.harness.set_responses(vec![
        faux_assistant_message(
            vec![
                faux_thinking("Let me think about how to approach this. I should write the file first and then read it back."),
                faux_text("I'll start by writing a file and reading it back."),
                faux_tool_call("write", write_args, None),
                faux_tool_call("read", read_args, None),
            ],
            FauxMessageOptions { stop_reason: Some(StopReason::ToolUse), ..Default::default() },
        )
        .into(),
        faux_assistant_message(
            vec![faux_text("All done. The file was written and read back successfully.")],
            FauxMessageOptions::default(),
        )
        .into(),
    ]);

    fx.terminal.send_input("Please write the file");
    fx.wait_for("the typed prompt", |s| s.contains("Please write the file")).await;
    fx.terminal.send_input("\r");
    fx.wait_for("the submitted prompt", |s| s.contains("Working")).await;
    let redraws_before = fx.mode.tui().full_redraws();
    // Rebuild the screen as the frames since now draw it.
    let mut screen: Vec<String> = fx.terminal.get_viewport().iter().map(|l| normalize(l.trim_end())).collect();
    fx.terminal.clear_writes();
    fx.wait_for("the final answer", |s| s.contains("All done. The file was written")).await;
    let calls = fx.terminal.write_calls();

    let mut history: Vec<Vec<String>> = vec![screen.clone()];
    let mut frames = 0;
    for call in &calls {
        if !call.contains(BSU) {
            continue; // title, progress and other non-frame writes
        }
        frames += 1;
        assert!(call.starts_with(BSU) && call.ends_with(ESU), "frame is not one synchronized write: {call:?}");
        assert!(!call.contains("\x1b[2J"), "frame {frames} cleared the screen: {call:?}");
        for (row, text) in frame_rows(call) {
            if (1..=ROWS).contains(&row) {
                screen[row - 1] = normalize(&text);
            }
        }
        // A screen equal to one shown 2-4 frames ago, with something else in between, is a bounce.
        let n = history.len();
        for back in 2..=4.min(n) {
            let earlier = &history[n - back];
            if *earlier == screen && history[n - back + 1..].iter().all(|h| *h != screen) {
                let previous = &history[n - 1];
                let diff: Vec<String> = (0..ROWS)
                    .filter(|&r| previous[r] != screen[r])
                    .map(|r| format!("row {}: {:?} -> {:?}", r + 1, previous[r], screen[r]))
                    .collect();
                panic!("frame {frames} bounced back to the screen from {back} frames earlier:\n{}", diff.join("\n"));
            }
        }
        history.push(screen.clone());
    }
    assert!(frames > 10, "expected the stream to render many frames, got {frames}");
    assert_eq!(fx.mode.tui().full_redraws(), redraws_before, "no full redraw while streaming and running tools");
    fx.cleanup();
}
