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

// Keep in sync with packages/durable/src/env/node.ts.
fn is_legacy_wsl_bash_path(path: &str) -> bool {
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
    if let Ok(system_root) = std::env::var("SystemRoot")
        && !system_root.is_empty()
    {
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

fn get_bash_shell_config(shell: &str) -> ShellConfig {
    if is_legacy_wsl_bash_path(shell) {
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

/// Matches harness-v2's pathExists: `access(F_OK)`.
fn shell_path_exists(path: &str) -> bool {
    crate::core::tools::node_fs::access_sync(path, crate::core::tools::node_fs::F_OK).is_ok()
}

fn find_executable_on_path(executable: &str) -> Option<String> {
    let lookup = if cfg!(windows) { "where" } else { "which" };
    let output = std::process::Command::new(lookup)
        .arg(executable)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_match = stdout.trim().split('\n').next()?.trim_end_matches('\r').to_string();
    if first_match.is_empty() {
        return None;
    }
    if cfg!(windows) && !shell_path_exists(&first_match) {
        return None;
    }
    // Unix: trust `which` output (handles Termux and special filesystems).
    Some(first_match)
}

/// Resolve shell configuration based on platform and an optional explicit shell path.
/// Resolution order:
/// 1. User-specified shellPath
/// 2. On Windows: Git Bash in known locations, then bash on PATH
/// 3. On Unix: /bin/bash, then bash on PATH, then fallback to sh
pub fn get_shell_config(custom_shell_path: Option<&str>) -> anyhow::Result<ShellConfig> {
    // 1. Check user-specified shell path
    if let Some(custom) = custom_shell_path.filter(|p| !p.is_empty()) {
        if shell_path_exists(custom) {
            return Ok(get_bash_shell_config(custom));
        }
        anyhow::bail!("Custom shell path not found: {custom}");
    }

    if cfg!(windows) {
        // 2. Try Git Bash in known locations
        let mut paths: Vec<String> = Vec::new();
        if let Ok(program_files) = std::env::var("ProgramFiles")
            && !program_files.is_empty()
        {
            paths.push(format!("{program_files}\\Git\\bin\\bash.exe"));
        }
        if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)")
            && !program_files_x86.is_empty()
        {
            paths.push(format!("{program_files_x86}\\Git\\bin\\bash.exe"));
        }
        for path in &paths {
            if shell_path_exists(path) {
                return Ok(get_bash_shell_config(path));
            }
        }

        // 3. Fallback: search bash.exe on PATH (Cygwin, MSYS2, WSL, etc.)
        if let Some(bash_on_path) = find_executable_on_path("bash.exe") {
            return Ok(get_bash_shell_config(&bash_on_path));
        }

        anyhow::bail!(
            "No bash shell found. Options:\n  1. Install Git for Windows: https://git-scm.com/download/win\n  2. Add your bash to PATH (Cygwin, MSYS2, etc.)\n  3. Set shellPath in settings.json\n\nSearched Git Bash in:\n{}",
            paths.iter().map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n")
        );
    }

    // Unix: try /bin/bash, then bash on PATH, then fallback to sh
    if shell_path_exists("/bin/bash") {
        return Ok(get_bash_shell_config("/bin/bash"));
    }
    if let Some(bash_on_path) = find_executable_on_path("bash") {
        return Ok(get_bash_shell_config(&bash_on_path));
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
    if !cfg!(windows) {
        anyhow::bail!("The powershell tool is only available on Windows.");
    }

    let mut shell = find_executable_on_path("pwsh.exe");
    // Installation directories may be on any drive and need not be on PATH.
    if shell.is_none()
        && let Ok(program_files) = std::env::var("ProgramFiles")
        && !program_files.is_empty()
    {
        let pwsh7 = format!("{program_files}\\PowerShell\\7\\pwsh.exe");
        if shell_path_exists(&pwsh7) {
            shell = Some(pwsh7);
        }
    }
    if shell.is_none() {
        shell = find_executable_on_path("powershell.exe");
    }
    if shell.is_none()
        && let Ok(system_root) = std::env::var("SystemRoot")
        && !system_root.is_empty()
    {
        let windows_power_shell = format!("{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
        if shell_path_exists(&windows_power_shell) {
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

    #[cfg(unix)]
    #[test]
    fn powershell_is_windows_only() {
        assert_eq!(
            get_power_shell_config().unwrap_err().to_string(),
            "The powershell tool is only available on Windows."
        );
    }
}
