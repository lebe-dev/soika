//! Contract tests for the `events` repository port.

use crate::fixtures::*;

/// Seed a project + a single issue, returning (project_id, issue_id).
///
/// Names are derived from fresh ids so `seed` can run more than once per
/// database without tripping UNIQUE constraints.
async fn seed(db: &AppState) -> (Id, Id) {
    let suffix = Id::new_v4();
    let team_id = insert_team(db, &format!("team-{suffix}")).await;
    let project_id = insert_project(
        db,
        team_id,
        &format!("project-{suffix}"),
        &format!("dsn-{suffix}"),
    )
    .await;
    let issue = db
        .issues
        .upsert_by_fingerprint(IssueUpsert {
            project_id,
            fingerprint: "fp".to_string(),
            title: "T".to_string(),
            culprit: None,
            level: None,
            environment: None,
            release: None,
            seen_at: chrono::Utc::now(),
        })
        .await
        .expect("seed issue");
    (project_id, issue.issue.id)
}

fn new_event(project_id: Id, issue_id: Id, at: Timestamp) -> NewEvent {
    NewEvent {
        event_id: at.timestamp_millis().to_string(),
        issue_id,
        project_id,
        payload: serde_json::json!({ "message": "boom", "ts": at.to_rfc3339() }),
        received_at: at,
    }
}

#[tokio::test]
async fn insert_roundtrips_payload() {
    let db = test_db().await;
    let (project_id, issue_id) = seed(&db).await;
    let repo = db.events.clone();

    let inserted = repo
        .insert(new_event(project_id, issue_id, chrono::Utc::now()))
        .await
        .unwrap();
    let fetched = repo.find_by_id(inserted.id).await.unwrap().unwrap();
    assert_eq!(fetched.payload["message"], "boom");
    assert_eq!(fetched.project_id, project_id);
    assert_eq!(fetched.issue_id, issue_id);
}

#[tokio::test]
async fn prune_keeps_n_most_recent_per_project() {
    let db = test_db().await;
    let (project_id, issue_id) = seed(&db).await;
    let repo = db.events.clone();

    let base = chrono::Utc::now();
    // Insert 5 events with strictly increasing timestamps.
    for i in 0..5 {
        let at = base + chrono::Duration::seconds(i);
        repo.insert(new_event(project_id, issue_id, at))
            .await
            .unwrap();
    }
    assert_eq!(repo.count_for_project(project_id).await.unwrap(), 5);

    // Keep the 2 most-recent → deletes 3.
    let deleted = repo
        .prune_events_over_retention(project_id, 2)
        .await
        .unwrap();
    assert_eq!(deleted, 3);
    assert_eq!(repo.count_for_project(project_id).await.unwrap(), 2);

    // The survivors are the newest two (seconds 3 and 4).
    let remaining = repo.recent_events(issue_id, 100).await.unwrap();
    assert_eq!(remaining.len(), 2);
    let newest = base + chrono::Duration::seconds(4);
    let second = base + chrono::Duration::seconds(3);
    assert_eq!(remaining[0].received_at.timestamp(), newest.timestamp());
    assert_eq!(remaining[1].received_at.timestamp(), second.timestamp());
}

#[tokio::test]
async fn prune_is_per_project_and_noop_when_under_limit() {
    let db = test_db().await;
    let (project_a, issue_a) = seed(&db).await;
    let (project_b, issue_b) = seed(&db).await;
    let repo = db.events.clone();

    let base = chrono::Utc::now();
    for i in 0..3 {
        let at = base + chrono::Duration::seconds(i);
        repo.insert(new_event(project_a, issue_a, at))
            .await
            .unwrap();
    }
    repo.insert(new_event(project_b, issue_b, base))
        .await
        .unwrap();

    // Pruning project A to 2 must not touch project B.
    let deleted = repo
        .prune_events_over_retention(project_a, 2)
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(repo.count_for_project(project_a).await.unwrap(), 2);
    assert_eq!(repo.count_for_project(project_b).await.unwrap(), 1);

    // Retention above the current count is a no-op.
    let deleted = repo
        .prune_events_over_retention(project_a, 100)
        .await
        .unwrap();
    assert_eq!(deleted, 0);
    assert_eq!(repo.count_for_project(project_a).await.unwrap(), 2);
}

/// A batch size smaller than the number of doomed rows must still delete all
/// of them (the loop runs until a chunk comes back short) and keep exactly
/// the newest `retention` events.
#[tokio::test]
async fn prune_deletes_every_batch_when_the_batch_is_smaller_than_the_backlog() {
    let db = test_db_with_write_policy(WritePolicy {
        delete_batch: 1,
        ..WritePolicy::default()
    })
    .await;
    let (project_id, issue_id) = seed(&db).await;
    let repo = db.events.clone();

    let base = chrono::Utc::now();
    for i in 0..10 {
        let at = base + chrono::Duration::seconds(i);
        repo.insert(new_event(project_id, issue_id, at))
            .await
            .unwrap();
    }

    // 10 events, keep 3 → 7 deletions across 7 one-row batches.
    let deleted = repo
        .prune_events_over_retention(project_id, 3)
        .await
        .unwrap();
    assert_eq!(deleted, 7);
    assert_eq!(repo.count_for_project(project_id).await.unwrap(), 3);

    // The survivors are the three newest.
    let remaining = repo.recent_events(issue_id, 100).await.unwrap();
    let kept: Vec<i64> = remaining
        .iter()
        .map(|e| e.received_at.timestamp())
        .collect();
    let expected: Vec<i64> = (7..10)
        .rev()
        .map(|i| (base + chrono::Duration::seconds(i)).timestamp())
        .collect();
    assert_eq!(kept, expected);
}

/// Same for the age-based sweep: batching must not cut the deletion short.
#[tokio::test]
async fn delete_older_than_batches_through_the_whole_backlog() {
    let db = test_db_with_write_policy(WritePolicy {
        delete_batch: 2,
        ..WritePolicy::default()
    })
    .await;
    let (project_id, issue_id) = seed(&db).await;
    let repo = db.events.clone();

    let base = chrono::Utc::now();
    for i in 0..9 {
        let at = base + chrono::Duration::seconds(i);
        repo.insert(new_event(project_id, issue_id, at))
            .await
            .unwrap();
    }

    let cutoff = base + chrono::Duration::seconds(7);
    let deleted = repo.delete_older_than(project_id, cutoff).await.unwrap();
    assert_eq!(deleted, 7, "seconds 0..6 are older than the cutoff");
    assert_eq!(repo.count_for_project(project_id).await.unwrap(), 2);
}

#[tokio::test]
async fn project_ids_with_events_is_distinct() {
    let db = test_db().await;
    let (project_a, issue_a) = seed(&db).await;
    let (project_b, issue_b) = seed(&db).await;
    let repo = db.events.clone();

    let now = chrono::Utc::now();
    repo.insert(new_event(project_a, issue_a, now))
        .await
        .unwrap();
    repo.insert(new_event(project_a, issue_a, now))
        .await
        .unwrap();
    repo.insert(new_event(project_b, issue_b, now))
        .await
        .unwrap();

    let mut ids = repo.project_ids_with_events().await.unwrap();
    ids.sort();
    let mut expected = vec![project_a, project_b];
    expected.sort();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn delete_older_than_cutoff() {
    let db = test_db().await;
    let (project_id, issue_id) = seed(&db).await;
    let repo = db.events.clone();

    let base = chrono::Utc::now();
    for i in 0..4 {
        let at = base + chrono::Duration::seconds(i);
        repo.insert(new_event(project_id, issue_id, at))
            .await
            .unwrap();
    }
    let cutoff = base + chrono::Duration::seconds(2);
    let deleted = repo.delete_older_than(project_id, cutoff).await.unwrap();
    assert_eq!(deleted, 2, "seconds 0 and 1 are strictly before cutoff");
    assert_eq!(repo.count_for_project(project_id).await.unwrap(), 2);
}

#[tokio::test]
async fn count_in_window_counts_from_since_inclusive() {
    let db = test_db().await;
    let (project_id, issue_id) = seed(&db).await;
    let repo = db.events.clone();

    let base = chrono::Utc::now() - chrono::Duration::seconds(100);
    // Five events at base + {0,1,2,3,4} seconds.
    for i in 0..5 {
        repo.insert(new_event(
            project_id,
            issue_id,
            base + chrono::Duration::seconds(i),
        ))
        .await
        .unwrap();
    }

    // since = base+2s → events at 2,3,4 are counted (inclusive lower bound).
    let since = base + chrono::Duration::seconds(2);
    assert_eq!(repo.count_in_window(issue_id, since).await.unwrap(), 3);

    // since at/before the first event counts them all.
    assert_eq!(repo.count_in_window(issue_id, base).await.unwrap(), 5);

    // A different issue is not counted.
    let other = Id::new_v4();
    assert_eq!(repo.count_in_window(other, base).await.unwrap(), 0);
}
