//! Port of `packages/coding-agent/src/core/tools/output-accumulator.ts`.

use crate::core::tools::truncate::TruncationResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputAccumulatorOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_file_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSnapshot {
    pub content: String,
    pub truncation: TruncationResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullOutput {
    pub content: String,
    /// Whether `content` omits part of the output.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputAccumulatorSnapshotOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persist_if_truncated: Option<bool>,
}

/// Streaming UTF-8 decoder with WHATWG `TextDecoder` replacement semantics.
#[derive(Default)]
pub(crate) struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub(crate) fn decode(&mut self, data: &[u8], stream: bool) -> String {
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(data);
        let mut out = String::new();
        let mut pos = 0;
        while pos < bytes.len() {
            match std::str::from_utf8(&bytes[pos..]) {
                Ok(text) => {
                    out.push_str(text);
                    pos = bytes.len();
                }
                Err(err) => {
                    let valid = err.valid_up_to();
                    // SAFETY-free: the prefix was validated by from_utf8.
                    out.push_str(std::str::from_utf8(&bytes[pos..pos + valid]).unwrap_or_default());
                    match err.error_len() {
                        Some(len) => {
                            out.push('\u{FFFD}');
                            pos += valid + len;
                        }
                        None => {
                            if stream {
                                self.pending = bytes[pos + valid..].to_vec();
                            } else {
                                out.push('\u{FFFD}');
                            }
                            pos = bytes.len();
                        }
                    }
                }
            }
        }
        out
    }
}

/// `new TextDecoder().decode(bytes)`.
pub(crate) fn decode_utf8_lossy(bytes: &[u8]) -> String {
    Utf8StreamDecoder::default().decode(bytes, false)
}

/// Output files: `<tmpdir>/<prefix>-<random hex><extension>`, created exclusively with mode 0600
/// (TS `utils/output-files.ts:createOutputFileStream`).
pub(crate) fn create_output_file(prefix: &str, extension: &str) -> std::io::Result<(String, std::fs::File)> {
    let random = uuid::Uuid::new_v4().simple().to_string();
    let name = format!("{prefix}-{}{extension}", &random[..16]);
    let path = std::env::temp_dir().join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    Ok((path.to_string_lossy().into_owned(), file))
}

struct AccumulatorState {
    decoder: Utf8StreamDecoder,
    raw_chunks: Vec<Vec<u8>>,
    tail_text: String,
    tail_bytes: usize,
    tail_starts_at_line_boundary: bool,
    total_raw_bytes: i64,
    total_decoded_bytes: i64,
    completed_lines: i64,
    total_lines: i64,
    current_line_bytes: i64,
    has_open_line: bool,
    finished: bool,
    temp_file_path: Option<String>,
    temp_file: Option<std::fs::File>,
    temp_file_error: Option<String>,
}

/// Incrementally tracks streaming output with bounded memory.
///
/// Appends decode chunks with a streaming UTF-8 decoder, keeps only a decoded
/// tail for display snapshots, and opens a temp file when the full output needs
/// to be preserved.
pub struct OutputAccumulator {
    max_lines: i64,
    max_bytes: i64,
    max_rolling_bytes: usize,
    temp_file_prefix: String,
    state: parking_lot::Mutex<AccumulatorState>,
}

impl OutputAccumulator {
    pub fn new(options: Option<&OutputAccumulatorOptions>) -> Self {
        use crate::core::tools::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES};
        let max_lines = options.and_then(|o| o.max_lines).unwrap_or(DEFAULT_MAX_LINES);
        let max_bytes = options.and_then(|o| o.max_bytes).unwrap_or(DEFAULT_MAX_BYTES);
        Self {
            max_lines,
            max_bytes,
            max_rolling_bytes: (max_bytes * 2).max(1) as usize,
            temp_file_prefix: options
                .and_then(|o| o.temp_file_prefix.clone())
                .unwrap_or_else(|| "pi-output".to_string()),
            state: parking_lot::Mutex::new(AccumulatorState {
                decoder: Utf8StreamDecoder::default(),
                raw_chunks: Vec::new(),
                tail_text: String::new(),
                tail_bytes: 0,
                tail_starts_at_line_boundary: true,
                total_raw_bytes: 0,
                total_decoded_bytes: 0,
                completed_lines: 0,
                total_lines: 0,
                current_line_bytes: 0,
                has_open_line: false,
                finished: false,
                temp_file_path: None,
                temp_file: None,
                temp_file_error: None,
            }),
        }
    }

    pub fn append(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        if state.finished {
            anyhow::bail!("Cannot append to a finished output accumulator");
        }
        state.total_raw_bytes += data.len() as i64;
        let text = state.decoder.decode(data, true);
        self.append_decoded_text(&mut state, &text);

        if state.temp_file.is_some() || self.should_use_temp_file(&state) {
            self.ensure_temp_file(&mut state);
            if let Some(file) = state.temp_file.as_mut() {
                use std::io::Write;
                if let Err(err) = file.write_all(data) {
                    state.temp_file_error = Some(err.to_string());
                }
            }
        } else if !data.is_empty() {
            state.raw_chunks.push(data.to_vec());
        }
        Ok(())
    }

    pub fn finish(&self) {
        let mut state = self.state.lock();
        if state.finished {
            return;
        }
        state.finished = true;
        let text = state.decoder.decode(&[], false);
        self.append_decoded_text(&mut state, &text);
        if self.should_use_temp_file(&state) {
            self.ensure_temp_file(&mut state);
        }
    }

    pub fn snapshot(&self, options: Option<&OutputAccumulatorSnapshotOptions>) -> OutputSnapshot {
        use crate::core::tools::truncate::{TruncationOptions, TruncationResultTruncatedBy, truncate_tail};
        let mut state = self.state.lock();
        let snapshot_text = Self::get_snapshot_text(&state);
        let tail_truncation = truncate_tail(
            &snapshot_text,
            Some(&TruncationOptions {
                max_lines: Some(self.max_lines),
                max_bytes: Some(self.max_bytes),
            }),
        );
        let truncated = state.total_lines > self.max_lines || state.total_decoded_bytes > self.max_bytes;
        let truncated_by = if truncated {
            Some(tail_truncation.truncated_by.unwrap_or(if state.total_decoded_bytes > self.max_bytes {
                TruncationResultTruncatedBy::Bytes
            } else {
                TruncationResultTruncatedBy::Lines
            }))
        } else {
            None
        };
        let truncation = TruncationResult {
            truncated,
            truncated_by,
            total_lines: state.total_lines,
            total_bytes: state.total_decoded_bytes,
            max_lines: self.max_lines,
            max_bytes: self.max_bytes,
            ..tail_truncation
        };

        if options.and_then(|o| o.persist_if_truncated).unwrap_or(false) && truncation.truncated {
            self.ensure_temp_file(&mut state);
        }

        OutputSnapshot {
            content: truncation.content.clone(),
            truncation,
            full_output_path: state.temp_file_path.clone(),
        }
    }

    pub async fn close_temp_file(&self) -> anyhow::Result<()> {
        let (file, error) = {
            let mut state = self.state.lock();
            (state.temp_file.take(), state.temp_file_error.take())
        };
        if let Some(error) = error {
            anyhow::bail!(error);
        }
        if let Some(file) = file {
            let file = tokio::fs::File::from_std(file);
            let mut file = file;
            use tokio::io::AsyncWriteExt;
            file.flush().await?;
            file.sync_all().await.ok();
        }
        Ok(())
    }

    /// The complete output, for callers that can take more than the display snapshot. Call after
    /// `finish()` and `closeTempFile()`. Output longer than `maxBytes` raw bytes keeps its first and
    /// last `maxBytes / 2` bytes around an omission marker.
    pub async fn read_full_output(&self, max_bytes: i64) -> anyhow::Result<FullOutput> {
        let (path, chunks) = {
            let state = self.state.lock();
            (state.temp_file_path.clone(), state.raw_chunks.concat())
        };
        let Some(path) = path else {
            return Ok(FullOutput {
                content: decode_utf8_lossy(&chunks),
                truncated: false,
            });
        };
        let bytes = tokio::fs::read(&path).await?;
        let size = bytes.len();
        let max_bytes = max_bytes.max(0) as usize;
        if size <= max_bytes {
            return Ok(FullOutput {
                content: decode_utf8_lossy(&bytes),
                truncated: false,
            });
        }
        let head_bytes = max_bytes / 2;
        let tail_bytes = max_bytes - head_bytes;
        let head = &bytes[..head_bytes];
        let tail = &bytes[size - tail_bytes..];
        // Cut at character boundaries: streaming decode holds back an incomplete trailing sequence,
        // and the tail skips leading continuation bytes.
        let head_text = Utf8StreamDecoder::default().decode(head, true);
        let mut tail_start = 0;
        while tail_start < tail.len() && (tail[tail_start] & 0xc0) == 0x80 {
            tail_start += 1;
        }
        let tail_text = decode_utf8_lossy(&tail[tail_start..]);
        let omitted = size - head_bytes - tail_bytes;
        Ok(FullOutput {
            content: format!("{head_text}\n\n[... {omitted} bytes omitted ...]\n\n{tail_text}"),
            truncated: true,
        })
    }

    pub fn get_last_line_bytes(&self) -> i64 {
        self.state.lock().current_line_bytes
    }

    fn append_decoded_text(&self, state: &mut AccumulatorState, text: &str) {
        if text.is_empty() {
            return;
        }
        let bytes = text.len();
        state.total_decoded_bytes += bytes as i64;
        state.tail_text.push_str(text);
        state.tail_bytes += bytes;
        if state.tail_bytes > self.max_rolling_bytes * 2 {
            self.trim_tail(state);
        }

        let newlines = text.matches('\n').count() as i64;
        if newlines == 0 {
            state.current_line_bytes += bytes as i64;
            state.has_open_line = true;
        } else {
            state.completed_lines += newlines;
            let last_newline = text.rfind('\n').unwrap_or(0);
            let tail = &text[last_newline + 1..];
            state.current_line_bytes = tail.len() as i64;
            state.has_open_line = !tail.is_empty();
        }
        state.total_lines = state.completed_lines + i64::from(state.has_open_line);
    }

    fn trim_tail(&self, state: &mut AccumulatorState) {
        let len = state.tail_text.len();
        if len <= self.max_rolling_bytes {
            state.tail_bytes = len;
            return;
        }
        let mut start = len - self.max_rolling_bytes;
        while start < len && !state.tail_text.is_char_boundary(start) {
            start += 1;
        }
        state.tail_starts_at_line_boundary = if start == 0 {
            state.tail_starts_at_line_boundary
        } else {
            state.tail_text.as_bytes()[start - 1] == b'\n'
        };
        state.tail_text = state.tail_text[start..].to_string();
        state.tail_bytes = state.tail_text.len();
    }

    fn get_snapshot_text(state: &AccumulatorState) -> String {
        if state.tail_starts_at_line_boundary {
            return state.tail_text.clone();
        }
        match state.tail_text.find('\n') {
            None => state.tail_text.clone(),
            Some(first_newline) => state.tail_text[first_newline + 1..].to_string(),
        }
    }

    fn should_use_temp_file(&self, state: &AccumulatorState) -> bool {
        state.total_raw_bytes > self.max_bytes
            || state.total_decoded_bytes > self.max_bytes
            || state.total_lines > self.max_lines
    }

    fn ensure_temp_file(&self, state: &mut AccumulatorState) {
        if state.temp_file_path.is_some() {
            return;
        }
        match create_output_file(&self.temp_file_prefix, ".log") {
            Ok((path, mut file)) => {
                use std::io::Write;
                for chunk in std::mem::take(&mut state.raw_chunks) {
                    if let Err(err) = file.write_all(&chunk) {
                        state.temp_file_error = Some(err.to_string());
                    }
                }
                state.temp_file_path = Some(path);
                state.temp_file = Some(file);
            }
            Err(err) => {
                state.temp_file_error = Some(err.to_string());
            }
        }
    }
}
