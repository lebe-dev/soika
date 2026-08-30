//! Email composition and SMTP delivery.
//!
//! Two responsibilities:
//!
//! 1. **Composition** ([`invite_email`], [`new_issue_email`], [`regression_email`]):
//!    functions that render an [`OutboundEmail`] from domain data. The issue
//!    notifications render their HTML + text bodies from Tera templates loaded
//!    via [`Templates`]; the rest are short inline plain-text messages.
//! 2. **Delivery** ([`send_via_smtp`]): a `lettre`-backed helper that the SMTP
//!    [`Mailer`](crate::ports::Mailer) adapter delegates to. Kept here (rather than
//!    inlined in the adapter) so the lettre wiring lives next to the message
//!    builders and is reusable.
//!
//! When SMTP is unset the app degrades gracefully — the no-op mailer simply drops
//! messages, and invite **links** still work.

use lettre::message::header::ContentType;
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::{AsyncSmtpTransport, AsyncSmtpTransportBuilder};
use lettre::{AsyncTransport, Message, Tokio1Executor};
use tera::{Context, Tera};

use crate::config::{Config, SmtpConfig};
use crate::domain::{Invite, Issue, Project};
use crate::error::{Error, Result};
use crate::ports::OutboundEmail;

/// Tera templates for outbound email, loaded once at startup and held in
/// [`AppState`](crate::state::AppState).
///
/// Templates live as files under a directory (default `templates/`, see
/// [`templates_dir`]) so operators can tweak the email look without rebuilding;
/// the directory is shipped alongside the binary (see the Dockerfile).
pub struct Templates {
    tera: Tera,
}

/// Template names that must be present after loading. Missing templates fail
/// startup (via [`Templates::load`]) rather than the first notification.
const REQUIRED_TEMPLATES: [&str; 2] = ["email/notification.html", "email/notification.txt"];

impl Templates {
    /// Load every template under `dir` (recursively). Fails fast if the glob
    /// can't be compiled or a [`REQUIRED_TEMPLATES`] entry is absent, so a
    /// misconfigured deployment is caught at boot, not at send time.
    pub fn load(dir: &str) -> Result<Self> {
        let glob = format!("{}/**/*", dir.trim_end_matches('/'));
        let tera =
            Tera::new(&glob).map_err(|e| Error::Template(format!("loading {glob:?}: {e}")))?;

        let names: std::collections::HashSet<&str> = tera.get_template_names().collect();
        for required in REQUIRED_TEMPLATES {
            if !names.contains(required) {
                return Err(Error::Template(format!(
                    "template {required:?} not found under {dir:?}"
                )));
            }
        }
        Ok(Self { tera })
    }

    /// Render a template to a `String`, mapping any Tera error to
    /// [`Error::Template`].
    fn render(&self, name: &str, ctx: &Context) -> Result<String> {
        self.tera
            .render(name, ctx)
            .map_err(|e| Error::Template(format!("rendering {name:?}: {e}")))
    }
}

/// The directory email templates are loaded from: `TEMPLATES_DIR` if set,
/// otherwise `templates` (relative to the working directory). The Dockerfile
/// copies `templates/` next to the binary and runs from that directory.
pub fn templates_dir() -> String {
    std::env::var("TEMPLATES_DIR").unwrap_or_else(|_| "templates".to_string())
}

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
        html_body: None,
    }
}

/// Build the "new issue" notification email (trigger 1).
///
/// Leads with the project and the error itself — the recipient cares what broke
/// and where, not the instance/organization name (which is demoted to a small
/// brand line in the footer). HTML + plain-text bodies are rendered from
/// `email/notification.*`.
pub fn new_issue_email(
    templates: &Templates,
    config: &Config,
    to: &str,
    project: &Project,
    issue: &Issue,
) -> Result<OutboundEmail> {
    notification_email(
        templates,
        config,
        to,
        project,
        issue,
        "new_issue",
        "New issue",
    )
}

/// Build the "regression" notification email (trigger 2).
pub fn regression_email(
    templates: &Templates,
    config: &Config,
    to: &str,
    project: &Project,
    issue: &Issue,
) -> Result<OutboundEmail> {
    notification_email(
        templates,
        config,
        to,
        project,
        issue,
        "regression",
        "Regression",
    )
}

/// Render a new-issue/regression notification into an [`OutboundEmail`] with an
/// HTML part and a plain-text fallback, both from Tera templates. The template
/// owns all chrome and copy; this only assembles the data context and the
/// (truncation-constrained) subject line.
///
/// `kind` selects the template branch (`"new_issue"` | `"regression"`);
/// `subject_kind` is the word used in the subject (`[project] <kind>: <title>`).
fn notification_email(
    templates: &Templates,
    config: &Config,
    to: &str,
    project: &Project,
    issue: &Issue,
    kind: &str,
    subject_kind: &str,
) -> Result<OutboundEmail> {
    let subject = format!(
        "[{}] {}: {}",
        project.name,
        subject_kind,
        truncate_subject(&issue.title),
    );

    let mut ctx = Context::new();
    ctx.insert("kind", kind);
    ctx.insert("project_name", &project.name);
    ctx.insert("title", &issue.title);
    ctx.insert("culprit", issue.culprit.as_deref().unwrap_or(""));
    ctx.insert("level", issue.level.as_deref().unwrap_or(""));
    ctx.insert("environment", issue.environment.as_deref().unwrap_or(""));
    ctx.insert("event_count", &issue.event_count);
    ctx.insert(
        "first_seen",
        &issue.first_seen.format("%Y-%m-%d %H:%M UTC").to_string(),
    );
    ctx.insert("issue_url", &issue_link(config, project, issue));
    ctx.insert("settings_url", &settings_link(config));
    ctx.insert("org_name", &config.organization_name);

    let html = templates.render("email/notification.html", &ctx)?;
    let body = templates.render("email/notification.txt", &ctx)?;

    Ok(OutboundEmail {
        to: to.to_string(),
        subject,
        body,
        html_body: Some(html),
    })
}

/// Build the "test email" used by the admin UI to verify SMTP delivery.
///
/// Self-contained and link-free: receiving it is the only signal needed to
/// confirm the SMTP relay, credentials, and From address all work.
pub fn test_email(config: &Config, to: &str) -> OutboundEmail {
    let org = &config.organization_name;
    let subject = format!("[{org}] Test email");
    let body = format!(
        "This is a test email from {org}.\n\n\
         If you can read this, SMTP delivery is configured correctly.\n\n\
         — {org}",
    );
    OutboundEmail {
        to: to.to_string(),
        subject,
        body,
        html_body: None,
    }
}

// --- Links for issue notifications --------------------------------------------

/// Deep link to the issue, mirroring the webhook payload's `issue_url` and the
/// SPA route `/projects/[id]/issues/[issueId]`.
fn issue_link(config: &Config, project: &Project, issue: &Issue) -> String {
    format!(
        "{}/projects/{}/issues/{}",
        config.base_url.trim_end_matches('/'),
        project.short_id,
        issue.short_id,
    )
}

/// Link to the profile page, where the per-user notification toggle lives.
fn settings_link(config: &Config) -> String {
    format!("{}/profile", config.base_url.trim_end_matches('/'))
}

/// Deliver an [`OutboundEmail`] over SMTP using `lettre`.
///
/// The SMTP [`Mailer`](crate::ports::Mailer) adapter delegates here. Returns
/// [`Error::Mail`] for any composition or transport failure so the caller can
/// log-and-continue (notifications never fail ingestion).
pub async fn send_via_smtp(smtp: &SmtpConfig, email: OutboundEmail) -> Result<()> {
    let from = smtp_from_mailbox(smtp)?;
    let message = build_message(from, email)?;

    let transport = build_transport(smtp)?;
    transport.send(message).await.map_err(|e| {
        // Surface transport failures (connect/TLS/auth-rejected/timeout) here.
        // NEVER log credentials or the message body — `e` is the transport error.
        tracing::warn!(error = %e, "smtp send failed");
        Error::Mail(format!("sending message: {e}"))
    })?;
    Ok(())
}

/// Build the `lettre` [`Message`] from an [`OutboundEmail`].
///
/// When `html_body` is set the message is `multipart/alternative` with the
/// plain-text `body` first and the HTML part last (RFC 2046: the last
/// alternative is the most-preferred, so clients render the HTML). Text-only
/// emails stay a single `text/plain` part.
fn build_message(from: Mailbox, email: OutboundEmail) -> Result<Message> {
    let to: Mailbox = email
        .to
        .parse()
        .map_err(|e| Error::Mail(format!("invalid recipient address {:?}: {e}", email.to)))?;

    let builder = Message::builder().from(from).to(to).subject(email.subject);
    let built = match email.html_body {
        Some(html) => builder.multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(email.body))
                .singlepart(SinglePart::html(html)),
        ),
        None => builder.header(ContentType::TEXT_PLAIN).body(email.body),
    };
    built.map_err(|e| {
        // Log the failure at the adapter so a misconfigured relay is visible
        // even though the caller log-and-continues. NEVER log credentials or
        // the message body — `e` carries only the lettre build error.
        tracing::warn!(error = %e, "smtp send failed");
        Error::Mail(format!("building message: {e}"))
    })
}

/// The SMTP submissions port (465) uses implicit TLS; other ports (587, 25) use
/// STARTTLS. This mirrors `lettre`'s `relay` vs. `starttls_relay` helpers.
const SUBMISSIONS_PORT: u16 = 465;

/// Build the async SMTP transport from config.
///
/// When `tls` is disabled the transport connects in plaintext via
/// `builder_dangerous` (no TLS, no STARTTLS) — for local relays such as
/// mailcrab/MailHog. Otherwise it selects implicit TLS on port 465 (`relay`) and
/// STARTTLS on every other port (`starttls_relay`, the common case for the
/// default port 587). Credentials are attached only when both username and
/// password are present.
fn build_transport(smtp: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    if !smtp.tls {
        let builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host);
        return Ok(with_credentials(builder.port(smtp.port), smtp).build());
    }

    let builder = if smtp.port == SUBMISSIONS_PORT {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
    }
    .map_err(|e| Error::Mail(format!("configuring SMTP relay {:?}: {e}", smtp.host)))?;

    Ok(with_credentials(builder.port(smtp.port), smtp).build())
}

/// Attach SMTP credentials to the builder when both username and password are
/// present; otherwise leave it unauthenticated.
fn with_credentials(
    builder: AsyncSmtpTransportBuilder,
    smtp: &SmtpConfig,
) -> AsyncSmtpTransportBuilder {
    let (Some(username), Some(password)) = (smtp.username.as_ref(), smtp.password.as_ref()) else {
        return builder;
    };
    builder.credentials(Credentials::new(username.clone(), password.clone()))
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
    use crate::domain::TeamRole;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn test_config() -> Config {
        Config {
            organization_name: "Acme".to_string(),
            database_url: "sqlite::memory:".to_string(),
            db: crate::config::DbConfig::default(),
            bind_addr: "0.0.0.0:8080".to_string(),
            base_url: "https://errors.example.com/".to_string(),
            secret_key: "secret".to_string(),
            allow_signup: false,
            default_events_retention: 1000,
            default_retention_days: 0,
            retention_cron: "0 */15 * * * *".to_string(),
            smtp: None,
            oidc: None,
            passkey: None,
            sentry: None,
            lockout: Default::default(),
            timezone: "UTC".to_string(),
        }
    }

    fn test_invite() -> Invite {
        Invite {
            token: "tok-123".to_string(),
            team_id: Uuid::nil(),
            role: TeamRole::Contributor,
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

    fn test_project() -> Project {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        Project {
            id: Uuid::nil(),
            short_id: "web001".to_string(),
            team_id: Uuid::nil(),
            name: "checkout-api".to_string(),
            slug: "checkout-api".to_string(),
            dsn_public_key: "pk".to_string(),
            retention_events: 1000,
            retention_days: 0,
            muted: false,
            webhook_url: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn test_issue(title: &str) -> Issue {
        let now = Utc.with_ymd_and_hms(2026, 1, 2, 9, 30, 0).unwrap();
        Issue {
            id: Uuid::nil(),
            short_id: "iss001".to_string(),
            project_id: Uuid::nil(),
            fingerprint: "fp".to_string(),
            title: title.to_string(),
            culprit: Some("checkout.handlers.charge".to_string()),
            level: Some("error".to_string()),
            environment: Some("production".to_string()),
            release: None,
            status: crate::domain::IssueStatus::Unresolved,
            muted_at: None,
            muted_until: None,
            mute_threshold: None,
            mute_window_seconds: None,
            first_seen: now,
            last_seen: now,
            event_count: 42,
            created_at: now,
            updated_at: now,
        }
    }

    /// Load the real templates from the repo `templates/` dir (CWD-independent),
    /// so these unit tests exercise the same files shipped in the image.
    fn test_templates() -> Templates {
        Templates::load(concat!(env!("CARGO_MANIFEST_DIR"), "/templates")).expect("templates load")
    }

    #[test]
    fn new_issue_email_leads_with_project_and_error() {
        let email = new_issue_email(
            &test_templates(),
            &test_config(),
            "dev@example.com",
            &test_project(),
            &test_issue("ValueError: boom"),
        )
        .unwrap();
        // Subject is project-scoped, not org-scoped.
        assert!(email.subject.contains("[checkout-api]"));
        assert!(email.subject.contains("New issue"));
        assert!(email.subject.contains("ValueError: boom"));
        assert!(!email.subject.contains("Acme"));

        let html = email.html_body.as_deref().expect("html part present");
        // Project and error are the hero; org is demoted to the footer fine print.
        assert!(html.contains("New issue in checkout-api"));
        assert!(html.contains("ValueError: boom"));
        assert!(html.contains("checkout.handlers.charge")); // culprit
        assert!(html.contains("production")); // environment
        assert!(html.contains("42")); // event count
        // Branding: primary color for chrome/CTA, red for an error severity.
        assert!(html.contains("#1d85e5")); // --primary
        assert!(html.contains("#dc2626")); // Tailwind red-600
        // Deep link to the issue and the unsubscribe/settings page.
        // Deep link uses the short public ids (web001/iss001), not the UUIDs —
        // mirrors the SPA route so the link resolves in the dashboard.
        assert!(html.contains("https://errors.example.com/projects/web001/issues/iss001"));
        assert!(html.contains("https://errors.example.com/profile"));
        // Footer explains why the recipient got this.
        assert!(html.contains("You're receiving this because"));
        // Plain-text fallback carries the same signal.
        assert!(email.body.contains("New issue in checkout-api"));
        assert!(email.body.contains("https://errors.example.com/profile"));
    }

    #[test]
    fn regression_email_mentions_regression() {
        let email = regression_email(
            &test_templates(),
            &test_config(),
            "dev@example.com",
            &test_project(),
            &test_issue("KeyError: nope"),
        )
        .unwrap();
        assert!(email.subject.contains("Regression"));
        assert!(email.subject.contains("[checkout-api]"));
        let html = email.html_body.as_deref().expect("html part present");
        assert!(html.contains("regressed in checkout-api"));
        assert!(html.contains("KeyError: nope"));
        assert!(email.body.contains("happening again"));
    }

    #[test]
    fn notification_html_escapes_user_controlled_title() {
        // Error titles come from untrusted SDK payloads — they must not inject
        // markup. Tera auto-escapes `{{ }}` in the `.html` template.
        let email = new_issue_email(
            &test_templates(),
            &test_config(),
            "dev@example.com",
            &test_project(),
            &test_issue("<script>alert('x')</script>"),
        )
        .unwrap();
        let html = email.html_body.unwrap();
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn notification_message_is_multipart_alternative_with_html() {
        let email = new_issue_email(
            &test_templates(),
            &test_config(),
            "dev@example.com",
            &test_project(),
            &test_issue("ValueError: boom"),
        )
        .unwrap();
        let from: Mailbox = "soika@example.com".parse().unwrap();
        let message = build_message(from, email).unwrap();
        let raw = String::from_utf8(message.formatted()).unwrap();
        // The wire message must be multipart/alternative carrying BOTH a
        // text/plain fallback and the styled text/html part.
        assert!(
            raw.contains("multipart/alternative"),
            "not multipart: {raw}"
        );
        assert!(raw.contains("text/plain"));
        assert!(raw.contains("text/html"));
        assert!(raw.contains("New issue in checkout-api"));
    }

    #[test]
    fn text_only_message_stays_single_text_plain_part() {
        // Invite/test emails have no HTML part — they must NOT become multipart.
        let email = invite_email(&test_config(), "dev@example.com", &test_invite());
        let from: Mailbox = "soika@example.com".parse().unwrap();
        let raw = String::from_utf8(build_message(from, email).unwrap().formatted()).unwrap();
        assert!(!raw.contains("multipart"));
        assert!(raw.contains("text/plain"));
    }

    #[test]
    fn severity_drives_accent_color() {
        let templates = test_templates();
        let config = test_config();
        let project = test_project();
        let render = |level: &str| {
            let mut issue = test_issue("boom");
            issue.level = Some(level.to_string());
            new_issue_email(&templates, &config, "d@e.com", &project, &issue)
                .unwrap()
                .html_body
                .unwrap()
        };
        assert!(render("error").contains("#dc2626")); // red-600
        assert!(render("fatal").contains("#dc2626"));
        assert!(render("warning").contains("#f59e0b")); // amber-500
        let info = render("info");
        assert!(info.contains("#1d85e5")); // falls back to primary
        assert!(!info.contains("#dc2626"));
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
    fn test_email_is_self_contained_and_org_branded() {
        let email = test_email(&test_config(), "admin@example.com");
        assert_eq!(email.to, "admin@example.com");
        assert!(email.subject.contains("Acme"));
        assert!(email.subject.contains("Test email"));
        assert!(email.body.contains("Acme"));
        // No link to render — it only proves delivery works.
        assert!(!email.body.contains("http"));
    }

    #[test]
    fn from_mailbox_defaults_to_org_host_when_unset() {
        let smtp = SmtpConfig {
            host: "mail.example.com".to_string(),
            port: 587,
            username: None,
            password: None,
            from: None,
            tls: true,
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
            tls: true,
        };
        let mbox = smtp_from_mailbox(&smtp).expect("configured from parses");
        assert_eq!(mbox.email.to_string(), "alerts@example.com");
    }

    #[test]
    fn build_transport_plaintext_when_tls_disabled() {
        // mailcrab/MailHog: plaintext relay, no TLS. Must build without error
        // even though no TLS feature is negotiated.
        let smtp = SmtpConfig {
            host: "localhost".to_string(),
            port: 1025,
            username: None,
            password: None,
            from: None,
            tls: false,
        };
        assert!(build_transport(&smtp).is_ok());
    }

    #[test]
    fn build_transport_tls_relay_builds() {
        let smtp = SmtpConfig {
            host: "smtp.example.com".to_string(),
            port: 587,
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            from: None,
            tls: true,
        };
        assert!(build_transport(&smtp).is_ok());
    }

    #[test]
    fn from_mailbox_rejects_garbage() {
        let smtp = SmtpConfig {
            host: "mail.example.com".to_string(),
            port: 587,
            username: None,
            password: None,
            from: Some("not an email".to_string()),
            tls: true,
        };
        assert!(matches!(smtp_from_mailbox(&smtp), Err(Error::Mail(_))));
    }
}
