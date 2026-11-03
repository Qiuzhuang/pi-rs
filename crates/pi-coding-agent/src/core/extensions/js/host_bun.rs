//! Subprocess backend: runs the extensions in a `bun` process and talks to it over a
//! line-delimited JSON protocol on stdio (see `js/bun/entry.mjs`).
//!
//! Why: Boa is an interpreted pure-Rust engine; with many JS extensions the difference to a
//! compiled runtime (bun, V8) shows up in module load time and in every hot path. The bridge
//! protocol was already JSON + handle markers, so the same glue (`bridge.js`, `pi-*.js`) runs
//! unchanged in bun.
//!
//! Threading model:
//!
//! - a dedicated reader thread consumes the child's stdout: host-call requests, responses to
//!   Rust-originated calls, tagged JS stdout;
//! - a dedicated writer thread serializes Rust -> child lines onto the child's stdin;
//! - synchronous host calls (`__piHost.call`) block the child's event loop on the fd-3 pipe,
//!   so their handlers must not call back into JS (`JsHost::reentrant_js` is false);
//! - blocking calls from Rust (render thread, UI thread) wait on a oneshot that the reader
//!   thread resolves; the child's event loop services the call itself.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use std::{fs, thread};

use futures::channel::oneshot;
use parking_lot::Mutex as PlMutex;
use serde_json::{Value, json};

use super::host::{BoxFuture, HandleOps, JsHandle, JsHost};

/// The entry script of the bun subprocess.
const BUN_ENTRY: &str = include_str!("../../../../js/bun/entry.mjs");
const JSRT_HOST_MJS: &str = include_str!("../../../../js/bun/jsrt-host.mjs");

/// Locate the `bun` binary: `PI_RS_BUN` first, then `PATH`.
pub(crate) fn find_bun() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("PI_RS_BUN")
        && !p.is_empty()
    {
        return Some(p.into());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join("bun");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// The shared directory holding the materialized virtual modules (`node_modules`) and the
/// entry script. All hosts in the process share it; the contents are compile-time constants.
fn virtual_dir() -> anyhow::Result<std::path::PathBuf> {
    static DIR: std::sync::LazyLock<std::sync::Mutex<Option<std::path::PathBuf>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
    let mut slot = DIR.lock().expect("virtual dir lock");
    // Serialized: the loser reuses the winner's finished directory instead of removing it.
    if slot.is_none() {
        *slot = Some(materialize()?);
    }
    Ok(slot.as_ref().expect("materialized").clone())
}

/// A re-export shim so an alias specifier resolves to the aliased virtual module.
fn re_export(target: &str) -> String {
    format!("export * from \"{}\";\n", target)
}

fn materialize() -> anyhow::Result<std::path::PathBuf> {
    use super::*;
    let stamp = format!(
        "{}-{}",
        std::process::id(),
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("system clock").as_nanos()
    );
    let dir = std::env::temp_dir().join(format!("pi-rs-bun-{stamp}"));
    let root = dir.join("node_modules");
    let mut files: Vec<(String, String)> = vec![
        ("pi-rs:bridge/index.js".into(), BRIDGE_JS.to_string()),
        ("pi-rs:marked/index.js".into(), MARKED_JS.to_string()),
        ("typebox/index.js".into(), TYPEBOX_MODULES[0].1.to_string()),
        ("typebox/value.js".into(), TYPEBOX_MODULES[1].1.to_string()),
        ("typebox/compile.js".into(), TYPEBOX_MODULES[2].1.to_string()),
        ("pi-rs:typebox/chunk-1.js".into(), TYPEBOX_MODULES[3].1.to_string()),
        ("pi-rs:typebox/chunk-2.js".into(), TYPEBOX_MODULES[4].1.to_string()),
        ("@earendil-works/pi-tui/index.js".into(), PI_TUI_JS.to_string()),
        ("@earendil-works/pi-ai/index.js".into(), PI_AI_JS.to_string()),
        ("@earendil-works/pi-ai/models.js".into(), PI_AI_MODELS_JS.to_string()),
        (
            "@earendil-works/pi-ai/providers/all.js".into(),
            PI_AI_PROVIDERS_ALL_JS.to_string(),
        ),
        ("@earendil-works/pi-ai/oauth.js".into(), "export {};\n".into()),
        ("@earendil-works/pi-ai/compat.js".into(), "export * from \"./index.js\";\n".into()),
        ("pi-rs:ai-core/index.js".into(), PI_AI_CORE_JS.to_string()),
        ("pi-rs:ai-faux/index.js".into(), PI_AI_FAUX_JS.to_string()),
        ("pi-rs:ai-rust/index.js".into(), PI_AI_RUST_JS.to_string()),
        ("pi-rs:sdk/index.js".into(), PI_SDK_JS.to_string()),
        ("@earendil-works/pi-agent-core/index.js".into(), PI_AGENT_CORE_JS.to_string()),
        (
            "@earendil-works/pi-coding-agent/index.js".into(),
            PI_CODING_AGENT_JS.to_string(),
        ),
        ("@ff-labs/fff-node/index.js".into(), FFF_NODE_JS.to_string()),
        ("jsrt:host/index.js".into(), JSRT_HOST_MJS.to_string()),
        // Aliases (same table as the Boa backend in `mod.rs`).
        ("@sinclair/typebox/index.js".into(), re_export("typebox")),
        ("@sinclair/typebox/value.js".into(), re_export("typebox/value")),
        ("@sinclair/typebox/compile.js".into(), re_export("typebox/compile")),
        ("@sinclair/typebox/compiler.js".into(), re_export("typebox/compile")),
        (
            "@mariozechner/pi-tui/index.js".into(),
            re_export("@earendil-works/pi-tui"),
        ),
        (
            "@mariozechner/pi-agent-core/index.js".into(),
            re_export("@earendil-works/pi-agent-core"),
        ),
        (
            "@mariozechner/pi-coding-agent/index.js".into(),
            re_export("@earendil-works/pi-coding-agent"),
        ),
        ("@mariozechner/pi-ai/index.js".into(), re_export("@earendil-works/pi-ai")),
        (
            "@mariozechner/pi-ai/compat.js".into(),
            re_export("@earendil-works/pi-ai/compat"),
        ),
        (
            "@mariozechner/pi-ai/models.js".into(),
            re_export("@earendil-works/pi-ai/models"),
        ),
        ("@mariozechner/pi-ai/oauth.js".into(), re_export("@earendil-works/pi-ai/oauth")),
        (
            "@mariozechner/pi-ai/providers/all.js".into(),
            re_export("@earendil-works/pi-ai/providers/all"),
        ),
    ];
    // The re-export shims are short; write them directly (the large sources are borrowed).
    for (name, source) in files.drain(..) {
        let path = root.join(&name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, source)?;
    }
    fs::write(dir.join("entry.mjs"), BUN_ENTRY)?;
    Ok(dir)
}

/// Env-gated JSONL frame log: set `PI_RS_JSONL_LOG=/path` to record every protocol frame
/// between Rust and the bun child (`R->C` stdin/FIFO writes, `C->R` stdout lines).
/// Zero cost when unset. Lines are truncated to 4000 bytes at a char boundary;
/// set `PI_RS_JSONL_FULL=1` to keep them whole.
fn jsonl_log(dir: &str, line: &str) {
    use std::io::Write;
    static F: std::sync::OnceLock<parking_lot::Mutex<std::fs::File>> = std::sync::OnceLock::new();
    if std::env::var_os("PI_RS_JSONL_LOG").is_none() {
        return;
    }
    let f = F.get_or_init(|| {
        let path = std::env::var("PI_RS_JSONL_LOG").unwrap();
        parking_lot::Mutex::new(
            std::fs::OpenOptions::new().create(true).append(true).open(path).expect("PI_RS_JSONL_LOG file"),
        )
    });
    let mut shown = line;
    if std::env::var_os("PI_RS_JSONL_FULL").is_none() && shown.len() > 4000 {
        let mut end = 4000;
        while end > 0 && !line.is_char_boundary(end) {
            end -= 1;
        }
        shown = &line[..end];
    }
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let _ = writeln!(f.lock(), "{ts} {dir} {shown}");
}

/// One Rust -> child request awaiting its response. Kept (with age and label) so the
/// reaper can report a call that never answers: a lost response hangs the JS side.
struct PendingCall {
    tx: oneshot::Sender<anyhow::Result<Value>>,
    label: String,
    started: Instant,
}

/// Max wait for a synchronous Rust -> child call. A legitimate sync call (a getter, a
/// component render) finishes in milliseconds, so a longer wait means the child event loop
/// is wedged (a lost response frame). The bounded wait turns that into a visible error
/// instead of parking the caller's thread forever. The most important caller is the TUI
/// frame thread (JS component render); if it parks, the whole UI freezes.
const SYNC_CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Consecutive sync-call failures before the bridge is declared degraded.
const SYNC_FAILURE_LIMIT: u32 = 3;
/// How long a degraded bridge fails fast before one retry.
const SYNC_DEGRADED_RETRY_MS: u64 = 15_000;

/// Wall clock in milliseconds (0 if the clock is before the epoch).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Turn a panic payload from a host op into an error response. A dropped response would
/// leave the child waiting forever (sync op: frozen event loop; async op: dead promise).
fn panic_to_host_error(payload: Box<dyn std::any::Any + Send>) -> anyhow::Error {
    let msg = crate::utils::uncaught::panic_message(&*payload);
    tracing::error!(target: "pi_js", "extension host op panicked; responding with error: {msg}");
    anyhow::anyhow!("extension host op panicked: {msg}")
}

pub struct BunInner {
    /// Protocol lines to the child's stdin (drained by the writer thread).
    write_tx: SyncSender<String>,
    /// Write end of the fd-3 pipe: synchronous host responses.
    hostresp: PlMutex<fs::File>,
    /// Outstanding Rust -> child requests, keyed by frame id. The reaper thread reports
    /// entries that stay pending too long: a lost response means the JS side waits forever.
    pending: PlMutex<HashMap<u64, PendingCall>>,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
    child: PlMutex<Child>,
    /// Tokio runtime for async host-call handlers.
    tokio: tokio::runtime::Handle,
    /// The bridge, registered by the embedder after start (weak: no cycle).
    bridge_slot: Arc<PlMutex<Weak<super::Bridge>>>,
    /// Cache-bust counter for extension re-imports (`clear_module_cache` + next import).
    bust: AtomicU64,
    /// Set by `clear_module_cache`; consumed by the next import.
    bust_pending: AtomicBool,
    /// Consecutive synchronous call failures (timeout, process exit); cleared on success.
    sync_failures: AtomicU32,
    /// Sync calls fail fast until this wall-clock time (ms): a wedged bridge must not park
    /// one new caller thread per failed call.
    degraded_until_ms: AtomicU64,
    /// FIFO file backing the sync host channel (unlinked on drop).
    fifo_path: Option<std::path::PathBuf>,
}

/// A [`JsHost`] backed by a `bun` subprocess.
pub struct BunHost {
    inner: Arc<BunInner>,
}

impl BunHost {
    /// Start the bun process for `cwd`. Returns the host and the bridge slot the embedder
    /// fills with the `Bridge` it builds on top of the host.
    pub fn start(
        cwd: &str,
        tokio: tokio::runtime::Handle,
    ) -> anyhow::Result<(Arc<BunHost>, Arc<PlMutex<Weak<super::Bridge>>>)> {
        let bun =
            find_bun().ok_or_else(|| anyhow::anyhow!("bun not found in PATH (set PI_RS_BUN to the binary)"))?;
        let dir = virtual_dir()?;
        let entry = dir.join("entry.mjs");

        // fd-3 channel: Rust writes synchronous host responses, the child blocks on it in
        // `__piHost.call` (the child's event loop is frozen for the duration of a sync call,
        // so it must be a dedicated channel, separate from the event-driven stdin).
        // The synchronous host channel is a FIFO the child opens itself (bun renumbers
        // inherited fds at startup, so a passed fd would not arrive as fd 3). Opened O_RDWR
        // here: a self-pipe, so neither side ever blocks on open or gets EOF.
        static FIFO_SEQ: AtomicU64 = AtomicU64::new(0);
        let fifo_seq = FIFO_SEQ.fetch_add(1, Ordering::SeqCst);
        let fifo_path = dir.join(format!("hostfifo-{fifo_seq}-{}", std::process::id()));
        let _ = fs::remove_file(&fifo_path);
        {
            use std::os::unix::ffi::OsStrExt;
            let c_path = std::ffi::CString::new(fifo_path.as_os_str().as_bytes().to_vec())
                .expect("fifo path has no NUL");
            // SAFETY: c_path is a valid NUL-terminated C string for the duration of the call.
            if unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) } != 0 {
                anyhow::bail!("mkfifo({}) failed: {}", fifo_path.display(), std::io::Error::last_os_error());
            }
        }
        // O_NONBLOCK: a reply write must never block on the FIFO when the child is no
        // longer draining it (it timed out its sync call and moved on). Bounded writes
        // in `respond_host_fifo` handle EAGAIN; a blocking write here could park the
        // writer (and the `hostresp` lock) forever and wedge every later sync reply.
        use std::os::unix::fs::OpenOptionsExt;
        let write_end = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo_path)?;

        let mut cmd = Command::new(&bun);
        cmd.arg(&entry);
        cmd.current_dir(cwd);
        cmd.env("NODE_PATH", dir.join("node_modules"));
        cmd.env("PI_RS_HOST_FIFO", &fifo_path);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let child = cmd.spawn()?;

        let bridge_slot: Arc<PlMutex<Weak<super::Bridge>>> = Arc::new(PlMutex::new(Weak::new()));
        let (write_tx, write_rx) = sync_channel::<String>(4096);
        let alive = Arc::new(AtomicBool::new(true));
        let inner = Arc::new(BunInner {
            write_tx,
            hostresp: PlMutex::new(write_end),
            fifo_path: Some(fifo_path),
            pending: PlMutex::new(HashMap::new()),
            // Frame ids live in a separate numeric space from the child's host-call ids (both
            // answer by id). Start high so the two counters can never collide.
            next_id: AtomicU64::new(1 << 40),
            alive: alive.clone(),
            child: PlMutex::new(child),
            tokio,
            bridge_slot: bridge_slot.clone(),
            bust: AtomicU64::new(0),
            bust_pending: AtomicBool::new(false),
            sync_failures: AtomicU32::new(0),
            degraded_until_ms: AtomicU64::new(0),
        });

        // Stdin writer thread: serialize protocol lines onto the child's stdin.
        //
        // Reaper thread: report and resolve Rust -> child calls pending for more than 60 s.
        // Resolving with an error (not just logging) is what bounds every waiter: a lost
        // response then costs one failed call instead of a parked thread or a dead await.
        {
            let inner_reaper = inner.clone();
            let alive_reaper = alive.clone();
            thread::Builder::new()
                .name("pi-bun-pending".into())
                .spawn(move || loop {
                    std::thread::sleep(Duration::from_secs(30));
                    if !alive_reaper.load(Ordering::SeqCst) {
                        break;
                    }
                    let now = Instant::now();
                    let stale = inner_reaper
                        .pending
                        .lock()
                        .iter()
                        .filter(|(_, p)| now.duration_since(p.started) > Duration::from_secs(60))
                        .map(|(id, p)| (*id, p.label.clone(), p.started.elapsed().as_secs()))
                        .collect::<Vec<_>>();
                    for (id, label, secs) in stale {
                        // Resolve the waiter with an error so it never hangs on the lost
                        // response; the one hung handler loses its call.
                        if let Some(p) = inner_reaper.pending.lock().remove(&id) {
                            let _ = p.tx.send(Err(anyhow::anyhow!(
                                "timed out after {secs}s waiting for the JS extension process to respond (the handler may be hung)"
                            )));
                        }
                        tracing::error!(
                            target: "pi_js",
                            id,
                            %label,
                            secs,
                            "JS call pending without response; the handler may be hung"
                        );
                        jsonl_log(
                            "R->C stale",
                            &format!("{{\"id\":{id},\"label\":\"{label}\",\"secs\":{secs}}}"),
                        );
                    }
                })
                .ok();
        }
        {
            let mut stdin = inner.child.lock().stdin.take().expect("stdin piped");
            let alive = alive.clone();
            thread::Builder::new()
                .name("pi-bun-stdin".into())
                .spawn(move || {
                    while let Ok(line) = write_rx.recv() {
                        if stdin.write_all(line.as_bytes()).is_err() || stdin.flush().is_err() {
                            break;
                        }
                    }
                    alive.store(false, Ordering::SeqCst);
                })
                .ok();
        }
        // Stderr thread: JS stderr (console errors, uncaught stacks) goes to the stderr sink.
        {
            let mut stderr = inner.child.lock().stderr.take().expect("stderr piped");
            thread::Builder::new()
                .name("pi-bun-stderr".into())
                .spawn(move || {
                    let mut out = std::io::stderr();
                    let mut buf = [0u8; 8192];
                    loop {
                        match stderr.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                let _ = out.write_all(&buf[..n]);
                            }
                            Err(_) => break,
                        }
                    }
                })
                .ok();
        }
        // Reader thread: the child's stdout. Supervised: a panic here would kill the only
        // reader of the child and orphan every pending Rust -> child call (a frozen
        // session). Reporting it crashes loudly instead.
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let inner_reader = inner.clone();
        thread::Builder::new()
            .name("pi-bun-stdout".into())
            .spawn(crate::utils::uncaught::supervise_fn(move || {
                let stdout = inner_reader.child.lock().stdout.take().expect("stdout piped");
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next() {
                    let Ok(line) = line else { break };
                    if line.is_empty() {
                        continue;
                    }
                    jsonl_log("C->R", &line);
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if v.get("type").and_then(Value::as_str) == Some("ready") {
                        let _ = ready_tx.send(());
                        continue;
                    }
                    BunInner::dispatch_line(&inner_reader, v);
                }
                inner_reader.mark_dead();
            }))
            .ok();
        // Waiter thread: notice child exit. `try_wait` in a loop: a blocking `wait()` would
        // hold the child lock and deadlock the reader thread (which needs the lock to
        // service the child's stdout) and the shutdown path (which needs it to kill).
        {
            let inner2 = inner.clone();
            thread::spawn(move || {
                loop {
                    match inner2.child.lock().try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                        Err(_) => break,
                    }
                }
                inner2.mark_dead();
            });
        }
        // Wait for the handshake (bounded: a failed startup should not hang the agent).
        if ready_rx.recv_timeout(Duration::from_secs(15)).is_err() {
            let _ = inner.child.lock().kill();
            anyhow::bail!("bun extension process did not start (check the stderr output above)");
        }
        tracing::info!(target: "pi_js", "bun extension backend started ({})", bun.display());
        Ok((Arc::new(BunHost { inner }), bridge_slot))
    }
}

impl Drop for BunInner {
    fn drop(&mut self) {
        self.pending.lock().drain();
        let _ = self.child.lock().kill();
        // Brief reap attempt; never block on `wait` (the lock must not be held while the
        // child is still running: other threads may need it).
        for _ in 0..40 {
            match self.child.lock().try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        if let Some(p) = self.fifo_path.take() {
            let _ = fs::remove_file(p);
        }
    }
}

impl BunInner {
    /// Send a message to the child; the response (matched by id) resolves the returned future.
    pub(crate) fn send(&self, mut msg: Value) -> oneshot::Receiver<anyhow::Result<Value>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        // A degraded bridge rejects up front: every call to a wedged child would fail
        // anyway, and rejecting here keeps waiters from piling up on it.
        if now_ms() < self.degraded_until_ms.load(Ordering::SeqCst) {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Err(anyhow::anyhow!(
                "the JS extension bridge is degraded (repeated failed calls); retrying in a few seconds"
            )));
            return rx;
        }
        msg["id"] = json!(id);
        let (tx, rx) = oneshot::channel();
        let label = msg
            .get("method")
            .and_then(Value::as_str)
            .or_else(|| msg.get("type").and_then(Value::as_str))
            .unwrap_or("call")
            .to_string();
        self.pending
            .lock()
            .insert(id, PendingCall { tx, label, started: Instant::now() });
        let wire = msg.to_string() + "\n";
        jsonl_log("R->C", &wire);
        // try_send, not a blocking send: this can run on the TUI thread (a sync call) or a
        // tokio worker, and neither may park on a wedged child. A full queue (4096 undelivered
        // frames) means the child is deeply wedged; the new call fails fast instead of piling
        // onto the reaper's 60 s wait, and healthy calls are unaffected.
        if !self.alive.load(Ordering::SeqCst)
            || self.write_tx.try_send(wire).is_err()
        {
            if let Some(p) = self.pending.lock().remove(&id) {
                let _ = p.tx.send(Err(anyhow::anyhow!("the JS extension bridge did not accept the call (it exited or is wedged)")));
            }
        }
        rx
    }

    /// A call into a JS value (`method` present: `this[method](...args)`, absent: `fn(...args)`).
    pub(crate) fn request_call(
        &self,
        handle: u64,
        method: Option<String>,
        args: Vec<Value>,
    ) -> oneshot::Receiver<anyhow::Result<Value>> {
        let mut msg = json!({ "type": "call", "handle": handle, "args": args });
        if let Some(method) = method {
            msg["method"] = json!(method);
        }
        self.send(msg)
    }

    pub(crate) fn request_get(&self, handle: u64, name: String) -> oneshot::Receiver<anyhow::Result<Value>> {
        self.send(json!({ "type": "get", "handle": handle, "name": name }))
    }

    /// Bounded wait plus failure bookkeeping for a synchronous Rust -> child call.
    /// The bounded wait is the hard guarantee: a lost response can never park the caller
    /// thread for more than [`SYNC_CALL_TIMEOUT`].
    fn sync_wait(
        &self,
        rx: oneshot::Receiver<anyhow::Result<Value>>,
        label: &str,
    ) -> anyhow::Result<Value> {
        let now = now_ms();
        if now < self.degraded_until_ms.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!(
                "the JS extension bridge is degraded (repeated failed calls); retrying in a few seconds"
            ));
        }
        match host_bun_wait(rx, SYNC_CALL_TIMEOUT) {
            Ok(value) => {
                self.sync_failures.store(0, Ordering::SeqCst);
                Ok(value)
            }
            Err(e) => {
                let n = self.sync_failures.fetch_add(1, Ordering::SeqCst) + 1;
                // A timed-out sync call is a strong wedge signal (legit sync calls finish
                // in milliseconds): degrade immediately instead of waiting for the
                // failure counter.
                let timed_out = format!("{e:#}").contains("timed out");
                if timed_out || n >= SYNC_FAILURE_LIMIT {
                    self.degraded_until_ms
                        .store(now + SYNC_DEGRADED_RETRY_MS, Ordering::SeqCst);
                    let note = if timed_out {
                        format!("JS extension call timed out; bridge degraded for {SYNC_DEGRADED_RETRY_MS}ms (the extension event loop may be wedged)")
                    } else {
                        format!("JS extension bridge degraded after {n} consecutive failed calls; calls fail fast for {SYNC_DEGRADED_RETRY_MS}ms")
                    };
                    tracing::error!(
                        target: "pi_js",
                        call = %label,
                        error = %format!("{e:#}"),
                        "{note}"
                    );
                }
                Err(e)
            }
        }
    }

    /// Synchronous call into a JS value; fails fast while the bridge is degraded.
    pub(crate) fn sync_call(
        &self,
        handle: u64,
        method: Option<String>,
        args: Vec<Value>,
    ) -> anyhow::Result<Value> {
        let now = now_ms();
        if now < self.degraded_until_ms.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!(
                "the JS extension bridge is degraded (repeated failed calls); retrying in a few seconds"
            ));
        }
        let label = method.clone().unwrap_or_else(|| "invoke".to_string());
        let rx = self.request_call(handle, method, args);
        self.sync_wait(rx, &label)
    }

    /// Synchronous property read; fails fast while the bridge is degraded.
    pub(crate) fn sync_get(&self, handle: u64, name: &str) -> anyhow::Result<Value> {
        let now = now_ms();
        if now < self.degraded_until_ms.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!(
                "the JS extension bridge is degraded (repeated failed calls); retrying in a few seconds"
            ));
        }
        let label = format!("get {name}");
        let rx = self.request_get(handle, name.to_string());
        self.sync_wait(rx, &label)
    }

    pub(crate) fn mark_dead(&self) {
        if !self.alive.swap(false, Ordering::SeqCst) {
            return;
        }
        for (_, p) in self.pending.lock().drain() {
            let _ = p.tx.send(Err(anyhow::anyhow!("the JS extension process exited")));
        }
    }

    /// Handle one protocol line from the child.
    fn dispatch_line(this: &Arc<BunInner>, v: Value) {
        match v.get("type").and_then(Value::as_str) {
            Some("host" | "hostAsync") => {
                let id = v.get("id").and_then(Value::as_u64).unwrap_or(0);
                let op = v.get("op").and_then(Value::as_str).unwrap_or_default().to_string();
                let payload = v.get("payload").cloned().unwrap_or_else(|| json!({}));
                let async_op = v["type"] == "hostAsync";
                let bridge = this.bridge_slot.lock().upgrade();
                let inner: Arc<BunInner> = this.clone();
                if async_op {
                    let handle = this.tokio.clone();
                    // If the spawn itself panics (runtime shutting down), the child would wait
                    // for a response that never comes; answer with an error instead.
                    let inner_async = inner.clone();
                    let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        handle.spawn(async move {
                            let res = match bridge {
                                Some(b) => futures::FutureExt::catch_unwind(
                                    std::panic::AssertUnwindSafe(async move {
                                        super::api::dispatch_async(b, op.clone(), payload).await
                                    }),
                                )
                                .await
                                .map_err(panic_to_host_error)
                                .and_then(|r| r),
                                None => Err(anyhow::anyhow!("pi extension host is shut down")),
                            };
                            inner_async.respond_host_stdin(id, res);
                        })
                    }));
                    if let Err(payload) = spawned {
                        inner.respond_host_stdin(id, Err(panic_to_host_error(payload)));
                    }
                } else {
                    // Separate thread: a handler may block (UI getters) while the reader thread
                    // keeps resolving Rust -> child responses. Entering the runtime context
                    // lets spawn_detached callers (pi.sendMessage, pi.sendUserMessage) schedule
                    // their work on the runtime; without it the future is dropped. The panic
                    // guard guarantees the FIFO response is written: a missing response freezes
                    // the child's event loop forever.
                    let handle = this.tokio.clone();
                    // If the thread spawn itself fails, the child would block on the FIFO for a
                    // response that never comes; answer with an error instead.
                    let inner_thread = inner.clone();
                    if let Err(e) = thread::Builder::new()
                        .name("pi-bun-hostcall".into())
                        .spawn(move || {
                            let _guard = handle.enter();
                            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                                match bridge {
                                    Some(b) => super::api::dispatch(&b, &op, payload),
                                    None => Err(anyhow::anyhow!("pi extension host is shut down")),
                                }
                            }))
                            .map_err(panic_to_host_error)
                            .and_then(|r| r);
                            inner_thread.respond_host_fifo(id, res);
                        })
                    {
                        inner.respond_host_fifo(id, Err(anyhow::anyhow!("host op thread spawn failed: {e}")));
                    }
                }
            }
            Some("jsout") => {
                if let Some(data) = v.get("data").and_then(Value::as_str) {
                    let _ = std::io::stdout().write_all(data.as_bytes());
                    let _ = std::io::stdout().flush();
                }
            }
            Some("exit") => {
                tracing::warn!(
                    target: "pi_js",
                    "JS extension called process.exit({})",
                    v.get("code").and_then(|c| c.as_u64()).unwrap_or(0)
                );
            }
            _ => {
                // A response to a Rust-originated request.
                if let Some(id) = v.get("id").and_then(Value::as_u64)
                    && let Some(p) = this.pending.lock().remove(&id)
                {
                    let res = match v.get("error") {
                        Some(e) => {
                            let msg = e
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("JS error")
                                .to_string();
                            Err(anyhow::anyhow!("{msg}"))
                        }
                        None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = p.tx.send(res);
                }
            }
        }
    }

    /// Write one synchronous host response to the FIFO. The child is blocked reading the FIFO
    /// (its event loop is frozen for the duration of the sync call), so this is the only channel
    /// it can see right now. The write is bounded: the child stops reading the FIFO when its
    /// own sync-call cap fires, and a blocking write of a full pipe buffer would park this
    /// thread and the `hostresp` lock forever. A reply that is still waiting for the child is
    /// delivered immediately (the child drains the FIFO while it spins); a reply the child no
    /// longer waits for is dropped with a loud log instead of wedging the channel.
    fn respond_host_fifo(&self, id: u64, res: anyhow::Result<Value>) {
        let msg = host_resp_msg(id, res);
        jsonl_log("R->C fifo", &msg);
        let mut wire = (msg + "\n").into_bytes();
        let deadline = Instant::now() + Duration::from_secs(5);
        match write_fifo_bounded(&self.hostresp, &mut wire, deadline) {
            FifoWrite::Complete => {}
            FifoWrite::Terminated => tracing::error!(
                target: "pi_js",
                id,
                "dropping sync host response after 5s: the child stopped reading the sync channel (its sync call likely timed out); the torn line is terminated so the child resynchronizes"
            ),
            FifoWrite::Failed(e) => tracing::error!(target: "pi_js", id, "dropping sync host response: {e:#}"),
        }
    }

    /// Resolve an asynchronous host call. Its promise resolves from a stdin frame (the child's
    /// event loop is live), so the response goes down the stdin pipe, not the FIFO. The send is
    /// bounded: a silently dropped response would leave the child's promise pending forever
    /// (a hung handler whose Rust-side call is then reaped at 60 s).
    fn respond_host_stdin(&self, id: u64, res: anyhow::Result<Value>) {
        let wire = host_resp_msg(id, res).to_string() + "\n";
        jsonl_log("R->C async", &wire);
        match self.write_tx.try_send(wire.clone()) {
            Ok(()) => {}
            Err(e @ std::sync::mpsc::TrySendError::Disconnected(_)) => {
                // The writer thread is gone: no further frame can get through.
                let _ = e;
                tracing::error!(target: "pi_js", id, "async host response not sent (stdin writer gone); the bridge is dead");
                self.mark_dead();
            }
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                // Queue full: 4096 undelivered frames. The child has not drained stdin for
                // longer than it takes to produce that backlog; it is wedged. Declare the
                // bridge dead so every waiter resolves instead of hanging to the reaper.
                tracing::error!(target: "pi_js", id, "async host response not sent (stdin queue full); the bridge is dead");
                self.mark_dead();
            }
        }
    }
}

/// Outcome of a bounded FIFO write.
#[derive(Debug)]
enum FifoWrite {
    /// Every byte of the reply is in the pipe.
    Complete,
    /// The deadline passed mid-line. The torn line was terminated with `\n` (when the buffer
    /// accepted it), so the child's line parser drops the torn line and the stream
    /// resynchronizes; at worst one later reply is lost to the torn line.
    Terminated,
    /// A real I/O error (the FIFO is gone).
    Failed(std::io::Error),
}

/// Write `wire` to a non-blocking FIFO within `deadline`. The lock is held only for one
/// non-blocking write per attempt, so a later reply may interleave while this one paces.
fn write_fifo_bounded(f: &PlMutex<fs::File>, wire: &mut Vec<u8>, deadline: Instant) -> FifoWrite {
    loop {
        match f.lock().write(wire) {
            Ok(0) if !wire.is_empty() => {
                return FifoWrite::Failed(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "the FIFO accepted no data",
                ))
            }
            Ok(n) => {
                *wire = wire.split_off(n);
                if wire.is_empty() {
                    return FifoWrite::Complete;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return FifoWrite::Failed(e),
        }
        if Instant::now() > deadline {
            // A partial line is now in the pipe buffer. Terminate it so the child's line
            // parser can drop the torn line and resynchronize. If even the byte does not
            // fit (the buffer is exactly full of torn bytes), the child's next line swallows
            // the rest and fails to parse the same way: one lost reply, then clean.
            let mut term = [b'\n'];
            let term_deadline = Instant::now() + Duration::from_millis(500);
            loop {
                match f.lock().write(&mut term) {
                    Ok(_) => return FifoWrite::Terminated,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => return FifoWrite::Terminated,
                }
                if Instant::now() > term_deadline {
                    return FifoWrite::Terminated;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// One JS value in the bun process.
struct BunHandle {
    host: Arc<BunInner>,
    id: u64,
}

impl HandleOps for BunHandle {
    fn id(&self) -> u64 {
        self.id
    }

    fn is_js_thread(&self) -> bool {
        false
    }

    fn to_value(&self) -> Value {
        json!({ "$jsHandle": self.id })
    }

    fn call(&self, args: Vec<Value>) -> BoxFuture<'static, anyhow::Result<Value>> {
        let rx = self.host.request_call(self.id, None, args);
        Box::pin(async move {
            rx.await
                .map_err(|_| anyhow::anyhow!("the JS extension process exited"))
                .and_then(|r| r)
        })
    }

    fn call_method(&self, method: String, args: Vec<Value>) -> BoxFuture<'static, anyhow::Result<Value>> {
        let rx = self.host.request_call(self.id, Some(method), args);
        Box::pin(async move {
            rx.await
                .map_err(|_| anyhow::anyhow!("the JS extension process exited"))
                .and_then(|r| r)
        })
    }

    fn call_blocking(&self, args: Vec<Value>) -> anyhow::Result<Value> {
        self.host.sync_call(self.id, None, args)
    }

    fn call_method_blocking(&self, method: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        self.host.sync_call(self.id, Some(method.to_string()), args)
    }

    fn get_blocking(&self, name: &str) -> anyhow::Result<Value> {
        self.host.sync_get(self.id, name)
    }
}

impl JsHost for BunHost {
    fn import(&self, specifier: &str) -> BoxFuture<'static, anyhow::Result<JsHandle>> {
        let inner = self.inner.clone();
        let mut spec = specifier.to_string();
        if inner.bust_pending.swap(false, Ordering::SeqCst) {
            let n = inner.bust.fetch_add(1, Ordering::SeqCst) + 1;
            spec.push_str(&format!("?pi-bust={n}"));
        }
        Box::pin(async move {
            let rx = inner.send(json!({ "type": "import", "specifier": spec }));
            let marker = rx
                .await
                .map_err(|_| anyhow::anyhow!("the JS extension process exited"))??;
            let id = marker.get("$jsHandle").and_then(Value::as_u64).ok_or_else(|| {
                anyhow::anyhow!("import({spec}) did not return a module namespace (got {marker})")
            })?;
            Ok(JsHandle::new(Arc::new(BunHandle { host: inner, id })))
        })
    }

    fn clear_module_cache(&self) -> BoxFuture<'static, anyhow::Result<()>> {
        // The next import gets a cache-bust query (bun keeps a separate cache entry per query).
        self.inner.bust_pending.store(true, Ordering::SeqCst);
        Box::pin(async move { Ok(()) })
    }

    fn adopt(&self, marker: &Value) -> Option<JsHandle> {
        let id = marker.get("$jsHandle")?.as_u64()?;
        Some(JsHandle::new(Arc::new(BunHandle { host: self.inner.clone(), id })))
    }

    // The default implementation blocks the caller; no JS-thread servicing is needed because
    // the child's event loop services calls itself. (The one UI job that calls JS while a
    // synchronous host call is in flight, `JsComponent::set_focused`, posts the call instead of
    // blocking when the backend is not re-entrant.)

    fn reentrant_js(&self) -> bool {
        false
    }

}



/// Build a host-response frame for `id`.
fn host_resp_msg(id: u64, res: anyhow::Result<Value>) -> String {
    let msg = match res {
        Ok(value) => json!({ "id": id, "type": "hostResp", "result": value }),
        Err(e) => json!({ "id": id, "type": "hostResp", "error": { "message": format!("{e:#}") } }),
    };
    msg.to_string()
}

/// Wait for a pending oneshot from a non-async context. The wait is bounded: a lost
/// response frame must never park the caller's thread for more than `timeout`.
fn host_bun_wait(
    rx: oneshot::Receiver<anyhow::Result<Value>>,
    timeout: Duration,
) -> anyhow::Result<Value> {
    host_bun_block_on(
        async move {
            rx.await
                .map_err(|_| anyhow::anyhow!("the JS extension process exited"))
                .and_then(|r| r)
        },
        timeout,
    )
}

fn host_bun_block_on<F: Future<Output = anyhow::Result<Value>>>(
    fut: F,
    timeout: Duration,
) -> anyhow::Result<Value> {
    let mut fut = Box::pin(fut);
    struct Unpark(thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let started = Instant::now();
    loop {
        if let std::task::Poll::Ready(r) = fut.as_mut().poll(&mut cx) {
            return r;
        }
        if started.elapsed() >= timeout {
            return Err(anyhow::anyhow!(
                "timed out after {timeout:?} waiting for the JS extension process to respond (the extension event loop may be wedged)"
            ));
        }
        thread::park_timeout(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_host() -> (tokio::runtime::Runtime, Arc<BunHost>) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (host, _slot) = BunHost::start("/tmp", rt.handle().clone()).unwrap();
        (rt, host)
    }

    fn write_module(source: &str) -> String {
        let path = std::env::temp_dir().join(format!(
            "pi-bun-host-test-{}-{}.mjs",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, source).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn send_signal(pid: u32, sig: &str) {
        std::process::Command::new("kill")
            .args([sig, &pid.to_string()])
            .status()
            .expect("kill(1) for the test child");
    }

    /// Regression (frozen TUI): a stopped child used to park the calling thread forever
    /// (the event loop thread rendering a JS component). A bounded wait must turn that
    /// into an error, and the bridge must recover once the child runs again.
    #[test]
    fn sync_call_is_bounded_when_the_child_is_wedged() {
        let (_rt, host) = start_host();
        let inner = host.inner.clone();
        let path = write_module("export default 1;\n");
        std::thread::sleep(Duration::from_millis(300));
        let ns = futures::executor::block_on(host.import(&path)).unwrap();
        let handle = BunHandle { host: inner.clone(), id: ns.id() };

        // Sanity while healthy.
        assert_eq!(handle.get_blocking("default").unwrap(), json!(1));

        // Wedge the child: its event loop no longer answers.
        let pid = inner.child.lock().id();
        send_signal(pid, "-STOP");
        std::thread::sleep(Duration::from_millis(200));

        let t0 = Instant::now();
        let err = handle.get_blocking("default").unwrap_err();
        let elapsed = t0.elapsed();
        assert!(elapsed < Duration::from_secs(15), "the call parked {elapsed:?}: {err:#}");
        assert!(err.to_string().contains("timed out"), "{err:#}");

        // Let the child run again, reset the circuit, and verify recovery.
        send_signal(pid, "-CONT");
        inner.sync_failures.store(0, Ordering::SeqCst);
        inner.degraded_until_ms.store(0, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(handle.get_blocking("default").unwrap(), json!(1));
        std::fs::remove_file(&path).ok();
    }

    /// Regression: when encoding the result fails (a circular value), the bridge used to
    /// send no response at all, so the calling thread parked forever. It must answer
    /// every call, with a result or an error.
    #[test]
    fn response_is_guaranteed_when_encoding_fails() {
        let (_rt, host) = start_host();
        let inner = host.inner.clone();
        let path = write_module("const o = { v: 1 }; o.self = o; export default o;\n");
        std::thread::sleep(Duration::from_millis(300));
        let ns = futures::executor::block_on(host.import(&path)).unwrap();
        let handle = BunHandle { host: inner.clone(), id: ns.id() };

        let t0 = Instant::now();
        let err = handle.get_blocking("default").unwrap_err();
        let elapsed = t0.elapsed();
        assert!(elapsed < Duration::from_secs(10), "the call parked {elapsed:?}: {err:#}");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("circular") || msg.contains("encode"),
            "unexpected error: {err:#}"
        );
        std::fs::remove_file(&path).ok();
    }

    /// A FIFO with no reader (the child timed out its sync call and stopped draining):
    /// a reply that fits the pipe buffer still lands; a reply that fills the buffer must
    /// give up at the deadline instead of parking the writer (and the `hostresp` lock)
    /// forever. Regression for the chronic sync-channel wedge behind the 65 s
    /// "waiting for the JS extension process to respond" errors.
    #[test]
    fn fifo_reply_write_is_bounded_without_a_reader() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::FromRawFd;
        let path = std::env::temp_dir().join(format!(
            "pi-bun-fifo-bounded-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // SAFETY: c_path is a valid NUL-terminated C string for the duration of the call.
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes().to_vec()).unwrap();
        // SAFETY: c_path is a valid NUL-terminated C string for the duration of the call.
        unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        let open = || {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .unwrap()
        };

        // 1. A small reply fits the pipe buffer: it lands even while nobody reads.
        {
            let f = PlMutex::new(open());
            let mut wire = b"{\"id\":1,\"type\":\"hostResp\"}\n".to_vec();
            let out = write_fifo_bounded(&f, &mut wire, Instant::now() + Duration::from_millis(500));
            assert!(matches!(out, FifoWrite::Complete), "small reply: {out:?}");
            assert!(wire.is_empty());
        }

        // 2. A reply larger than the pipe buffer with no reader: the write must stop at the
        //    deadline (Torn or Terminated), never park. 256 KiB >> the 64 KiB pipe buffer.
        {
            let f = PlMutex::new(open());
            let mut wire = vec![b'x'; 256 * 1024];
            let t0 = Instant::now();
            let out = write_fifo_bounded(&f, &mut wire, Instant::now() + Duration::from_millis(200));
            let elapsed = t0.elapsed();
            assert!(
                matches!(out, FifoWrite::Terminated | FifoWrite::Failed(_)),
                "big reply with no reader: {out:?}"
            );
            assert!(elapsed < Duration::from_secs(5), "the write parked {elapsed:?}");
        }

        // 3. With a reader draining the FIFO, a large reply completes in full.
        {
            let f = open();
            let (mut rd, _wr) = unsafe {
                // A second O_RDONLY end so the reader thread has its own description;
                // the shared `f` above stays the writer.
                let fd = libc::open(
                    c_path.as_ptr(),
                    libc::O_RDONLY | libc::O_NONBLOCK,
                );
                assert!(fd >= 0, "second open of the FIFO");
                let file = fs::File::from_raw_fd(fd);
                (file, ())
            };
            let reader = std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = [0u8; 8192];
                loop {
                    match rd.read(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => break,
                    }
                }
            });
            let f = PlMutex::new(f);
            let mut wire = vec![b'y'; 200 * 1024];
            let out = write_fifo_bounded(&f, &mut wire, Instant::now() + Duration::from_secs(5));
            assert!(matches!(out, FifoWrite::Complete), "big reply with a reader: {out:?}");
            assert!(wire.is_empty());
            // Close the writer end so the reader's read returns EOF and it can finish.
            drop(f);
            let _ = reader.join();
        }

        std::fs::remove_file(&path).ok();
    }
}
