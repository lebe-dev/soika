//! OpenID Connect core: the authorization-code + PKCE + nonce flow.
//!
//! This module is HTTP-framework agnostic (no axum here). It exposes:
//!   * [`OidcProvider`] — a small port trait (`authorize_url` / `exchange_code`)
//!     so handlers depend on an interface and tests can inject fakes.
//!   * [`OidcClient`] — the production adapter wrapping
//!     [`openidconnect::core::CoreClient`], built once at startup via OIDC
//!     discovery (a network call). It uses the existing rustls `reqwest`
//!     async HTTP client — no native-tls/openssl.
//!   * [`OidcClaims`] — the normalized identity we care about (`email`,
//!     `email_verified`, `display_name`).
//!   * Signed, transient state-cookie helpers ([`encode_state_cookie`] /
//!     [`decode_state_cookie`]) that carry `{state, nonce, pkce_verifier, next}`
//!     between `/login` and `/callback` without a server-side store, reusing
//!     [`crate::auth::signing`].
//!
//! Security: PKCE is always S256, a CSRF `state` is bound to the
//! signed cookie, and the ID-token signature / `aud` / `iss` / `exp` / `nonce`
//! are validated by the library inside [`OidcClient::exchange_code`].

use async_trait::async_trait;
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreIdTokenClaims, CoreProviderMetadata,
};
use openidconnect::url::Url;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse,
};
use serde::{Deserialize, Serialize};

use crate::config::OidcConfig;
use crate::error::{Error, Result};

use super::signing;

/// The fully-configured [`CoreClient`] type as produced by
/// [`CoreClient::from_provider_metadata`] + `set_redirect_uri`: the auth
/// endpoint is always set, token/userinfo endpoints are optional (`MaybeSet`),
/// and the remaining endpoints are unused. openidconnect 4 encodes endpoint
/// presence in the type, so a struct field must spell out these states.
type DiscoveredClient = CoreClient<
    EndpointSet,      // authorization endpoint
    EndpointNotSet,   // device authorization endpoint
    EndpointNotSet,   // introspection endpoint
    EndpointNotSet,   // revocation endpoint
    EndpointMaybeSet, // token endpoint
    EndpointMaybeSet, // userinfo endpoint
>;

/// Lifetime of the transient OIDC state cookie: the browser only
/// needs it for the brief redirect round-trip to the provider.
pub const STATE_COOKIE: &str = "soika_oidc_state";

/// Max age of the state cookie / signed payload, in seconds (10 minutes).
pub const STATE_TTL_SECS: i64 = 600;

/// Normalized identity claims we extract from a validated ID-token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcClaims {
    /// The user's email address (lower-cased, trimmed).
    pub email: String,
    /// Whether the provider asserts the email is verified. We require `true`
    /// before auto-provisioning; defaults to `false` if absent.
    pub email_verified: bool,
    /// A human-friendly display name (`name`, else `preferred_username`, else
    /// the local part of the email). Never empty on success.
    pub display_name: String,
}

/// Output of [`OidcProvider::authorize_url`]: the provider URL to redirect the
/// browser to, plus the secrets the callback must remember.
pub struct AuthorizeRequest {
    /// The full authorization URL (contains `state`, `code_challenge`, `scope`,
    /// `redirect_uri`, `nonce`).
    pub url: Url,
    /// CSRF token echoed back as the `state` query parameter on callback.
    pub csrf_state: CsrfToken,
    /// Nonce bound into the ID-token and verified on exchange.
    pub nonce: Nonce,
    /// PKCE verifier replayed (privately) during the code exchange.
    pub pkce_verifier: PkceCodeVerifier,
}

/// Port trait for the OIDC flow so handlers and tests can depend on an
/// interface rather than the concrete [`OidcClient`].
#[async_trait]
pub trait OidcProvider: Send + Sync {
    /// Build a fresh authorization URL with PKCE (S256), CSRF state and nonce.
    fn authorize_url(&self) -> AuthorizeRequest;

    /// Exchange the authorization `code` for tokens, validate the ID-token
    /// (signature/aud/iss/exp/nonce via the library), and return normalized
    /// [`OidcClaims`].
    async fn exchange_code(
        &self,
        code: String,
        pkce_verifier: PkceCodeVerifier,
        nonce: Nonce,
    ) -> Result<OidcClaims>;

    /// Human-readable provider name for the SSO button.
    fn provider_name(&self) -> &str;

    /// Optional allow-list of email domains for auto-provisioning.
    fn allowed_email_domains(&self) -> &[String];

    /// Whether newly provisioned accounts require admin approval before they can
    /// hold a session (`OAUTH_REQUIRE_APPROVAL`).
    fn require_approval(&self) -> bool;
}

/// Production [`OidcProvider`] backed by [`openidconnect::core::CoreClient`].
pub struct OidcClient {
    client: DiscoveredClient,
    scopes: Vec<String>,
    provider_name: String,
    allowed_email_domains: Vec<String>,
    require_approval: bool,
}

impl OidcClient {
    /// Build the client by performing OIDC discovery against the issuer. This is
    /// an async network call; run it once at startup (fail-fast).
    pub async fn discover(config: &OidcConfig) -> Result<Self> {
        let issuer = IssuerUrl::new(config.issuer_url.clone())
            .map_err(|e| Error::validation(format!("invalid OAUTH_ISSUER_URL: {e}")))?;
        let redirect = RedirectUrl::new(config.redirect_url.clone())
            .map_err(|e| Error::validation(format!("invalid OAUTH_REDIRECT_URL: {e}")))?;

        let http_client = build_http_client()?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(|e| Error::internal(format!("OIDC discovery failed: {e}")))?;

        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.client_id.clone()),
            Some(ClientSecret::new(config.client_secret.clone())),
        )
        .set_redirect_uri(redirect);

        Ok(Self {
            client,
            scopes: config.scopes.clone(),
            provider_name: config.provider_name.clone(),
            allowed_email_domains: config.allowed_email_domains.clone(),
            require_approval: config.require_approval,
        })
    }
}

#[async_trait]
impl OidcProvider for OidcClient {
    fn authorize_url(&self) -> AuthorizeRequest {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let mut builder = self.client.authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        );
        for scope in &self.scopes {
            builder = builder.add_scope(Scope::new(scope.clone()));
        }
        let (url, csrf_state, nonce) = builder.set_pkce_challenge(pkce_challenge).url();

        AuthorizeRequest {
            url,
            csrf_state,
            nonce,
            pkce_verifier,
        }
    }

    async fn exchange_code(
        &self,
        code: String,
        pkce_verifier: PkceCodeVerifier,
        nonce: Nonce,
    ) -> Result<OidcClaims> {
        let http_client = build_http_client()?;
        let token_response = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|e| Error::Auth(format!("OIDC token endpoint unavailable: {e}")))?
            .set_pkce_verifier(pkce_verifier)
            .request_async(&http_client)
            .await
            .map_err(|e| Error::Auth(format!("OIDC token exchange failed: {e}")))?;

        let id_token = token_response
            .id_token()
            .ok_or_else(|| Error::Auth("provider did not return an ID token".into()))?;

        // Validates signature (JWKS), `aud`, `iss`, `exp` and `nonce`.
        let claims = id_token
            .claims(&self.client.id_token_verifier(), &nonce)
            .map_err(|e| Error::Auth(format!("ID token verification failed: {e}")))?;

        normalize_claims(claims)
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    fn allowed_email_domains(&self) -> &[String] {
        &self.allowed_email_domains
    }

    fn require_approval(&self) -> bool {
        self.require_approval
    }
}

/// Build the rustls-backed async reqwest client used for OIDC network calls
/// (discovery + token exchange). Redirects are disabled to avoid SSRF, per the
/// openidconnect guidance.
fn build_http_client() -> Result<openidconnect::reqwest::Client> {
    openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Error::internal(format!("failed to build OIDC HTTP client: {e}")))
}

/// Normalize verified ID-token claims into our [`OidcClaims`].
///
/// Fails with [`Error::Auth`] when no email is present. The display name falls
/// back from `name` → `preferred_username` → the email local part.
fn normalize_claims(claims: &CoreIdTokenClaims) -> Result<OidcClaims> {
    let email = claims
        .email()
        .map(|e| e.as_str().trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .ok_or_else(|| Error::Auth("provider did not return an email claim".into()))?;

    let email_verified = claims.email_verified().unwrap_or(false);

    let name = claims
        .name()
        .and_then(|localized| localized.get(None))
        .map(|n| n.as_str().trim().to_string())
        .filter(|n| !n.is_empty());
    let preferred = claims
        .preferred_username()
        .map(|u| u.as_str().trim().to_string())
        .filter(|u| !u.is_empty());

    let display_name = name
        .or(preferred)
        .unwrap_or_else(|| email_local_part(&email));

    Ok(OidcClaims {
        email,
        email_verified,
        display_name,
    })
}

/// The portion of `email` before the `@`, used as a last-resort display name.
fn email_local_part(email: &str) -> String {
    email
        .split_once('@')
        .map(|(local, _)| local.to_string())
        .unwrap_or_else(|| email.to_string())
}

/// Whether `email`'s domain is permitted by `allowed_domains`. An
/// empty allow-list permits any domain. Comparison is case-insensitive.
pub fn email_domain_allowed(email: &str, allowed_domains: &[String]) -> bool {
    if allowed_domains.is_empty() {
        return true;
    }
    let Some((_, domain)) = email.rsplit_once('@') else {
        return false;
    };
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return false;
    }
    allowed_domains
        .iter()
        .any(|d| d.trim().eq_ignore_ascii_case(&domain))
}

/// The transient flow state carried in the signed state cookie.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatePayload {
    /// CSRF token; compared against the `state` query parameter on callback.
    pub state: String,
    /// Nonce replayed to verify the ID-token.
    pub nonce: String,
    /// PKCE verifier replayed during code exchange.
    pub pkce_verifier: String,
    /// Optional post-login redirect target (already validated to start with `/`).
    #[serde(default)]
    pub next: Option<String>,
    /// Unix epoch (seconds) after which this payload is considered expired.
    pub expires_at: i64,
}

/// Serialize + HMAC-sign the flow state into the value of the state cookie.
///
/// `now_unix` is the current time in seconds; the payload expires at
/// `now_unix + STATE_TTL_SECS`.
pub fn encode_state_cookie(
    secret_key: &str,
    state: &str,
    nonce: &str,
    pkce_verifier: &str,
    next: Option<String>,
    now_unix: i64,
) -> Result<String> {
    let payload = StatePayload {
        state: state.to_string(),
        nonce: nonce.to_string(),
        pkce_verifier: pkce_verifier.to_string(),
        next,
        expires_at: now_unix + STATE_TTL_SECS,
    };
    let json = serde_json::to_string(&payload)?;
    Ok(signing::sign_str(secret_key.as_bytes(), &json))
}

/// Verify the signature, deserialize and expiry-check the state cookie.
///
/// Returns [`Error::Auth`] when the signature is invalid, the payload is
/// malformed, or the payload has expired relative to `now_unix`.
pub fn decode_state_cookie(secret_key: &str, signed: &str, now_unix: i64) -> Result<StatePayload> {
    let json = signing::verify_str(secret_key.as_bytes(), signed)?;
    let payload: StatePayload =
        serde_json::from_str(&json).map_err(|_| Error::Auth("malformed OIDC state".into()))?;
    if payload.expires_at <= now_unix {
        return Err(Error::Auth("OIDC state expired".into()));
    }
    Ok(payload)
}

/// Build the `Set-Cookie` value that stores the signed flow state:
/// `HttpOnly`, `SameSite=Lax`, `Path=/`, short `Max-Age`, `Secure` behind TLS.
pub fn build_state_cookie(signed_value: &str, secure: bool) -> String {
    let mut cookie = format!(
        "{STATE_COOKIE}={signed_value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={STATE_TTL_SECS}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build the `Set-Cookie` value that clears the state cookie after the callback.
pub fn build_clearing_state_cookie(secure: bool) -> String {
    let mut cookie = format!("{STATE_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-oidc-secret";

    fn test_config() -> OidcConfig {
        OidcConfig {
            kind: crate::config::OAuthProviderKind::Oidc,
            issuer_url: "https://idp.example.com".into(),
            client_id: "client-123".into(),
            client_secret: "secret-456".into(),
            redirect_url: "https://app.example.com/auth/oidc/callback".into(),
            scopes: vec!["openid".into(), "email".into(), "profile".into()],
            provider_name: "GitLab".into(),
            allowed_email_domains: vec![],
            require_approval: false,
        }
    }

    /// Build an `OidcClient` from a fixed provider metadata without hitting the
    /// network, so we can unit-test `authorize_url`.
    fn offline_client(cfg: &OidcConfig) -> OidcClient {
        use openidconnect::core::{CoreJsonWebKeySet, CoreProviderMetadata};
        use openidconnect::{
            AuthUrl, EmptyAdditionalProviderMetadata, JsonWebKeySetUrl, ResponseTypes, TokenUrl,
        };

        let metadata = CoreProviderMetadata::new(
            IssuerUrl::new(cfg.issuer_url.clone()).unwrap(),
            AuthUrl::new(format!("{}/oauth/authorize", cfg.issuer_url)).unwrap(),
            JsonWebKeySetUrl::new(format!("{}/oauth/jwks", cfg.issuer_url)).unwrap(),
            vec![ResponseTypes::new(vec![
                openidconnect::core::CoreResponseType::Code,
            ])],
            vec![openidconnect::core::CoreSubjectIdentifierType::Public],
            vec![openidconnect::core::CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
            EmptyAdditionalProviderMetadata {},
        )
        .set_token_endpoint(Some(
            TokenUrl::new(format!("{}/oauth/token", cfg.issuer_url)).unwrap(),
        ))
        .set_jwks(CoreJsonWebKeySet::new(vec![]));

        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(cfg.client_id.clone()),
            Some(ClientSecret::new(cfg.client_secret.clone())),
        )
        .set_redirect_uri(RedirectUrl::new(cfg.redirect_url.clone()).unwrap());

        OidcClient {
            client,
            scopes: cfg.scopes.clone(),
            provider_name: cfg.provider_name.clone(),
            allowed_email_domains: cfg.allowed_email_domains.clone(),
            require_approval: cfg.require_approval,
        }
    }

    #[test]
    fn authorize_url_contains_required_params() {
        let cfg = test_config();
        let client = offline_client(&cfg);
        let req = client.authorize_url();

        let pairs: std::collections::HashMap<String, String> =
            req.url.query_pairs().into_owned().collect();

        // CSRF state present and matches the returned token.
        assert_eq!(
            pairs.get("state").map(String::as_str),
            Some(req.csrf_state.secret().as_str())
        );
        // PKCE S256.
        assert!(pairs.contains_key("code_challenge"));
        assert_eq!(
            pairs.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        // nonce present and matches.
        assert_eq!(
            pairs.get("nonce").map(String::as_str),
            Some(req.nonce.secret().as_str())
        );
        // redirect_uri matches config.
        assert_eq!(
            pairs.get("redirect_uri").map(String::as_str),
            Some(cfg.redirect_url.as_str())
        );
        // scope contains all configured scopes.
        let scope = pairs.get("scope").expect("scope present");
        for s in &cfg.scopes {
            assert!(scope.split(' ').any(|x| x == s), "scope missing {s}");
        }
    }

    #[test]
    fn authorize_url_targets_provider_authorize_endpoint() {
        let cfg = test_config();
        let client = offline_client(&cfg);
        let req = client.authorize_url();
        assert_eq!(req.url.scheme(), "https");
        assert_eq!(req.url.host_str(), Some("idp.example.com"));
        assert_eq!(req.url.path(), "/oauth/authorize");
    }

    // --- claims normalization ------------------------------------------------

    fn claims_with(
        email: Option<&str>,
        email_verified: Option<bool>,
        name: Option<&str>,
        preferred: Option<&str>,
    ) -> CoreIdTokenClaims {
        use openidconnect::{
            Audience, EndUserEmail, EndUserName, EndUserUsername, LocalizedClaim, StandardClaims,
            SubjectIdentifier,
        };

        let mut standard = StandardClaims::new(SubjectIdentifier::new("sub-1".into()));
        if let Some(e) = email {
            standard = standard.set_email(Some(EndUserEmail::new(e.into())));
        }
        if let Some(v) = email_verified {
            standard = standard.set_email_verified(Some(v));
        }
        if let Some(n) = name {
            let mut localized = LocalizedClaim::new();
            localized.insert(None, EndUserName::new(n.into()));
            standard = standard.set_name(Some(localized));
        }
        if let Some(p) = preferred {
            standard = standard.set_preferred_username(Some(EndUserUsername::new(p.into())));
        }

        CoreIdTokenClaims::new(
            IssuerUrl::new("https://idp.example.com".into()).unwrap(),
            vec![Audience::new("client-123".into())],
            chrono::Utc::now() + chrono::Duration::hours(1),
            chrono::Utc::now(),
            standard,
            openidconnect::EmptyAdditionalClaims {},
        )
    }

    #[test]
    fn normalize_prefers_name_lowercases_email() {
        let claims = claims_with(
            Some("Alice@Example.COM"),
            Some(true),
            Some("Alice A"),
            Some("alice"),
        );
        let out = normalize_claims(&claims).unwrap();
        assert_eq!(out.email, "alice@example.com");
        assert!(out.email_verified);
        assert_eq!(out.display_name, "Alice A");
    }

    #[test]
    fn normalize_falls_back_to_preferred_username() {
        let claims = claims_with(Some("bob@example.com"), Some(true), None, Some("bobby"));
        let out = normalize_claims(&claims).unwrap();
        assert_eq!(out.display_name, "bobby");
    }

    #[test]
    fn normalize_falls_back_to_email_local_part() {
        let claims = claims_with(Some("carol@example.com"), Some(false), None, None);
        let out = normalize_claims(&claims).unwrap();
        assert_eq!(out.display_name, "carol");
        assert!(!out.email_verified);
    }

    #[test]
    fn normalize_missing_email_verified_defaults_false() {
        let claims = claims_with(Some("dave@example.com"), None, None, None);
        let out = normalize_claims(&claims).unwrap();
        assert!(!out.email_verified);
    }

    #[test]
    fn normalize_rejects_missing_email() {
        let claims = claims_with(None, Some(true), Some("No Email"), None);
        let err = normalize_claims(&claims).unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    }

    // --- email domain whitelist ---------------------------------------------

    #[test]
    fn empty_whitelist_allows_any_domain() {
        assert!(email_domain_allowed("anyone@whatever.io", &[]));
    }

    #[test]
    fn whitelist_allows_listed_domain_case_insensitive() {
        let allowed = vec!["example.com".to_string(), "itkey.com".to_string()];
        assert!(email_domain_allowed("user@Example.com", &allowed));
        assert!(email_domain_allowed("user@ITKEY.COM", &allowed));
    }

    #[test]
    fn whitelist_rejects_unlisted_domain() {
        let allowed = vec!["example.com".to_string()];
        assert!(!email_domain_allowed("user@evil.com", &allowed));
    }

    #[test]
    fn whitelist_rejects_email_without_domain() {
        let allowed = vec!["example.com".to_string()];
        assert!(!email_domain_allowed("not-an-email", &allowed));
    }

    // --- state cookie sign/verify -------------------------------------------

    #[test]
    fn state_cookie_round_trips() {
        let now = 1_000_000;
        let signed = encode_state_cookie(
            SECRET,
            "csrf-state",
            "the-nonce",
            "pkce-verifier",
            Some("/dashboard".into()),
            now,
        )
        .unwrap();

        let payload = decode_state_cookie(SECRET, &signed, now + 1).unwrap();
        assert_eq!(payload.state, "csrf-state");
        assert_eq!(payload.nonce, "the-nonce");
        assert_eq!(payload.pkce_verifier, "pkce-verifier");
        assert_eq!(payload.next.as_deref(), Some("/dashboard"));
        assert_eq!(payload.expires_at, now + STATE_TTL_SECS);
    }

    #[test]
    fn state_cookie_rejects_forgery() {
        let now = 1_000_000;
        let signed = encode_state_cookie(SECRET, "csrf", "nonce", "pkce", None, now).unwrap();
        let err = decode_state_cookie("different-secret", &signed, now).unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    }

    #[test]
    fn state_cookie_rejects_tampered_payload() {
        let now = 1_000_000;
        let signed = encode_state_cookie(SECRET, "csrf", "nonce", "pkce", None, now).unwrap();
        // Flip a character in the payload portion, keep the signature.
        let (payload_b64, sig) = signed.split_once('.').unwrap();
        let mut chars: Vec<char> = payload_b64.chars().collect();
        chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
        let forged: String = chars.into_iter().collect();
        let forged = format!("{forged}.{sig}");
        let err = decode_state_cookie(SECRET, &forged, now).unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    }

    #[test]
    fn state_cookie_rejects_expired_payload() {
        let now = 1_000_000;
        let signed = encode_state_cookie(SECRET, "csrf", "nonce", "pkce", None, now).unwrap();
        // Verify well past expiry.
        let err = decode_state_cookie(SECRET, &signed, now + STATE_TTL_SECS + 1).unwrap_err();
        assert!(matches!(err, Error::Auth(_)), "got {err:?}");
    }

    #[test]
    fn build_state_cookie_sets_attributes() {
        let cookie = build_state_cookie("signed-value", true);
        assert!(cookie.starts_with("soika_oidc_state=signed-value"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains(&format!("Max-Age={STATE_TTL_SECS}")));
        assert!(cookie.contains("Secure"));

        let insecure = build_state_cookie("v", false);
        assert!(!insecure.contains("Secure"));
    }

    #[test]
    fn clearing_state_cookie_expires_immediately() {
        let cookie = build_clearing_state_cookie(false);
        assert!(cookie.contains("Max-Age=0"));
    }
}
