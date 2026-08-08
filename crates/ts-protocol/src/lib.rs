//! Versioned transport-neutral JSON-RPC contracts and bounded framing.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fmt,
    io::{self, Read, Write},
};
use token_shrinker_types::{ProtocolVersion, RequestId};

/// JSON-RPC request with explicit protocol, authentication, and deadline metadata.
#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcRequest {
    /// Must equal `2.0`.
    pub jsonrpc: String,
    /// Correlation identifier.
    pub id: RequestId,
    /// Client protocol generation.
    pub protocol_version: ProtocolVersion,
    /// Ephemeral discovery token proving access to the user-only state file.
    pub auth_token: String,
    /// Stable method name.
    pub method: String,
    /// Method parameters.
    pub params: Value,
    /// Optional Unix deadline in milliseconds.
    pub deadline_unix_ms: Option<i64>,
}

impl fmt::Debug for RpcRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RpcRequest")
            .field("jsonrpc", &self.jsonrpc)
            .field("id", &self.id)
            .field("protocol_version", &self.protocol_version)
            .field("auth_token", &"[REDACTED]")
            .field("method", &self.method)
            .field("params", &self.params)
            .field("deadline_unix_ms", &self.deadline_unix_ms)
            .finish()
    }
}

impl RpcRequest {
    /// Validates the transport-neutral request envelope.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] for invalid JSON-RPC, method, auth, or major version.
    pub fn validate(&self, expected_auth: &str) -> Result<(), ProtocolError> {
        if self.jsonrpc != "2.0" {
            return Err(ProtocolError::InvalidJsonRpc);
        }
        if !self
            .protocol_version
            .is_compatible_with(ProtocolVersion::CURRENT)
        {
            return Err(ProtocolError::IncompatibleVersion(self.protocol_version));
        }
        if self.auth_token.len() != 64
            || !self
                .auth_token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || !constant_time_eq(self.auth_token.as_bytes(), expected_auth.as_bytes())
        {
            return Err(ProtocolError::Authentication);
        }
        if self.method.is_empty()
            || self.method.len() > 128
            || !self
                .method
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte))
        {
            return Err(ProtocolError::InvalidMethod);
        }
        Ok(())
    }
}

/// JSON-RPC error object with stable code and content-free data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcError {
    /// Stable numeric JSON-RPC/application code.
    pub code: i32,
    /// Stable human-readable category.
    pub message: String,
    /// Optional stable machine code; never source content.
    pub data_code: Option<String>,
}

/// JSON-RPC response containing exactly one result or error.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcResponse {
    /// Always `2.0`.
    pub jsonrpc: String,
    /// Correlation identifier.
    pub id: RequestId,
    /// Server protocol version.
    pub protocol_version: ProtocolVersion,
    /// Successful payload.
    pub result: Option<Value>,
    /// Structured failure.
    pub error: Option<RpcError>,
}

impl RpcResponse {
    /// Builds a successful response.
    #[must_use]
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            protocol_version: ProtocolVersion::CURRENT,
            result: Some(result),
            error: None,
        }
    }

    /// Builds a structured error response.
    #[must_use]
    pub fn failure(
        id: RequestId,
        code: i32,
        message: impl Into<String>,
        data_code: Option<String>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            protocol_version: ProtocolVersion::CURRENT,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data_code,
            }),
        }
    }

    /// Checks the exactly-one-of result/error invariant.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.result.is_some() != self.error.is_some()
    }
}

/// Writes one length-prefixed JSON frame.
///
/// # Errors
///
/// Returns [`ProtocolError`] when serialization, bounds, or I/O fails.
pub fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
    max_frame_bytes: u32,
) -> Result<(), ProtocolError> {
    let payload = serde_json::to_vec(value).map_err(ProtocolError::Json)?;
    let length =
        u32::try_from(payload.len()).map_err(|_| ProtocolError::FrameTooLarge(u64::MAX))?;
    if length == 0 || length > max_frame_bytes {
        return Err(ProtocolError::FrameTooLarge(u64::from(length)));
    }
    writer
        .write_all(&length.to_be_bytes())
        .map_err(ProtocolError::Io)?;
    writer.write_all(&payload).map_err(ProtocolError::Io)?;
    writer.flush().map_err(ProtocolError::Io)
}

/// Reads one bounded length-prefixed JSON frame.
///
/// # Errors
///
/// Returns [`ProtocolError`] for EOF, zero/oversized frames, malformed JSON, or I/O failure.
pub fn read_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut impl Read,
    max_frame_bytes: u32,
) -> Result<T, ProtocolError> {
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header).map_err(ProtocolError::Io)?;
    let length = u32::from_be_bytes(header);
    if length == 0 || length > max_frame_bytes {
        return Err(ProtocolError::FrameTooLarge(u64::from(length)));
    }
    let allocation =
        usize::try_from(length).map_err(|_| ProtocolError::FrameTooLarge(u64::from(length)))?;
    let mut payload = vec![0; allocation];
    reader.read_exact(&mut payload).map_err(ProtocolError::Io)?;
    serde_json::from_slice(&payload).map_err(ProtocolError::Json)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

/// Protocol validation or framing failure.
#[derive(Debug)]
pub enum ProtocolError {
    /// JSON-RPC version was not 2.0.
    InvalidJsonRpc,
    /// Protocol major versions differ.
    IncompatibleVersion(ProtocolVersion),
    /// Authentication token was invalid.
    Authentication,
    /// Method name was malformed.
    InvalidMethod,
    /// Frame was empty or over the configured cap.
    FrameTooLarge(u64),
    /// JSON serialization or decoding failed.
    Json(serde_json::Error),
    /// Framing I/O failed.
    Io(io::Error),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJsonRpc => formatter.write_str("JSON-RPC version must be 2.0"),
            Self::IncompatibleVersion(version) => {
                write!(formatter, "incompatible protocol version {version}")
            }
            Self::Authentication => formatter.write_str("IPC authentication failed"),
            Self::InvalidMethod => formatter.write_str("invalid JSON-RPC method"),
            Self::FrameTooLarge(size) => {
                write!(formatter, "invalid or oversized frame: {size} bytes")
            }
            Self::Json(error) => write!(formatter, "JSON frame failed: {error}"),
            Self::Io(error) => write!(formatter, "frame I/O failed: {error}"),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RpcRequest {
        RpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: RequestId::new("frame-1").expect("ID"),
            protocol_version: ProtocolVersion::CURRENT,
            auth_token: "a".repeat(64),
            method: "health".to_owned(),
            params: Value::Null,
            deadline_unix_ms: None,
        }
    }

    #[test]
    fn frame_round_trip_and_envelope_validation() {
        let request = request();
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request, 4096).expect("write frame");
        let decoded: RpcRequest = read_frame(&mut bytes.as_slice(), 4096).expect("read frame");
        assert_eq!(decoded, request);
        decoded.validate(&"a".repeat(64)).expect("valid request");
    }

    #[test]
    fn oversized_header_is_rejected_before_payload_allocation() {
        let bytes = u32::MAX.to_be_bytes();
        assert!(matches!(
            read_frame::<RpcRequest>(&mut bytes.as_slice(), 1024),
            Err(ProtocolError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn malformed_frames_never_panic() {
        let mut state = 0x1234_5678_u32;
        for length in 0..512_usize {
            let mut bytes = vec![0_u8; length];
            for byte in &mut bytes {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *byte = state.to_le_bytes()[0];
            }
            let _ = read_frame::<RpcRequest>(&mut bytes.as_slice(), 1024);
        }
    }

    #[test]
    fn wrong_auth_and_major_versions_are_rejected() {
        let mut request = request();
        assert!(matches!(
            request.validate(&"b".repeat(64)),
            Err(ProtocolError::Authentication)
        ));
        request.protocol_version.major += 1;
        assert!(matches!(
            request.validate(&"a".repeat(64)),
            Err(ProtocolError::IncompatibleVersion(_))
        ));
    }
}
