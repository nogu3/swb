//! Cloud backend: SwitchBot Web API v1.1 (one-shot request/response).

pub mod auth;
pub mod types;

use serde_json::{json, Value};

use crate::emit::{ErrorKind, SwbError};
use types::ApiResponse;

pub const DEFAULT_BASE: &str = "https://api.switch-bot.com";

pub struct Client {
    token: String,
    secret: String,
    base: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(token: String, secret: String) -> Self {
        Self::with_base(token, secret, DEFAULT_BASE.to_string())
    }

    /// `base` override exists for tests against a local mock server.
    pub fn with_base(token: String, secret: String, base: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .build();
        Self {
            token,
            secret,
            base,
            agent,
        }
    }

    pub fn get(&self, path: &str) -> Result<Value, SwbError> {
        self.request("GET", path, None)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value, SwbError> {
        self.request("POST", path, Some(body))
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, SwbError> {
        let t = current_millis()?;
        let nonce = uuid::Uuid::new_v4().to_string();
        let sig = auth::sign(&self.token, &self.secret, t, &nonce)?;
        let url = format!("{}{}", self.base, path);
        tracing::debug!(method, path, "cloud request");

        let request = self
            .agent
            .request(method, &url)
            .set("Authorization", &self.token)
            .set("sign", &sig)
            .set("t", &t.to_string())
            .set("nonce", &nonce)
            .set("Content-Type", "application/json; charset=utf-8");

        let result = match body {
            Some(b) => request.send_string(&b.to_string()),
            None => request.call(),
        };

        let text = match result {
            Ok(response) => response
                .into_string()
                .map_err(|e| SwbError::other(format!("reading response: {e}")))?,
            Err(ureq::Error::Status(401, _)) => {
                return Err(SwbError::auth("HTTP 401: invalid token or signature"))
            }
            Err(ureq::Error::Status(code, response)) => {
                let text = response.into_string().unwrap_or_default();
                return Err(SwbError::device(format!("HTTP {code}: {text}")));
            }
            Err(ureq::Error::Transport(t)) => {
                let msg = t.to_string();
                let kind = if msg.contains("timed out") {
                    ErrorKind::Timeout
                } else {
                    ErrorKind::Other
                };
                return Err(SwbError {
                    kind,
                    detail: format!("request failed: {msg}"),
                });
            }
        };

        parse_response(&text)
    }
}

/// `statusCode == 100` → body; other statusCode → device error;
/// non-JSON → pass the raw text through instead of failing.
pub fn parse_response(text: &str) -> Result<Value, SwbError> {
    match serde_json::from_str::<ApiResponse>(text) {
        Ok(resp) if resp.status_code == 100 => Ok(resp.body),
        Ok(resp) => Err(SwbError::device(format!(
            "statusCode {}: {}",
            resp.status_code, resp.message
        ))),
        Err(_) => Ok(json!({ "raw": text })),
    }
}

fn current_millis() -> Result<u64, SwbError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .map_err(|e| SwbError::other(format!("system clock before epoch: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::ErrorKind;

    #[test]
    fn success_response_yields_body() {
        let body =
            parse_response(r#"{"statusCode":100,"message":"success","body":{"deviceList":[]}}"#)
                .unwrap();
        assert_eq!(body["deviceList"], serde_json::json!([]));
    }

    #[test]
    fn non_success_status_code_is_device_error() {
        let err = parse_response(r#"{"statusCode":190,"message":"wrong deviceId"}"#).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Device);
        assert!(err.detail.contains("190"));
    }

    #[test]
    fn unparseable_body_is_passed_through_raw() {
        let body = parse_response("<html>gateway error</html>").unwrap();
        assert_eq!(body["raw"], "<html>gateway error</html>");
    }
}
