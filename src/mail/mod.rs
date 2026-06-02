//! Email composition and SMTP delivery.
//!
//! Two responsibilities:
//!
//! 1. **Composition** ([`invite_email`], [`new_issue_email`], [`regression_email`]):
//!    pure functions that render an [`OutboundEmail`] from domain data. They have
//!    no I/O, so they are trivially unit-testable.
//! 2. **Delivery** ([`send_via_smtp`]): a `lettre`-backed helper that the SMTP
//!    [`Mailer`](crate::ports::Mailer) adapter delegates to. Kept here (rather than
//!    inlined in the adapter) so the lettre wiring lives next to the message
//!    builders and is reusable.
//!
//! When SMTP is unset the app degrades gracefully — the no-op mailer simply drops
//! messages, and invite **links** still work.

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncTransport, Message, Tokio1Executor};

use crate::config::{Config, SmtpConfig};
use crate::domain::Invite;
use crate::error::{Error, Result};
use crate::ports::OutboundEmail;

/// Build the invite email for a generated invite link.
///
/// The link is always usable even without SMTP, so the body leads with the
/// copyable URL.
pub fn invite_email(config: &Config, to: &str, invite: &Invite) -> OutboundEmail {
    let link = invite_link(config, &invite.token);
    let org = &config.organization_name;
    let subject = format!("You've been invited to {org}");
    let body = format!(
        "Hello,\n\n\
         You have been invited to join a project on {org}.\n\n\
         Accept the invitation by opening this link:\n\
         {link}\n\n\
         This invite expires on {expires}.\n\n\
         — {org}",
        expires = invite.expires_at.format("%Y-%m-%d %H:%M UTC"),
    );
    OutboundEmail {
        to: to.to_string(),
        subject,
        body,
    }
}

/// Build the "new issue" notification email (trigger 1).
pub fn new_issue_email(config: &Config, to: &str, issue_title: &str) -> OutboundEmail {
    let org = &config.organization_name;
    let subject = format!("[{org}] New issue: {}", truncate_subject(issue_title));
    let body = format!(
        "A new issue was detected on {org}:\n\n\
         {issue_title}\n\n\
         View it in the dashboard:\n\
         {base}\n\n\
         — {org}",
        base = config.base_url.trim_end_matches('/'),
    );
    OutboundEmail {
        to: to.to_string(),
        subject,
        body,
    }
}

/// Build the "regression" notification email (trigger 2).
pub fn regression_email(config: &Config, to: &str, issue_title: &str) -> OutboundEmail {
    let org = &config.organization_name;
    let subject = format!("[{org}] Regression: {}", truncate_subject(issue_title));
    let body = format!(
        "A resolved issue has regressed on {org}:\n\n\
         {issue_title}\n\n\
         It received a new event and is unresolved again.\n\n\
         View it in the dashboard:\n\
         {base}\n\n\
         — {org}",
        base = config.base_url.trim_end_matches('/'),
    );
    OutboundEmail {
        to: to.to_string(),
        subject,
        body,
    }
}

/// Deliver an [`OutboundEmail`] over SMTP using `lettre`.
///
/// The SMTP [`Mailer`](crate::ports::Mailer) adapter delegates here. Returns
/// [`Error::Mail`] for any composition or transport failure so the caller can
/// log-and-continue (notifications never fail ingestion).
pub async fn send_via_smtp(smtp: &SmtpConfig, email: OutboundEmail) -> Result<()> {
    let from = smtp_from_mailbox(smtp)?;
    let to: Mailbox = email
        .to
        .parse()
        .map_err(|e| Error::Mail(format!("invalid recipient address {:?}: {e}", email.to)))?;

    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(email.subject)
        .header(ContentType::TEXT_PLAIN)
        .body(email.body)
        .map_err(|e| Error::Mail(format!("building message: {e}")))?;

    let transport = build_transport(smtp)?;
    transport
        .send(message)
        .await
        .map_err(|e| Error::Mail(format!("sending message: {e}")))?;
    Ok(())
}

/// The SMTP submissions port (465) uses implicit TLS; other ports (587, 25) use
/// STARTTLS. This mirrors `lettre`'s `relay` vs. `starttls_relay` helpers.
const SUBMISSIONS_PORT: u16 = 465;

/// Build the async SMTP transport from config.
///
/// Selects implicit TLS on port 465 (`relay`) and STARTTLS on every other port
/// (`starttls_relay`, the common case for the default port 587). Credentials are
/// attached only when both username and password are present.
fn build_transport(smtp: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let builder = if smtp.port == SUBMISSIONS_PORT {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
    }
    .map_err(|e| Error::Mail(format!("configuring SMTP relay {:?}: {e}", smtp.host)))?;

    let mut builder = builder.port(smtp.port);

    if let (Some(username), Some(password)) = (smtp.username.as_ref(), smtp.password.as_ref()) {
        builder = builder.credentials(Credentials::new(username.clone(), password.clone()));
    }

    Ok(builder.build())
}

/// Resolve the From mailbox, defaulting to `soika@<smtp-host>` when `SMTP_FROM`
/// is unset.
fn smtp_from_mailbox(smtp: &SmtpConfig) -> Result<Mailbox> {
    let raw = smtp
        .from
        .clone()
        .unwrap_or_else(|| format!("soika@{}", smtp.host));
    raw.parse()
        .map_err(|e| Error::Mail(format!("invalid SMTP_FROM address {raw:?}: {e}")))
}

/// Build the public `{BASE_URL}/invite/{token}` link.
fn invite_link(config: &Config, token: &str) -> String {
    format!("{}/invite/{token}", config.base_url.trim_end_matches('/'))
}

/// Keep subject lines short and single-line.
fn truncate_subject(s: &str) -> String {
    const MAX: usize = 120;
    let s = s.replace(['\n', '\r'], " ");
    let s = s.trim();
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let truncated: String = s.chars().take(MAX).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Role;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn test_config() -> Config {
        Config {
            organization_name: "Acme".to_string(),
            database_url: "sqlite::memory:".to_string(),
            bind_addr: "0.0.0.0:8080".to_string(),
            base_url: "https://errors.example.com/".to_string(),
            secret_key: "secret".to_string(),
            allow_signup: false,
            default_events_retention: 1000,
            default_retention_days: 0,
            retention_cron: "0 */15 * * * *".to_string(),
            smtp: None,
            oidc: None,
            lockout: Default::default(),
        }
    }

    fn test_invite() -> Invite {
        Invite {
            token: "tok-123".to_string(),
            project_id: Uuid::nil(),
            role: Role::Member,
            email: Some("dev@example.com".to_string()),
            created_by: None,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            expires_at: Utc.with_ymd_and_hms(2026, 1, 8, 0, 0, 0).unwrap(),
            accepted_at: None,
        }
    }

    #[test]
    fn invite_email_contains_link_with_token_and_no_double_slash() {
        let email = invite_email(&test_config(), "dev@example.com", &test_invite());
        assert_eq!(email.to, "dev@example.com");
        // base_url has a trailing slash; the link must not double it.
        assert!(
            email
                .body
                .contains("https://errors.example.com/invite/tok-123")
        );
        assert!(!email.body.contains("com//invite"));
        assert!(email.subject.contains("Acme"));
        assert!(email.body.contains("2026-01-08"));
    }

    #[test]
    fn new_issue_email_has_org_and_title() {
        let email = new_issue_email(&test_config(), "dev@example.com", "ValueError: boom");
        assert!(email.subject.contains("Acme"));
        assert!(email.subject.contains("New issue"));
        assert!(email.subject.contains("ValueError: boom"));
        assert!(email.body.contains("ValueError: boom"));
        assert!(email.body.contains("https://errors.example.com"));
    }

    #[test]
    fn regression_email_mentions_regression() {
        let email = regression_email(&test_config(), "dev@example.com", "KeyError: nope");
        assert!(email.subject.contains("Regression"));
        assert!(email.body.contains("regressed"));
        assert!(email.body.contains("KeyError: nope"));
    }

    #[test]
    fn subject_is_truncated_and_single_line() {
        let long = format!("line1\nline2{}", "x".repeat(300));
        let out = truncate_subject(&long);
        assert!(!out.contains('\n'));
        assert!(out.chars().count() <= 121); // 120 + ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn from_mailbox_defaults_to_org_host_when_unset() {
        let smtp = SmtpConfig {
            host: "mail.example.com".to_string(),
            port: 587,
            username: None,
            password: None,
            from: None,
        };
        let mbox = smtp_from_mailbox(&smtp).expect("default from parses");
        assert_eq!(mbox.email.to_string(), "soika@mail.example.com");
    }

    #[test]
    fn from_mailbox_uses_configured_from() {
        let smtp = SmtpConfig {
            host: "mail.example.com".to_string(),
            port: 587,
            username: None,
            password: None,
            from: Some("Alerts <alerts@example.com>".to_string()),
        };
        let mbox = smtp_from_mailbox(&smtp).expect("configured from parses");
        assert_eq!(mbox.email.to_string(), "alerts@example.com");
    }

    #[test]
    fn from_mailbox_rejects_garbage() {
        let smtp = SmtpConfig {
            host: "mail.example.com".to_string(),
            port: 587,
            username: None,
            password: None,
            from: Some("not an email".to_string()),
        };
        assert!(matches!(smtp_from_mailbox(&smtp), Err(Error::Mail(_))));
    }
}
