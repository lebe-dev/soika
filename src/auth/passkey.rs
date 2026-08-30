//! Passkey (WebAuthn) core: relying-party construction and the signed
//! challenge-cookie that carries ceremony state between the two requests.
//!
//! WebAuthn is a two-step ceremony: the server issues a challenge, the browser
//! signs it, and the server verifies the response against the state it issued.
//! That state MUST be remembered server-side — soika keeps it in an HMAC-signed,
//! short-lived, `HttpOnly` cookie (the same shape as the OIDC state cookie in
//! [`crate::auth::oidc`]) rather than a table or an in-process map: the value is
//! tamper-evident, expires on its own, and needs no write on every sign-in click.
//!
//! Two cookie names are used so a registration state can never be replayed into
//! an authentication ceremony (or vice versa).

use serde::Serialize;
use serde::de::DeserializeOwned;
use webauthn_rs::prelude::{Passkey as WebauthnPasskey, Url, Webauthn, WebauthnBuilder};

use crate::config::PasskeyConfig;
use crate::domain::Id;
use crate::error::{Error, Result};

use super::signing;

/// Cookie holding the *registration* ceremony state.
pub const PASSKEY_REGISTRATION_COOKIE: &str = "soika_passkey_reg";

/// Cookie holding the *authentication* ceremony state.
pub const PASSKEY_AUTHENTICATION_COOKIE: &str = "soika_passkey_auth";

/// Build the WebAuthn relying party from the operator's configuration.
///
/// Fails fast (at startup, from `build_state`) when the relying party id and
/// the origin disagree — a mismatch would otherwise surface as an opaque
/// browser-side error on the first sign-in attempt.
pub fn build_webauthn(config: &PasskeyConfig) -> Result<Webauthn> {
    let origin = parse_origin(&config.rp_origin, "PASSKEY_RP_ORIGIN")?;

    let mut builder = WebauthnBuilder::new(&config.rp_id, &origin)
        .map_err(|e| Error::validation(format!("invalid passkey configuration: {e}")))?
        .rp_name(&config.rp_name)
        .allow_subdomains(config.allow_subdomains)
        .timeout(config.timeout);

    for extra in &config.extra_origins {
        let url = parse_origin(extra, "PASSKEY_EXTRA_ORIGINS")?;
        builder = builder.append_allowed_origin(&url);
    }

    builder
        .build()
        .map_err(|e| Error::validation(format!("invalid passkey configuration: {e}")))
}

fn parse_origin(raw: &str, key: &str) -> Result<Url> {
    Url::parse(raw).map_err(|e| Error::validation(format!("{key}: '{raw}' is not a URL ({e})")))
}

/// The raw credential id of a verified credential, URL-safe base64 (no padding).
///
/// This is the key the credential is stored and looked up under; the same
/// encoding the browser uses for `PublicKeyCredential.id`.
pub fn credential_id_b64(passkey: &WebauthnPasskey) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD.encode(passkey.cred_id().as_slice())
}

/// Decode a stored credential id back into the raw bytes WebAuthn expects.
pub fn decode_credential_id(encoded: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| Error::internal(format!("stored credential id is unreadable: {e}")))
}

/// Encode raw credential id bytes the same way [`credential_id_b64`] does.
pub fn encode_credential_id(bytes: &[u8]) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD.encode(bytes)
}

// ---------------------------------------------------------------------------
// Challenge cookie
// ---------------------------------------------------------------------------

/// Signed cookie payload: the library's ceremony state plus an expiry and, for
/// registration, the user the ceremony was started for.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChallengePayload<S> {
    /// Ceremony state owned by the WebAuthn library.
    pub state: S,
    /// Account the ceremony belongs to; `None` for the usernameless login flow,
    /// where the account is only known once the assertion is identified.
    #[serde(default)]
    pub user_id: Option<Id>,
    /// Unix epoch (seconds) after which the payload is refused.
    pub expires_at: i64,
}

/// Serialize + HMAC-sign a ceremony state into a cookie value.
pub fn encode_challenge<S: Serialize>(
    secret_key: &str,
    state: &S,
    user_id: Option<Id>,
    now_unix: i64,
    ttl_secs: i64,
) -> Result<String> {
    let payload = ChallengePayload {
        state,
        user_id,
        expires_at: now_unix + ttl_secs,
    };
    let json = serde_json::to_string(&payload)?;
    Ok(signing::sign_str(secret_key.as_bytes(), &json))
}

/// Verify, deserialize and expiry-check a challenge cookie.
///
/// Every failure mode is [`Error::Auth`]: a bad signature, a payload that does
/// not match the expected ceremony, and an expired challenge are all "start the
/// ceremony again", and the caller must not be told which.
pub fn decode_challenge<S: DeserializeOwned>(
    secret_key: &str,
    signed: &str,
    now_unix: i64,
) -> Result<ChallengePayload<S>> {
    let json = signing::verify_str(secret_key.as_bytes(), signed)?;
    let payload: ChallengePayload<S> =
        serde_json::from_str(&json).map_err(|_| Error::Auth("malformed passkey state".into()))?;
    if payload.expires_at <= now_unix {
        return Err(Error::Auth("passkey challenge expired".into()));
    }
    Ok(payload)
}

/// Build the `Set-Cookie` value storing a signed ceremony state:
/// `HttpOnly`, `SameSite=Lax`, `Path=/`, short `Max-Age`, `Secure` behind TLS.
pub fn build_challenge_cookie(
    name: &str,
    signed_value: &str,
    ttl_secs: i64,
    secure: bool,
) -> String {
    let mut cookie =
        format!("{name}={signed_value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={ttl_secs}");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build the `Set-Cookie` value clearing a ceremony cookie once it is spent.
pub fn build_clearing_challenge_cookie(name: &str, secure: bool) -> String {
    let mut cookie = format!("{name}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const SECRET: &str = "test-passkey-secret";

    fn test_config() -> PasskeyConfig {
        PasskeyConfig {
            rp_id: "example.com".into(),
            rp_name: "soika".into(),
            rp_origin: "https://example.com".into(),
            extra_origins: Vec::new(),
            allow_subdomains: false,
            timeout: Duration::from_secs(60),
            challenge_ttl: Duration::from_secs(300),
            max_per_user: 10,
        }
    }

    #[test]
    fn builds_a_relying_party_from_config() {
        let webauthn = build_webauthn(&test_config()).expect("build");
        let origins = webauthn.get_allowed_origins();
        assert!(origins.iter().any(|o| o.as_str().contains("example.com")));
    }

    #[test]
    fn extra_origins_are_accepted() {
        let mut config = test_config();
        config.extra_origins = vec!["http://localhost:4200".into()];
        let webauthn = build_webauthn(&config).expect("build");
        assert!(
            webauthn
                .get_allowed_origins()
                .iter()
                .any(|o| o.as_str().starts_with("http://localhost:4200"))
        );
    }

    #[test]
    fn rp_id_not_matching_the_origin_is_rejected() {
        let mut config = test_config();
        config.rp_id = "other.example.org".into();
        let err = build_webauthn(&config).unwrap_err();
        assert!(matches!(err, Error::Validation(_)), "got {err:?}");
    }

    #[test]
    fn malformed_origin_is_rejected() {
        let mut config = test_config();
        config.rp_origin = "example.com".into();
        let err = build_webauthn(&config).unwrap_err();
        assert!(matches!(err, Error::Validation(_)), "got {err:?}");
    }

    #[test]
    fn challenge_round_trips() {
        let user_id = Id::new_v4();
        let signed = encode_challenge(SECRET, &"state-1".to_string(), Some(user_id), 1_000, 300)
            .expect("encode");
        let payload: ChallengePayload<String> =
            decode_challenge(SECRET, &signed, 1_100).expect("decode");
        assert_eq!(payload.state, "state-1");
        assert_eq!(payload.user_id, Some(user_id));
    }

    #[test]
    fn expired_challenge_is_rejected() {
        let signed =
            encode_challenge(SECRET, &"state-1".to_string(), None, 1_000, 300).expect("encode");
        let err = decode_challenge::<String>(SECRET, &signed, 1_301).unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    }

    #[test]
    fn challenge_signed_with_another_secret_is_rejected() {
        let signed =
            encode_challenge(SECRET, &"state-1".to_string(), None, 1_000, 300).expect("encode");
        let err = decode_challenge::<String>("other-secret", &signed, 1_100).unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    }

    #[test]
    fn a_registration_state_does_not_decode_as_an_authentication_state() {
        // Type confusion between the two ceremonies must fail, not silently
        // reinterpret bytes.
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Registration {
            reg: u8,
        }
        #[derive(Debug, serde::Serialize, serde::Deserialize)]
        struct Authentication {
            auth: u8,
        }

        let signed = encode_challenge(SECRET, &Registration { reg: 1 }, None, 1_000, 300).unwrap();
        let err = decode_challenge::<Authentication>(SECRET, &signed, 1_100).unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    }

    #[test]
    fn cookies_carry_the_hardening_attributes() {
        let cookie = build_challenge_cookie(PASSKEY_REGISTRATION_COOKIE, "value", 300, true);
        assert!(cookie.starts_with("soika_passkey_reg=value"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Max-Age=300"));
        assert!(cookie.contains("Secure"));

        let insecure = build_challenge_cookie(PASSKEY_AUTHENTICATION_COOKIE, "value", 300, false);
        assert!(!insecure.contains("Secure"));

        let cleared = build_clearing_challenge_cookie(PASSKEY_REGISTRATION_COOKIE, false);
        assert!(cleared.contains("Max-Age=0"));
    }
}
