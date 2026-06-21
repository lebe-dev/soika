//! Password hashing & verification — argon2id with a random salt.
//!
//! Hashes are stored as PHC strings (e.g. `$argon2id$v=19$m=...$salt$hash`),
//! which embed the algorithm, parameters and salt, so verification is
//! self-describing and future parameter changes remain backward-compatible.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, Salt, SaltString};
use rand::Rng;

use crate::error::{Error, Result};

/// Hash a plaintext password with argon2id and a fresh random salt.
///
/// Returns the PHC string to persist in `users.password_hash`.
pub fn hash_password(password: &str) -> Result<String> {
    if password.is_empty() {
        return Err(Error::validation("password must not be empty"));
    }

    // password-hash's `SaltString::generate` ties us to its (older) rand_core
    // version; instead draw salt bytes from the workspace `rand` and b64-encode.
    let mut salt_bytes = [0u8; Salt::RECOMMENDED_LENGTH];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| Error::internal(format!("salt generation failed: {e}")))?;
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| Error::internal(format!("argon2 hashing failed: {e}")))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored argon2 PHC hash.
///
/// Returns `Ok(false)` for a well-formed hash that simply does not match, and
/// an error only when the stored hash itself is malformed.
pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| Error::internal(format!("stored password hash is malformed: {e}")))?;

    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(Error::internal(format!("argon2 verification failed: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_round_trip() {
        let hash = hash_password("correct horse battery staple").expect("hashing should succeed");
        // PHC string form.
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct horse battery staple", &hash).unwrap());
    }

    #[test]
    fn verify_rejects_wrong_password() {
        let hash = hash_password("s3cret-passphrase").unwrap();
        assert!(!verify_password("not-the-password", &hash).unwrap());
    }

    #[test]
    fn distinct_salts_produce_distinct_hashes() {
        let a = hash_password("same-input").unwrap();
        let b = hash_password("same-input").unwrap();
        assert_ne!(a, b, "random salt should make hashes differ");
        // ...yet both verify against the same plaintext.
        assert!(verify_password("same-input", &a).unwrap());
        assert!(verify_password("same-input", &b).unwrap());
    }

    #[test]
    fn empty_password_is_rejected() {
        let err = hash_password("").unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn malformed_hash_errors() {
        let err = verify_password("whatever", "not-a-phc-string").unwrap_err();
        assert!(matches!(err, Error::Internal(_)));
    }
}
