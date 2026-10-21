//! Contract tests for the `projects` repository port.

use crate::fixtures::*;

async fn seed_team(db: &AppState) -> Id {
    insert_team(db, "team-a").await
}

/// Add a fresh user to `team_id` with the given role and return the user id.
async fn seed_member(db: &AppState, team_id: Id, email: &str, role: TeamRole) -> Id {
    let user_id = insert_user(db, email).await;
    db.teams
        .add_member(team_id, user_id, role)
        .await
        .expect("add member");
    user_id
}

fn new_project(team_id: Id, slug: &str, dsn: &str) -> NewProject {
    NewProject {
        team_id,
        name: format!("Project {slug}"),
        slug: slug.to_string(),
        dsn_public_key: dsn.to_string(),
        retention_events: 1000,
        retention_days: 0,
        webhook_url: None,
    }
}

#[tokio::test]
async fn create_and_find_by_dsn() {
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    let created = repo
        .create(new_project(team_id, "alpha", "dsn-key-alpha"))
        .await
        .unwrap();
    assert_eq!(created.team_id, team_id);
    assert!(!created.muted);

    let found = repo.find_by_dsn("dsn-key-alpha").await.unwrap().unwrap();
    assert_eq!(found.id, created.id);
    assert!(repo.find_by_dsn("nope").await.unwrap().is_none());
}

#[tokio::test]
async fn list_for_user_returns_team_projects() {
    // Variant A (closed membership): a user sees a project iff they belong to
    // the owning team. Regression: the join must qualify its projection, since
    // `team_members` also has a `created_at` (an unqualified `*` was ambiguous).
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    let project = repo
        .create(new_project(team_id, "alpha", "dsn-alpha"))
        .await
        .unwrap();

    let user_id = seed_member(&db, team_id, "m@example.com", TeamRole::Contributor).await;

    let mine = repo.list_for_user(user_id).await.unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].id, project.id);

    // A user in no team sees nothing.
    let stranger = repo.list_for_user(Id::new_v4()).await.unwrap();
    assert!(stranger.is_empty());
}

#[tokio::test]
async fn list_for_user_dedups_multiple_projects_in_one_team() {
    // A team member sees every project the team owns, each exactly once.
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    let p1 = repo
        .create(new_project(team_id, "alpha", "dsn-team-alpha"))
        .await
        .unwrap();
    let p2 = repo
        .create(new_project(team_id, "beta", "dsn-team-beta"))
        .await
        .unwrap();

    let user_id = seed_member(&db, team_id, "t@example.com", TeamRole::Admin).await;

    let mut mine = repo.list_for_user(user_id).await.unwrap();
    mine.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0].id, p1.id);
    assert_eq!(mine[1].id, p2.id);
}

#[tokio::test]
async fn create_assigns_short_id_resolvable_via_find_by_short_id() {
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    let created = repo
        .create(new_project(team_id, "alpha", "dsn-short"))
        .await
        .unwrap();

    // The short id is a compact 6-char lowercase-alphanumeric code, distinct
    // from the UUID, and resolves back to the same project.
    assert_eq!(created.short_id.len(), 6);
    assert!(created.short_id.chars().all(|c| c.is_ascii_alphanumeric()));
    assert_ne!(created.short_id, created.id.to_string());

    let found = repo
        .find_by_short_id(&created.short_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.id, created.id);
    assert!(repo.find_by_short_id("zzzzzz").await.unwrap().is_none());
}

#[tokio::test]
async fn short_ids_are_unique_across_projects() {
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    let a = repo
        .create(new_project(team_id, "a", "dsn-a"))
        .await
        .unwrap();
    let b = repo
        .create(new_project(team_id, "b", "dsn-b"))
        .await
        .unwrap();
    assert_ne!(a.short_id, b.short_id);
}

#[tokio::test]
async fn duplicate_dsn_is_conflict() {
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    repo.create(new_project(team_id, "a", "same-dsn"))
        .await
        .unwrap();
    let err = repo
        .create(new_project(team_id, "b", "same-dsn"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn partial_update_leaves_other_fields() {
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    let p = repo
        .create(new_project(team_id, "alpha", "dsn-alpha"))
        .await
        .unwrap();

    let updated = repo
        .update(
            p.id,
            ProjectUpdate {
                muted: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(updated.muted);
    assert_eq!(updated.name, p.name, "name preserved by COALESCE update");
    assert_eq!(updated.retention_events, 1000);
    assert_eq!(updated.retention_days, 0, "retention_days untouched");
}

#[tokio::test]
async fn webhook_url_round_trips_through_create_and_partial_update() {
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    // Create without a webhook URL: NULL round-trips to None.
    let without = repo
        .create(new_project(team_id, "no-hook", "dsn-no-hook"))
        .await
        .unwrap();
    assert_eq!(without.webhook_url, None);
    let reread = repo.find_by_id(without.id).await.unwrap().unwrap();
    assert_eq!(reread.webhook_url, None);

    // Create WITH a webhook URL: it survives the round-trip.
    let mut new = new_project(team_id, "hook", "dsn-hook");
    new.webhook_url = Some("https://hook.example/x".into());
    let with = repo.create(new).await.unwrap();
    assert_eq!(with.webhook_url.as_deref(), Some("https://hook.example/x"));
    let reread = repo.find_by_id(with.id).await.unwrap().unwrap();
    assert_eq!(
        reread.webhook_url.as_deref(),
        Some("https://hook.example/x")
    );

    // Some(Some(url)) sets the webhook URL on an existing project.
    let set = repo
        .update(
            without.id,
            ProjectUpdate {
                webhook_url: Some(Some("https://h".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(set.webhook_url.as_deref(), Some("https://h"));

    // None (e.g. a muted-only update) leaves the existing webhook unchanged.
    let unchanged = repo
        .update(
            without.id,
            ProjectUpdate {
                muted: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(unchanged.muted);
    assert_eq!(
        unchanged.webhook_url.as_deref(),
        Some("https://h"),
        "None leaves webhook_url unchanged"
    );

    // Some(None) clears the webhook to NULL (disable the channel). This must
    // actually persist NULL, not silently keep the old value.
    let cleared = repo
        .update(
            without.id,
            ProjectUpdate {
                webhook_url: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        cleared.webhook_url, None,
        "Some(None) clears webhook_url to NULL"
    );
    let reread = repo.find_by_id(without.id).await.unwrap().unwrap();
    assert_eq!(reread.webhook_url, None, "cleared NULL is persisted");
    assert!(
        reread.muted,
        "clearing webhook_url leaves other fields (muted) intact"
    );
}

#[tokio::test]
async fn retention_days_round_trips_through_create_and_update() {
    let db = test_db().await;
    let team_id = seed_team(&db).await;
    let repo = db.projects.clone();

    // Create with an explicit age-based retention window.
    let mut new = new_project(team_id, "beta", "dsn-beta");
    new.retention_days = 7;
    let created = repo.create(new).await.unwrap();
    assert_eq!(created.retention_days, 7, "created value persisted");

    // Some(value) updates the column.
    let updated = repo
        .update(
            created.id,
            ProjectUpdate {
                retention_days: Some(30),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.retention_days, 30, "Some(value) updates the column");

    // None leaves the column unchanged.
    let unchanged = repo
        .update(
            created.id,
            ProjectUpdate {
                name: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        unchanged.retention_days, 30,
        "None leaves retention_days unchanged"
    );
    assert_eq!(unchanged.name, "Renamed");
}
