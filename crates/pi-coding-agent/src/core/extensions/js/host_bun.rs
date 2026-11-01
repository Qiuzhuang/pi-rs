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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
        let write_end = fs::OpenOptions::new().read(true).write(true).open(&fifo_path)?;

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
        });

        // Stdin writer thread: serialize protocol lines onto the child's stdin.
        //
        // Reaper thread: report Rust -> child calls pending for more than 60 s. Without
        // this, a lost response (a dropped task, a hung JS handler) is invisible.
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
        let wire = msg.to_string();
        jsonl_log("R->C", &wire);
        if !self.alive.load(Ordering::SeqCst) || self.write_tx.send(wire + "\n").is_err() {
            if let Some(p) = self.pending.lock().remove(&id) {
                let _ = p.tx.send(Err(anyhow::anyhow!("the JS extension process exited")));
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
    /// it can see right now.
    fn respond_host_fifo(&self, id: u64, res: anyhow::Result<Value>) {
        let msg = host_resp_msg(id, res);
        jsonl_log("R->C fifo", &msg);
        let mut f = self.hostresp.lock();
        let _ = f.write_all((msg + "\n").as_bytes());
    }

    /// Resolve an asynchronous host call. Its promise resolves from a stdin frame (the child's
    /// event loop is live), so the response goes down the stdin pipe, not the FIFO.
    fn respond_host_stdin(&self, id: u64, res: anyhow::Result<Value>) {
        let msg = host_resp_msg(id, res);
        jsonl_log("R->C async", &msg);
        let _ = self.write_tx.send(msg + "\n");
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
        host_bun_wait(self.host.request_call(self.id, None, args))
    }

    fn call_method_blocking(&self, method: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        host_bun_wait(self.host.request_call(self.id, Some(method.to_string()), args))
    }

    fn get_blocking(&self, name: &str) -> anyhow::Result<Value> {
        host_bun_wait(self.host.request_get(self.id, name.to_string()))
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

/// Wait for a pending oneshot from a non-async context.
fn host_bun_wait(rx: oneshot::Receiver<anyhow::Result<Value>>) -> anyhow::Result<Value> {
    host_bun_block_on(async move {
        rx.await
            .map_err(|_| anyhow::anyhow!("the JS extension process exited"))
            .and_then(|r| r)
    })
}

fn host_bun_block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = Box::pin(fut);
    struct Unpark(thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    loop {
        if let std::task::Poll::Ready(r) = fut.as_mut().poll(&mut cx) {
            return r;
        }
        thread::park_timeout(Duration::from_millis(1));
    }
}
