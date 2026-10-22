//! Port of `packages/coding-agent/src/utils/clipboard.ts`.
//!
//! The TS version uses pi-tui's native platform helpers (`getNativeClipboard`, prebuilt Node
//! addons on macOS/Windows/X11). pi-tui's Rust port has no native addon
//! (`pi_tui::native_platform::get_native_clipboard` returns `None`); [`get_native_clipboard`]
//! adds a command-backed reader on macOS (`pbpaste`) and Windows (PowerShell `Get-Clipboard`),
//! so writes always go through the platform commands (`pbcopy`, `clip`, `wl-copy`, `xclip`,
//! `xsel`, `termux-clipboard-set`) and OSC 52.

use std::sync::Arc;

use base64::Engine;
use futures::future::BoxFuture;
pub use pi_tui::native_platform::{ClipboardRead, NativeClipboard};

use super::clipboard_command::{RunClipboardCommandOptions, run_clipboard_command};
use super::wsl::{EnvMap, is_wsl};

const MAX_OSC52_ENCODED_LENGTH: usize = 100_000;

/// Reads the clipboard through a platform command.
struct CommandNativeClipboard {
    command: &'static str,
    args: &'static [&'static str],
}

impl NativeClipboard for CommandNativeClipboard {
    fn get_text(&self) -> pi_tui::event_loop::BoxFuture<Result<ClipboardRead<String>, String>> {
        let command = self.command;
        let args: Vec<String> = self.args.iter().map(|s| s.to_string()).collect();
        Box::pin(async move {
            let options = RunClipboardCommandOptions { timeout_ms: Some(5000), ..Default::default() };
            match run_clipboard_command(command, &args, Some(&options)).await {
                Ok(Some(bytes)) if bytes.is_empty() => Ok(ClipboardRead::Empty),
                Ok(Some(bytes)) => Ok(ClipboardRead::Data(String::from_utf8_lossy(&bytes).into_owned())),
                _ => Ok(ClipboardRead::Unavailable),
            }
        })
    }

    fn get_image(&self) -> pi_tui::event_loop::BoxFuture<Result<ClipboardRead<Vec<u8>>, String>> {
        Box::pin(async { Ok(ClipboardRead::Unavailable) })
    }
}

/// The Node-style platform name (`process.platform`).
pub fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Load a clipboard helper without opening the display until a read is requested.
pub fn get_native_clipboard() -> Option<Arc<dyn NativeClipboard>> {
    if let Some(native) = pi_tui::native_platform::get_native_clipboard() {
        return Some(native);
    }
    match node_platform() {
        "darwin" => Some(Arc::new(CommandNativeClipboard { command: "pbpaste", args: &[] })),
        "win32" => Some(Arc::new(CommandNativeClipboard {
            command: "powershell.exe",
            args: &["-NoProfile", "-Command", "Get-Clipboard -Raw"],
        })),
        _ => None,
    }
}

type CommandRunner =
    Arc<dyn Fn(String, Vec<String>, RunClipboardCommandOptions) -> BoxFuture<'static, Option<Vec<u8>>> + Send + Sync>;

/// Injectable process environment (the TS tests mock `node:os`, pi-tui and the command runner).
#[derive(Clone)]
pub(crate) struct ClipboardHost {
    pub platform: String,
    pub env: EnvMap,
    pub native: Arc<dyn Fn() -> Option<Arc<dyn NativeClipboard>> + Send + Sync>,
    pub run: CommandRunner,
    pub write_stdout: Arc<dyn Fn(&str) + Send + Sync>,
}

impl ClipboardHost {
    fn process() -> Self {
        Self {
            platform: node_platform().to_string(),
            env: std::env::vars().collect(),
            native: Arc::new(get_native_clipboard),
            run: Arc::new(|command, args, options| {
                Box::pin(async move { run_clipboard_command(&command, &args, Some(&options)).await.ok().flatten() })
            }),
            write_stdout: Arc::new(|text| {
                use std::io::Write;
                let mut stdout = std::io::stdout();
                let _ = stdout.write_all(text.as_bytes());
                let _ = stdout.flush();
            }),
        }
    }

    fn env(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }

    async fn run(&self, command: &str, args: &[&str], options: RunClipboardCommandOptions) -> Option<Vec<u8>> {
        (self.run)(command.to_string(), args.iter().map(|s| s.to_string()).collect(), options).await
    }
}

/// Remote sessions also emit OSC 52 so the client terminal receives the copy. PI_CLIPBOARD_OSC52=1
/// covers remote clients that leave no SSH markers, such as multiplexer attaches (#10114, #10835)
/// or IDE daemons (#10826).
fn is_remote_session(host: &ClipboardHost) -> bool {
    host.env("SSH_CONNECTION").is_some()
        || host.env("SSH_CLIENT").is_some()
        || host.env("MOSH_CONNECTION").is_some()
        || host.env("PI_CLIPBOARD_OSC52") == Some("1")
}

fn emit_osc52(host: &ClipboardHost, text: &str) -> bool {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    if encoded.len() > MAX_OSC52_ENCODED_LENGTH {
        return false;
    }
    (host.write_stdout)(&format!("\x1b]52;c;{encoded}\x07"));
    true
}

/// WSL without WSLg has no Linux display, so the Windows clipboard is written through
/// interop. PowerShell reads the text from a file because `clip.exe` and PowerShell stdin
/// decode piped bytes with the console code page, which mangles non-ASCII UTF-8.
async fn copy_via_windows_clipboard(host: &ClipboardHost, text: &str) -> bool {
    let tmp_file = std::env::temp_dir().join(format!("pi-wsl-clip-{}.txt", uuid::Uuid::new_v4()));
    let tmp_path = tmp_file.to_string_lossy().into_owned();
    let result = async {
        write_private_file(&tmp_file, text).ok()?;
        let win_path = host
            .run("wslpath", &["-w", &tmp_path], RunClipboardCommandOptions { timeout_ms: Some(1000), ..Default::default() })
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())?;
        if win_path.is_empty() {
            return Some(false);
        }
        let script = format!(
            "Set-Clipboard -Value ([System.IO.File]::ReadAllText('{}', [System.Text.Encoding]::UTF8))",
            win_path.replace('\'', "''")
        );
        let result = host
            .run(
                "powershell.exe",
                &["-NoProfile", "-Command", &script],
                RunClipboardCommandOptions { timeout_ms: Some(5000), ..Default::default() },
            )
            .await;
        Some(result.is_some())
    }
    .await
    .unwrap_or(false);
    // The file may not have been created.
    let _ = std::fs::remove_file(&tmp_file);
    result
}

fn write_private_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options.open(path)?.write_all(text.as_bytes())
}

type ReadTextOverride = Arc<dyn Fn() -> futures::future::BoxFuture<'static, anyhow::Result<Option<String>>> + Send + Sync>;

static READ_TEXT_OVERRIDE: parking_lot::RwLock<Option<ReadTextOverride>> = parking_lot::RwLock::new(None);

/// Route [`read_clipboard_text`] to `read` instead of the system clipboard, or restore it with `None`.
/// The equivalent of the TS tests' `vi.mock("../src/utils/clipboard.ts")`.
#[doc(hidden)]
pub fn set_read_clipboard_text_override(read: Option<ReadTextOverride>) {
    *READ_TEXT_OVERRIDE.write() = read;
}

/// Read plain text from the system clipboard.
pub async fn read_clipboard_text() -> anyhow::Result<Option<String>> {
    let read = READ_TEXT_OVERRIDE.read().clone();
    if let Some(read) = read {
        return read().await;
    }
    Ok(read_clipboard_text_with(&ClipboardHost::process()).await)
}

pub(crate) async fn read_clipboard_text_with(host: &ClipboardHost) -> Option<String> {
    let mut commands: Vec<(&str, Vec<&str>)> = Vec::new();
    // Termux reports platform "android", not "linux" (#10391).
    if host.env("TERMUX_VERSION").is_some() {
        commands.push(("termux-clipboard-get", vec![]));
    }
    if host.platform == "linux" {
        if host.env("WAYLAND_DISPLAY").is_some() {
            commands.push(("wl-paste", vec!["--no-newline", "--type", "text"]));
        }
        if host.env("DISPLAY").is_some() {
            commands.push(("xclip", vec!["-selection", "clipboard", "-out"]));
            commands.push(("xsel", vec!["--clipboard", "--output"]));
        }
    }
    for (command, args) in commands {
        let options = RunClipboardCommandOptions { timeout_ms: Some(5000), ..Default::default() };
        if let Some(bytes) = host.run(command, &args, options).await {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            return if text.is_empty() { None } else { Some(text) };
        }
    }
    let native = (host.native)()?;
    match native.get_text().await {
        Ok(ClipboardRead::Data(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// Read file paths, such as Finder file copies, from the native clipboard.
pub async fn read_clipboard_file_paths() -> anyhow::Result<Option<Vec<String>>> {
    let Some(native) = get_native_clipboard() else {
        return Ok(None);
    };
    let Some(read) = native.get_file_paths() else {
        return Ok(None);
    };
    match read.await.map_err(|e| anyhow::anyhow!(e))? {
        ClipboardRead::Data(paths) if !paths.is_empty() => Ok(Some(paths)),
        _ => Ok(None),
    }
}

type CopyOverride = Arc<dyn Fn(String) -> futures::future::BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

static COPY_OVERRIDE: parking_lot::RwLock<Option<CopyOverride>> = parking_lot::RwLock::new(None);

/// Route [`copy_to_clipboard`] to `copy` instead of the system clipboard, or restore it with `None`.
/// The equivalent of the TS tests' `vi.mock("../src/utils/clipboard.ts")`.
#[doc(hidden)]
pub fn set_copy_to_clipboard_override(copy: Option<CopyOverride>) {
    *COPY_OVERRIDE.write() = copy;
}

pub async fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let copy = COPY_OVERRIDE.read().clone();
    if let Some(copy) = copy {
        return copy(text.to_string()).await;
    }
    copy_to_clipboard_with(&ClipboardHost::process(), text).await
}

pub(crate) async fn copy_to_clipboard_with(host: &ClipboardHost, text: &str) -> anyhow::Result<()> {
    let p = host.platform.as_str();
    let mut copied = false;
    // Direct writes precede OSC 52 so the terminal cannot race the native writer.
    // Linux tools retain clipboard selection ownership after this call returns.
    if p != "linux"
        && let Some(clipboard) = (host.native)()
        && let Some(write) = clipboard.set_text(text)
        && write.await.is_ok()
    {
        copied = true;
    }
    if !copied {
        let mut commands: Vec<(&str, Vec<&str>)> = Vec::new();
        if p == "darwin" {
            commands.push(("pbcopy", vec![]));
        } else if p == "win32" {
            commands.push(("clip", vec![]));
        } else {
            if host.env("TERMUX_VERSION").is_some() {
                commands.push(("termux-clipboard-set", vec![]));
            }
            if host.env("WAYLAND_DISPLAY").is_some() {
                commands.push(("wl-copy", vec![]));
            }
            if host.env("DISPLAY").is_some() {
                commands.push(("xclip", vec!["-selection", "clipboard"]));
                commands.push(("xsel", vec!["--clipboard", "--input"]));
            }
        }
        for (command, args) in commands {
            let options = RunClipboardCommandOptions {
                input: Some(text.to_string()),
                timeout_ms: Some(5000),
                ..Default::default()
            };
            if host.run(command, &args, options).await.is_some() {
                copied = true;
                break;
            }
        }
    }
    let mut osc52_emitted = false;
    if !copied && p == "linux" && is_wsl(Some(&host.env)) {
        // Windows Terminal supports OSC 52; prefer it over the slower PowerShell round trip.
        if host.env("WT_SESSION").is_some() {
            osc52_emitted = emit_osc52(host, text);
        }
        copied = osc52_emitted || copy_via_windows_clipboard(host, text).await;
    }
    // OSC 52 cannot be verified, so a desktop session with a display reports the failure
    // instead (#9618). Without a display the terminal is the only clipboard route (containers,
    // WSL without WSLg), and remote sessions always emit it to reach the client clipboard.
    let headless = p == "linux"
        && host.env("DISPLAY").is_none()
        && host.env("WAYLAND_DISPLAY").is_none()
        && host.env("TERMUX_VERSION").is_none();
    let mut oversized = false;
    if !osc52_emitted && (is_remote_session(host) || (!copied && headless)) {
        if emit_osc52(host, text) {
            copied = true;
        } else {
            oversized = true;
        }
    }
    if copied {
        return Ok(());
    }
    if oversized {
        anyhow::bail!("Clipboard unavailable: text exceeds the OSC 52 size limit");
    }
    if host.env("TERMUX_VERSION").is_some() {
        anyhow::bail!("Clipboard unavailable: install the Termux:API app and `termux-api` package");
    }
    if p == "linux" {
        if host.env("WAYLAND_DISPLAY").is_some() {
            anyhow::bail!("Clipboard unavailable: install `wl-clipboard` (`wl-copy`) or check Wayland access");
        }
        if host.env("DISPLAY").is_some() {
            anyhow::bail!("Clipboard unavailable: install `xclip` or `xsel`, or check X11 access");
        }
    }
    anyhow::bail!("Clipboard unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    type Call = (String, Vec<String>, RunClipboardCommandOptions);

    struct MockNative {
        text: Mutex<Result<Option<String>, String>>,
        set_result: Mutex<Option<String>>, // Some(error) to fail
        set_calls: Arc<Mutex<Vec<String>>>,
        writable: bool,
    }

    impl NativeClipboard for MockNative {
        fn get_text(&self) -> pi_tui::event_loop::BoxFuture<Result<ClipboardRead<String>, String>> {
            let result = match &*self.text.lock() {
                Ok(Some(text)) => Ok(ClipboardRead::Data(text.clone())),
                Ok(None) => Ok(ClipboardRead::Empty),
                Err(e) => Err(e.clone()),
            };
            Box::pin(async move { result })
        }
        fn get_image(&self) -> pi_tui::event_loop::BoxFuture<Result<ClipboardRead<Vec<u8>>, String>> {
            Box::pin(async { Ok(ClipboardRead::Unavailable) })
        }
        fn set_text(&self, text: &str) -> Option<pi_tui::event_loop::BoxFuture<Result<(), String>>> {
            if !self.writable {
                return None;
            }
            self.set_calls.lock().push(text.to_string());
            let result = match &*self.set_result.lock() {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            };
            Some(Box::pin(async move { result }))
        }
    }

    /// Answers a mocked clipboard command with its output (`None` = failure).
    type Responder = Arc<dyn Fn(&str, &[String]) -> Option<Vec<u8>> + Send + Sync>;

    struct Fixture {
        host: ClipboardHost,
        calls: Arc<Mutex<Vec<Call>>>,
        native_calls: Arc<Mutex<usize>>,
        osc52: Arc<Mutex<Vec<String>>>,
        native: Arc<MockNative>,
        native_enabled: Arc<Mutex<bool>>,
        responder: Arc<Mutex<Responder>>,
    }

    fn fixture(platform: &str, env: &[(&str, &str)]) -> Fixture {
        let calls: Arc<Mutex<Vec<Call>>> = Arc::default();
        let native_calls: Arc<Mutex<usize>> = Arc::default();
        let osc52: Arc<Mutex<Vec<String>>> = Arc::default();
        let native = Arc::new(MockNative {
            text: Mutex::new(Ok(None)),
            set_result: Mutex::new(None),
            set_calls: Arc::default(),
            writable: true,
        });
        let native_enabled = Arc::new(Mutex::new(true));
        let responder: Arc<Mutex<Responder>> = Arc::new(Mutex::new(Arc::new(|_, _| Some(Vec::new()))));
        let host = ClipboardHost {
            platform: platform.to_string(),
            env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            native: {
                let native = native.clone();
                let native_calls = native_calls.clone();
                let enabled = native_enabled.clone();
                Arc::new(move || {
                    *native_calls.lock() += 1;
                    if *enabled.lock() { Some(native.clone() as Arc<dyn NativeClipboard>) } else { None }
                })
            },
            run: {
                let calls = calls.clone();
                let responder = responder.clone();
                Arc::new(move |command, args, options| {
                    calls.lock().push((command.clone(), args.clone(), options));
                    let responder = responder.lock().clone();
                    let result = responder(&command, &args);
                    Box::pin(async move { result })
                })
            },
            write_stdout: {
                let osc52 = osc52.clone();
                Arc::new(move |text| osc52.lock().push(text.to_string()))
            },
        };
        Fixture { host, calls, native_calls, osc52, native, native_enabled, responder }
    }

    impl Fixture {
        fn respond(&self, f: impl Fn(&str, &[String]) -> Option<Vec<u8>> + Send + Sync + 'static) {
            *self.responder.lock() = Arc::new(f);
        }
        fn names(&self) -> Vec<String> {
            self.calls.lock().iter().map(|c| c.0.clone()).collect()
        }
    }

    fn timeout_opts() -> RunClipboardCommandOptions {
        RunClipboardCommandOptions { timeout_ms: Some(5000), ..Default::default() }
    }

    fn input_opts(text: &str) -> RunClipboardCommandOptions {
        RunClipboardCommandOptions { input: Some(text.into()), timeout_ms: Some(5000), ..Default::default() }
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // readClipboardText
    #[tokio::test]
    async fn awaits_native_clipboard_text_and_catches_rejected_reads() {
        let f = fixture("darwin", &[]);
        *f.native.text.lock() = Ok(Some("clipboard text".into()));
        assert_eq!(read_clipboard_text_with(&f.host).await.as_deref(), Some("clipboard text"));
        *f.native.text.lock() = Err("clipboard unavailable".to_string());
        assert_eq!(read_clipboard_text_with(&f.host).await, None);
    }

    #[tokio::test]
    async fn command_result_stops_fallback() {
        let cases: [(&str, &str, Vec<&str>, Vec<&str>); 3] = [
            ("WAYLAND_DISPLAY", "wl-paste", vec!["--no-newline", "--type", "text"], vec!["wl-paste"]),
            ("DISPLAY", "xclip", vec!["-selection", "clipboard", "-out"], vec!["xclip"]),
            ("DISPLAY", "xsel", vec!["--clipboard", "--output"], vec!["xclip", "xsel"]),
        ];
        for (env, command, args, calls) in cases {
            for text in ["clipboard text", ""] {
                // Regression test for #7248: empty Wayland content must not fall through to stale X11.
                let f = fixture("linux", &[("DISPLAY", ":0"), (env, "1")]);
                let cmd = command.to_string();
                let t = text.to_string();
                f.respond(move |name, _| if name == cmd { Some(t.clone().into_bytes()) } else { None });
                let expected = if text.is_empty() { None } else { Some(text.to_string()) };
                assert_eq!(read_clipboard_text_with(&f.host).await, expected);
                assert_eq!(f.names(), strings(&calls));
                let last = f.calls.lock().last().cloned().unwrap();
                assert_eq!(last, (command.to_string(), strings(&args), timeout_opts()));
                assert_eq!(*f.native_calls.lock(), 0);
            }
        }
    }

    #[tokio::test]
    async fn uses_native_x11_after_command_failures() {
        for text in [Some("native text"), Some(""), None] {
            let f = fixture("linux", &[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")]);
            f.respond(|_, _| None);
            *f.native.text.lock() = Ok(text.map(str::to_string));
            let expected = text.filter(|t| !t.is_empty()).map(str::to_string);
            assert_eq!(read_clipboard_text_with(&f.host).await, expected);
            assert_eq!(*f.native_calls.lock(), 1);
            assert_eq!(f.names(), strings(&["wl-paste", "xclip", "xsel"]));
        }
    }

    #[tokio::test]
    async fn termux_reads_termux_clipboard_get_on_android() {
        for text in ["clipboard text", ""] {
            // Regression test for #10391: Termux reports platform "android".
            let f = fixture("android", &[("TERMUX_VERSION", "0.119")]);
            let t = text.to_string();
            f.respond(move |_, _| Some(t.clone().into_bytes()));
            let expected = if text.is_empty() { None } else { Some(text.to_string()) };
            assert_eq!(read_clipboard_text_with(&f.host).await, expected);
            assert_eq!(*f.calls.lock(), vec![("termux-clipboard-get".to_string(), vec![], timeout_opts())]);
            assert_eq!(*f.native_calls.lock(), 0);
        }
    }

    #[tokio::test]
    async fn falls_back_to_x11_tools_when_wl_paste_is_unavailable() {
        let f = fixture("linux", &[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);
        f.respond(|name, _| if name == "wl-paste" { None } else { Some(b"X11 text".to_vec()) });
        assert_eq!(read_clipboard_text_with(&f.host).await.as_deref(), Some("X11 text"));
        assert_eq!(*f.native_calls.lock(), 0);
    }

    // copyToClipboard
    #[tokio::test]
    async fn local_native_success_skips_osc52_and_commands() {
        let f = fixture("darwin", &[]);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(*f.native.set_calls.lock(), vec!["hello".to_string()]);
        assert!(f.osc52.lock().is_empty());
        assert!(f.calls.lock().is_empty());
    }

    #[tokio::test]
    async fn linux_skips_the_native_writer() {
        let f = fixture("linux", &[("DISPLAY", ":0")]);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(*f.native_calls.lock(), 0);
        assert_eq!(f.calls.lock()[0], ("xclip".to_string(), strings(&["-selection", "clipboard"]), input_opts("hello")));
    }

    #[tokio::test]
    async fn emits_remote_osc52_after_the_native_write() {
        let f = fixture("darwin", &[("SSH_CONNECTION", "client server")]);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(f.osc52.lock().len(), 1);
        assert!(f.calls.lock().is_empty());
    }

    #[tokio::test]
    async fn pi_clipboard_osc52_emits_after_a_successful_copy() {
        // Regression test for #10826, #10835, #10114: remote clients without SSH markers.
        for (platform, display) in [("darwin", None), ("win32", None), ("linux", Some(":99"))] {
            let mut env = vec![("PI_CLIPBOARD_OSC52", "1")];
            if let Some(display) = display {
                env.push(("DISPLAY", display));
            }
            let f = fixture(platform, &env);
            copy_to_clipboard_with(&f.host, "hello").await.unwrap();
            if platform == "linux" {
                assert_eq!(
                    f.calls.lock()[0],
                    ("xclip".to_string(), strings(&["-selection", "clipboard"]), input_opts("hello"))
                );
            } else {
                assert_eq!(*f.native.set_calls.lock(), vec!["hello".to_string()]);
            }
            assert_eq!(*f.osc52.lock(), vec!["\x1b]52;c;aGVsbG8=\x07".to_string()]);
        }
    }

    #[tokio::test]
    async fn a_rejected_native_write_falls_back_to_pbcopy() {
        let f = fixture("darwin", &[]);
        *f.native.set_result.lock() = Some("native failed".into());
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(*f.calls.lock(), vec![("pbcopy".to_string(), vec![], input_opts("hello"))]);
        assert!(f.osc52.lock().is_empty());
    }

    #[tokio::test]
    async fn a_read_only_native_clipboard_uses_the_command_writer() {
        let mut f = fixture("darwin", &[]);
        let read_only = Arc::new(MockNative {
            text: Mutex::new(Ok(None)),
            set_result: Mutex::new(None),
            set_calls: Arc::default(),
            writable: false,
        });
        f.host.native = Arc::new(move || Some(read_only.clone() as Arc<dyn NativeClipboard>));
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(f.calls.lock().len(), 1);
    }

    #[tokio::test]
    async fn tries_xclip_and_xsel_after_wl_copy_fails() {
        let f = fixture("linux", &[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);
        f.respond(|name, _| if name == "xsel" { Some(vec![]) } else { None });
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(f.names(), strings(&["wl-copy", "xclip", "xsel"]));
        assert!(f.osc52.lock().is_empty());
    }

    #[tokio::test]
    async fn local_linux_failure_does_not_report_an_unverified_osc52_write_as_success() {
        // Regression test for #9618.
        let f = fixture("linux", &[("DISPLAY", ":0")]);
        f.respond(|_, _| None);
        let err = copy_to_clipboard_with(&f.host, "hello").await.unwrap_err();
        assert_eq!(err.to_string(), "Clipboard unavailable: install `xclip` or `xsel`, or check X11 access");
        assert_eq!(f.names(), strings(&["xclip", "xsel"]));
        assert!(f.osc52.lock().is_empty());
    }

    #[tokio::test]
    async fn display_less_linux_falls_back_to_osc52() {
        // Regression test for #9688: containers without X11/Wayland access.
        let f = fixture("linux", &[]);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert!(f.calls.lock().is_empty());
        assert_eq!(f.osc52.lock().len(), 1);
    }

    #[tokio::test]
    async fn wsl_without_a_display_writes_the_windows_clipboard_through_powershell() {
        // Regression test for #9688: WSL with WSLg disabled.
        let f = fixture("linux", &[("WSL_DISTRO_NAME", "Ubuntu")]);
        let written: Arc<Mutex<Option<String>>> = Arc::default();
        let w = written.clone();
        f.respond(move |name, args| {
            if name != "wslpath" {
                return Some(vec![]);
            }
            *w.lock() = std::fs::read_to_string(&args[1]).ok();
            Some(b"\\\\wsl.localhost\\Ubuntu\\tmp\\clip.txt\n".to_vec())
        });
        copy_to_clipboard_with(&f.host, "héllo").await.unwrap();
        assert_eq!(f.names(), strings(&["wslpath", "powershell.exe"]));
        assert_eq!(written.lock().as_deref(), Some("héllo"));
        let calls = f.calls.lock().clone();
        assert!(!std::path::Path::new(&calls[0].1[1]).exists());
        assert!(calls[1].1[2].contains("Set-Clipboard"));
        assert!(calls[1].1[2].contains("'\\\\wsl.localhost\\Ubuntu\\tmp\\clip.txt'"));
        assert!(f.osc52.lock().is_empty());
    }

    #[tokio::test]
    async fn wsl_falls_back_to_osc52_when_windows_interop_is_unavailable() {
        let f = fixture("linux", &[("WSL_DISTRO_NAME", "Ubuntu")]);
        f.respond(|_, _| None);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(f.names(), strings(&["wslpath"]));
        assert_eq!(f.osc52.lock().len(), 1);
    }

    #[tokio::test]
    async fn wsl_in_windows_terminal_prefers_osc52_over_powershell() {
        let f = fixture("linux", &[("WSL_DISTRO_NAME", "Ubuntu"), ("WT_SESSION", "session")]);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert!(f.calls.lock().is_empty());
        assert_eq!(f.osc52.lock().len(), 1);
    }

    #[tokio::test]
    async fn wsl_in_windows_terminal_emits_osc52_once_in_a_remote_session() {
        let f = fixture(
            "linux",
            &[("WSL_DISTRO_NAME", "Ubuntu"), ("WT_SESSION", "session"), ("SSH_CONNECTION", "client server")],
        );
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert!(f.calls.lock().is_empty());
        assert_eq!(f.osc52.lock().len(), 1);
    }

    #[tokio::test]
    async fn wsl_in_windows_terminal_uses_powershell_for_oversized_osc52_payloads() {
        let f = fixture("linux", &[("WSL_DISTRO_NAME", "Ubuntu"), ("WT_SESSION", "session")]);
        f.respond(|name, _| if name == "wslpath" { Some(b"C:\\clip.txt".to_vec()) } else { Some(vec![]) });
        copy_to_clipboard_with(&f.host, &"x".repeat(80_000)).await.unwrap();
        assert_eq!(f.names(), strings(&["wslpath", "powershell.exe"]));
        assert!(f.osc52.lock().is_empty());
    }

    #[tokio::test]
    async fn wsl_with_a_display_prefers_the_linux_clipboard_tools() {
        let f = fixture("linux", &[("WSL_DISTRO_NAME", "Ubuntu"), ("WAYLAND_DISPLAY", "wayland-0")]);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(f.names(), strings(&["wl-copy"]));
        assert!(f.osc52.lock().is_empty());
    }

    #[tokio::test]
    async fn reports_the_wayland_clipboard_tool_instead_of_the_x11_fallback() {
        let f = fixture("linux", &[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);
        f.respond(|_, _| None);
        let err = copy_to_clipboard_with(&f.host, "hello").await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "Clipboard unavailable: install `wl-clipboard` (`wl-copy`) or check Wayland access"
        );
        assert_eq!(f.names(), strings(&["wl-copy", "xclip", "xsel"]));
    }

    #[tokio::test]
    async fn termux_on_android_writes_through_termux_clipboard_set() {
        let f = fixture("android", &[("TERMUX_VERSION", "0.119")]);
        *f.native_enabled.lock() = false;
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(*f.calls.lock(), vec![("termux-clipboard-set".to_string(), vec![], input_opts("hello"))]);
        assert!(f.osc52.lock().is_empty());
    }

    #[tokio::test]
    async fn reports_the_termux_api_requirement_on_android() {
        // Regression test for #10391: Termux reports platform "android".
        let f = fixture("android", &[("TERMUX_VERSION", "0.119")]);
        *f.native_enabled.lock() = false;
        f.respond(|_, _| None);
        let err = copy_to_clipboard_with(&f.host, "hello").await.unwrap_err();
        assert_eq!(err.to_string(), "Clipboard unavailable: install the Termux:API app and `termux-api` package");
    }

    #[tokio::test]
    async fn uses_osc52_when_native_and_command_writes_fail_in_a_remote_session() {
        let f = fixture("darwin", &[("SSH_CONNECTION", "client server")]);
        *f.native.set_result.lock() = Some("native failed".into());
        f.respond(|_, _| None);
        copy_to_clipboard_with(&f.host, "hello").await.unwrap();
        assert_eq!(f.osc52.lock().len(), 1);
    }

    #[tokio::test]
    async fn does_not_emit_oversized_osc52_payloads() {
        let f = fixture("darwin", &[("SSH_CONNECTION", "client server")]);
        *f.native.set_result.lock() = Some("native failed".into());
        f.respond(|_, _| None);
        let err = copy_to_clipboard_with(&f.host, &"x".repeat(80_000)).await.unwrap_err();
        assert_eq!(err.to_string(), "Clipboard unavailable: text exceeds the OSC 52 size limit");
        assert!(f.osc52.lock().is_empty());
    }
}
