//! Time-ordered UUIDv7 generation (port of `utils/uuid.ts`).

use parking_lot::Mutex;
use rand::RngCore;

const MAX_UUID_V7_TIMESTAMP: i64 = 0xffff_ffff_ffff;
const MAX_SEQUENCE: u64 = (1 << 41) - 1;

struct State {
    last_ordinary_timestamp: i64,
    sequence: Option<u64>,
}

static STATE: Mutex<State> = Mutex::new(State { last_ordinary_timestamp: -1, sequence: None });

#[derive(Debug, Clone, thiserror::Error)]
pub enum UuidError {
    #[error("UUIDv7 timestamp must be an integer between 0 and {MAX_UUID_V7_TIMESTAMP}")]
    InvalidTimestamp,
    #[error("UUIDv7 generator sequence exhausted")]
    SequenceExhausted,
}

/// Generate a time-ordered UUIDv7. A supplied timestamp is preserved for follower ids.
pub fn try_uuidv7(timestamp_ms: Option<i64>) -> Result<String, UuidError> {
    let requested = timestamp_ms.unwrap_or_else(super::now_ms);
    if !(0..=MAX_UUID_V7_TIMESTAMP).contains(&requested) {
        return Err(UuidError::InvalidTimestamp);
    }
    let mut state = STATE.lock();
    let effective = match timestamp_ms {
        None => requested.max(state.last_ordinary_timestamp),
        Some(t) => t,
    };
    if timestamp_ms.is_none() {
        state.last_ordinary_timestamp = effective;
    }
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let sequence = match state.sequence {
        None => {
            ((bytes[1] as u64) << 32)
                | ((bytes[2] as u64) << 24)
                | ((bytes[3] as u64) << 16)
                | ((bytes[4] as u64) << 8)
                | bytes[5] as u64
        }
        Some(s) => {
            if s == MAX_SEQUENCE {
                return Err(UuidError::SequenceExhausted);
            }
            s + 1
        }
    };
    state.sequence = Some(sequence);
    drop(state);

    let timestamp = effective as u64;
    for index in (0..=5).rev() {
        bytes[index] = ((timestamp >> ((5 - index) * 8)) & 0xff) as u8;
    }
    bytes[6] = 0x70 | ((sequence >> 37) & 0x0f) as u8;
    bytes[7] = ((sequence >> 29) & 0xff) as u8;
    bytes[8] = 0x80 | ((sequence >> 23) & 0x3f) as u8;
    bytes[9] = ((sequence >> 15) & 0xff) as u8;
    bytes[10] = ((sequence >> 7) & 0xff) as u8;
    bytes[11] = (((sequence & 0x7f) << 1) as u8) | (bytes[11] & 0x01);

    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        hex[0..4].concat(),
        hex[4..6].concat(),
        hex[6..8].concat(),
        hex[8..10].concat(),
        hex[10..].concat()
    ))
}

/// Generate a time-ordered UUIDv7 for the current time.
pub fn uuidv7() -> String {
    try_uuidv7(None).expect("current time is a valid UUIDv7 timestamp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_ordered_v7_ids() {
        let a = uuidv7();
        let b = uuidv7();
        assert!(a < b, "{a} < {b}");
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "7");
        let v: u8 = u8::from_str_radix(&a[19..20], 16).unwrap();
        assert_eq!(v & 0xc, 0x8);
    }

    #[test]
    fn preserves_supplied_timestamp() {
        let id = try_uuidv7(Some(0x0123_4567_89ab)).unwrap();
        assert!(id.starts_with("01234567-89ab-7"));
        assert!(try_uuidv7(Some(-1)).is_err());
    }
}
