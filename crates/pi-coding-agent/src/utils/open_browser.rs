//! Port of `packages/coding-agent/src/utils/open-browser.ts`.

/// Open a URL or file in the platform browser/default handler.
///
/// This intentionally never invokes a shell. On Windows, do not use
/// `cmd /c start`: cmd.exe re-parses metacharacters (&, |, ^, ...) before
/// `start` runs, which would make attacker-controlled URLs injectable.
pub fn open_browser(target: &str) {
    todo!("port utils/open-browser.ts:openBrowser")
}
