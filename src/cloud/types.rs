//! API request/response types.

use serde::Deserialize;
use serde_json::Value;

/// Envelope every v1.1 endpoint returns.
#[derive(Debug, Deserialize)]
pub struct ApiResponse {
    #[serde(rename = "statusCode")]
    pub status_code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub body: Value,
}
