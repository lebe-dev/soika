//! Notification orchestration.
//!
//! Decides who to notify on new-issue / regression events and dispatches over
//! three channels under the SAME suppression rules:
//!
//! - **email** — one message per opted-in member of the project's owning team;
//! - **Alertmanager** — durable firing/resolved alerts with configurable templates;
//! - **webhook** ([`Project::webhook_url`]) — a single HTTP POST JSON payload to
//!   a per-project URL, generalizing the email-only path so channels like
//!   Telegram can be added later through the same seam.
//!
//! All channels respect three independent suppression rules:
//!
//! - **project mute** ([`Project::muted`]) — suppresses notifications for the
//!   whole project;
//! - **issue mute** ([`IssueStatus::Muted`]) — suppresses notifications for the
//!   single issue, while ingestion/counting continue;
//! - **per-user opt-out** ([`User::notifications_enabled`]) — a profile setting
//!   that excludes a user from all email. (Webhook delivery is
//!   project-scoped and has no per-user opt-out.)
//!
//! The suppression decisions are factored into pure helpers ([`is_suppressed`],
//! [`recipients`]) so they can be unit-tested without I/O, and the webhook wire
//! shape is built by the pure [`build_webhook_payload`] helper. The async entry
//! points are invoked from the ingest pipeline and never propagate
//! transient delivery failures back to ingestion — those are logged.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

use crate::domain::{Issue, IssueStatus, Project, Timestamp, User};
use crate::mail;
use crate::state::AppState;

/// Hard cap on how long a single webhook POST may take, so a hung endpoint
/// cannot stall the ingest task indefinitely. Delivery failures are
/// always swallowed, but a missing timeout would still block ingestion.
const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a notification was (not) sent — kept small so callers can branch/log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suppression {
    /// Notifications are not suppressed; deliver.
    None,
    /// The whole project is muted.
    ProjectMuted,
    /// The specific issue is muted.
    IssueMuted,
}

/// Decide whether notifications for `issue` in `project` are suppressed.
///
/// Project mute takes precedence over issue mute for reporting purposes, but
/// either being set means no notification is sent.
pub fn is_suppressed(project: &Project, issue: &Issue) -> Suppression {
    if project.muted {
        return Suppression::ProjectMuted;
    }
    if issue.status == IssueStatus::Muted {
        return Suppression::IssueMuted;
    }
    Suppression::None
}

/// Whether a single user should receive notifications (per-user opt-out).
pub fn user_opted_in(user: &User) -> bool {
    user.notifications_enabled
}

/// The email addresses to notify for the owning team's members, after applying
/// the per-user opt-out. Project/issue mute is handled separately by
/// [`is_suppressed`]; this only filters the member list.
///
/// `members` is the team membership joined with each user's profile. The role
/// is irrelevant for notifications (admins and contributors are notified alike),
/// so it is ignored here.
pub fn recipients<R>(members: &[(User, R)]) -> Vec<String> {
    members
        .iter()
        .filter(|(user, _)| user_opted_in(user))
        .map(|(user, _)| user.email.clone())
        .collect()
}

/// Notify relevant members that a new issue was seen (trigger 1).
pub async fn notify_new_issue(
    state: &AppState,
    project: &Project,
    issue: &Issue,
) -> crate::error::Result<()> {
    dispatch(state, project, issue, Trigger::NewIssue).await
}

/// Notify relevant members that a resolved issue regressed (trigger 2).
pub async fn notify_regression(
    state: &AppState,
    project: &Project,
    issue: &Issue,
) -> crate::error::Result<()> {
    dispatch(state, project, issue, Trigger::Regression).await
}

/// The notification trigger, selecting which email template to render and the
/// webhook annotation's `trigger` string. Crate-visible so the pure
/// [`build_webhook_payload`] helper can expose it in its signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    NewIssue,
    Regression,
}

impl Trigger {
    /// Stable wire string used in the webhook payload. Distinct from
    /// the email templates so downstream consumers can branch on it.
    fn as_str(&self) -> &'static str {
        match self {
            Trigger::NewIssue => "new_issue",
            Trigger::Regression => "regression",
        }
    }
}

/// JSON body POSTed to a project's [`Project::webhook_url`] on a new-issue or
/// regression notification, using the Alertmanager v4 webhook format.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebhookPayload {
    pub version: &'static str,
    pub group_key: String,
    pub truncated_alerts: u64,
    pub status: &'static str,
    pub receiver: &'static str,
    pub group_labels: BTreeMap<&'static str, String>,
    pub common_labels: BTreeMap<&'static str, String>,
    pub common_annotations: BTreeMap<&'static str, String>,
    #[serde(rename = "externalURL")]
    pub external_url: String,
    pub alerts: Vec<WebhookAlert>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebhookAlert {
    pub status: &'static str,
    pub labels: BTreeMap<&'static str, String>,
    pub annotations: BTreeMap<&'static str, String>,
    pub starts_at: Timestamp,
    /// Go's zero time: the firing alert has no known end.
    pub ends_at: &'static str,
    #[serde(rename = "generatorURL")]
    pub generator_url: String,
    pub fingerprint: String,
}

/// Build the webhook payload for an issue notification (pure; unit-tested).
///
/// `generatorURL` is `{base_url}/projects/{project_short_id}/issues/{issue_short_id}`
/// with any trailing slash on `base_url` trimmed (mirroring `mail.rs` link
/// building and the frontend route `/projects/[id]/issues/[issueId]`, both of
/// which use the short public ids — not the UUIDs — so the link resolves in the
/// SPA).
pub(crate) fn build_webhook_payload(
    base_url: &str,
    project: &Project,
    issue: &Issue,
    trigger: Trigger,
) -> WebhookPayload {
    let base = base_url.trim_end_matches('/');
    let issue_url = format!(
        "{base}/projects/{}/issues/{}",
        project.short_id, issue.short_id
    );
    let group_labels = BTreeMap::from([
        ("project_id", project.id.to_string()),
        ("issue_id", issue.id.to_string()),
    ]);
    let mut labels = group_labels.clone();
    labels.insert("alertname", "SoikaIssue".to_string());
    labels.insert("project", project.slug.clone());
    if let Some(level) = &issue.level {
        labels.insert("severity", level.clone());
    }
    // Changing values belong in annotations so a regression keeps its identity.
    let annotations = BTreeMap::from([
        ("summary", issue.title.clone()),
        ("trigger", trigger.as_str().to_string()),
        ("event_count", issue.event_count.to_string()),
    ]);
    let alert = WebhookAlert {
        status: "firing",
        labels: labels.clone(),
        annotations: annotations.clone(),
        starts_at: match trigger {
            Trigger::NewIssue => issue.first_seen,
            Trigger::Regression => issue.last_seen,
        },
        ends_at: "0001-01-01T00:00:00Z",
        generator_url: issue_url,
        // The issue UUID stays unique across projects with the same grouping hash.
        fingerprint: issue.id.simple().to_string(),
    };
    WebhookPayload {
        version: "4",
        group_key: format!("soika:{}:{}", project.id, issue.id),
        truncated_alerts: 0,
        status: "firing",
        receiver: "soika",
        group_labels,
        common_labels: labels,
        common_annotations: annotations,
        external_url: base.to_string(),
        alerts: vec![alert],
    }
}

/// Shared dispatch path for both triggers: apply suppression, resolve recipients,
/// render the email, and deliver to each (logging per-recipient failures).
async fn dispatch(
    state: &AppState,
    project: &Project,
    issue: &Issue,
    trigger: Trigger,
) -> crate::error::Result<()> {
    // Mute checks first — cheapest, and short-circuit before any I/O. This gates
    // BOTH channels: a project with a webhook_url set but muted is still
    // suppressed.
    match is_suppressed(project, issue) {
        Suppression::None => {}
        suppression => {
            tracing::debug!(
                issue_id = %issue.id,
                project_id = %project.id,
                ?suppression,
                "notification suppressed",
            );
            return Ok(());
        }
    }

    // Each project-scoped channel is independent of SMTP. Delivery failures
    // are logged without failing ingestion.
    if let Err(error) = crate::alertmanager::notify(state, project, issue, trigger.as_str()).await {
        tracing::warn!(error = %error, issue_id = %issue.id, channel = "alertmanager", "notification delivery failed");
    }

    if state.config.webhooks_enabled
        && let Some(url) = project.webhook_url.as_deref()
    {
        send_webhook(state, project, issue, trigger, url).await;
    } else if !state.config.webhooks_enabled {
        tracing::debug!(issue_id = %issue.id, "webhooks disabled; skipping webhook notification");
    }

    // If email isn't even configured, skip the member lookup entirely.
    if !state.mailer.is_enabled() {
        tracing::debug!(issue_id = %issue.id, "mailer disabled; skipping email notification");
        return Ok(());
    }

    // Recipients are the members of the team that owns the project (Variant A:
    // project access derives from team membership). The team role is irrelevant
    // for notifications, so it is ignored by `recipients`.
    let members = state.teams.members(project.team_id).await?;
    let recipients = recipients(&members);
    if recipients.is_empty() {
        return Ok(());
    }

    let mut delivered = 0u64;
    let mut failed = 0u64;
    for to in recipients {
        let email = match trigger {
            Trigger::NewIssue => {
                mail::new_issue_email(&state.templates, &state.config, &to, project, issue)
            }
            Trigger::Regression => {
                mail::regression_email(&state.templates, &state.config, &to, project, issue)
            }
        };
        // A template error affects every recipient identically — log once and
        // stop rather than re-rendering (and re-logging) for each address.
        let email = match email {
            Ok(email) => email,
            Err(err) => {
                tracing::error!(error = %err, issue_id = %issue.id, "rendering notification email failed");
                return Ok(());
            }
        };
        // A failure to one recipient must not block the others or fail ingestion.
        if let Err(err) = state.mailer.send(email).await {
            failed += 1;
            tracing::warn!(error = %err, issue_id = %issue.id, "notification delivery failed");
        } else {
            delivered += 1;
        }
    }
    tracing::info!(issue_id = %issue.id, project_id = %project.id, delivered, failed, channel = "email", "notification delivery completed");

    Ok(())
}

/// POST the webhook payload, swallowing every error.
///
/// Build/connect failures and non-2xx responses are logged via `tracing::warn!`
/// and never propagated — a webhook outage must NOT fail ingestion, mirroring
/// the per-recipient email behaviour. A short [`WEBHOOK_TIMEOUT`] guards against
/// a hung endpoint stalling the ingest task.
async fn send_webhook(
    state: &AppState,
    project: &Project,
    issue: &Issue,
    trigger: Trigger,
    url: &str,
) {
    let payload = build_webhook_payload(&state.config.base_url, project, issue, trigger);
    let client = reqwest::Client::new();
    let result = client
        .post(url)
        .timeout(WEBHOOK_TIMEOUT)
        .json(&payload)
        .send()
        .await
        .and_then(|resp| resp.error_for_status());

    if let Err(err) = result {
        tracing::warn!(
            error = %err.without_url(),
            issue_id = %issue.id,
            project_id = %project.id,
            "webhook delivery failed",
        );
    } else {
        tracing::info!(issue_id = %issue.id, project_id = %project.id, channel = "webhook", "notification delivered");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueStatus, TeamRole};
    use chrono::Utc;
    use uuid::Uuid;

    fn project(muted: bool) -> Project {
        let now = Utc::now();
        Project {
            orphaned_at: None,
            id: Uuid::new_v4(),
            short_id: "web001".to_string(),
            team_id: Uuid::new_v4(),
            name: "web".to_string(),
            slug: "web".to_string(),
            dsn_public_key: "pk".to_string(),
            retention_events: 1000,
            retention_days: 0,
            muted,
            webhook_url: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// A project with a webhook URL configured, otherwise like [`project`].
    fn project_with_webhook(muted: bool) -> Project {
        Project {
            webhook_url: Some("https://hook.example/x".to_string()),
            ..project(muted)
        }
    }

    fn issue(status: IssueStatus) -> Issue {
        let now = Utc::now();
        Issue {
            id: Uuid::new_v4(),
            short_id: "iss001".to_string(),
            project_id: Uuid::new_v4(),
            fingerprint: "fp".to_string(),
            title: "ValueError: boom".to_string(),
            culprit: None,
            level: Some("error".to_string()),
            environment: None,
            release: None,
            status,
            muted_at: None,
            muted_until: None,
            mute_threshold: None,
            mute_window_seconds: None,
            first_seen: now,
            last_seen: now,
            event_count: 1,
            created_at: now,
            updated_at: now,
        }
    }

    fn user(email: &str, notifications_enabled: bool) -> User {
        let now = Utc::now();
        User {
            id: Uuid::new_v4(),
            email: email.to_string(),
            display_name: email.to_string(),
            password_hash: "hash".to_string(),
            instance_role: crate::domain::InstanceRole::Member,
            notifications_enabled,
            locale: None,
            auth_provider: crate::domain::AuthProvider::Local,
            status: crate::domain::UserStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn not_suppressed_when_project_and_issue_active() {
        assert_eq!(
            is_suppressed(&project(false), &issue(IssueStatus::Unresolved)),
            Suppression::None
        );
    }

    #[test]
    fn project_mute_suppresses() {
        assert_eq!(
            is_suppressed(&project(true), &issue(IssueStatus::Unresolved)),
            Suppression::ProjectMuted
        );
    }

    #[test]
    fn issue_mute_suppresses() {
        assert_eq!(
            is_suppressed(&project(false), &issue(IssueStatus::Muted)),
            Suppression::IssueMuted
        );
    }

    #[test]
    fn project_mute_takes_precedence_over_issue_mute() {
        // Both muted → reported as project-level (the broader scope).
        assert_eq!(
            is_suppressed(&project(true), &issue(IssueStatus::Muted)),
            Suppression::ProjectMuted
        );
    }

    #[test]
    fn resolved_issue_is_not_suppressed_by_status() {
        // Only `Muted` suppresses; resolved issues still notify on regression.
        assert_eq!(
            is_suppressed(&project(false), &issue(IssueStatus::Resolved)),
            Suppression::None
        );
    }

    #[test]
    fn recipients_exclude_opted_out_users() {
        let members: Vec<(User, TeamRole)> = vec![
            (user("a@example.com", true), TeamRole::Admin),
            (user("b@example.com", false), TeamRole::Contributor),
            (user("c@example.com", true), TeamRole::Contributor),
        ];
        let to = recipients(&members);
        assert_eq!(to, vec!["a@example.com", "c@example.com"]);
    }

    #[test]
    fn recipients_empty_when_all_opted_out() {
        let members: Vec<(User, TeamRole)> = vec![
            (user("a@example.com", false), TeamRole::Admin),
            (user("b@example.com", false), TeamRole::Contributor),
        ];
        assert!(recipients(&members).is_empty());
    }

    #[test]
    fn recipients_notify_admins_and_members_alike() {
        let members: Vec<(User, TeamRole)> = vec![
            (user("admin@example.com", true), TeamRole::Admin),
            (user("member@example.com", true), TeamRole::Contributor),
        ];
        let to = recipients(&members);
        assert_eq!(to.len(), 2);
        assert!(to.contains(&"admin@example.com".to_string()));
        assert!(to.contains(&"member@example.com".to_string()));
    }

    #[test]
    fn user_opt_in_reflects_flag() {
        assert!(user_opted_in(&user("a@example.com", true)));
        assert!(!user_opted_in(&user("a@example.com", false)));
    }

    // --- Webhook channel -----------------------------------------

    #[test]
    fn build_webhook_payload_matches_alertmanager_v4_contract() {
        let project = project_with_webhook(false);
        let issue = issue(IssueStatus::Unresolved);
        let payload = serde_json::to_value(build_webhook_payload(
            "https://errors.example.com/",
            &project,
            &issue,
            Trigger::NewIssue,
        ))
        .unwrap();
        let labels = serde_json::json!({
            "alertname": "SoikaIssue",
            "project": project.slug,
            "project_id": project.id,
            "issue_id": issue.id,
            "severity": "error",
        });
        let annotations = serde_json::json!({
            "summary": issue.title,
            "trigger": "new_issue",
            "event_count": "1",
        });
        assert_eq!(
            payload,
            serde_json::json!({
                "version": "4",
                "groupKey": format!("soika:{}:{}", project.id, issue.id),
                "truncatedAlerts": 0,
                "status": "firing",
                "receiver": "soika",
                "groupLabels": { "project_id": project.id, "issue_id": issue.id },
                "commonLabels": labels,
                "commonAnnotations": annotations,
                "externalURL": "https://errors.example.com",
                "alerts": [{
                    "status": "firing",
                    "labels": labels,
                    "annotations": annotations,
                    "startsAt": issue.first_seen,
                    "endsAt": "0001-01-01T00:00:00Z",
                    "generatorURL": "https://errors.example.com/projects/web001/issues/iss001",
                    "fingerprint": issue.id.simple().to_string(),
                }],
            })
        );
    }

    #[test]
    fn regression_keeps_alert_identity_and_starts_at_latest_occurrence() {
        let project = project_with_webhook(false);
        let mut issue = issue(IssueStatus::Unresolved);
        let original = serde_json::to_value(build_webhook_payload(
            "https://errors.example.com",
            &project,
            &issue,
            Trigger::NewIssue,
        ))
        .unwrap();
        issue.last_seen += chrono::Duration::hours(1);
        issue.event_count = 42;
        issue.title = "Updated title".to_string();
        let payload = serde_json::to_value(build_webhook_payload(
            "https://errors.example.com",
            &project,
            &issue,
            Trigger::Regression,
        ))
        .unwrap();
        assert_eq!(payload["groupKey"], original["groupKey"]);
        assert_eq!(
            payload["alerts"][0]["fingerprint"],
            original["alerts"][0]["fingerprint"]
        );
        assert_eq!(
            payload["alerts"][0]["labels"],
            original["alerts"][0]["labels"]
        );
        assert_eq!(payload["alerts"][0]["annotations"]["trigger"], "regression");
        assert_eq!(payload["alerts"][0]["annotations"]["event_count"], "42");
        assert_eq!(
            payload["alerts"][0]["annotations"]["summary"],
            "Updated title"
        );
        assert_eq!(
            payload["alerts"][0]["startsAt"],
            serde_json::json!(issue.last_seen)
        );
        assert_eq!(
            payload["alerts"][0]["generatorURL"],
            original["alerts"][0]["generatorURL"]
        );
    }

    #[test]
    fn webhook_without_level_has_only_string_labels_and_annotations() {
        let project = project_with_webhook(false);
        let mut issue = issue(IssueStatus::Unresolved);
        issue.level = None;
        let payload = serde_json::to_value(build_webhook_payload(
            "https://errors.example.com",
            &project,
            &issue,
            Trigger::NewIssue,
        ))
        .unwrap();
        assert!(payload["commonLabels"].get("severity").is_none());
        for field in ["commonLabels", "commonAnnotations"] {
            assert!(
                payload[field]
                    .as_object()
                    .unwrap()
                    .values()
                    .all(serde_json::Value::is_string)
            );
        }
        assert_eq!(payload["commonLabels"], payload["alerts"][0]["labels"]);
        assert_eq!(
            payload["commonAnnotations"],
            payload["alerts"][0]["annotations"]
        );
    }

    #[test]
    fn webhook_does_not_bypass_project_mute() {
        // A project with a webhook configured but muted is STILL ProjectMuted —
        // the suppression gate runs before either channel.
        assert_eq!(
            is_suppressed(&project_with_webhook(true), &issue(IssueStatus::Unresolved)),
            Suppression::ProjectMuted
        );
    }

    #[test]
    fn webhook_does_not_bypass_issue_mute() {
        // Likewise, a muted issue suppresses the webhook channel.
        assert_eq!(
            is_suppressed(&project_with_webhook(false), &issue(IssueStatus::Muted)),
            Suppression::IssueMuted
        );
    }

    #[test]
    fn webhook_fires_when_not_suppressed() {
        // Sanity: a webhook-enabled, unmuted project on an active issue is not
        // suppressed, so the webhook channel would run.
        assert_eq!(
            is_suppressed(
                &project_with_webhook(false),
                &issue(IssueStatus::Unresolved)
            ),
            Suppression::None
        );
    }

    // --- Webhook delivery integration -----------------------------
    //
    // These exercise the real `send_webhook` POST and its wiring inside
    // `dispatch` against a tiny local HTTP server (no external mock dependency).
    // They lock in the contract that the doc comments promise: the webhook fires
    // before the mailer-enabled early-return, suppression skips the POST, and a
    // non-2xx / unreachable endpoint never fails ingestion.

    use crate::config::Config;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A minimal one-shot HTTP server capturing the first request's body and
    /// replying with `status`. Returns its base URL and a handle to the captured
    /// body (populated once a request arrives).
    struct CapturedRequest {
        body: Arc<Mutex<Option<String>>>,
    }

    async fn spawn_test_server(status: u16) -> (String, CapturedRequest) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = Arc::new(Mutex::new(None));
        let captured = body.clone();

        tokio::spawn(async move {
            // Accept a single connection; the test only sends one POST.
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            // Read whatever the client sends. reqwest sends headers + body in
            // one go for a small JSON payload, so a single read suffices; we also
            // record the raw bytes and extract the body after the blank line.
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).to_string();
            let payload = raw
                .split_once("\r\n\r\n")
                .map(|(_, b)| b.to_string())
                .unwrap_or_default();
            *captured.lock().unwrap() = Some(payload);

            let reason = if status == 200 { "OK" } else { "Error" };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.flush().await;
        });

        (format!("http://{addr}"), CapturedRequest { body })
    }

    fn test_config(base_url: String) -> Config {
        Config {
            management: Default::default(),
            organization_name: "soika".into(),
            database_url: "sqlite::memory:".into(),
            db: crate::config::DbConfig::default(),
            bind_addr: "127.0.0.1:0".into(),
            base_url,
            secret_key: "secret".into(),
            allow_signup: false,
            admin_enabled: true,
            email_login_enabled: true,
            sso_allow_display_name_change: false,
            admin_users: Default::default(),
            default_events_retention: 1000,
            default_retention_days: 0,
            retention_cron: "0 */15 * * * *".into(),
            // No SMTP: the mailer is disabled, proving the webhook fires
            // independently of SMTP configuration.
            alertmanager_url: None,
            webhooks_enabled: true,
            smtp_enabled: false,
            smtp: None,
            oidc: None,
            passkey: None,
            sentry: None,
            lockout: Default::default(),
            timezone: "UTC".to_string(),
        }
    }

    /// Build an [`AppState`] with a disabled (no-op) mailer and a fresh in-memory
    /// database. The DB is only queried once the mailer is enabled, so a muted /
    /// webhook-only dispatch never touches it.
    async fn test_state(base_url: String) -> AppState {
        test_state_with_webhooks(base_url, true).await
    }

    async fn test_state_with_webhooks(base_url: String, webhooks_enabled: bool) -> AppState {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        let mut config = test_config(base_url);
        config.webhooks_enabled = webhooks_enabled;
        let state = crate::build_state(pool, config).unwrap();
        assert!(
            !state.mailer.is_enabled(),
            "test state must use the disabled mailer to prove SMTP-independent webhook delivery"
        );
        state
    }

    /// A webhook-enabled project pointed at `url`, unmuted by default.
    fn project_at(url: &str, muted: bool) -> Project {
        Project {
            webhook_url: Some(url.to_string()),
            ..project(muted)
        }
    }

    #[tokio::test]
    async fn dispatch_posts_webhook_when_unmuted_even_with_mailer_disabled() {
        let (url, captured) = spawn_test_server(200).await;
        let state = test_state(url.clone()).await;
        let project = project_at(&url, false);
        let issue = issue(IssueStatus::Unresolved);

        // Mailer is disabled, yet the webhook POST must still happen (it fires
        // before the mailer-enabled early-return).
        let result = notify_new_issue(&state, &project, &issue).await;
        assert!(result.is_ok(), "dispatch returned {result:?}");

        let body = captured
            .body
            .lock()
            .unwrap()
            .clone()
            .expect("webhook server received no POST");
        let json: serde_json::Value = serde_json::from_str(&body).expect("body was not JSON");
        assert_eq!(json["version"], "4");
        assert_eq!(json["status"], "firing");
        assert_eq!(json["alerts"].as_array().unwrap().len(), 1);
        let alert = &json["alerts"][0];
        assert_eq!(alert["annotations"]["trigger"], "new_issue");
        assert_eq!(alert["labels"]["issue_id"], issue.id.to_string());
        assert_eq!(alert["labels"]["project_id"], project.id.to_string());
        assert_eq!(alert["annotations"]["summary"], issue.title);
        assert_eq!(
            alert["annotations"]["event_count"],
            issue.event_count.to_string()
        );
        assert_eq!(
            alert["generatorURL"],
            format!(
                "{url}/projects/{}/issues/{}",
                project.short_id, issue.short_id
            )
        );
    }

    #[tokio::test]
    async fn dispatch_skips_webhook_when_globally_disabled() {
        let (url, captured) = spawn_test_server(200).await;
        let state = test_state_with_webhooks(url.clone(), false).await;
        let project = project_at(&url, false);
        let issue = issue(IssueStatus::Unresolved);

        notify_new_issue(&state, &project, &issue).await.unwrap();
        tokio::task::yield_now().await;

        assert!(
            captured.body.lock().unwrap().is_none(),
            "globally disabled webhooks must not receive a POST"
        );
    }

    #[tokio::test]
    async fn dispatch_uses_regression_trigger_for_regressions() {
        let (url, captured) = spawn_test_server(200).await;
        let state = test_state(url.clone()).await;
        let project = project_at(&url, false);
        let issue = issue(IssueStatus::Resolved);

        notify_regression(&state, &project, &issue).await.unwrap();

        let body = captured.body.lock().unwrap().clone().expect("no POST");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], "firing");
        assert_eq!(json["alerts"][0]["annotations"]["trigger"], "regression");
    }

    #[tokio::test]
    async fn dispatch_skips_webhook_when_project_muted() {
        let (url, captured) = spawn_test_server(200).await;
        let state = test_state(url.clone()).await;
        // Muted project: the suppression gate runs before either channel, so no
        // POST should be sent.
        let project = project_at(&url, true);
        let issue = issue(IssueStatus::Unresolved);

        let result = notify_new_issue(&state, &project, &issue).await;
        assert!(result.is_ok(), "suppressed dispatch must still be Ok");

        // Give any (erroneous) spawned POST a chance to arrive before asserting.
        tokio::task::yield_now().await;
        assert!(
            captured.body.lock().unwrap().is_none(),
            "muted project must not POST to the webhook"
        );
    }

    #[tokio::test]
    async fn dispatch_skips_webhook_when_issue_muted() {
        let (url, captured) = spawn_test_server(200).await;
        let state = test_state(url.clone()).await;
        let project = project_at(&url, false);
        // Muted issue suppresses the webhook channel too.
        let issue = issue(IssueStatus::Muted);

        notify_new_issue(&state, &project, &issue).await.unwrap();

        tokio::task::yield_now().await;
        assert!(
            captured.body.lock().unwrap().is_none(),
            "muted issue must not POST to the webhook"
        );
    }

    #[tokio::test]
    async fn dispatch_swallows_non_2xx_webhook_response() {
        // A 500 from the endpoint must NOT fail ingestion: dispatch returns Ok and
        // the error is logged, mirroring per-recipient email behaviour.
        let (url, captured) = spawn_test_server(500).await;
        let state = test_state(url.clone()).await;
        let project = project_at(
            &format!("{url}/secret-webhook-path?token=secret-webhook-token"),
            false,
        );
        let issue = issue(IssueStatus::Unresolved);

        use tracing::instrument::WithSubscriber;
        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish();
        // Without any global subscriber, a parallel test that hits the same
        // `warn!` first caches its interest as "never", and the scoped
        // subscriber below would then see no events.
        static GLOBAL_SUBSCRIBER: std::sync::Once = std::sync::Once::new();
        GLOBAL_SUBSCRIBER.call_once(|| {
            let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
        });
        let result = notify_new_issue(&state, &project, &issue)
            .with_subscriber(subscriber)
            .await;
        let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("webhook delivery failed"), "{logs}");
        assert!(logs.contains("500"), "{logs}");
        assert!(!logs.contains("secret-webhook"), "{logs}");
        assert!(
            result.is_ok(),
            "a 500 webhook response must not fail ingestion, got {result:?}"
        );
        // The POST was still attempted (the body was captured) before the 500.
        assert!(
            captured.body.lock().unwrap().is_some(),
            "the webhook POST should have been sent despite the 500"
        );
    }

    #[tokio::test]
    async fn dispatch_swallows_unreachable_webhook_endpoint() {
        // An unreachable endpoint (connection refused) must also be swallowed.
        // Bind a listener to grab a free port, then drop it so nothing listens.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);

        let state = test_state(dead_url.clone()).await;
        let project = project_at(&dead_url, false);
        let issue = issue(IssueStatus::Unresolved);

        let result = notify_new_issue(&state, &project, &issue).await;
        assert!(
            result.is_ok(),
            "an unreachable webhook endpoint must not fail ingestion, got {result:?}"
        );
    }
}
