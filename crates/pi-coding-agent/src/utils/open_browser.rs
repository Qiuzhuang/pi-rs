//! Port of `packages/coding-agent/src/utils/open-browser.ts`.

use std::process::{Command, Stdio};

/// Open a URL or file in the platform browser/default handler.
///
/// This intentionally never invokes a shell. On Windows, do not use
/// `cmd /c start`: cmd.exe re-parses metacharacters (&, |, ^, ...) before
/// `start` runs, which would make attacker-controlled URLs injectable.
type OpenOverride = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

static OPEN_OVERRIDE: parking_lot::RwLock<Option<OpenOverride>> = parking_lot::RwLock::new(None);

/// Route [`open_browser`] to `open` instead of the platform launcher, or restore it with `None`.
/// The equivalent of the TS tests' `vi.mock("../src/utils/open-browser.ts")`.
#[doc(hidden)]
pub fn set_open_browser_override(open: Option<OpenOverride>) {
    *OPEN_OVERRIDE.write() = open;
}

pub fn open_browser(target: &str) {
    let open = OPEN_OVERRIDE.read().clone();
    if let Some(open) = open {
        open(target);
        return;
    }
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![target])
    } else if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler", target])
    } else {
        ("xdg-open", vec![target])
    };

    // Launcher failures (for example, missing xdg-open) are ignored. Browser launch is
    // best-effort: callers still present the target to the user.
    let mut command = Command::new(cmd);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    if let Ok(mut child) = command.spawn() {
        // Reap the launcher in the background so it does not linger as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}
