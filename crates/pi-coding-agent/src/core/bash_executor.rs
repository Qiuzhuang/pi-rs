//! Port of `packages/coding-agent/src/core/bash-executor.ts`.
//!
//! Bash command execution with streaming support and cancellation.
//!
//! This module provides a unified bash execution implementation used by:
//! - AgentSession.executeBash() for interactive and RPC modes
//! - Direct calls from modes that need bash execution

use crate::core::tools::bash::{BashOperations, BashOperationsOptions};
use crate::core::tools::truncate::{DEFAULT_MAX_BYTES, truncate_tail};
use crate::utils::ansi::{split_incomplete_ansi_suffix, strip_ansi};
use crate::utils::output_files::create_output_file_stream;
use crate::utils::shell::sanitize_binary_output;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct BashExecutorOptions {
    /// Callback for streaming output chunks (already sanitized)
    pub on_chunk: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// AbortSignal for cancellation
    pub signal: Option<CancellationToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashResult {
    /// Combined stdout + stderr output (sanitized, possibly truncated)
    pub output: String,
    /// Process exit code (undefined if killed/cancelled)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// Whether the command was cancelled via signal
    pub cancelled: bool,
    /// Whether the output was truncated
    pub truncated: bool,
    /// Path to temp file containing full output (if output exceeded truncation threshold)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

/// Streaming UTF-8 decoder (`TextDecoder` with `{ stream: true }`): incomplete trailing
/// sequences wait for the next chunk, invalid bytes become U+FFFD.
#[derive(Default)]
pub(crate) struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub(crate) fn decode(&mut self, data: &[u8], stream: bool) -> String {
        self.pending.extend_from_slice(data);
        let mut out = String::new();
        let mut bytes: &[u8] = &self.pending;
        loop {
            match std::str::from_utf8(bytes) {
                Ok(valid) => {
                    out.push_str(valid);
                    bytes = &[];
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    out.push_str(std::str::from_utf8(&bytes[..valid_up_to]).unwrap_or_default());
                    match error.error_len() {
                        Some(len) => {
                            out.push('\u{FFFD}');
                            bytes = &bytes[valid_up_to + len..];
                        }
                        None => {
                            // Incomplete sequence at the end.
                            bytes = &bytes[valid_up_to..];
                            if !stream {
                                out.push('\u{FFFD}');
                                bytes = &[];
                            }
                            break;
                        }
                    }
                }
            }
        }
        self.pending = bytes.to_vec();
        out
    }
}

struct ExecutorState {
    output_chunks: VecDeque<String>,
    output_bytes: usize,
    temp_file_path: Option<String>,
    temp_file: Option<std::fs::File>,
    total_bytes: usize,
    decoder: Utf8StreamDecoder,
    /// Unfinished escape sequence at the end of the previous chunk, completed by the next chunk.
    pending_ansi: String,
}

impl ExecutorState {
    fn ensure_temp_file(&mut self) {
        if self.temp_file_path.is_some() {
            return;
        }
        if let Ok(created) = create_output_file_stream("pi-bash", ".log") {
            let mut file = created.stream;
            for chunk in &self.output_chunks {
                let _ = file.write_all(chunk.as_bytes());
            }
            self.temp_file_path = Some(created.path);
            self.temp_file = Some(file);
        }
    }

    fn append_text(
        &mut self,
        raw_text: &str,
        on_chunk: Option<&Arc<dyn Fn(String) + Send + Sync>>,
    ) -> Option<String> {
        // Sanitize: strip ANSI, replace binary garbage, normalize newlines
        let stripped = strip_ansi(raw_text).unwrap_or_else(|_| raw_text.to_string());
        let text = sanitize_binary_output(&stripped).replace('\r', "");
        if text.is_empty() {
            return None;
        }
        let max_output_bytes = (DEFAULT_MAX_BYTES * 2) as usize;

        // Start writing to temp file if exceeds threshold
        if self.total_bytes > DEFAULT_MAX_BYTES as usize {
            self.ensure_temp_file();
        }
        if let Some(file) = self.temp_file.as_mut() {
            let _ = file.write_all(text.as_bytes());
        }

        // Keep rolling buffer
        self.output_bytes += text.len();
        self.output_chunks.push_back(text.clone());
        while self.output_bytes > max_output_bytes && self.output_chunks.len() > 1 {
            if let Some(removed) = self.output_chunks.pop_front() {
                self.output_bytes -= removed.len();
            }
        }
        on_chunk.map(|_| text)
    }
}

/// Execute a bash command using custom BashOperations.
/// Used for remote execution (SSH, containers, etc.).
pub async fn execute_bash_with_operations(
    command: &str,
    cwd: &str,
    operations: &BashOperations,
    options: Option<&BashExecutorOptions>,
) -> anyhow::Result<BashResult> {
    let on_chunk = options.and_then(|o| o.on_chunk.clone());
    let signal = options.and_then(|o| o.signal.clone());
    let state = Arc::new(Mutex::new(ExecutorState {
        output_chunks: VecDeque::new(),
        output_bytes: 0,
        temp_file_path: None,
        temp_file: None,
        total_bytes: 0,
        decoder: Utf8StreamDecoder::default(),
        pending_ansi: String::new(),
    }));

    let on_data = {
        let state = state.clone();
        let on_chunk = on_chunk.clone();
        Arc::new(move |data: Vec<u8>| {
            let text = {
                let mut state = state.lock();
                state.total_bytes += data.len();
                let decoded = state.decoder.decode(&data, true);
                let combined = format!("{}{}", std::mem::take(&mut state.pending_ansi), decoded);
                let split = split_incomplete_ansi_suffix(&combined);
                state.pending_ansi = split.pending;
                state.append_text(&split.complete, on_chunk.as_ref())
            };
            // Call back without holding the lock.
            if let (Some(text), Some(cb)) = (text, on_chunk.as_ref()) {
                cb(text);
            }
        })
    };

    let result = (operations.exec)(
        command.to_string(),
        cwd.to_string(),
        BashOperationsOptions {
            on_data,
            signal: signal.clone(),
            timeout: None,
            env: None,
        },
    )
    .await;
    let exit_code = match result {
        Ok(result) => result.exit_code,
        Err(error) => {
            // An aborted command still returns the output it produced so far
            if !signal.as_ref().is_some_and(|s| s.is_cancelled()) {
                state.lock().temp_file.take();
                return Err(error);
            }
            None
        }
    };

    // Flush the decoder and any pending escape sequence.
    let flushed = {
        let mut state = state.lock();
        let decoded = state.decoder.decode(&[], false);
        let rest = format!("{}{}", std::mem::take(&mut state.pending_ansi), decoded);
        state.append_text(&rest, on_chunk.as_ref())
    };
    if let (Some(text), Some(cb)) = (flushed, on_chunk.as_ref()) {
        cb(text);
    }

    let mut state = state.lock();
    let full_output: String = state.output_chunks.iter().map(String::as_str).collect();
    let truncation = truncate_tail(&full_output, None);
    if truncation.truncated {
        state.ensure_temp_file();
    }
    if let Some(mut file) = state.temp_file.take() {
        let _ = file.flush();
    }
    let cancelled = signal.as_ref().is_some_and(|s| s.is_cancelled());

    Ok(BashResult {
        output: if truncation.truncated {
            truncation.content
        } else {
            full_output
        },
        exit_code: if cancelled { None } else { exit_code },
        cancelled,
        truncated: truncation.truncated,
        full_output_path: state.temp_file_path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_decoder_handles_split_sequences() {
        let mut decoder = Utf8StreamDecoder::default();
        let bytes = "héllo".as_bytes();
        let a = decoder.decode(&bytes[..2], true);
        let b = decoder.decode(&bytes[2..], true);
        assert_eq!(format!("{a}{b}"), "héllo");
        assert_eq!(decoder.decode(&[0xff], true), "\u{FFFD}");
        assert_eq!(decoder.decode(&[0xc3], true), "");
        assert_eq!(decoder.decode(&[], false), "\u{FFFD}");
    }
}
