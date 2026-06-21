//! HMAC-SHA256 signing utilities used for opaque, tamper-evident
//! session cookies and invite tokens.
//!
//! A signed value has the shape `<payload>.<sig>` where:
//!   * `<payload>` is the URL-safe base64 (no padding) of the raw bytes;
//!   * `<sig>` is the URL-safe base64 (no padding) of `HMAC_SHA256(secret, payload)`.
//!
//! Verification is constant-time (via `hmac`'s `verify_slice`) so it does not
//! leak timing information. The signature proves the value was minted by this
//! instance; it is NOT encryption — never put secrets in the payload.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::error::{Error, Result};

type HmacSha256 = Hmac<Sha256>;

/// Sign `payload` bytes, producing the `<b64payload>.<b64sig>` string.
pub fn sign(secret: &[u8], payload: &[u8]) -> String {
    let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
    let sig = mac(secret, encoded_payload.as_bytes());
    let encoded_sig = URL_SAFE_NO_PAD.encode(sig);
    format!("{encoded_payload}.{encoded_sig}")
}

/// Sign a string payload — convenience over [`sign`].
pub fn sign_str(secret: &[u8], payload: &str) -> String {
    sign(secret, payload.as_bytes())
}

/// Verify a signed token and return the original payload bytes.
///
/// Returns `Error::Auth` when the token is malformed or the signature does not
/// validate against `secret`.
pub fn verify(secret: &[u8], token: &str) -> Result<Vec<u8>> {
    let (encoded_payload, encoded_sig) = token
        .split_once('.')
        .ok_or_else(|| Error::Auth("malformed signed token".into()))?;

    let expected_sig = URL_SAFE_NO_PAD
        .decode(encoded_sig)
        .map_err(|_| Error::Auth("malformed signature encoding".into()))?;

    let mut hmac = HmacSha256::new_from_slice(secret)
        .map_err(|e| Error::internal(format!("hmac key error: {e}")))?;
    hmac.update(encoded_payload.as_bytes());
    // Constant-time comparison.
    hmac.verify_slice(&expected_sig)
        .map_err(|_| Error::Auth("invalid signature".into()))?;

    let payload = URL_SAFE_NO_PAD
        .decode(encoded_payload)
        .map_err(|_| Error::Auth("malformed payload encoding".into()))?;
    Ok(payload)
}

/// Verify a signed token and return the payload as a UTF-8 string.
pub fn verify_str(secret: &[u8], token: &str) -> Result<String> {
    let bytes = verify(secret, token)?;
    String::from_utf8(bytes).map_err(|_| Error::Auth("payload is not valid UTF-8".into()))
}

fn mac(secret: &[u8], message: &[u8]) -> Vec<u8> {
    let mut hmac = HmacSha256::new_from_slice(secret).expect("HMAC accepts keys of any length");
    hmac.update(message);
    hmac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret-key";

    #[test]
    fn sign_then_verify_round_trips() {
        let token = sign_str(SECRET, "session-id-123");
        assert!(token.contains('.'));
        let payload = verify_str(SECRET, &token).unwrap();
        assert_eq!(payload, "session-id-123");
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let token = sign_str(SECRET, "session-id-123");
        // Flip the payload but keep the signature.
        let (_, sig) = token.split_once('.').unwrap();
        let forged = format!("{}.{}", URL_SAFE_NO_PAD.encode("evil"), sig);
        let err = verify_str(SECRET, &forged).unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let token = sign_str(SECRET, "session-id-123");
        let err = verify_str(b"different-secret", &token).unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
    }

    #[test]
    fn missing_separator_is_rejected() {
        let err = verify_str(SECRET, "no-separator-here").unwrap_err();
        assert!(matches!(err, Error::Auth(_)));
    }
}
