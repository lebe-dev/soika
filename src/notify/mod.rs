//! Notification orchestration.
//!
//! Decides who to notify on new-issue / regression events and dispatches over
//! two channels under the SAME suppression rules:
//!
//! - **email** — one message per opted-in project member;
//! - **webhook** ([`Project::webhook_url`]) — a single HTTP POST JSON payload to
//!   a per-project URL, generalizing the email-only path so channels like
//!   Telegram can be added later through the same seam.
//!
//! Both channels respect three independent suppression rules:
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

use std::time::Duration;

use serde::Serialize;

use crate::domain::{Id, Issue, IssueStatus, Project, User};
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

/// The email addresses to notify for a project's members, after applying the
/// per-user opt-out. Project/issue mute is handled separately by
/// [`is_suppressed`]; this only filters the member list.
///
/// `members` is the project membership joined with each user's profile. The role
/// is irrelevant for notifications (both admins and members are notified), so it
/// is ignored here.
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
/// webhook payload's `trigger` string. Crate-visible so the pure
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
/// regression notification. The shape is stable wire contract.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct WebhookPayload {
    /// `"new_issue"` or `"regression"` (see [`Trigger::as_str`]).
    pub trigger: &'static str,
    pub issue_id: Id,
    pub project_id: Id,
    pub title: String,
    pub level: Option<String>,
    pub event_count: i64,
    /// Deep link to the issue in the dashboard.
    pub issue_url: String,
}

/// Build the webhook payload for an issue notification (pure; unit-tested).
///
/// `issue_url` is `{base_url}/projects/{project_id}/issues/{issue_id}` with any
/// trailing slash on `base_url` trimmed (mirroring `mail.rs` link building and
/// the frontend route `/projects/[id]/issues/[issueId]`). The project id is
/// taken from the issue itself so the link is always self-consistent.
pub(crate) fn build_webhook_payload(
    base_url: &str,
    _project: &Project,
    issue: &Issue,
    trigger: Trigger,
) -> WebhookPayload {
    let base = base_url.trim_end_matches('/');
    let issue_url = format!("{base}/projects/{}/issues/{}", issue.project_id, issue.id);
    WebhookPayload {
        trigger: trigger.as_str(),
        issue_id: issue.id,
        project_id: issue.project_id,
        title: issue.title.clone(),
        level: issue.level.clone(),
        event_count: issue.event_count,
        issue_url,
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

    // Webhook channel: fires independently of SMTP, so deployments
    // with no mailer configured still get notified. Failures are swallowed.
    if let Some(url) = project.webhook_url.as_deref() {
        send_webhook(state, project, issue, trigger, url).await;
    }

    // If email isn't even configured, skip the member lookup entirely.
    if !state.mailer.is_enabled() {
        tracing::debug!(issue_id = %issue.id, "mailer disabled; skipping email notification");
        return Ok(());
    }

    let members = state.memberships.members(project.id).await?;
    let recipients = recipients(&members);
    if recipients.is_empty() {
        return Ok(());
    }

    for to in recipients {
        let email = match trigger {
            Trigger::NewIssue => mail::new_issue_email(&state.config, &to, &issue.title),
            Trigger::Regression => mail::regression_email(&state.config, &to, &issue.title),
        };
        // A failure to one recipient must not block the others or fail ingestion.
        if let Err(err) = state.mailer.send(email).await {
            tracing::warn!(error = %err, recipient = %to, issue_id = %issue.id, "notification delivery failed");
        }
    }

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
            error = %err,
            issue_id = %issue.id,
            project_id = %project.id,
            "webhook delivery failed",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueStatus, Role};
    use chrono::Utc;
    use uuid::Uuid;

    fn project(muted: bool) -> Project {
        let now = Utc::now();
        Project {
            id: Uuid::new_v4(),
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
            project_id: Uuid::new_v4(),
            fingerprint: "fp".to_string(),
            title: "ValueError: boom".to_string(),
            culprit: None,
            level: Some("error".to_string()),
            environment: None,
            release: None,
            status,
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
            is_admin: false,
            notifications_enabled,
            auth_provider: crate::domain::AuthProvider::Local,
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
        let members: Vec<(User, Role)> = vec![
            (user("a@example.com", true), Role::Admin),
            (user("b@example.com", false), Role::Member),
            (user("c@example.com", true), Role::Member),
        ];
        let to = recipients(&members);
        assert_eq!(to, vec!["a@example.com", "c@example.com"]);
    }

    #[test]
    fn recipients_empty_when_all_opted_out() {
        let members: Vec<(User, Role)> = vec![
            (user("a@example.com", false), Role::Admin),
            (user("b@example.com", false), Role::Member),
        ];
        assert!(recipients(&members).is_empty());
    }

    #[test]
    fn recipients_notify_admins_and_members_alike() {
        let members: Vec<(User, Role)> = vec![
            (user("admin@example.com", true), Role::Admin),
            (user("member@example.com", true), Role::Member),
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
    fn build_webhook_payload_sets_all_fields_and_issue_url() {
        let project = project_with_webhook(false);
        let issue = issue(IssueStatus::Unresolved);
        // base_url has a trailing slash; the issue_url must not double it.
        let payload = build_webhook_payload(
            "https://errors.example.com/",
            &project,
            &issue,
            Trigger::NewIssue,
        );

        assert_eq!(payload.trigger, "new_issue");
        assert_eq!(payload.issue_id, issue.id);
        assert_eq!(payload.project_id, issue.project_id);
        assert_eq!(payload.title, issue.title);
        assert_eq!(payload.level, issue.level);
        assert_eq!(payload.event_count, issue.event_count);
        assert_eq!(
            payload.issue_url,
            format!(
                "https://errors.example.com/projects/{}/issues/{}",
                issue.project_id, issue.id
            )
        );
        assert!(!payload.issue_url.contains("com//projects"));
    }

    #[test]
    fn build_webhook_payload_uses_regression_trigger_string() {
        let project = project_with_webhook(false);
        let issue = issue(IssueStatus::Unresolved);
        let payload = build_webhook_payload(
            "https://errors.example.com",
            &project,
            &issue,
            Trigger::Regression,
        );
        assert_eq!(payload.trigger, "regression");
        // No trailing slash on base_url: still a single slash before /projects.
        assert_eq!(
            payload.issue_url,
            format!(
                "https://errors.example.com/projects/{}/issues/{}",
                issue.project_id, issue.id
            )
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
            organization_name: "soika".into(),
            database_url: "sqlite::memory:".into(),
            bind_addr: "127.0.0.1:0".into(),
            base_url,
            secret_key: "secret".into(),
            allow_signup: false,
            default_events_retention: 1000,
            default_retention_days: 0,
            retention_cron: "0 */15 * * * *".into(),
            // No SMTP: the mailer is disabled, proving the webhook fires
            // independently of SMTP configuration.
            smtp: None,
            oidc: None,
        }
    }

    /// Build an [`AppState`] with a disabled (no-op) mailer and a fresh in-memory
    /// database. The DB is only queried once the mailer is enabled, so a muted /
    /// webhook-only dispatch never touches it.
    async fn test_state(base_url: String) -> AppState {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        let state = crate::build_state(pool, test_config(base_url));
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
        assert_eq!(json["trigger"], "new_issue");
        assert_eq!(json["issue_id"], issue.id.to_string());
        assert_eq!(json["project_id"], issue.project_id.to_string());
        assert_eq!(json["title"], issue.title);
        assert_eq!(json["event_count"], issue.event_count);
        assert_eq!(
            json["issue_url"],
            format!("{url}/projects/{}/issues/{}", issue.project_id, issue.id)
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
        assert_eq!(json["trigger"], "regression");
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
        let project = project_at(&url, false);
        let issue = issue(IssueStatus::Unresolved);

        let result = notify_new_issue(&state, &project, &issue).await;
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
