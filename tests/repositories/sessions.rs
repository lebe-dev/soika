//! Contract tests for the `sessions` repository port.

use crate::fixtures::*;

use chrono::Duration;

#[tokio::test]
async fn create_find_delete() {
    let db = test_db().await;
    let user_id = insert_user(&db, "s@example.com").await;
    let repo = db.sessions.clone();

    let expires = chrono::Utc::now() + Duration::hours(1);
    let created = repo
        .create("tok-1".to_string(), user_id, expires)
        .await
        .unwrap();
    assert_eq!(created.user_id, user_id);

    let found = repo.find("tok-1").await.unwrap().unwrap();
    assert_eq!(found.id, "tok-1");

    repo.delete("tok-1").await.unwrap();
    assert!(repo.find("tok-1").await.unwrap().is_none());
}

#[tokio::test]
async fn expired_session_is_not_returned_and_can_be_purged() {
    let db = test_db().await;
    let user_id = insert_user(&db, "e@example.com").await;
    let repo = db.sessions.clone();

    let past = chrono::Utc::now() - Duration::hours(1);
    repo.create("old".to_string(), user_id, past).await.unwrap();

    // find skips expired sessions.
    assert!(repo.find("old").await.unwrap().is_none());

    let purged = repo.delete_expired(chrono::Utc::now()).await.unwrap();
    assert_eq!(purged, 1);
}

#[tokio::test]
async fn delete_for_user_removes_all() {
    let db = test_db().await;
    let user_id = insert_user(&db, "d@example.com").await;
    let repo = db.sessions.clone();

    let expires = chrono::Utc::now() + Duration::hours(1);
    repo.create("a".to_string(), user_id, expires)
        .await
        .unwrap();
    repo.create("b".to_string(), user_id, expires)
        .await
        .unwrap();

    repo.delete_for_user(user_id).await.unwrap();
    assert!(repo.find("a").await.unwrap().is_none());
    assert!(repo.find("b").await.unwrap().is_none());
}

/// A session returned by `create` equals the one read back, even when the
/// caller's expiry carries sub-microsecond precision.
#[tokio::test]
async fn created_session_matches_a_fresh_read() {
    use chrono::TimeZone;

    let db = test_db().await;
    let user_id = insert_user(&db, "precise@example.com").await;
    let expires = chrono::Utc
        .timestamp_opt(4_000_000_000, 987_654_321)
        .single()
        .unwrap();

    let created = db
        .sessions
        .create("precise".to_string(), user_id, expires)
        .await
        .unwrap();
    let found = db.sessions.find("precise").await.unwrap().unwrap();

    assert_eq!(found.created_at, created.created_at);
    assert_eq!(found.expires_at, created.expires_at);
}

#[tokio::test]
async fn authentication_challenges_are_expiring_purpose_bound_and_single_use() {
    let state = test_db().await;
    let now = chrono::Utc::now();
    let repo = &state.auth_challenges;
    repo.create(
        "one-use",
        "login",
        "state",
        now + chrono::Duration::minutes(5),
    )
    .await
    .unwrap();
    assert!(
        repo.take("one-use", "register", now)
            .await
            .unwrap()
            .is_none()
    );
    let (a, b) = tokio::join!(
        repo.take("one-use", "login", now),
        repo.take("one-use", "login", now)
    );
    let results = [a.unwrap(), b.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|value| value.as_deref() == Some("state"))
            .count(),
        1
    );
    assert_eq!(results.iter().filter(|value| value.is_none()).count(), 1);
    repo.create(
        "expired",
        "login",
        "state",
        now - chrono::Duration::seconds(1),
    )
    .await
    .unwrap();
    assert!(repo.take("expired", "login", now).await.unwrap().is_none());
}
