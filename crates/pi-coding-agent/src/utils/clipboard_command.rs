//! Port of `packages/coding-agent/src/utils/clipboard-command.ts`.

use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunClipboardCommandOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_buffer_bytes: Option<i64>,
}

const DEFAULT_TIMEOUT_MS: i64 = 3000;
const DEFAULT_MAX_BUFFER_BYTES: i64 = 50 * 1024 * 1024;

/// Undefined means the command failed; an empty buffer is a successful result.
pub async fn run_clipboard_command(
    command: &str,
    args: &[String],
    options: Option<&RunClipboardCommandOptions>,
) -> anyhow::Result<Option<Vec<u8>>> {
    let input = options.and_then(|o| o.input.clone());
    let timeout_ms = options.and_then(|o| o.timeout_ms).unwrap_or(DEFAULT_TIMEOUT_MS);
    let max_buffer = options.and_then(|o| o.max_buffer_bytes).unwrap_or(DEFAULT_MAX_BUFFER_BYTES);

    // Clipboard writers can daemonize. Do not give them output pipes to retain.
    // Detach on POSIX so the daemon leaves pi's process group and Ctrl+Z suspend
    // (SIGTSTP to the group) does not stop the clipboard owner (#11179).
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(if input.is_none() { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::null());
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => return Ok(None),
    };

    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();

    let run = async {
        let child = &mut child;
        // A writer may exit before consuming all input; write errors are ignored.
        if let Some(mut pipe) = stdin.take() {
            if let Some(text) = &input {
                let _ = pipe.write_all(text.as_bytes()).await;
            }
            let _ = pipe.shutdown().await;
            drop(pipe);
        }
        let mut chunks = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let mut buf = [0u8; 64 * 1024];
            loop {
                match pipe.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if (chunks.len() + n) as i64 > max_buffer {
                            return None;
                        }
                        chunks.extend_from_slice(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }
        }
        match child.wait().await {
            Ok(status) if status.success() => Some(chunks),
            _ => None,
        }
    };

    let result = tokio::time::timeout(Duration::from_millis(timeout_ms.max(0) as u64), run).await;
    match result {
        Ok(Some(bytes)) => Ok(Some(bytes)),
        _ => {
            // Timeout or buffer overflow: kill the child (the `run` future was dropped, which
            // closed our pipe ends).
            let _ = child.start_kill();
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Vec<String> {
        vec!["-c".to_string(), script.to_string()]
    }

    #[tokio::test]
    async fn preserves_binary_output_and_distinguishes_empty_success_from_failure() {
        assert_eq!(
            run_clipboard_command("sh", &sh("printf '\\000\\377\\n'"), None).await.unwrap(),
            Some(vec![0, 255, 10])
        );
        assert_eq!(run_clipboard_command("sh", &sh("true"), None).await.unwrap(), Some(vec![]));
        assert_eq!(run_clipboard_command("sh", &sh("exit 1"), None).await.unwrap(), None);
        assert_eq!(run_clipboard_command("pi-clipboard-command-does-not-exist", &[], None).await.unwrap(), None);
    }

    #[tokio::test]
    async fn sends_unicode_input_to_clipboard_writers() {
        let options = RunClipboardCommandOptions { input: Some("café 日本語".into()), ..Default::default() };
        assert_eq!(
            run_clipboard_command("sh", &sh("test \"$(cat)\" = 'café 日本語'"), Some(&options)).await.unwrap(),
            Some(vec![])
        );
    }

    #[tokio::test]
    async fn times_out_without_blocking_the_event_loop() {
        let ticks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let t = ticks.clone();
        let ticker = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        });
        let options = RunClipboardCommandOptions { timeout_ms: Some(200), ..Default::default() };
        let started = std::time::Instant::now();
        assert_eq!(run_clipboard_command("sleep", &["5".to_string()], Some(&options)).await.unwrap(), None);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(ticks.load(std::sync::atomic::Ordering::SeqCst) > 5);
        ticker.abort();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_commands_outside_pis_process_group() {
        let child_pgid = run_clipboard_command("sh", &sh("ps -o pgid= -p $$"), None).await.unwrap();
        let own_pgid = run_clipboard_command(
            "ps",
            &["-o".to_string(), "pgid=".to_string(), "-p".to_string(), std::process::id().to_string()],
            None,
        )
        .await
        .unwrap();
        let (Some(child), Some(own)) = (child_pgid, own_pgid) else {
            // `ps` is not available in this environment.
            return;
        };
        let child = String::from_utf8(child).unwrap().trim().to_string();
        let own = String::from_utf8(own).unwrap().trim().to_string();
        assert!(child.chars().all(|c| c.is_ascii_digit()) && !child.is_empty());
        assert!(own.chars().all(|c| c.is_ascii_digit()) && !own.is_empty());
        assert_ne!(child, own);
    }

    #[tokio::test]
    async fn rejects_output_above_the_buffer_limit() {
        let options = RunClipboardCommandOptions { max_buffer_bytes: Some(16), ..Default::default() };
        assert_eq!(
            run_clipboard_command("sh", &sh("head -c 1024 /dev/zero"), Some(&options)).await.unwrap(),
            None
        );
    }
}
