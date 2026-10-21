//! Contract tests for the `issues` repository port.

use crate::fixtures::*;

fn upsert(project_id: Id, fp: &str, at: Timestamp) -> IssueUpsert {
    IssueUpsert {
        project_id,
        fingerprint: fp.to_string(),
        title: format!("Error {fp}"),
        culprit: Some("main".into()),
        level: Some("error".into()),
        environment: None,
        release: None,
        seen_at: at,
    }
}

/// Like [`upsert`] but lets a test vary the first-class filterable fields
/// (level / environment / release) so seeded rows differ.
fn upsert_with(
    project_id: Id,
    fp: &str,
    at: Timestamp,
    level: Option<&str>,
    environment: Option<&str>,
    release: Option<&str>,
) -> IssueUpsert {
    IssueUpsert {
        level: level.map(str::to_string),
        environment: environment.map(str::to_string),
        release: release.map(str::to_string),
        ..upsert(project_id, fp, at)
    }
}

#[tokio::test]
async fn upsert_assigns_short_id_resolvable_via_find_by_short_id() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let created = repo
        .upsert_by_fingerprint(upsert(project_id, "fp-short", chrono::Utc::now()))
        .await
        .unwrap()
        .issue;

    // A new issue gets a compact 6-char public code that resolves back to it.
    assert_eq!(created.short_id.len(), 6);
    assert!(created.short_id.chars().all(|c| c.is_ascii_alphanumeric()));

    let found = repo
        .find_by_short_id(&created.short_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.id, created.id);
    assert!(repo.find_by_short_id("zzzzzz").await.unwrap().is_none());
}

#[tokio::test]
async fn first_upsert_is_new_then_increments() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let t0 = chrono::Utc::now();
    let first = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", t0))
        .await
        .unwrap();
    assert!(first.is_new);
    assert!(!first.is_regression);
    assert_eq!(first.issue.event_count, 1);

    let t1 = t0 + chrono::Duration::seconds(5);
    let second = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", t1))
        .await
        .unwrap();
    assert!(!second.is_new, "same fingerprint must not be new");
    assert!(!second.is_regression);
    assert_eq!(
        second.issue.id, first.issue.id,
        "same fingerprint → same issue"
    );
    assert_eq!(second.issue.event_count, 2);
    assert!(second.issue.last_seen > first.issue.last_seen);
}

#[tokio::test]
async fn upsert_on_resolved_issue_is_regression() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let created = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    repo.set_status(created.issue.id, IssueStatus::Resolved)
        .await
        .unwrap();

    let regressed = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    assert!(!regressed.is_new);
    assert!(regressed.is_regression, "resolved → new event must regress");
    assert_eq!(regressed.issue.status, IssueStatus::Unresolved);
    assert_eq!(regressed.issue.event_count, 2);
}

#[tokio::test]
async fn muted_issue_stays_muted_on_new_event() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let created = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    repo.set_status(created.issue.id, IssueStatus::Muted)
        .await
        .unwrap();

    let again = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    assert!(!again.is_regression);
    assert_eq!(again.issue.status, IssueStatus::Muted);
    assert_eq!(
        again.issue.event_count, 2,
        "muted issue keeps counting events"
    );
}

#[tokio::test]
async fn mute_forever_sets_status_and_leaves_period_null() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let created = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    let now = chrono::Utc::now();
    let muted = repo
        .mute(created.issue.id, MuteSpec::Forever, now)
        .await
        .unwrap();

    assert_eq!(muted.status, IssueStatus::Muted);
    assert!(muted.muted_at.is_some(), "mute baseline is recorded");
    assert_eq!(muted.muted_until, None);
    assert_eq!(muted.mute_threshold, None);
    assert_eq!(muted.mute_window_seconds, None);
}

#[tokio::test]
async fn mute_until_persists_expiry() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let created = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    let now = chrono::Utc::now();
    let until = now + chrono::Duration::days(1);
    let muted = repo
        .mute(created.issue.id, MuteSpec::Until(until), now)
        .await
        .unwrap();

    assert_eq!(muted.status, IssueStatus::Muted);
    assert_eq!(muted.muted_until.unwrap().timestamp(), until.timestamp());
    assert_eq!(muted.mute_threshold, None);
}

#[tokio::test]
async fn mute_event_rate_persists_threshold_and_window() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let created = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    let muted = repo
        .mute(
            created.issue.id,
            MuteSpec::EventRate {
                threshold: 5,
                window_seconds: 3600,
            },
            chrono::Utc::now(),
        )
        .await
        .unwrap();

    assert_eq!(muted.mute_threshold, Some(5));
    assert_eq!(muted.mute_window_seconds, Some(3600));
    assert_eq!(muted.muted_until, None);
}

#[tokio::test]
async fn set_status_clears_mute_bookkeeping() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let created = repo
        .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
        .await
        .unwrap();
    let now = chrono::Utc::now();
    repo.mute(
        created.issue.id,
        MuteSpec::Until(now + chrono::Duration::days(1)),
        now,
    )
    .await
    .unwrap();

    // Re-opening (unresolve) must wipe the stale expiry.
    let reopened = repo
        .set_status(created.issue.id, IssueStatus::Unresolved)
        .await
        .unwrap();
    assert_eq!(reopened.status, IssueStatus::Unresolved);
    assert_eq!(reopened.muted_at, None);
    assert_eq!(reopened.muted_until, None);
    assert_eq!(reopened.mute_threshold, None);
    assert_eq!(reopened.mute_window_seconds, None);
}

#[tokio::test]
async fn clear_expired_mutes_only_unmutes_passed_time_based() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let now = chrono::Utc::now();

    // (a) expired time-based mute → should be cleared.
    let expired = repo
        .upsert_by_fingerprint(upsert(project_id, "expired", now))
        .await
        .unwrap();
    repo.mute(
        expired.issue.id,
        MuteSpec::Until(now - chrono::Duration::hours(1)),
        now,
    )
    .await
    .unwrap();

    // (b) future time-based mute → must stay muted.
    let future = repo
        .upsert_by_fingerprint(upsert(project_id, "future", now))
        .await
        .unwrap();
    repo.mute(
        future.issue.id,
        MuteSpec::Until(now + chrono::Duration::hours(1)),
        now,
    )
    .await
    .unwrap();

    // (c) event-rate mute (no muted_until) → never cleared by this sweep.
    let rate = repo
        .upsert_by_fingerprint(upsert(project_id, "rate", now))
        .await
        .unwrap();
    repo.mute(
        rate.issue.id,
        MuteSpec::EventRate {
            threshold: 5,
            window_seconds: 3600,
        },
        now,
    )
    .await
    .unwrap();

    let cleared = repo.clear_expired_mutes(now).await.unwrap();
    assert_eq!(cleared, 1, "only the expired time-based mute is cleared");

    assert_eq!(
        repo.find_by_id(expired.issue.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        IssueStatus::Unresolved
    );
    assert_eq!(
        repo.find_by_id(future.issue.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        IssueStatus::Muted
    );
    assert_eq!(
        repo.find_by_id(rate.issue.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        IssueStatus::Muted
    );
}

#[tokio::test]
async fn list_filters_by_status() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let a = repo
        .upsert_by_fingerprint(upsert(project_id, "fpa", chrono::Utc::now()))
        .await
        .unwrap();
    repo.upsert_by_fingerprint(upsert(project_id, "fpb", chrono::Utc::now()))
        .await
        .unwrap();
    repo.set_status(a.issue.id, IssueStatus::Resolved)
        .await
        .unwrap();

    let unresolved = repo
        .list(
            project_id,
            IssueFilter {
                status: Some(IssueStatus::Unresolved),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].fingerprint, "fpb");

    let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn unresolved_counts_groups_by_project_and_skips_resolved() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    // Two unresolved + one resolved → the resolved one must not be counted.
    repo.upsert_by_fingerprint(upsert(project_id, "fpa", chrono::Utc::now()))
        .await
        .unwrap();
    repo.upsert_by_fingerprint(upsert(project_id, "fpb", chrono::Utc::now()))
        .await
        .unwrap();
    let resolved = repo
        .upsert_by_fingerprint(upsert(project_id, "fpc", chrono::Utc::now()))
        .await
        .unwrap();
    repo.set_status(resolved.issue.id, IssueStatus::Resolved)
        .await
        .unwrap();

    // A project with no unresolved issues is absent from the map (callers
    // read a missing key as 0).
    let empty_project = Id::new_v4();
    let counts = repo
        .unresolved_counts(&[project_id, empty_project])
        .await
        .unwrap();
    assert_eq!(counts.get(&project_id).copied(), Some(2));
    assert_eq!(counts.get(&empty_project), None);

    // Empty input short-circuits to an empty map.
    assert!(repo.unresolved_counts(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn list_filters_by_query() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    repo.upsert_by_fingerprint(upsert(project_id, "needle", chrono::Utc::now()))
        .await
        .unwrap();
    repo.upsert_by_fingerprint(upsert(project_id, "haystack", chrono::Utc::now()))
        .await
        .unwrap();

    // Titles are "Error needle" / "Error haystack".
    let hits = repo
        .list(
            project_id,
            IssueFilter {
                query: Some("needle".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].fingerprint, "needle");
}

#[tokio::test]
async fn list_orders_by_event_count() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    // "frequent" gets 3 events at older timestamps (event_count = 3).
    let base = chrono::Utc::now();
    for i in 0..3 {
        let at = base + chrono::Duration::seconds(i);
        repo.upsert_by_fingerprint(upsert(project_id, "frequent", at))
            .await
            .unwrap();
    }
    // "rare" gets a single event with the LATEST timestamp, so it would sort
    // first under LastSeen but must lose to "frequent" under EventCount.
    let latest = base + chrono::Duration::seconds(100);
    repo.upsert_by_fingerprint(upsert(project_id, "rare", latest))
        .await
        .unwrap();

    let by_count = repo
        .list(
            project_id,
            IssueFilter {
                sort: IssueSort::EventCount,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(by_count.len(), 2);
    assert_eq!(
        by_count[0].fingerprint, "frequent",
        "higher event_count sorts first under EventCount"
    );
    assert_eq!(by_count[1].fingerprint, "rare");

    // Sanity: under the default LastSeen ordering, "rare" (latest) leads.
    let by_recency = repo.list(project_id, IssueFilter::default()).await.unwrap();
    assert_eq!(by_recency[0].fingerprint, "rare");
}

#[tokio::test]
async fn first_upsert_persists_environment_and_release() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let with_env = repo
        .upsert_by_fingerprint(upsert_with(
            project_id,
            "fp1",
            chrono::Utc::now(),
            Some("warning"),
            Some("production"),
            Some("1.2.3"),
        ))
        .await
        .unwrap();
    assert_eq!(with_env.issue.level.as_deref(), Some("warning"));
    assert_eq!(with_env.issue.environment.as_deref(), Some("production"));
    assert_eq!(with_env.issue.release.as_deref(), Some("1.2.3"));

    // Read back through find_by_id to confirm the columns round-trip.
    let read = repo.find_by_id(with_env.issue.id).await.unwrap().unwrap();
    assert_eq!(read.environment.as_deref(), Some("production"));
    assert_eq!(read.release.as_deref(), Some("1.2.3"));

    // A NULL environment/release decodes to None.
    let bare = repo
        .upsert_by_fingerprint(upsert_with(
            project_id,
            "fp2",
            chrono::Utc::now(),
            Some("error"),
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(bare.issue.environment, None);
    assert_eq!(bare.issue.release, None);
}

#[tokio::test]
async fn list_filters_by_level() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "err",
        chrono::Utc::now(),
        Some("error"),
        None,
        None,
    ))
    .await
    .unwrap();
    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "warn",
        chrono::Utc::now(),
        Some("warning"),
        None,
        None,
    ))
    .await
    .unwrap();

    let only_warnings = repo
        .list(
            project_id,
            IssueFilter {
                level: Some("warning".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(only_warnings.len(), 1);
    assert_eq!(only_warnings[0].fingerprint, "warn");

    let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn list_filters_by_environment() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "prod",
        chrono::Utc::now(),
        Some("error"),
        Some("production"),
        None,
    ))
    .await
    .unwrap();
    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "staging",
        chrono::Utc::now(),
        Some("error"),
        Some("staging"),
        None,
    ))
    .await
    .unwrap();
    // A row with NULL environment must be excluded when the filter is set.
    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "noenv",
        chrono::Utc::now(),
        Some("error"),
        None,
        None,
    ))
    .await
    .unwrap();

    let prod = repo
        .list(
            project_id,
            IssueFilter {
                environment: Some("production".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(prod.len(), 1, "NULL-environment rows are excluded");
    assert_eq!(prod[0].fingerprint, "prod");

    // With no environment filter, all three rows (incl. NULL) are returned.
    let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn list_filters_by_release() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "v1",
        chrono::Utc::now(),
        Some("error"),
        None,
        Some("1.0.0"),
    ))
    .await
    .unwrap();
    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "v2",
        chrono::Utc::now(),
        Some("error"),
        None,
        Some("2.0.0"),
    ))
    .await
    .unwrap();

    let v2 = repo
        .list(
            project_id,
            IssueFilter {
                release: Some("2.0.0".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(v2.len(), 1);
    assert_eq!(v2[0].fingerprint, "v2");
}

/// Insert a raw event row for an issue (mirrors the event.rs seed pattern)
/// so override_fingerprint's event re-pointing can be asserted.
async fn insert_event(db: &AppState, project_id: Id, issue_id: Id, at: Timestamp) -> Id {
    db.events
        .insert(NewEvent {
            event_id: at.timestamp_millis().to_string(),
            issue_id,
            project_id,
            payload: serde_json::json!({}),
            received_at: at,
        })
        .await
        .expect("insert event")
        .id
}

async fn count_events_for_issue(db: &AppState, issue_id: Id) -> i64 {
    let events = db.events.recent_events(issue_id, 500).await.unwrap();
    events.len() as i64
}

#[tokio::test]
async fn override_fingerprint_renames_when_no_collision() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let a = repo
        .upsert_by_fingerprint(upsert(project_id, "fpa", chrono::Utc::now()))
        .await
        .unwrap();

    let renamed = repo
        .override_fingerprint(a.issue.id, "renamed".to_string())
        .await
        .unwrap();

    // Same issue, new fingerprint.
    assert_eq!(renamed.id, a.issue.id);
    assert_eq!(renamed.fingerprint, "renamed");

    // The new fingerprint resolves to this issue; the old one is gone.
    let by_new = repo
        .find_by_fingerprint(project_id, "renamed")
        .await
        .unwrap();
    assert_eq!(by_new.map(|i| i.id), Some(a.issue.id));
    assert!(
        repo.find_by_fingerprint(project_id, "fpa")
            .await
            .unwrap()
            .is_none(),
        "old fingerprint must no longer resolve"
    );
}

#[tokio::test]
async fn override_fingerprint_merges_into_existing_target() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    // Issue A already holds the destination fingerprint "fpa"; it is the
    // collision the merge folds INTO the addressed issue, then deletes.
    // event_count = 2 (two upserts).
    let t0 = chrono::Utc::now();
    let a = repo
        .upsert_by_fingerprint(upsert(project_id, "fpa", t0))
        .await
        .unwrap();
    let t0b = t0 + chrono::Duration::seconds(5);
    repo.upsert_by_fingerprint(upsert(project_id, "fpa", t0b))
        .await
        .unwrap();
    let a = repo.find_by_id(a.issue.id).await.unwrap().unwrap();
    assert_eq!(a.event_count, 2);

    // Issue B is the addressed (target) issue; we override ITS fingerprint
    // to "fpa". Its first_seen is earlier than A's and its last_seen later
    // than A's, so the folded min/max is observable. event_count = 2.
    let t1_first = t0 - chrono::Duration::seconds(60);
    let b = repo
        .upsert_by_fingerprint(upsert(project_id, "fpb", t1_first))
        .await
        .unwrap();
    let t1_last = t0b + chrono::Duration::seconds(60);
    repo.upsert_by_fingerprint(upsert(project_id, "fpb", t1_last))
        .await
        .unwrap();
    let b = repo.find_by_id(b.issue.id).await.unwrap().unwrap();
    assert_eq!(b.event_count, 2);

    // Seed raw events for BOTH issues so re-pointing can be asserted.
    insert_event(&db, project_id, a.id, t0).await;
    insert_event(&db, project_id, a.id, t0b).await;
    insert_event(&db, project_id, b.id, t1_first).await;
    insert_event(&db, project_id, b.id, t1_last).await;
    assert_eq!(count_events_for_issue(&db, a.id).await, 2);
    assert_eq!(count_events_for_issue(&db, b.id).await, 2);

    // Override B's fingerprint to "fpa" — collides with A → MERGE. The
    // addressed issue (B) survives; the colliding source (A) is folded in
    // and deleted.
    let survivor = repo
        .override_fingerprint(b.id, "fpa".to_string())
        .await
        .unwrap();

    // The surviving issue is the addressed target (B) now holding "fpa".
    assert_eq!(survivor.id, b.id, "addressed issue B survives");
    assert_eq!(survivor.fingerprint, "fpa");
    assert_eq!(
        survivor.event_count,
        a.event_count + b.event_count,
        "event_count is summed (4)"
    );
    // Earliest first_seen (B's) and latest last_seen (B's) across both.
    assert_eq!(survivor.first_seen.timestamp(), b.first_seen.timestamp());
    assert_eq!(survivor.last_seen.timestamp(), b.last_seen.timestamp());

    // The colliding source A is deleted; only one issue with "fpa" remains.
    assert!(
        repo.find_by_id(a.id).await.unwrap().is_none(),
        "colliding source issue is deleted"
    );
    let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
    assert_eq!(all.len(), 1, "exactly one issue remains after merge");

    // All events were re-pointed onto the survivor; none reference A.
    assert_eq!(count_events_for_issue(&db, a.id).await, 0);
    assert_eq!(count_events_for_issue(&db, survivor.id).await, 4);
}

#[tokio::test]
async fn override_fingerprint_merge_adopts_source_extremes() {
    // Regression guard: the survivor must take the EARLIEST first_seen and the
    // LATEST last_seen across both issues even when those extremes belong to
    // the *source* (the deleted colliding issue), not the addressed survivor.
    // The sibling merge test seeds both extremes on the survivor, so it cannot
    // tell the min/max fold from "just keep the target's own timestamps".
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let t0 = chrono::Utc::now();

    // Source A ("fpa") spans the WIDER window: first_seen = t0-60s (earliest of
    // all four events), last_seen = t0+60s (latest). event_count = 2.
    let a = repo
        .upsert_by_fingerprint(upsert(
            project_id,
            "fpa",
            t0 - chrono::Duration::seconds(60),
        ))
        .await
        .unwrap();
    repo.upsert_by_fingerprint(upsert(
        project_id,
        "fpa",
        t0 + chrono::Duration::seconds(60),
    ))
    .await
    .unwrap();
    let a = repo.find_by_id(a.issue.id).await.unwrap().unwrap();

    // Target/survivor B ("fpb") sits entirely INSIDE A's window: first_seen =
    // t0, last_seen = t0+5s. event_count = 2.
    let b = repo
        .upsert_by_fingerprint(upsert(project_id, "fpb", t0))
        .await
        .unwrap();
    repo.upsert_by_fingerprint(upsert(project_id, "fpb", t0 + chrono::Duration::seconds(5)))
        .await
        .unwrap();
    let b = repo.find_by_id(b.issue.id).await.unwrap().unwrap();

    // Override B → "fpa": collides with A → MERGE; the addressed issue B
    // survives and the source A is folded in and deleted.
    let survivor = repo
        .override_fingerprint(b.id, "fpa".to_string())
        .await
        .unwrap();

    assert_eq!(survivor.id, b.id, "addressed issue B survives");
    assert_eq!(
        survivor.event_count,
        a.event_count + b.event_count,
        "event_count is summed (4)"
    );
    // The extremes come from the SOURCE (A), not the survivor (B) — exactly the
    // case the sibling merge test does not exercise.
    assert_eq!(
        survivor.first_seen.timestamp(),
        a.first_seen.timestamp(),
        "survivor adopts the source's earlier first_seen"
    );
    assert_eq!(
        survivor.last_seen.timestamp(),
        a.last_seen.timestamp(),
        "survivor adopts the source's later last_seen"
    );
    assert!(
        repo.find_by_id(a.id).await.unwrap().is_none(),
        "colliding source issue is deleted"
    );
}

#[tokio::test]
async fn override_fingerprint_noop_when_unchanged() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let a = repo
        .upsert_by_fingerprint(upsert(project_id, "fpa", chrono::Utc::now()))
        .await
        .unwrap();

    let same = repo
        .override_fingerprint(a.issue.id, "fpa".to_string())
        .await
        .unwrap();
    assert_eq!(same.id, a.issue.id);
    assert_eq!(same.fingerprint, "fpa");
    assert_eq!(same.event_count, a.issue.event_count);
}

#[tokio::test]
async fn list_composes_status_level_environment_release() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    // The matching row: error / production / 1.0.0, left unresolved.
    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "match",
        chrono::Utc::now(),
        Some("error"),
        Some("production"),
        Some("1.0.0"),
    ))
    .await
    .unwrap();
    // Same env/release/level but resolved → excluded by the status filter.
    let resolved = repo
        .upsert_by_fingerprint(upsert_with(
            project_id,
            "resolved",
            chrono::Utc::now(),
            Some("error"),
            Some("production"),
            Some("1.0.0"),
        ))
        .await
        .unwrap();
    repo.set_status(resolved.issue.id, IssueStatus::Resolved)
        .await
        .unwrap();
    // Differing release → excluded by the release filter.
    repo.upsert_by_fingerprint(upsert_with(
        project_id,
        "otherrel",
        chrono::Utc::now(),
        Some("error"),
        Some("production"),
        Some("2.0.0"),
    ))
    .await
    .unwrap();

    let hits = repo
        .list(
            project_id,
            IssueFilter {
                status: Some(IssueStatus::Unresolved),
                level: Some("error".into()),
                environment: Some("production".into()),
                release: Some("1.0.0".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "all sentinels compose to a single match");
    assert_eq!(hits[0].fingerprint, "match");
}

#[tokio::test]
async fn upsert_refreshes_title_on_repeat_event() {
    use chrono::TimeZone;

    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let base = |title: &str, at: i64| IssueUpsert {
        project_id,
        fingerprint: "fp-1".to_string(),
        title: title.to_string(),
        culprit: None,
        level: None,
        environment: None,
        release: None,
        seen_at: chrono::Utc.timestamp_opt(at, 0).single().unwrap(),
    };

    // First event freezes an (already parameterized) title.
    let first = repo
        .upsert_by_fingerprint(base("Error: Not found: /download/<id>", 100))
        .await
        .expect("first upsert");
    assert!(first.is_new);

    // A second event in the same group refreshes the title (last-write-wins),
    // so an issue never keeps a stale early sample.
    let second = repo
        .upsert_by_fingerprint(base("Error: Not found: /download/<id>", 200))
        .await
        .expect("second upsert");
    assert!(!second.is_new);
    assert_eq!(second.issue.id, first.issue.id, "same group");
    assert_eq!(second.issue.title, "Error: Not found: /download/<id>");
    assert_eq!(second.issue.event_count, 2);
}

#[tokio::test]
async fn list_query_is_case_insensitive() {
    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    repo.upsert_by_fingerprint(upsert(project_id, "Needle", chrono::Utc::now()))
        .await
        .unwrap();

    // Title is "Error Needle"; both backends match regardless of ASCII case.
    for query in ["needle", "NEEDLE", "error nee"] {
        let hits = repo
            .list(
                project_id,
                IssueFilter {
                    query: Some(query.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(hits.len(), 1, "query {query:?}");
    }
}

/// Concurrent ingestion of the same brand-new fingerprint must create exactly
/// one issue and count every event: SQLite serializes the writers, Postgres
/// relies on `ON CONFLICT` plus a row lock.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_upserts_of_one_fingerprint_create_a_single_issue() {
    const WRITERS: usize = 16;

    let db = test_db().await;
    let project_id = seed_project(&db).await;

    let mut tasks = Vec::with_capacity(WRITERS);
    for _ in 0..WRITERS {
        let repo = db.issues.clone();
        tasks.push(tokio::spawn(async move {
            repo.upsert_by_fingerprint(upsert(project_id, "race", chrono::Utc::now()))
                .await
        }));
    }

    let mut created = 0;
    for task in tasks {
        let outcome = task.await.expect("join").expect("upsert");
        if outcome.is_new {
            created += 1;
        }
    }
    assert_eq!(created, 1, "exactly one writer creates the issue");

    let issue = db
        .issues
        .find_by_fingerprint(project_id, "race")
        .await
        .unwrap()
        .expect("issue exists");
    assert_eq!(issue.event_count, WRITERS as i64);
}

/// The issue a write returns is exactly what a later read yields — including
/// timestamps, which Postgres stores at microsecond precision.
#[tokio::test]
async fn returned_issue_matches_a_fresh_read() {
    use chrono::TimeZone;

    let db = test_db().await;
    let project_id = seed_project(&db).await;
    let repo = db.issues.clone();

    let at = chrono::Utc
        .timestamp_opt(1_700_000_000, 123_456_789)
        .single()
        .unwrap();
    let outcome = repo
        .upsert_by_fingerprint(upsert(project_id, "precise", at))
        .await
        .unwrap();
    let muted = repo
        .mute(outcome.issue.id, MuteSpec::Until(at), at)
        .await
        .unwrap();

    let read = repo.find_by_id(muted.id).await.unwrap().unwrap();
    assert_eq!(read.first_seen, muted.first_seen);
    assert_eq!(read.last_seen, muted.last_seen);
    assert_eq!(read.muted_until, muted.muted_until);
    assert_eq!(read.muted_at, muted.muted_at);
}
