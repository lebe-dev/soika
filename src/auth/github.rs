//! GitHub OAuth2 provider (NOT OpenID Connect).
//!
//! GitHub OAuth Apps speak plain OAuth 2.0: there is no OIDC discovery document
//! and no ID-token. This adapter implements the same [`OidcProvider`] port as the
//! generic [`OidcClient`](super::oidc::OidcClient), so the HTTP handlers and the
//! signed state-cookie round-trip are reused unchanged. Identity is read from the
//! REST API (`/user` + `/user/emails`) with the access token rather than from a
//! verified ID-token.
//!
//! The shared [`AuthorizeRequest`] still carries a `nonce` and `pkce_verifier` so
//! it can round-trip through the state cookie, but GitHub's OAuth2 flow uses
//! neither — only the CSRF `state` is meaningful — so [`exchange_code`] ignores
//! both.

use async_trait::async_trait;
use openidconnect::url::Url;
use openidconnect::{CsrfToken, Nonce, PkceCodeChallenge, PkceCodeVerifier};
use serde::Deserialize;

use crate::config::OidcConfig;
use crate::error::{Error, Result};

use super::oidc::{AuthorizeRequest, OidcClaims, OidcProvider};

const AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_API: &str = "https://api.github.com/user";
const EMAILS_API: &str = "https://api.github.com/user/emails";
/// GitHub's REST API rejects requests without a User-Agent.
const USER_AGENT: &str = "soika";
/// Pin the REST API version GitHub serves (recommended `Accept` value).
const GH_ACCEPT: &str = "application/vnd.github+json";

/// Production [`OidcProvider`] for GitHub OAuth Apps.
pub struct GithubProvider {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    redirect_url: String,
    scopes: Vec<String>,
    provider_name: String,
    allowed_email_domains: Vec<String>,
    require_approval: bool,
}

impl GithubProvider {
    /// Build the provider from the OAuth config. Validates the redirect URL up
    /// front (fail-fast at startup, mirroring [`OidcClient::discover`]). Unlike
    /// OIDC there is no network call — GitHub has no discovery endpoint.
    pub fn new(config: &OidcConfig) -> Result<Self> {
        Url::parse(&config.redirect_url)
            .map_err(|e| Error::validation(format!("invalid OAUTH_REDIRECT_URL: {e}")))?;

        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| Error::internal(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            http,
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
            redirect_url: config.redirect_url.clone(),
            scopes: config.scopes.clone(),
            provider_name: config.provider_name.clone(),
            allowed_email_domains: config.allowed_email_domains.clone(),
            require_approval: config.require_approval,
        })
    }

    /// Resolve a verified email via `/user/emails` (requires the `user:email`
    /// scope). Prefers the primary verified address, then any verified one.
    /// Fails with [`Error::Forbidden`] when GitHub exposes no verified email.
    async fn primary_verified_email(&self, access_token: &str) -> Result<String> {
        let emails: Vec<GithubEmail> = self
            .http
            .get(EMAILS_API)
            .bearer_auth(access_token)
            .header(reqwest::header::ACCEPT, GH_ACCEPT)
            .send()
            .await
            .map_err(|e| Error::Auth(format!("GitHub emails request failed: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Auth(format!("GitHub emails request returned an error: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Auth(format!("GitHub emails response was not valid JSON: {e}")))?;

        let chosen = emails
            .iter()
            .find(|e| e.primary && e.verified)
            .or_else(|| emails.iter().find(|e| e.verified))
            .ok_or_else(|| Error::Forbidden("no verified email is available from GitHub".into()))?;

        Ok(chosen.email.trim().to_ascii_lowercase())
    }
}

#[async_trait]
impl OidcProvider for GithubProvider {
    fn authorize_url(&self) -> AuthorizeRequest {
        let csrf_state = CsrfToken::new_random();
        let scope = self.scopes.join(" ");
        // GitHub uses only `state` for CSRF; nonce/PKCE are not part of its OAuth2
        // flow. We still mint them so the shared state cookie round-trips, but the
        // exchange ignores them.
        let (_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let nonce = Nonce::new_random();

        let url = Url::parse_with_params(
            AUTHORIZE_URL,
            &[
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", self.redirect_url.as_str()),
                ("scope", scope.as_str()),
                ("state", csrf_state.secret().as_str()),
            ],
        )
        .expect("GitHub authorize URL with params is valid");

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
        _pkce_verifier: PkceCodeVerifier,
        _nonce: Nonce,
    ) -> Result<OidcClaims> {
        // 1) Exchange the authorization code for an access token. GitHub returns
        //    HTTP 200 even on error, carrying `{ error, error_description }`, so we
        //    branch on the presence of `access_token` rather than the status.
        let token: TokenResponse = self
            .http
            .post(TOKEN_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code", code.as_str()),
                ("redirect_uri", self.redirect_url.as_str()),
            ])
            .send()
            .await
            .map_err(|e| Error::Auth(format!("GitHub token exchange request failed: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Auth(format!("GitHub token response was not valid JSON: {e}")))?;

        let access_token = match token.access_token {
            Some(t) if !t.is_empty() => t,
            _ => {
                let detail = token
                    .error_description
                    .or(token.error)
                    .unwrap_or_else(|| "no access_token returned".into());
                return Err(Error::Auth(format!(
                    "GitHub token exchange failed: {detail}"
                )));
            }
        };

        // 2) Fetch the user profile (for the display name).
        let user: GithubUser = self
            .http
            .get(USER_API)
            .bearer_auth(&access_token)
            .header(reqwest::header::ACCEPT, GH_ACCEPT)
            .send()
            .await
            .map_err(|e| Error::Auth(format!("GitHub user request failed: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Auth(format!("GitHub user request returned an error: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Auth(format!("GitHub user response was not valid JSON: {e}")))?;

        // 3) Resolve a verified email. GitHub guarantees account emails carry a
        //    `verified` flag, so we treat the chosen address as verified.
        let email = self.primary_verified_email(&access_token).await?;

        let display_name = user
            .name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or(user.login);

        Ok(OidcClaims {
            email,
            email_verified: true,
            display_name,
        })
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

/// `POST /login/oauth/access_token` response (JSON form).
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Subset of `GET /user` we consume.
#[derive(Debug, Deserialize)]
struct GithubUser {
    login: String,
    name: Option<String>,
}

/// An entry from `GET /user/emails`.
#[derive(Debug, Deserialize)]
struct GithubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OAuthProviderKind;

    fn config() -> OidcConfig {
        OidcConfig {
            kind: OAuthProviderKind::Github,
            issuer_url: "https://github.com".into(),
            client_id: "client-id".into(),
            client_secret: "client-secret".into(),
            redirect_url: "https://app.example.com/auth/oidc/callback".into(),
            scopes: vec!["read:user".into(), "user:email".into()],
            provider_name: "GitHub".into(),
            allowed_email_domains: vec![],
            require_approval: false,
        }
    }

    #[test]
    fn authorize_url_targets_github_with_state_and_scope() {
        let provider = GithubProvider::new(&config()).expect("build provider");
        let req = provider.authorize_url();

        assert!(
            req.url
                .as_str()
                .starts_with("https://github.com/login/oauth/authorize?"),
            "got {}",
            req.url
        );

        let params: std::collections::HashMap<_, _> = req.url.query_pairs().collect();
        assert_eq!(
            params.get("client_id").map(|c| c.as_ref()),
            Some("client-id")
        );
        assert_eq!(
            params.get("redirect_uri").map(|c| c.as_ref()),
            Some("https://app.example.com/auth/oidc/callback")
        );
        assert_eq!(
            params.get("scope").map(|c| c.as_ref()),
            Some("read:user user:email")
        );
        // The `state` query param must equal the returned CSRF token (bound into
        // the signed state cookie by the handler).
        assert_eq!(
            params.get("state").map(|c| c.as_ref()),
            Some(req.csrf_state.secret().as_str())
        );
    }

    #[test]
    fn new_rejects_invalid_redirect_url() {
        let mut cfg = config();
        cfg.redirect_url = "not a url".into();
        assert!(GithubProvider::new(&cfg).is_err());
    }
}
