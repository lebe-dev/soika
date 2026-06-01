//! Notification orchestration (MVP §12).
//!
//! Decides who to notify on new-issue / regression events and dispatches the
//! emails, respecting three independent suppression rules:
//!
//! - **project mute** ([`Project::muted`]) — suppresses notifications for the
//!   whole project (§8.2);
//! - **issue mute** ([`IssueStatus::Muted`]) — suppresses notifications for the
//!   single issue, while ingestion/counting continue (§8.1);
//! - **per-user opt-out** ([`User::notifications_enabled`]) — a profile setting
//!   that excludes a user from all email (§10.3).
//!
//! The suppression decisions are factored into pure helpers ([`is_suppressed`],
//! [`recipients`]) so they can be unit-tested without I/O. The async entry points
//! are invoked from the ingest pipeline (§5.4) and never propagate transient mail
//! failures back to ingestion — those are logged per-recipient.

use crate::domain::{Issue, IssueStatus, Project, User};
use crate::mail;
use crate::state::AppState;

/// Why a notification was (not) sent — kept small so callers can branch/log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suppression {
    /// Notifications are not suppressed; deliver.
    None,
    /// The whole project is muted (§8.2).
    ProjectMuted,
    /// The specific issue is muted (§8.1).
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

/// Whether a single user should receive notifications (per-user opt-out, §10.3).
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

/// Notify relevant members that a new issue was seen (§12 trigger 1).
pub async fn notify_new_issue(
    state: &AppState,
    project: &Project,
    issue: &Issue,
) -> crate::error::Result<()> {
    dispatch(state, project, issue, Trigger::NewIssue).await
}

/// Notify relevant members that a resolved issue regressed (§12 trigger 2).
pub async fn notify_regression(
    state: &AppState,
    project: &Project,
    issue: &Issue,
) -> crate::error::Result<()> {
    dispatch(state, project, issue, Trigger::Regression).await
}

/// The notification trigger, selecting which email template to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    NewIssue,
    Regression,
}

/// Shared dispatch path for both triggers: apply suppression, resolve recipients,
/// render the email, and deliver to each (logging per-recipient failures).
async fn dispatch(
    state: &AppState,
    project: &Project,
    issue: &Issue,
    trigger: Trigger,
) -> crate::error::Result<()> {
    // Mute checks first — cheapest, and short-circuit before any DB lookup.
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

    // If email isn't even configured, skip the member lookup entirely (§3).
    if !state.mailer.is_enabled() {
        tracing::debug!(issue_id = %issue.id, "mailer disabled; skipping notification");
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
            muted,
            created_at: now,
            updated_at: now,
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
}
