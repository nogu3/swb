//! Output line types, error type, and serialization.
//!
//! stdout is pure structured JSON; errors are one-line JSON on stderr.

use std::fmt::Write as _;

use serde::Serialize;
use time::macros::format_description;
use time::OffsetDateTime;

pub type Values = serde_json::Map<String, serde_json::Value>;

/// Lowercase hex, no separators.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// RFC 3339 UTC with millisecond precision, e.g. `2026-07-15T06:00:00.123Z`.
pub fn now_ts() -> String {
    let fmt = format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
    );
    OffsetDateTime::now_utc().format(&fmt).unwrap_or_default()
}

/// One BLE scan JSONL line. `mfr_hex` is the raw manufacturer data as
/// transmitted (little-endian company ID prefix included) — the lossless
/// layer. `model`/`values` appear only when a decode dictionary matched.
#[derive(Debug, Serialize)]
pub struct ScanLine {
    pub ts: String,
    pub addr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rssi: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfr_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svc_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Values>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    Timeout,
    Adapter,
    Auth,
    Device,
    Other,
}

impl ErrorKind {
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Usage => 2,
            ErrorKind::Timeout => 3,
            ErrorKind::Adapter => 4,
            ErrorKind::Auth => 5,
            ErrorKind::Device => 6,
            ErrorKind::Other => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Usage => "usage",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Adapter => "adapter",
            ErrorKind::Auth => "auth",
            ErrorKind::Device => "device",
            ErrorKind::Other => "other",
        }
    }
}

#[derive(Debug)]
pub struct SwbError {
    pub kind: ErrorKind,
    pub detail: String,
}

impl SwbError {
    fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        SwbError {
            kind,
            detail: detail.into(),
        }
    }

    pub fn usage(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, detail)
    }

    pub fn timeout(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Timeout, detail)
    }

    pub fn adapter(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Adapter, detail)
    }

    pub fn auth(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Auth, detail)
    }

    pub fn device(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Device, detail)
    }

    pub fn other(detail: impl Into<String>) -> Self {
        Self::new(ErrorKind::Other, detail)
    }

    /// Machine-readable stderr line.
    pub fn to_json(&self) -> String {
        // Build JSON with controlled field order: kind first, then detail.
        let detail_escaped = serde_json::to_string(&self.detail).unwrap_or_default();
        format!(
            "{{\"error\":{{\"kind\":\"{}\",\"detail\":{}}}}}",
            self.kind.as_str(),
            detail_escaped
        )
    }
}

impl std::fmt::Display for SwbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.detail)
    }
}

impl std::error::Error for SwbError {}

/// Emit one JSON document/line on stdout.
pub fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(s) => println!("{s}"),
        Err(e) => tracing::error!("failed to serialize output: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encodes_lowercase() {
        assert_eq!(hex(&[0x69, 0x09, 0x00, 0xff]), "690900ff");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn now_ts_is_rfc3339_utc_millis() {
        let ts = now_ts();
        // e.g. 2026-07-15T06:00:00.123Z
        assert_eq!(ts.len(), 24);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[19..20], ".");
    }

    #[test]
    fn scan_line_omits_absent_fields() {
        let line = ScanLine {
            ts: "2026-07-15T00:00:00.000Z".to_string(),
            addr: "XX:XX:XX:XX:XX:XX".to_string(),
            rssi: Some(-61),
            mfr_hex: Some("6909aa".to_string()),
            svc_hex: None,
            model: None,
            values: None,
        };
        let json = serde_json::to_string(&line).unwrap();
        assert_eq!(
            json,
            r#"{"ts":"2026-07-15T00:00:00.000Z","addr":"XX:XX:XX:XX:XX:XX","rssi":-61,"mfr_hex":"6909aa"}"#
        );
    }

    #[test]
    fn error_kinds_map_to_exit_codes() {
        assert_eq!(ErrorKind::Usage.exit_code(), 2);
        assert_eq!(ErrorKind::Timeout.exit_code(), 3);
        assert_eq!(ErrorKind::Adapter.exit_code(), 4);
        assert_eq!(ErrorKind::Auth.exit_code(), 5);
        assert_eq!(ErrorKind::Device.exit_code(), 6);
        assert_eq!(ErrorKind::Other.exit_code(), 1);
    }

    #[test]
    fn error_serializes_to_error_object() {
        let err = SwbError::auth("missing credentials");
        assert_eq!(
            err.to_json(),
            r#"{"error":{"kind":"auth","detail":"missing credentials"}}"#
        );
    }
}
