//! AWS event stream binary framing (`application/vnd.amazon.eventstream`), as decoded by
//! `@smithy/eventstream-codec`.
//!
//! A message is: total length (u32), headers length (u32), prelude CRC32, headers, payload,
//! message CRC32. All integers are big-endian.

use std::collections::BTreeMap;

/// A decoded header value.
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderValue {
    Bool(bool),
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Bytes(Vec<u8>),
    String(String),
    Timestamp(i64),
    Uuid([u8; 16]),
}

impl HeaderValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            HeaderValue::String(s) => Some(s),
            _ => None,
        }
    }
}

/// One event stream message.
#[derive(Debug, Clone, PartialEq)]
pub struct EventMessage {
    pub headers: BTreeMap<String, HeaderValue>,
    pub body: Vec<u8>,
}

impl EventMessage {
    pub fn header_str(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(HeaderValue::as_str)
    }
}

const PRELUDE_LENGTH: usize = 8;
const MINIMUM_MESSAGE_LENGTH: usize = PRELUDE_LENGTH + 4 + 4;

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn decode_headers(bytes: &[u8]) -> anyhow::Result<BTreeMap<String, HeaderValue>> {
    let mut headers = BTreeMap::new();
    let mut position = 0;
    let need = |position: usize, n: usize| -> anyhow::Result<()> {
        if position + n > bytes.len() {
            anyhow::bail!("Malformed event stream headers");
        }
        Ok(())
    };
    while position < bytes.len() {
        need(position, 1)?;
        let name_length = bytes[position] as usize;
        position += 1;
        need(position, name_length + 1)?;
        let name = String::from_utf8_lossy(&bytes[position..position + name_length]).into_owned();
        position += name_length;
        let kind = bytes[position];
        position += 1;
        let value = match kind {
            0 => HeaderValue::Bool(true),
            1 => HeaderValue::Bool(false),
            2 => {
                need(position, 1)?;
                position += 1;
                HeaderValue::Byte(bytes[position - 1] as i8)
            }
            3 => {
                need(position, 2)?;
                position += 2;
                HeaderValue::Short(i16::from_be_bytes([bytes[position - 2], bytes[position - 1]]))
            }
            4 => {
                need(position, 4)?;
                position += 4;
                HeaderValue::Int(read_u32(bytes, position - 4) as i32)
            }
            5 | 8 => {
                need(position, 8)?;
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&bytes[position..position + 8]);
                position += 8;
                let value = i64::from_be_bytes(buf);
                if kind == 5 { HeaderValue::Long(value) } else { HeaderValue::Timestamp(value) }
            }
            6 | 7 => {
                need(position, 2)?;
                let length = u16::from_be_bytes([bytes[position], bytes[position + 1]]) as usize;
                position += 2;
                need(position, length)?;
                let data = bytes[position..position + length].to_vec();
                position += length;
                if kind == 6 { HeaderValue::Bytes(data) } else { HeaderValue::String(String::from_utf8_lossy(&data).into_owned()) }
            }
            9 => {
                need(position, 16)?;
                let mut uuid = [0u8; 16];
                uuid.copy_from_slice(&bytes[position..position + 16]);
                position += 16;
                HeaderValue::Uuid(uuid)
            }
            other => anyhow::bail!("Unrecognized header type tag: {other}"),
        };
        headers.insert(name, value);
    }
    Ok(headers)
}

/// Decode one complete message (`bytes` holds exactly one frame).
pub fn decode_message(bytes: &[u8]) -> anyhow::Result<EventMessage> {
    if bytes.len() < MINIMUM_MESSAGE_LENGTH {
        anyhow::bail!("Provided message too short to accommodate event stream message overhead");
    }
    let total_length = read_u32(bytes, 0) as usize;
    if total_length != bytes.len() {
        anyhow::bail!("Reported message length does not match received message length");
    }
    let headers_length = read_u32(bytes, 4) as usize;
    let prelude_crc = read_u32(bytes, PRELUDE_LENGTH);
    let message_crc = read_u32(bytes, total_length - 4);
    if prelude_crc != crc32fast::hash(&bytes[..PRELUDE_LENGTH]) {
        anyhow::bail!(
            "The prelude checksum specified in the message ({prelude_crc}) does not match the calculated CRC32 checksum ({})",
            crc32fast::hash(&bytes[..PRELUDE_LENGTH])
        );
    }
    let computed = crc32fast::hash(&bytes[..total_length - 4]);
    if message_crc != computed {
        anyhow::bail!("The message checksum ({computed}) did not match the expected value of {message_crc}");
    }
    let headers_start = PRELUDE_LENGTH + 4;
    let headers_end = headers_start + headers_length;
    if headers_end > total_length - 4 {
        anyhow::bail!("Malformed event stream headers");
    }
    Ok(EventMessage {
        headers: decode_headers(&bytes[headers_start..headers_end])?,
        body: bytes[headers_end..total_length - 4].to_vec(),
    })
}

/// Incremental decoder: feed bytes, receive complete messages.
#[derive(Debug, Default)]
pub struct EventStreamDecoder {
    buffer: Vec<u8>,
}

impl EventStreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> anyhow::Result<Vec<EventMessage>> {
        self.buffer.extend_from_slice(bytes);
        let mut messages = Vec::new();
        while self.buffer.len() >= 4 {
            let total_length = read_u32(&self.buffer, 0) as usize;
            if total_length < MINIMUM_MESSAGE_LENGTH {
                anyhow::bail!("Provided message too short to accommodate event stream message overhead");
            }
            if self.buffer.len() < total_length {
                break;
            }
            let frame: Vec<u8> = self.buffer.drain(..total_length).collect();
            messages.push(decode_message(&frame)?);
        }
        Ok(messages)
    }

    /// Bytes of an incomplete trailing frame.
    pub fn remaining(&self) -> usize {
        self.buffer.len()
    }
}

/// Encode a message (used by tests and fixtures). Supports string, bytes and bool headers.
pub fn encode_message(headers: &[(&str, HeaderValue)], body: &[u8]) -> Vec<u8> {
    let mut encoded_headers = Vec::new();
    for (name, value) in headers {
        encoded_headers.push(name.len() as u8);
        encoded_headers.extend_from_slice(name.as_bytes());
        match value {
            HeaderValue::Bool(true) => encoded_headers.push(0),
            HeaderValue::Bool(false) => encoded_headers.push(1),
            HeaderValue::Byte(v) => {
                encoded_headers.push(2);
                encoded_headers.push(*v as u8);
            }
            HeaderValue::Short(v) => {
                encoded_headers.push(3);
                encoded_headers.extend_from_slice(&v.to_be_bytes());
            }
            HeaderValue::Int(v) => {
                encoded_headers.push(4);
                encoded_headers.extend_from_slice(&v.to_be_bytes());
            }
            HeaderValue::Long(v) => {
                encoded_headers.push(5);
                encoded_headers.extend_from_slice(&v.to_be_bytes());
            }
            HeaderValue::Bytes(v) => {
                encoded_headers.push(6);
                encoded_headers.extend_from_slice(&(v.len() as u16).to_be_bytes());
                encoded_headers.extend_from_slice(v);
            }
            HeaderValue::String(v) => {
                encoded_headers.push(7);
                encoded_headers.extend_from_slice(&(v.len() as u16).to_be_bytes());
                encoded_headers.extend_from_slice(v.as_bytes());
            }
            HeaderValue::Timestamp(v) => {
                encoded_headers.push(8);
                encoded_headers.extend_from_slice(&v.to_be_bytes());
            }
            HeaderValue::Uuid(v) => {
                encoded_headers.push(9);
                encoded_headers.extend_from_slice(v);
            }
        }
    }
    let total = MINIMUM_MESSAGE_LENGTH + encoded_headers.len() + body.len();
    let mut message = Vec::with_capacity(total);
    message.extend_from_slice(&(total as u32).to_be_bytes());
    message.extend_from_slice(&(encoded_headers.len() as u32).to_be_bytes());
    let prelude_crc = crc32fast::hash(&message);
    message.extend_from_slice(&prelude_crc.to_be_bytes());
    message.extend_from_slice(&encoded_headers);
    message.extend_from_slice(body);
    let crc = crc32fast::hash(&message);
    message.extend_from_slice(&crc.to_be_bytes());
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_streams_across_chunks() {
        let frame = encode_message(
            &[(":message-type", HeaderValue::String("event".into())), (":event-type", HeaderValue::String("messageStart".into()))],
            br#"{"role":"assistant"}"#,
        );
        let mut decoder = EventStreamDecoder::new();
        let mut doubled = frame.clone();
        doubled.extend_from_slice(&frame);
        let (a, b) = doubled.split_at(7);
        assert!(decoder.push(a).unwrap().is_empty());
        let messages = decoder.push(b).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].header_str(":event-type"), Some("messageStart"));
        assert_eq!(messages[0].body, br#"{"role":"assistant"}"#);
        assert_eq!(decoder.remaining(), 0);
    }

    #[test]
    fn rejects_corrupted_frames() {
        let mut frame = encode_message(&[(":message-type", HeaderValue::String("event".into()))], b"{}");
        let last = frame.len() - 5;
        frame[last] ^= 0xff;
        assert!(decode_message(&frame).unwrap_err().to_string().contains("message checksum"));
    }
}
