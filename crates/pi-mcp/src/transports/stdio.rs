use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::future::BoxFuture;
use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, oneshot, watch};

use super::transport::{
    DEFAULT_MAX_MESSAGE_BYTES, McpTransport, McpTransportSendOptions, TransportEvents,
};
use crate::error::{Error, Result};
use crate::protocol::jsonrpc::{JsonRpcMessage, parse_json_rpc_message};

const DEFAULT_MAX_STDERR_BYTES: usize = 64 * 1024;
const DEFAULT_CLOSE_TIMEOUT_MS: u64 = 2_000;
/// How long a server gets to exit on its own after stdin closes, before it is sent SIGTERM.
const STDIN_CLOSE_GRACE_MS: u64 = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StderrMode {
    /// Capture stderr (see [`StdioTransport::stderr`] and `on_stderr`).
    #[default]
    Pipe,
    /// Let the server write to this process's stderr.
    Inherit,
}

pub type StderrListener = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct StdioTransportOptions {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Added to (or, with `inherit_env: false`, replacing) the environment of this process.
    pub env: HashMap<String, String>,
    /// Default: `true`.
    pub inherit_env: bool,
    pub stderr: StderrMode,
    pub on_stderr: Option<StderrListener>,
    pub max_message_bytes: Option<usize>,
    pub max_stderr_bytes: Option<usize>,
    /// Time to wait for the server to exit after SIGTERM before sending SIGKILL. Default: 2000.
    pub close_timeout_ms: Option<u64>,
}

impl StdioTransportOptions {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            inherit_env: true,
            stderr: StderrMode::Pipe,
            on_stderr: None,
            max_message_bytes: None,
            max_stderr_bytes: None,
            close_timeout_ms: None,
        }
    }

    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, args: I) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn on_stderr(mut self, listener: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_stderr = Some(Arc::new(listener));
        self
    }

    pub fn close_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.close_timeout_ms = Some(timeout_ms);
        self
    }
}

impl std::fmt::Debug for StdioTransportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioTransportOptions")
            .field("command", &self.command)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("inherit_env", &self.inherit_env)
            .field("stderr", &self.stderr)
            .finish_non_exhaustive()
    }
}

type WriteRequest = (Vec<u8>, oneshot::Sender<Result<()>>);

#[derive(Default)]
struct ChildState {
    pid: Option<u32>,
    exited: bool,
}

struct Inner {
    options: StdioTransportOptions,
    events: TransportEvents,
    started: AtomicBool,
    closed: AtomicBool,
    child: Mutex<ChildState>,
    stdin: Mutex<Option<mpsc::UnboundedSender<WriteRequest>>>,
    stdout_buffer: Mutex<Vec<u8>>,
    stderr_buffer: Mutex<Vec<u8>>,
    /// Becomes `true` once the process exited and its output streams closed (Node's `close` event).
    close_event: watch::Sender<bool>,
}

/// Runs an MCP server as a child process and exchanges newline-delimited JSON-RPC over its stdio.
///
/// On Unix the server gets its own process group, so closing the transport also terminates the
/// server's children; groups still alive when this process exits are sent SIGTERM.
#[derive(Clone)]
pub struct StdioTransport {
    inner: Arc<Inner>,
}

impl StdioTransport {
    pub fn new(options: StdioTransportOptions) -> Self {
        let (close_event, _) = watch::channel(false);
        Self {
            inner: Arc::new(Inner {
                options,
                events: TransportEvents::default(),
                started: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                child: Mutex::new(ChildState::default()),
                stdin: Mutex::new(None),
                stdout_buffer: Mutex::new(Vec::new()),
                stderr_buffer: Mutex::new(Vec::new()),
                close_event,
            }),
        }
    }

    pub fn options(&self) -> &StdioTransportOptions {
        &self.inner.options
    }

    /// Process ID of the running server.
    pub fn pid(&self) -> Option<u32> {
        self.inner.child.lock().pid
    }

    /// The captured stderr tail (at most `max_stderr_bytes`).
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.inner.stderr_buffer.lock()).into_owned()
    }
}

fn spawn_error(command: &str, error: &std::io::Error) -> Error {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT".to_string(),
        std::io::ErrorKind::PermissionDenied => "EACCES".to_string(),
        _ => error.to_string(),
    };
    Error::Other(format!("spawn {command} {code}"))
}

impl Inner {
    fn spawn(self: &Arc<Self>) -> Result<()> {
        let options = &self.options;
        let mut command = platform::command(&options.command, &options.args, options);
        if let Some(cwd) = &options.cwd {
            command.current_dir(cwd);
        }
        if !options.inherit_env {
            command.env_clear();
        }
        command.envs(&options.env);
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        command.stderr(match options.stderr {
            StderrMode::Inherit => Stdio::inherit(),
            StderrMode::Pipe => Stdio::piped(),
        });
        platform::configure(&mut command);
        let mut child: Child = command
            .spawn()
            .map_err(|error| spawn_error(&options.command, &error))?;
        let pid = child.id();
        self.child.lock().pid = pid;
        if let Some(pid) = pid {
            platform::register_group(pid);
        }

        let (sender, receiver) = mpsc::unbounded_channel::<WriteRequest>();
        *self.stdin.lock() = Some(sender);
        if let Some(stdin) = child.stdin.take() {
            tokio::spawn(self.clone().write_stdin(stdin, receiver));
        }
        let stdout_task = child.stdout.take().map(|stdout| {
            let inner = self.clone();
            tokio::spawn(async move { inner.read_stream(stdout, true).await })
        });
        let stderr_task = child.stderr.take().map(|stderr| {
            let inner = self.clone();
            tokio::spawn(async move { inner.read_stream(stderr, false).await })
        });
        let inner = self.clone();
        tokio::spawn(async move {
            let _ = child.wait().await;
            inner.child.lock().exited = true;
            if let Some(pid) = pid {
                platform::unregister_group(pid);
            }
            if let Some(task) = stdout_task {
                let _ = task.await;
            }
            if let Some(task) = stderr_task {
                let _ = task.await;
            }
            inner.handle_close();
        });
        Ok(())
    }

    async fn write_stdin(
        self: Arc<Self>,
        mut stdin: ChildStdin,
        mut receiver: mpsc::UnboundedReceiver<WriteRequest>,
    ) {
        while let Some((payload, done)) = receiver.recv().await {
            let result = async {
                stdin.write_all(&payload).await?;
                stdin.flush().await
            }
            .await;
            match result {
                Ok(()) => {
                    let _ = done.send(Ok(()));
                }
                Err(error) => {
                    let error = Error::Other(format!("write {}", error_code(&error)));
                    if !self.closed.load(Ordering::SeqCst) {
                        self.events.emit_error(error.clone());
                    }
                    let _ = done.send(Err(error));
                }
            }
        }
        // Dropping stdin closes it, which asks the server to exit.
    }

    async fn read_stream(self: Arc<Self>, mut stream: impl AsyncRead + Unpin, stdout: bool) {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) => break,
                Ok(read) => {
                    if stdout {
                        self.handle_stdout(&chunk[..read]);
                    } else {
                        self.handle_stderr(&chunk[..read]);
                    }
                }
                Err(error) => {
                    self.events.emit_error(Error::Other(error.to_string()));
                    break;
                }
            }
        }
    }

    fn handle_close(&self) {
        *self.stdin.lock() = None;
        let incomplete = {
            let mut buffer = self.stdout_buffer.lock();
            let incomplete = !String::from_utf8_lossy(&buffer).trim().is_empty();
            buffer.clear();
            incomplete
        };
        {
            let mut child = self.child.lock();
            child.pid = None;
        }
        if incomplete {
            self.events.emit_error(Error::other(
                "MCP stdio server closed with an incomplete JSON-RPC message",
            ));
        }
        let _ = self.close_event.send(true);
        self.events.emit_close();
    }

    fn handle_stdout(&self, chunk: &[u8]) {
        let max_message_bytes = self
            .options
            .max_message_bytes
            .unwrap_or(DEFAULT_MAX_MESSAGE_BYTES);
        let mut lines = Vec::new();
        let mut errors = Vec::new();
        {
            let mut buffer = self.stdout_buffer.lock();
            buffer.extend_from_slice(chunk);
            loop {
                let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') else {
                    if buffer.len() > max_message_bytes {
                        buffer.clear();
                        errors.push(Error::Other(format!(
                            "MCP stdio message exceeds {max_message_bytes} bytes"
                        )));
                    }
                    break;
                };
                let line: Vec<u8> = buffer.drain(..=newline).take(newline).collect();
                if line.len() > max_message_bytes {
                    lines.push(Err(Error::Other(format!(
                        "MCP stdio message exceeds {max_message_bytes} bytes"
                    ))));
                    continue;
                }
                lines.push(Ok(line));
            }
        }
        for line in lines {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    self.events.emit_error(error);
                    continue;
                }
            };
            let text = String::from_utf8_lossy(&line);
            let text = text.strip_suffix('\r').unwrap_or(&text);
            if text.trim().is_empty() {
                continue;
            }
            let parsed = serde_json::from_str(text)
                .map_err(|error| Error::Other(error.to_string()))
                .and_then(parse_json_rpc_message);
            match parsed {
                Ok(message) => self.events.emit_message(message),
                Err(error) => self.events.emit_error(error),
            }
        }
        for error in errors {
            self.events.emit_error(error);
        }
    }

    fn handle_stderr(&self, chunk: &[u8]) {
        let max_stderr_bytes = self
            .options
            .max_stderr_bytes
            .unwrap_or(DEFAULT_MAX_STDERR_BYTES);
        {
            let mut buffer = self.stderr_buffer.lock();
            buffer.extend_from_slice(chunk);
            if buffer.len() > max_stderr_bytes {
                let excess = buffer.len() - max_stderr_bytes;
                buffer.drain(..excess);
            }
        }
        if let Some(listener) = &self.options.on_stderr {
            listener(&String::from_utf8_lossy(chunk));
        }
    }

    async fn close(self: Arc<Self>) -> Result<()> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let (pid, exited) = {
            let child = self.child.lock();
            (child.pid, child.exited)
        };
        let Some(pid) = pid else {
            self.events.emit_close();
            return Ok(());
        };
        // Closing stdin asks the server to exit.
        self.stdin.lock().take();
        if exited {
            return Ok(());
        }
        // Shutdown per the spec: close stdin and let the server exit, then SIGTERM, then SIGKILL.
        let close_timeout_ms = self
            .options
            .close_timeout_ms
            .unwrap_or(DEFAULT_CLOSE_TIMEOUT_MS);
        let grace = STDIN_CLOSE_GRACE_MS.min(close_timeout_ms);
        let mut closed = self.close_event.subscribe();
        let wait_closed = async {
            while !*closed.borrow_and_update() {
                if closed.changed().await.is_err() {
                    break;
                }
            }
        };
        tokio::pin!(wait_closed);
        let term = tokio::time::sleep(Duration::from_millis(grace));
        tokio::pin!(term);
        let kill = tokio::time::sleep(Duration::from_millis(grace + close_timeout_ms));
        tokio::pin!(kill);
        let mut term_sent = false;
        let mut kill_sent = false;
        loop {
            tokio::select! {
                _ = &mut wait_closed => break,
                _ = &mut term, if !term_sent => {
                    term_sent = true;
                    platform::kill_process_tree(pid, platform::Signal::Term);
                }
                _ = &mut kill, if !kill_sent => {
                    kill_sent = true;
                    platform::kill_process_tree(pid, platform::Signal::Kill);
                }
            }
        }
        // Children of the server that ignored stdin closing would otherwise outlive it.
        platform::kill_process_tree(pid, platform::Signal::Term);
        Ok(())
    }
}

fn error_code(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::BrokenPipe => "EPIPE".to_string(),
        _ => error.to_string(),
    }
}

impl McpTransport for StdioTransport {
    fn start(&self) -> BoxFuture<'static, Result<()>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            if inner.started.swap(true, Ordering::SeqCst) {
                return Err(Error::other("MCP stdio transport already started"));
            }
            if inner.closed.load(Ordering::SeqCst) {
                return Err(Error::connection_closed());
            }
            inner.spawn()
        })
    }

    fn send(
        &self,
        message: JsonRpcMessage,
        _options: McpTransportSendOptions,
    ) -> BoxFuture<'static, Result<()>> {
        let queued = (|| {
            let stdin = self.inner.stdin.lock();
            let writer = match stdin.as_ref() {
                Some(writer)
                    if self.inner.started.load(Ordering::SeqCst)
                        && !self.inner.closed.load(Ordering::SeqCst) =>
                {
                    writer
                }
                _ => return Err(Error::connection_closed()),
            };
            let mut payload =
                serde_json::to_vec(&message).map_err(|error| Error::Other(error.to_string()))?;
            payload.push(b'\n');
            let (done, result) = oneshot::channel();
            writer
                .send((payload, done))
                .map_err(|_| Error::connection_closed())?;
            Ok(result)
        })();
        Box::pin(async move {
            queued?
                .await
                .unwrap_or_else(|_| Err(Error::connection_closed()))
        })
    }

    fn close(&self) -> BoxFuture<'static, Result<()>> {
        Box::pin(self.inner.clone().close())
    }

    fn events(&self) -> &TransportEvents {
        &self.inner.events
    }
}

#[cfg(unix)]
mod platform {
    use std::collections::HashSet;
    use std::sync::Once;

    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use tokio::process::Command;

    use super::StdioTransportOptions;

    #[derive(Clone, Copy)]
    pub enum Signal {
        Term,
        Kill,
    }

    /// Process groups of running servers, killed if the host exits without closing them.
    static LIVE_PROCESS_GROUPS: Lazy<Mutex<HashSet<i32>>> =
        Lazy::new(|| Mutex::new(HashSet::new()));
    static EXIT_HOOK: Once = Once::new();

    extern "C" fn kill_live_groups() {
        if let Some(groups) = LIVE_PROCESS_GROUPS.try_lock() {
            for pid in groups.iter() {
                // SAFETY: kill(2) with a negative pid signals a process group; no memory is touched.
                unsafe {
                    libc::kill(-pid, libc::SIGTERM);
                }
            }
        }
    }

    pub fn command(program: &str, args: &[String], _options: &StdioTransportOptions) -> Command {
        let mut command = Command::new(program);
        command.args(args);
        command
    }

    /// Own process group, so closing the transport can terminate the server's children too.
    pub fn configure(command: &mut Command) {
        command.process_group(0);
    }

    pub fn register_group(pid: u32) {
        EXIT_HOOK.call_once(|| {
            // SAFETY: registers a plain extern "C" function to run at process exit.
            unsafe {
                libc::atexit(kill_live_groups);
            }
        });
        LIVE_PROCESS_GROUPS.lock().insert(pid as i32);
    }

    pub fn unregister_group(pid: u32) {
        LIVE_PROCESS_GROUPS.lock().remove(&(pid as i32));
    }

    pub fn kill_process_tree(pid: u32, signal: Signal) {
        let signal = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        let pid = pid as i32;
        // SAFETY: kill(2) only sends a signal.
        unsafe {
            // Negative pid: the whole group, so wrappers like `npx` or `uvx` do not leave the server behind.
            if libc::kill(-pid, signal) == 0 {
                return;
            }
            // The group is gone or was never created; fall back to the direct child.
            libc::kill(pid, signal);
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::path::{Path, PathBuf};

    use tokio::process::Command;

    use super::StdioTransportOptions;

    #[derive(Clone, Copy)]
    pub enum Signal {
        Term,
        Kill,
    }

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn env_value(options: &StdioTransportOptions, key: &str) -> Option<String> {
        let configured = options
            .env
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.clone());
        configured.or_else(|| {
            if options.inherit_env {
                std::env::var(key).ok()
            } else {
                None
            }
        })
    }

    /// Resolve the command like cross-spawn: search `PATH` with `PATHEXT`.
    fn resolve(program: &str, options: &StdioTransportOptions) -> Option<PathBuf> {
        let extensions: Vec<String> = env_value(options, "PATHEXT")
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.to_ascii_lowercase())
            .collect();
        let candidates = |base: &Path| -> Vec<PathBuf> {
            let mut list = vec![base.to_path_buf()];
            for ext in &extensions {
                let mut name = base.as_os_str().to_owned();
                name.push(ext);
                list.push(PathBuf::from(name));
            }
            list
        };
        let path = Path::new(program);
        let cwd = options.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
        if path.components().count() > 1 || path.is_absolute() {
            let base = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            return candidates(&base)
                .into_iter()
                .find(|candidate| candidate.is_file());
        }
        let mut dirs = vec![cwd];
        if let Some(search) = env_value(options, "PATH") {
            dirs.extend(std::env::split_paths(&search));
        }
        dirs.iter()
            .flat_map(|dir| candidates(&dir.join(path)))
            .find(|candidate| candidate.is_file())
    }

    /// Escape an argument for cmd.exe the way cross-spawn does.
    fn escape_argument(argument: &str) -> String {
        let mut escaped = String::from("\"");
        let mut backslashes = 0;
        for ch in argument.chars() {
            match ch {
                '\\' => backslashes += 1,
                '"' => {
                    escaped.push_str(&"\\".repeat(backslashes * 2 + 1));
                    escaped.push('"');
                    backslashes = 0;
                }
                _ => {
                    escaped.push_str(&"\\".repeat(backslashes));
                    escaped.push(ch);
                    backslashes = 0;
                }
            }
        }
        escaped.push_str(&"\\".repeat(backslashes * 2));
        escaped.push('"');
        let mut meta = String::new();
        for ch in escaped.chars() {
            if "()%!^\"<>&|".contains(ch) {
                meta.push('^');
            }
            meta.push(ch);
        }
        meta
    }

    pub fn command(program: &str, args: &[String], options: &StdioTransportOptions) -> Command {
        let resolved = resolve(program, options);
        let is_shim = resolved
            .as_ref()
            .and_then(|path| path.extension())
            .is_some_and(|ext| {
                let ext = ext.to_string_lossy().to_ascii_lowercase();
                ext == "cmd" || ext == "bat"
            });
        if is_shim {
            let shim = resolved.unwrap();
            let line = std::iter::once(escape_argument(&shim.to_string_lossy()))
                .chain(args.iter().map(|arg| escape_argument(arg)))
                .collect::<Vec<_>>()
                .join(" ");
            let mut command =
                Command::new(env_value(options, "COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
            command.raw_arg("/d /s /c");
            command.raw_arg(format!("\"{line}\""));
            return command;
        }
        let mut command = Command::new(
            resolved
                .map(PathBuf::into_os_string)
                .unwrap_or_else(|| program.into()),
        );
        command.args(args);
        command
    }

    pub fn configure(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    pub fn register_group(_pid: u32) {}

    pub fn unregister_group(_pid: u32) {}

    /// Windows has no graceful signals, and `.cmd` shims run through cmd.exe; killing only that
    /// would leave the server running.
    pub fn kill_process_tree(pid: u32, _signal: Signal) {
        let _ = std::process::Command::new("taskkill")
            .args(["/pid", &pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }

    use std::os::windows::process::CommandExt as _;
}
