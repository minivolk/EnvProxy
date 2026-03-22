//! Wire protocol for communication between `libenvproxy.so` and `envproxy-agent`.
//!
//! Uses a compact binary format optimized for low-latency Unix socket communication.
//! The protocol is intentionally simple to avoid pulling in serialization dependencies
//! in the `LD_PRELOAD` library (which must remain minimal).
//!
//! # Wire Format
//!
//! ## Request
//! ```text
//! [1 byte: version] [2 bytes: key_len (big-endian)] [key_len bytes: key]
//! ```
//!
//! ## Response
//! ```text
//! [1 byte: status] [2 bytes: val_len (big-endian)] [val_len bytes: value]
//! ```
//!
//! ## Status Codes
//! - `0x00` — Found: value follows
//! - `0x01` — Not found: the key does not exist in any backend
//! - `0x02` — Error: an error message follows in the value field
//! - `0x03` — Passthrough: the caller should fall back to the real `getenv`

/// Current protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum key length (64 KiB should be more than enough for any env var name).
pub const MAX_KEY_LEN: usize = u16::MAX as usize;

/// Maximum value length (64 KiB).
pub const MAX_VALUE_LEN: usize = u16::MAX as usize;

/// Default socket path for the agent.
pub const DEFAULT_SOCKET_PATH: &str = "/tmp/envproxy/agent.sock";

/// Response status codes from the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    /// Key was found; value follows.
    Found = 0x00,
    /// Key was not found in any backend.
    NotFound = 0x01,
    /// An error occurred; error message follows in the value field.
    Error = 0x02,
    /// The caller should fall back to the real `getenv`.
    Passthrough = 0x03,
}

impl Status {
    /// Parse a status byte into a `Status` variant.
    #[must_use]
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Found),
            0x01 => Some(Self::NotFound),
            0x02 => Some(Self::Error),
            0x03 => Some(Self::Passthrough),
            _ => None,
        }
    }
}

/// Encode a request into the wire format.
///
/// Returns `None` if the key exceeds [`MAX_KEY_LEN`].
#[must_use]
pub fn encode_request(key: &[u8]) -> Option<Vec<u8>> {
    let key_len = key.len();
    if key_len > MAX_KEY_LEN {
        return None;
    }

    // We already checked key_len <= MAX_KEY_LEN (u16::MAX), so this cast is safe.
    #[expect(clippy::cast_possible_truncation, reason = "guarded by MAX_KEY_LEN check above")]
    let key_len_u16 = key_len as u16;
    let mut buf = Vec::with_capacity(1 + 2 + key_len);
    buf.push(PROTOCOL_VERSION);
    buf.extend_from_slice(&key_len_u16.to_be_bytes());
    buf.extend_from_slice(key);
    Some(buf)
}

/// Encode a response into the wire format.
///
/// Returns `None` if the value exceeds [`MAX_VALUE_LEN`].
#[must_use]
pub fn encode_response(status: Status, value: &[u8]) -> Option<Vec<u8>> {
    let val_len = value.len();
    if val_len > MAX_VALUE_LEN {
        return None;
    }

    // We already checked val_len <= MAX_VALUE_LEN (u16::MAX), so this cast is safe.
    #[expect(clippy::cast_possible_truncation, reason = "guarded by MAX_VALUE_LEN check above")]
    let val_len_u16 = val_len as u16;
    let mut buf = Vec::with_capacity(1 + 2 + val_len);
    buf.push(status as u8);
    buf.extend_from_slice(&val_len_u16.to_be_bytes());
    buf.extend_from_slice(value);
    Some(buf)
}

/// Decoded request from the wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedRequest {
    /// Protocol version from the client.
    pub version: u8,
    /// The environment variable key being requested.
    pub key: Vec<u8>,
}

/// Decoded response from the wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedResponse {
    /// Status of the lookup.
    pub status: Status,
    /// The value (or error message, depending on status).
    pub value: Vec<u8>,
}

/// Errors that can occur during protocol decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Not enough bytes to decode the header.
    Incomplete,
    /// The status byte is not a recognized value.
    InvalidStatus(u8),
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Incomplete => write!(f, "incomplete message: not enough bytes"),
            Self::InvalidStatus(b) => write!(f, "invalid status byte: {b:#04x}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decode a request from a byte buffer.
///
/// # Errors
///
/// Returns [`DecodeError::Incomplete`] if the buffer is too short.
pub fn decode_request(buf: &[u8]) -> Result<DecodedRequest, DecodeError> {
    if buf.len() < 3 {
        return Err(DecodeError::Incomplete);
    }

    let version = buf[0];
    let key_len = u16::from_be_bytes([buf[1], buf[2]]) as usize;

    if buf.len() < 3 + key_len {
        return Err(DecodeError::Incomplete);
    }

    Ok(DecodedRequest { version, key: buf[3..3 + key_len].to_vec() })
}

/// Decode a response from a byte buffer.
///
/// # Errors
///
/// Returns [`DecodeError::Incomplete`] if the buffer is too short, or
/// [`DecodeError::InvalidStatus`] if the status byte is unrecognized.
pub fn decode_response(buf: &[u8]) -> Result<DecodedResponse, DecodeError> {
    if buf.len() < 3 {
        return Err(DecodeError::Incomplete);
    }

    let status = Status::from_byte(buf[0]).ok_or(DecodeError::InvalidStatus(buf[0]))?;
    let val_len = u16::from_be_bytes([buf[1], buf[2]]) as usize;

    if buf.len() < 3 + val_len {
        return Err(DecodeError::Incomplete);
    }

    Ok(DecodedResponse { status, value: buf[3..3 + val_len].to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_should_preserve_key() {
        let key = b"DATABASE_URL";
        let encoded = encode_request(key).expect("encoding should succeed");
        let decoded = decode_request(&encoded).expect("decoding should succeed");

        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.key, key);
    }

    #[test]
    fn response_roundtrip_should_preserve_value() {
        let value = b"postgres://localhost/mydb";
        let encoded = encode_response(Status::Found, value).expect("encoding should succeed");
        let decoded = decode_response(&encoded).expect("decoding should succeed");

        assert_eq!(decoded.status, Status::Found);
        assert_eq!(decoded.value, value);
    }

    #[test]
    fn response_not_found_should_have_empty_value() {
        let encoded = encode_response(Status::NotFound, b"").expect("encoding should succeed");
        let decoded = decode_response(&encoded).expect("decoding should succeed");

        assert_eq!(decoded.status, Status::NotFound);
        assert!(decoded.value.is_empty());
    }

    #[test]
    fn decode_request_should_fail_on_incomplete_header() {
        let result = decode_request(&[0x01]);
        assert_eq!(result, Err(DecodeError::Incomplete));
    }

    #[test]
    fn decode_request_should_fail_on_truncated_key() {
        // Header says key is 5 bytes, but only 2 follow
        let buf = [PROTOCOL_VERSION, 0x00, 0x05, b'A', b'B'];
        let result = decode_request(&buf);
        assert_eq!(result, Err(DecodeError::Incomplete));
    }

    #[test]
    fn decode_response_should_fail_on_invalid_status() {
        let buf = [0xFF, 0x00, 0x00];
        let result = decode_response(&buf);
        assert_eq!(result, Err(DecodeError::InvalidStatus(0xFF)));
    }

    #[test]
    fn encode_request_should_reject_oversized_key() {
        let key = vec![b'A'; MAX_KEY_LEN + 1];
        assert!(encode_request(&key).is_none());
    }

    #[test]
    fn encode_response_should_reject_oversized_value() {
        let value = vec![b'B'; MAX_VALUE_LEN + 1];
        assert!(encode_response(Status::Found, &value).is_none());
    }
}
