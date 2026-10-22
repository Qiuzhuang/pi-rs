//! Port of `packages/coding-agent/src/utils/shell.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShellConfigCommandTransport {
    #[serde(rename = "argv")]
    Argv,
    #[serde(rename = "stdin")]
    Stdin,
}

impl ShellConfigCommandTransport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Argv => "argv",
            Self::Stdin => "stdin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_transport: Option<ShellConfigCommandTransport>,
}

/// What shell resolution reads from the system: TS reads `process.platform`, `process.env`,
/// `accessSync` and `spawnSync` directly, and its tests mock them. [`SystemShellHost`] is the real
/// implementation; tests substitute a fake.
pub(crate) trait ShellHost {
    fn is_windows(&self) -> bool;
    fn env(&self, name: &str) -> Option<String>;
    /// `access(path, F_OK)` succeeded.
    fn path_exists(&self, path: &str) -> bool;
    /// Run `where`/`which` for `executable`: `None` when it could not run, else
    /// `(exited successfully, stdout)`.
    fn lookup(&self, command: &str, executable: &str) -> Option<(bool, String)>;
}

pub(crate) struct SystemShellHost;

impl ShellHost for SystemShellHost {
    fn is_windows(&self) -> bool {
        cfg!(windows)
    }
    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn path_exists(&self, path: &str) -> bool {
        crate::core::tools::node_fs::access_sync(path, crate::core::tools::node_fs::F_OK).is_ok()
    }
    fn lookup(&self, command: &str, executable: &str) -> Option<(bool, String)> {
        let output = std::process::Command::new(command)
            .arg(executable)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        Some((output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned()))
    }
}

// Keep in sync with packages/durable/src/env/node.ts.
fn is_legacy_wsl_bash_path(host: &dyn ShellHost, path: &str) -> bool {
    fn win32_normalize_lower(path: &str) -> String {
        let replaced = path.replace('/', "\\");
        let mut out = String::with_capacity(replaced.len());
        let mut prev_sep = false;
        for (i, c) in replaced.chars().enumerate() {
            if c == '\\' {
                // Keep a leading UNC double separator.
                if prev_sep && i > 1 {
                    continue;
                }
                prev_sep = true;
            } else {
                prev_sep = false;
            }
            out.push(c);
        }
        out.to_lowercase()
    }
    static DRIVE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)^[a-z]:[\\/]").unwrap());
    static FALLBACK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^[a-z]:\\windows\\(?:system32|sysnative)\\bash\.exe$").unwrap()
    });
    let normalized = win32_normalize_lower(path);
    if let Some(system_root) = host.env("SystemRoot").filter(|s| !s.is_empty()) {
        if !DRIVE.is_match(&system_root) {
            return false;
        }
        return ["System32", "Sysnative"].iter().any(|directory| {
            normalized == win32_normalize_lower(&format!("{system_root}\\{directory}\\bash.exe"))
        });
    }
    // Fall back to conventional launcher paths only when the Windows directory is unknown.
    FALLBACK.is_match(&normalized)
}

fn get_bash_shell_config(host: &dyn ShellHost, shell: &str) -> ShellConfig {
    if is_legacy_wsl_bash_path(host, shell) {
        ShellConfig {
            shell: shell.to_string(),
            args: vec!["-s".to_string()],
            command_transport: Some(ShellConfigCommandTransport::Stdin),
        }
    } else {
        ShellConfig {
            shell: shell.to_string(),
            args: vec!["-c".to_string()],
            command_transport: None,
        }
    }
}

fn find_executable_on_path(host: &dyn ShellHost, executable: &str) -> Option<String> {
    let windows = host.is_windows();
    let (success, stdout) = host.lookup(if windows { "where" } else { "which" }, executable)?;
    if !success || stdout.is_empty() {
        return None;
    }
    let first_match = stdout.trim().split('\n').next()?.trim_end_matches('\r').to_string();
    if first_match.is_empty() {
        return None;
    }
    // Windows: check the returned path. Unix: trust `which` output (handles Termux and special
    // filesystems).
    if windows && !host.path_exists(&first_match) {
        return None;
    }
    Some(first_match)
}

/// Resolve shell configuration based on platform and an optional explicit shell path.
/// Resolution order:
/// 1. User-specified shellPath
/// 2. On Windows: Git Bash in known locations, then bash on PATH
/// 3. On Unix: /bin/bash, then bash on PATH, then fallback to sh
pub fn get_shell_config(custom_shell_path: Option<&str>) -> anyhow::Result<ShellConfig> {
    resolve_shell_config(&SystemShellHost, custom_shell_path)
}

pub(crate) fn resolve_shell_config(host: &dyn ShellHost, custom_shell_path: Option<&str>) -> anyhow::Result<ShellConfig> {
    // 1. Check user-specified shell path
    if let Some(custom) = custom_shell_path.filter(|p| !p.is_empty()) {
        if host.path_exists(custom) {
            return Ok(get_bash_shell_config(host, custom));
        }
        anyhow::bail!("Custom shell path not found: {custom}");
    }

    if host.is_windows() {
        // 2. Try Git Bash in known locations
        let mut paths: Vec<String> = Vec::new();
        if let Some(program_files) = host.env("ProgramFiles").filter(|s| !s.is_empty()) {
            paths.push(format!("{program_files}\\Git\\bin\\bash.exe"));
        }
        if let Some(program_files_x86) = host.env("ProgramFiles(x86)").filter(|s| !s.is_empty()) {
            paths.push(format!("{program_files_x86}\\Git\\bin\\bash.exe"));
        }
        for path in &paths {
            if host.path_exists(path) {
                return Ok(get_bash_shell_config(host, path));
            }
        }

        // 3. Fallback: search bash.exe on PATH (Cygwin, MSYS2, WSL, etc.)
        if let Some(bash_on_path) = find_executable_on_path(host, "bash.exe") {
            return Ok(get_bash_shell_config(host, &bash_on_path));
        }

        anyhow::bail!(
            "No bash shell found. Options:\n  1. Install Git for Windows: https://git-scm.com/download/win\n  2. Add your bash to PATH (Cygwin, MSYS2, etc.)\n  3. Set shellPath in settings.json\n\nSearched Git Bash in:\n{}",
            paths.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
        );
    }

    // Unix: try /bin/bash, then bash on PATH, then fallback to sh
    if host.path_exists("/bin/bash") {
        return Ok(get_bash_shell_config(host, "/bin/bash"));
    }
    if let Some(bash_on_path) = find_executable_on_path(host, "bash") {
        return Ok(get_bash_shell_config(host, &bash_on_path));
    }
    Ok(ShellConfig {
        shell: "sh".to_string(),
        args: vec!["-c".to_string()],
        command_transport: None,
    })
}

pub const POWERSHELL_ARGS: &[&str] = &[
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-Command",
];

/// Resolve PowerShell on Windows, preferring PowerShell 7 when available.
pub fn get_power_shell_config() -> anyhow::Result<ShellConfig> {
    resolve_power_shell_config(&SystemShellHost)
}

/// `win32.join` of an absolute root and plain segments.
fn win32_join(root: &str, segments: &[&str]) -> String {
    let mut joined = root.trim_end_matches(['\\', '/']).to_string();
    for segment in segments {
        joined.push('\\');
        joined.push_str(segment);
    }
    joined
}

pub(crate) fn resolve_power_shell_config(host: &dyn ShellHost) -> anyhow::Result<ShellConfig> {
    if !host.is_windows() {
        anyhow::bail!("The powershell tool is only available on Windows.");
    }

    let mut shell = find_executable_on_path(host, "pwsh.exe");
    // Installation directories may be on any drive and need not be on PATH.
    if shell.is_none()
        && let Some(program_files) = host.env("ProgramFiles").filter(|s| !s.is_empty())
    {
        let pwsh7 = win32_join(&program_files, &["PowerShell", "7", "pwsh.exe"]);
        if host.path_exists(&pwsh7) {
            shell = Some(pwsh7);
        }
    }
    if shell.is_none() {
        shell = find_executable_on_path(host, "powershell.exe");
    }
    if shell.is_none()
        && let Some(system_root) = host.env("SystemRoot").filter(|s| !s.is_empty())
    {
        let windows_power_shell = win32_join(&system_root, &["System32", "WindowsPowerShell", "v1.0", "powershell.exe"]);
        if host.path_exists(&windows_power_shell) {
            shell = Some(windows_power_shell);
        }
    }

    let Some(shell) = shell else {
        anyhow::bail!("No PowerShell executable found. Install PowerShell or add powershell.exe/pwsh.exe to PATH.");
    };
    Ok(ShellConfig {
        shell,
        args: POWERSHELL_ARGS.iter().map(|s| s.to_string()).collect(),
        command_transport: None,
    })
}

/// Environment for spawned shells (TS `NodeJS.ProcessEnv`).
pub fn get_shell_env() -> indexmap::IndexMap<String, String> {
    let bin_dir = crate::config::get_bin_dir();
    let mut env: indexmap::IndexMap<String, String> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    let path_key = env
        .keys()
        .find(|key| key.eq_ignore_ascii_case("path"))
        .cloned()
        .unwrap_or_else(|| "PATH".to_string());
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let current_path = env.get(&path_key).cloned().unwrap_or_default();
    let has_bin_dir = current_path
        .split(delimiter)
        .filter(|entry| !entry.is_empty())
        .any(|entry| entry == bin_dir);
    let updated_path = if has_bin_dir {
        current_path
    } else {
        [bin_dir.as_str(), current_path.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(delimiter)
    };
    env.insert(path_key, updated_path);
    env
}

/// Sanitize binary output for display/storage.
/// Removes characters that crash string-width or cause display issues:
/// - Control characters (except tab, newline, carriage return)
/// - Unicode interlinear annotation characters U+FFF9..U+FFFB (crash string-width due to a bug)
pub fn sanitize_binary_output(str: &str) -> String {
    str.chars()
        .filter(|&c| {
            !matches!(c,
                '\u{00}'..='\u{08}' | '\u{0B}' | '\u{0C}' | '\u{0E}'..='\u{1F}' | '\u{FFF9}'..='\u{FFFB}'
            )
        })
        .collect()
}

/// Detached child processes must be tracked so they can be killed on parent
/// shutdown signals (SIGHUP/SIGTERM).
static TRACKED_DETACHED_CHILD_PIDS: std::sync::LazyLock<parking_lot::Mutex<indexmap::IndexSet<i64>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(indexmap::IndexSet::new()));

pub fn track_detached_child_pid(pid: i64) {
    TRACKED_DETACHED_CHILD_PIDS.lock().insert(pid);
}

pub fn untrack_detached_child_pid(pid: i64) {
    TRACKED_DETACHED_CHILD_PIDS.lock().shift_remove(&pid);
}

pub fn kill_tracked_detached_children() {
    let pids: Vec<i64> = {
        let mut tracked = TRACKED_DETACHED_CHILD_PIDS.lock();
        let pids = tracked.iter().copied().collect();
        tracked.clear();
        pids
    };
    for pid in pids {
        kill_process_tree(pid);
    }
}

/// Kill a process and all its children (cross-platform)
pub fn kill_process_tree(pid: i64) {
    #[cfg(windows)]
    {
        // Use the trusted System32 executable so cleanup does not depend on PATH.
        let system_root = std::env::var("SystemRoot")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "C:\\Windows".to_string());
        let taskkill = format!("{system_root}\\System32\\taskkill.exe");
        let _ = std::process::Command::new(taskkill)
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return;
        };
        // SAFETY: kill(2) with a negative pid signals the process group; no memory is touched.
        let group_result = unsafe { libc::kill(-pid, libc::SIGKILL) };
        if group_result != 0 {
            // Fallback to killing just the child if process group kill fails.
            // SAFETY: as above.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_control_characters() {
        assert_eq!(sanitize_binary_output("a\u{0}b\tc\nd\r\u{1b}e\u{FFFA}f😀"), "ab\tc\nd\ref😀");
    }

    #[test]
    fn shell_env_puts_bin_dir_on_path() {
        let env = get_shell_env();
        let bin_dir = crate::config::get_bin_dir();
        let path_key = env.keys().find(|k| k.eq_ignore_ascii_case("path")).unwrap();
        assert!(env[path_key].split(':').any(|p| p == bin_dir));
    }

    #[cfg(unix)]
    #[test]
    fn custom_shell_path_must_exist() {
        let err = get_shell_config(Some("/definitely/not/a/shell")).unwrap_err();
        assert_eq!(err.to_string(), "Custom shell path not found: /definitely/not/a/shell");
        let config = get_shell_config(Some("/bin/sh")).unwrap();
        assert_eq!(config.shell, "/bin/sh");
        assert_eq!(config.args, vec!["-c"]);
        assert!(config.command_transport.is_none());
    }

    // ---- Ports of powershell-config.test.ts, shell-path-exists.test.ts and
    // windows-shell-config.test.ts, which mock process.platform, env, accessSync and spawnSync. ----

    type ExistsFn = Box<dyn Fn(&str) -> bool>;
    type LookupFn = Box<dyn Fn(&str, &str) -> Option<(bool, String)>>;

    struct FakeHost {
        windows: bool,
        env: std::collections::HashMap<String, String>,
        exists: ExistsFn,
        /// One-shot `access` results consumed before `exists` (TS `mockImplementationOnce`).
        exists_once: std::cell::RefCell<std::collections::VecDeque<bool>>,
        lookup: LookupFn,
        lookups: std::cell::RefCell<Vec<(String, String)>>,
    }

    impl FakeHost {
        fn new(windows: bool) -> Self {
            Self {
                windows,
                env: Default::default(),
                exists: Box::new(|_| true),
                exists_once: Default::default(),
                lookup: Box::new(|_, _| Some((false, String::new()))),
                lookups: Default::default(),
            }
        }
        fn env(mut self, name: &str, value: &str) -> Self {
            self.env.insert(name.into(), value.into());
            self
        }
        fn exists(mut self, f: impl Fn(&str) -> bool + 'static) -> Self {
            self.exists = Box::new(f);
            self
        }
        fn exists_once(self, result: bool) -> Self {
            self.exists_once.borrow_mut().push_back(result);
            self
        }
        fn lookup(mut self, f: impl Fn(&str, &str) -> Option<(bool, String)> + 'static) -> Self {
            self.lookup = Box::new(f);
            self
        }
    }

    impl ShellHost for FakeHost {
        fn is_windows(&self) -> bool {
            self.windows
        }
        fn env(&self, name: &str) -> Option<String> {
            self.env.get(name).cloned()
        }
        fn path_exists(&self, path: &str) -> bool {
            if let Some(result) = self.exists_once.borrow_mut().pop_front() {
                return result;
            }
            (self.exists)(path)
        }
        fn lookup(&self, command: &str, executable: &str) -> Option<(bool, String)> {
            self.lookups.borrow_mut().push((command.into(), executable.into()));
            (self.lookup)(command, executable)
        }
    }

    const PWSH7: &str = "D:\\Applications\\PowerShell\\7\\pwsh.exe";
    const WINDOWS_POWERSHELL: &str = "E:\\WinNT\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";

    fn powershell_host() -> FakeHost {
        FakeHost::new(true).env("ProgramFiles", "D:\\Applications").env("SystemRoot", "E:\\WinNT").exists(|_| false)
    }

    fn powershell(shell: &str) -> (String, Vec<String>) {
        (shell.to_string(), POWERSHELL_ARGS.iter().map(|s| s.to_string()).collect())
    }

    fn config_of(config: ShellConfig) -> (String, Vec<String>) {
        assert!(config.command_transport.is_none());
        (config.shell, config.args)
    }

    // #9490: discover PowerShell outside PATH without assuming a drive or Windows directory name.
    #[test]
    fn powershell_prefers_powershell_7_on_path_over_known_installation_locations() {
        let shell = "E:\\Tools\\pwsh.exe";
        let host = powershell_host().exists(|_| true).lookup(move |_, _| Some((true, format!("{shell}\r\n"))));
        assert_eq!(config_of(resolve_power_shell_config(&host).unwrap()), powershell(shell));
    }

    #[test]
    fn powershell_finds_powershell_7_under_program_files_before_windows_powershell_on_path() {
        let host = powershell_host()
            .lookup(|_, exe| {
                if exe == "powershell.exe" { Some((true, WINDOWS_POWERSHELL.into())) } else { Some((false, String::new())) }
            })
            .exists(|path| path == PWSH7 || path == WINDOWS_POWERSHELL);
        assert_eq!(config_of(resolve_power_shell_config(&host).unwrap()), powershell(PWSH7));
    }

    #[test]
    fn powershell_prefers_windows_powershell_on_path_over_its_system_installation() {
        let shell = "E:\\Tools\\powershell.exe";
        let host = powershell_host()
            .lookup(move |_, exe| {
                if exe == "pwsh.exe" { Some((false, String::new())) } else { Some((true, shell.into())) }
            })
            .exists(move |path| path == shell || path == WINDOWS_POWERSHELL);
        assert_eq!(config_of(resolve_power_shell_config(&host).unwrap()), powershell(shell));
    }

    #[test]
    fn powershell_finds_windows_powershell_under_system_root_when_path_lookup_is_unavailable() {
        let host = powershell_host().lookup(|_, _| None).exists(|path| path == WINDOWS_POWERSHELL);
        assert_eq!(config_of(resolve_power_shell_config(&host).unwrap()), powershell(WINDOWS_POWERSHELL));
    }

    #[test]
    fn powershell_reports_an_error_when_no_powershell_installation_exists() {
        let error = resolve_power_shell_config(&powershell_host()).unwrap_err();
        assert!(error.to_string().contains("No PowerShell executable found"));
    }

    // powershell-tool.test.ts
    #[test]
    fn powershell_uses_process_local_execution_policy_bypass() {
        assert_eq!(POWERSHELL_ARGS, ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command"]);
    }

    const ALIASES_DIR: &str = "D:\\Users\\test\\AppData\\Local\\Microsoft\\WindowsApps";

    // nodejs/node#36790: runnable Store aliases pass access(F_OK) but fail existsSync's stat check.
    #[test]
    fn windows_store_aliases_found_on_path_are_accepted() {
        for executable in ["bash.exe", "pwsh.exe"] {
            let alias = format!("{ALIASES_DIR}\\{executable}");
            let output = format!("{alias}\r\n");
            let host = FakeHost::new(true).lookup(move |_, _| Some((true, output.clone())));
            let config =
                if executable == "pwsh.exe" { resolve_power_shell_config(&host) } else { resolve_shell_config(&host, None) };
            assert_eq!(config.unwrap().shell, alias);
        }
    }

    #[test]
    fn windows_accepts_an_explicitly_configured_shell_alias() {
        let alias = format!("{ALIASES_DIR}\\bash.exe");
        assert_eq!(resolve_shell_config(&FakeHost::new(true), Some(&alias)).unwrap().shell, alias);
    }

    #[test]
    fn windows_still_rejects_path_results_that_fail_the_existence_check() {
        let windows_power_shell = "D:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
        let host = FakeHost::new(true)
            .lookup(move |_, exe| {
                Some((true, if exe == "pwsh.exe" { format!("{ALIASES_DIR}\\pwsh.exe") } else { windows_power_shell.into() }))
            })
            .exists_once(false);
        assert_eq!(resolve_power_shell_config(&host).unwrap().shell, windows_power_shell);
    }

    // The access(F_OK) check is used on every platform.
    #[test]
    fn unix_accepts_bin_bash_when_access_succeeds() {
        assert_eq!(resolve_shell_config(&FakeHost::new(false), None).unwrap().shell, "/bin/bash");
    }

    #[test]
    fn unix_falls_back_to_bash_on_path_when_bin_bash_fails_access() {
        let host = FakeHost::new(false).exists_once(false).lookup(|_, _| Some((true, "/usr/local/bin/bash\n".into())));
        assert_eq!(resolve_shell_config(&host, None).unwrap().shell, "/usr/local/bin/bash");
        assert_eq!(host.lookups.borrow().clone(), vec![("which".to_string(), "bash".to_string())]);
    }

    #[test]
    fn unix_rejects_an_explicitly_configured_shell_path_that_fails_access() {
        let host = FakeHost::new(false).exists_once(false);
        assert_eq!(
            resolve_shell_config(&host, Some("/missing/bash")).unwrap_err().to_string(),
            "Custom shell path not found: /missing/bash"
        );
    }

    // windows-shell-config.test.ts (#9490): legacy WSL launchers under the actual Windows directory.
    #[test]
    fn legacy_wsl_bash_detection() {
        for (system_root, shell, stdin) in [
            (Some("D:\\WinNT\\"), "d:/WINNT/System32/BASH.EXE", true),
            (Some("D:\\WinNT"), "D:\\WinNT\\Sysnative\\bash.exe", true),
            (None, "E:\\Windows\\System32\\bash.exe", true),
            (Some("D:\\WinNT"), "D:\\Git\\bin\\bash.exe", false),
            (Some("D:\\WinNT"), "E:\\Windows\\System32\\bash.exe", false),
            (Some("WinNT"), "C:\\Windows\\System32\\bash.exe", false),
        ] {
            let mut host = FakeHost::new(true);
            if let Some(root) = system_root {
                host = host.env("SystemRoot", root);
            }
            let config = resolve_shell_config(&host, Some(shell)).unwrap();
            assert_eq!(config.shell, shell);
            if stdin {
                assert_eq!(config.args, vec!["-s"], "{shell}");
                assert_eq!(config.command_transport, Some(ShellConfigCommandTransport::Stdin));
            } else {
                assert_eq!(config.args, vec!["-c"], "{shell}");
                assert!(config.command_transport.is_none());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn powershell_is_windows_only() {
        assert_eq!(
            get_power_shell_config().unwrap_err().to_string(),
            "The powershell tool is only available on Windows."
        );
    }
}
