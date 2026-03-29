//! Wire protocol for communication between `libenvproxy.so` and `envproxy-agent`.
//!
//! Uses a compact binary format optimized for low-latency Unix socket communication.
//! The protocol is intentionally simple to avoid pulling in serialization dependencies
//! in the `LD_PRELOAD` library (which must remain minimal).
//!
//! # Wire Format
//!
//! ## Request (v1)
//! ```text
//! [1 byte: version=1] [2 bytes: key_len (BE)] [key_len bytes: key]
//! ```
//!
//! ## Request (v2 — includes current env var value for `vault:` resolution)
//! ```text
//! [1 byte: version=2] [2 bytes: key_len (BE)] [key_len bytes: key] [2 bytes: val_len (BE)] [val_len bytes: value]
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

/// Protocol version 1 (key only).
pub const PROTOCOL_V1: u8 = 1;

/// Protocol version 2 (key + current env var value for `vault:` prefix resolution).
pub const PROTOCOL_V2: u8 = 2;

/// Current protocol version used by the `.so` library.
pub const PROTOCOL_VERSION: u8 = PROTOCOL_V2;

/// Prefix for Vault secret references in env var values.
pub const VAULT_PREFIX: &str = "vault:";

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

/// Encode a v1 request (key only) into the wire format.
///
/// Returns `None` if the key exceeds [`MAX_KEY_LEN`].
#[must_use]
pub fn encode_request(key: &[u8]) -> Option<Vec<u8>> {
    encode_request_versioned(PROTOCOL_V1, key, &[])
}

/// Encode a v2 request (key + current env var value) into the wire format.
///
/// Used when the real env var value starts with `vault:` so the agent
/// can parse the Vault path and resolve the secret.
///
/// Returns `None` if the key or value exceeds their maximum lengths.
#[must_use]
pub fn encode_request_v2(key: &[u8], value: &[u8]) -> Option<Vec<u8>> {
    encode_request_versioned(PROTOCOL_V2, key, value)
}

/// Internal: encode a request with a specific version.
fn encode_request_versioned(version: u8, key: &[u8], value: &[u8]) -> Option<Vec<u8>> {
    let key_len = key.len();
    if key_len > MAX_KEY_LEN {
        return None;
    }

    #[expect(clippy::cast_possible_truncation, reason = "guarded by MAX_KEY_LEN check above")]
    let key_len_u16 = key_len as u16;

    if version == PROTOCOL_V1 {
        let mut buf = Vec::with_capacity(1 + 2 + key_len);
        buf.push(version);
        buf.extend_from_slice(&key_len_u16.to_be_bytes());
        buf.extend_from_slice(key);
        return Some(buf);
    }

    // v2: key + value
    let val_len = value.len();
    if val_len > MAX_VALUE_LEN {
        return None;
    }

    #[expect(clippy::cast_possible_truncation, reason = "guarded by MAX_VALUE_LEN check above")]
    let val_len_u16 = val_len as u16;
    let mut buf = Vec::with_capacity(1 + 2 + key_len + 2 + val_len);
    buf.push(version);
    buf.extend_from_slice(&key_len_u16.to_be_bytes());
    buf.extend_from_slice(key);
    buf.extend_from_slice(&val_len_u16.to_be_bytes());
    buf.extend_from_slice(value);
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
    /// The current env var value (v2 only). If the value starts with `vault:`,
    /// the agent should resolve the secret from Vault instead of the default backend.
    pub value: Option<Vec<u8>>,
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
/// Supports both v1 (key only) and v2 (key + value) formats.
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

    let key = buf[3..3 + key_len].to_vec();

    // v1: key only
    if version == PROTOCOL_V1 {
        return Ok(DecodedRequest { version, key, value: None });
    }

    // v2: key + value
    let val_offset = 3 + key_len;
    if buf.len() < val_offset + 2 {
        return Err(DecodeError::Incomplete);
    }

    let val_len = u16::from_be_bytes([buf[val_offset], buf[val_offset + 1]]) as usize;

    if buf.len() < val_offset + 2 + val_len {
        return Err(DecodeError::Incomplete);
    }

    let value = buf[val_offset + 2..val_offset + 2 + val_len].to_vec();

    Ok(DecodedRequest { version, key, value: if value.is_empty() { None } else { Some(value) } })
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

/// Parse a `vault:` prefixed value into mount, path, key, and optional version.
///
/// Format: `vault:<mount>/data/<path>#<key>` or `vault:<mount>/data/<path>#<key>#<version>`
///
/// Examples:
/// - `vault:secret/data/myapp/config#DATABASE_URL` → mount=`secret`, path=`myapp/config`, key=`DATABASE_URL`
/// - `vault:secret/data/myapp/config#DATABASE_URL#3` → same, version=3
///
/// Returns `None` if the format is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRef {
    /// Vault mount path (e.g., `secret`).
    pub mount: String,
    /// Secret path within the mount (e.g., `myapp/config`).
    pub path: String,
    /// Key within the secret's data map (e.g., `DATABASE_URL`).
    pub key: String,
    /// Optional secret version (0 = latest).
    pub version: Option<u64>,
}

/// Parse a `vault:` prefixed string into its components.
///
/// Returns `None` if the string doesn't start with `vault:` or is malformed.
#[must_use]
pub fn parse_vault_ref(value: &str) -> Option<VaultRef> {
    let rest = value.strip_prefix(VAULT_PREFIX)?;

    // Split on '#' to get path and key (and optional version)
    let hash_pos = rest.find('#')?;
    let full_path = &rest[..hash_pos];
    let after_hash = &rest[hash_pos + 1..];

    if after_hash.is_empty() {
        return None;
    }

    // Parse key and optional version: "KEY" or "KEY#3"
    let (key, version) = if let Some(ver_pos) = after_hash.find('#') {
        let k = &after_hash[..ver_pos];
        let v = after_hash[ver_pos + 1..].parse::<u64>().ok()?;
        (k.to_owned(), Some(v))
    } else {
        (after_hash.to_owned(), None)
    };

    // Parse mount and path from full_path.
    // Format: "secret/data/myapp/config" → mount="secret", path="myapp/config"
    // The "/data/" segment is part of the KV v2 API path, strip it.
    let (mount, path) = if let Some(data_pos) = full_path.find("/data/") {
        let m = &full_path[..data_pos];
        let p = &full_path[data_pos + 6..]; // skip "/data/"
        (m.to_owned(), p.to_owned())
    } else {
        // No /data/ segment — treat first segment as mount, rest as path
        let first_slash = full_path.find('/')?;
        let m = &full_path[..first_slash];
        let p = &full_path[first_slash + 1..];
        (m.to_owned(), p.to_owned())
    };

    if mount.is_empty() || path.is_empty() || key.is_empty() {
        return None;
    }

    Some(VaultRef { mount, path, key, version })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── v1 request tests ─────────────────────────────────────────────

    #[test]
    fn v1_request_roundtrip_should_preserve_key() {
        let key = b"DATABASE_URL";
        let encoded = encode_request(key).expect("encoding should succeed");
        let decoded = decode_request(&encoded).expect("decoding should succeed");

        assert_eq!(decoded.version, PROTOCOL_V1);
        assert_eq!(decoded.key, key);
        assert_eq!(decoded.value, None);
    }

    // ── v2 request tests ─────────────────────────────────────────────

    #[test]
    fn v2_request_roundtrip_should_preserve_key_and_value() {
        let key = b"DATABASE_URL";
        let value = b"vault:secret/data/myapp/config#DATABASE_URL";
        let encoded = encode_request_v2(key, value).expect("encoding should succeed");
        let decoded = decode_request(&encoded).expect("decoding should succeed");

        assert_eq!(decoded.version, PROTOCOL_V2);
        assert_eq!(decoded.key, key);
        assert_eq!(decoded.value, Some(value.to_vec()));
    }

    #[test]
    fn v2_request_with_empty_value_should_decode_as_none() {
        let key = b"PLAIN_VAR";
        let encoded = encode_request_v2(key, b"").expect("encoding should succeed");
        let decoded = decode_request(&encoded).expect("decoding should succeed");

        assert_eq!(decoded.version, PROTOCOL_V2);
        assert_eq!(decoded.key, key);
        assert_eq!(decoded.value, None);
    }

    // ── response tests ───────────────────────────────────────────────

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

    // ── error cases ──────────────────────────────────────────────────

    #[test]
    fn decode_request_should_fail_on_incomplete_header() {
        let result = decode_request(&[0x01]);
        assert_eq!(result, Err(DecodeError::Incomplete));
    }

    #[test]
    fn decode_request_should_fail_on_truncated_key() {
        let buf = [PROTOCOL_V1, 0x00, 0x05, b'A', b'B'];
        let result = decode_request(&buf);
        assert_eq!(result, Err(DecodeError::Incomplete));
    }

    #[test]
    fn decode_request_v2_should_fail_on_truncated_value() {
        // v2 header with key "AB" but value length header missing
        let buf = [PROTOCOL_V2, 0x00, 0x02, b'A', b'B'];
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

    #[test]
    fn encode_request_v2_should_reject_oversized_value() {
        let key = b"KEY";
        let value = vec![b'B'; MAX_VALUE_LEN + 1];
        assert!(encode_request_v2(key, &value).is_none());
    }

    // ── vault ref parsing ────────────────────────────────────────────

    #[test]
    fn parse_vault_ref_with_data_segment() {
        let r =
            parse_vault_ref("vault:secret/data/myapp/config#DATABASE_URL").expect("should parse");
        assert_eq!(r.mount, "secret");
        assert_eq!(r.path, "myapp/config");
        assert_eq!(r.key, "DATABASE_URL");
        assert_eq!(r.version, None);
    }

    #[test]
    fn parse_vault_ref_with_version() {
        let r =
            parse_vault_ref("vault:secret/data/myapp/config#DATABASE_URL#3").expect("should parse");
        assert_eq!(r.mount, "secret");
        assert_eq!(r.path, "myapp/config");
        assert_eq!(r.key, "DATABASE_URL");
        assert_eq!(r.version, Some(3));
    }

    #[test]
    fn parse_vault_ref_without_data_segment() {
        let r = parse_vault_ref("vault:secret/myapp/config#DATABASE_URL").expect("should parse");
        assert_eq!(r.mount, "secret");
        assert_eq!(r.path, "myapp/config");
        assert_eq!(r.key, "DATABASE_URL");
    }

    #[test]
    fn parse_vault_ref_nested_path() {
        let r = parse_vault_ref("vault:secret/data/team/prod/db#password").expect("should parse");
        assert_eq!(r.mount, "secret");
        assert_eq!(r.path, "team/prod/db");
        assert_eq!(r.key, "password");
    }

    #[test]
    fn parse_vault_ref_should_reject_no_prefix() {
        assert!(parse_vault_ref("secret/data/myapp#key").is_none());
    }

    #[test]
    fn parse_vault_ref_should_reject_no_key() {
        assert!(parse_vault_ref("vault:secret/data/myapp/config").is_none());
    }

    #[test]
    fn parse_vault_ref_should_reject_empty_key() {
        assert!(parse_vault_ref("vault:secret/data/myapp/config#").is_none());
    }

    #[test]
    fn parse_vault_ref_should_reject_no_path() {
        assert!(parse_vault_ref("vault:#key").is_none());
    }
}
