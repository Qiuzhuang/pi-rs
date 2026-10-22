//! Port of `packages/coding-agent/src/modes/rpc/jsonl.ts`.

use std::sync::Arc;

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Serialize a single strict JSONL record.
///
/// Framing is LF-only. Payload strings may contain other Unicode separators such as
/// U+2028 and U+2029. Clients must split records on `\n` only.
pub fn serialize_json_line<T: Serialize + ?Sized>(value: &T) -> String {
    let mut line = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    line.push('\n');
    line
}

/// Incremental LF-only line splitter (the buffering part of `attachJsonlLineReader`).
///
/// This intentionally does not split on other Unicode separators: those are valid inside
/// JSON strings, so splitting on them would not implement strict JSONL framing. A trailing
/// `\r` is removed from each line.
#[derive(Debug, Default)]
pub struct JsonlLineSplitter {
    buffer: Vec<u8>,
}

impl JsonlLineSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    fn emit(line: &[u8]) -> String {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        String::from_utf8_lossy(line).into_owned()
    }

    /// Feed a chunk and return the complete lines it finished.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut lines = Vec::new();
        let mut start = 0;
        while let Some(offset) = self.buffer[start..].iter().position(|b| *b == b'\n') {
            let end = start + offset;
            lines.push(Self::emit(&self.buffer[start..end]));
            start = end + 1;
        }
        self.buffer.drain(..start);
        lines
    }

    /// End of stream: the final line without a trailing LF, if any.
    pub fn finish(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }
        let line = Self::emit(&self.buffer);
        self.buffer.clear();
        Some(line)
    }
}

/// Reads LF-delimited lines from an async stream.
pub struct JsonlLineReader<R> {
    stream: R,
    splitter: JsonlLineSplitter,
    pending: std::collections::VecDeque<String>,
    ended: bool,
}

impl<R: AsyncRead + Unpin> JsonlLineReader<R> {
    pub fn new(stream: R) -> Self {
        Self { stream, splitter: JsonlLineSplitter::new(), pending: Default::default(), ended: false }
    }

    /// The next line, or `None` at end of stream.
    pub async fn next_line(&mut self) -> std::io::Result<Option<String>> {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            if let Some(line) = self.pending.pop_front() {
                return Ok(Some(line));
            }
            if self.ended {
                return Ok(None);
            }
            let n = self.stream.read(&mut buf).await?;
            if n == 0 {
                self.ended = true;
                if let Some(line) = self.splitter.finish() {
                    self.pending.push_back(line);
                }
                continue;
            }
            self.pending.extend(self.splitter.push(&buf[..n]));
        }
    }
}

/// Attach an LF-only JSONL reader to a stream: a background task calls `on_line` for every
/// line, then `on_end` (if given) at end of stream. Returns a function that detaches the reader.
///
/// This intentionally does not use a generic line reader that splits on additional Unicode
/// separators that are valid inside JSON strings.
pub fn attach_jsonl_line_reader<R>(
    stream: R,
    on_line: Arc<dyn Fn(String) + Send + Sync>,
    on_end: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Arc<dyn Fn() + Send + Sync>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let token = tokio_util::sync::CancellationToken::new();
    let task_token = token.clone();
    tokio::spawn(async move {
        let mut reader = JsonlLineReader::new(stream);
        loop {
            let line = tokio::select! {
                _ = task_token.cancelled() => return,
                line = reader.next_line() => line,
            };
            match line {
                Ok(Some(line)) => on_line(line),
                _ => break,
            }
        }
        if !task_token.is_cancelled()
            && let Some(on_end) = on_end
        {
            on_end();
        }
    });
    Arc::new(move || token.cancel())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use serde_json::json;

    async fn read_all(data: &[u8]) -> Vec<String> {
        let mut reader = JsonlLineReader::new(std::io::Cursor::new(data.to_vec()));
        let mut lines = Vec::new();
        while let Some(line) = reader.next_line().await.unwrap() {
            lines.push(line);
        }
        lines
    }

    #[test]
    fn serializes_strict_jsonl_records_without_escaping_unicode_separators() {
        let line = serialize_json_line(&json!({"text": "a\u{2028}b\u{2029}c"}));
        assert!(line.contains("a\u{2028}b\u{2029}c"));
        assert!(line.ends_with('\n'));
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed, json!({"text": "a\u{2028}b\u{2029}c"}));
    }

    #[tokio::test]
    async fn splits_on_lf_only_and_preserves_separators_inside_payloads() {
        let lines = read_all(serialize_json_line(&json!({"text": "a\u{2028}b\u{2029}c"})).as_bytes()).await;
        assert_eq!(lines.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(parsed, json!({"text": "a\u{2028}b\u{2029}c"}));
    }

    #[tokio::test]
    async fn handles_crlf_delimited_input() {
        assert_eq!(read_all(b"{\"a\":1}\r\n{\"b\":2}\r\n").await, vec!["{\"a\":1}", "{\"b\":2}"]);
    }

    #[tokio::test]
    async fn emits_a_final_line_without_trailing_lf() {
        assert_eq!(read_all(b"{\"a\":1}").await, vec!["{\"a\":1}"]);
    }

    #[test]
    fn splits_multibyte_characters_across_chunks() {
        let mut splitter = JsonlLineSplitter::new();
        let bytes = "é\n".as_bytes();
        assert!(splitter.push(&bytes[..1]).is_empty());
        assert_eq!(splitter.push(&bytes[1..]), vec!["é".to_string()]);
        assert_eq!(splitter.finish(), None);
    }

    #[tokio::test]
    async fn attached_reader_reports_lines_and_end() {
        let lines: Arc<Mutex<Vec<String>>> = Arc::default();
        let ended = Arc::new(tokio::sync::Notify::new());
        let l = lines.clone();
        let e = ended.clone();
        let _detach = attach_jsonl_line_reader(
            std::io::Cursor::new(b"a\nb".to_vec()),
            Arc::new(move |line| l.lock().push(line)),
            Some(Arc::new(move || e.notify_one())),
        );
        ended.notified().await;
        assert_eq!(*lines.lock(), vec!["a".to_string(), "b".to_string()]);
    }
}
