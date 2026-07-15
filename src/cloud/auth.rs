//! SwitchBot API v1.1 HMAC-SHA256 request signing.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::emit::SwbError;

/// Base64 HMAC-SHA256 of `token + t + nonce`, keyed by `secret`.
pub fn sign(token: &str, secret: &str, t_millis: u64, nonce: &str) -> Result<String, SwbError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| SwbError::auth("invalid secret key"))?;
    mac.update(format!("{token}{t_millis}{nonce}").as_bytes());
    Ok(STANDARD.encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known vector, independently computed with Python's hmac/hashlib:
    /// HMAC-SHA256(key="test-secret",
    ///             msg="test-token1700000000000" + nonce) |> base64
    #[test]
    fn sign_matches_known_vector() {
        let sig = sign(
            "test-token",
            "test-secret",
            1_700_000_000_000,
            "8f0b3b7e-9c3a-4e0d-9d2a-1234567890ab",
        )
        .unwrap();
        assert_eq!(sig, "cm/zYZDTxkq2+u9zmB2EEBHUqSwJWaWTZWkW6vT2LQc=");
    }
}
