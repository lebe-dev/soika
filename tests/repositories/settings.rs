//! Contract tests for the `settings` repository port.

use crate::fixtures::*;

#[tokio::test]
async fn defaults_from_migration() {
    let repo = test_db().await.settings.clone();
    let settings = repo.get().await.unwrap();
    assert!(!settings.allow_signup);
    assert_eq!(settings.org_name, "soika");
}

#[tokio::test]
async fn toggle_allow_signup() {
    let repo = test_db().await.settings.clone();
    let updated = repo.set_allow_signup(true).await.unwrap();
    assert!(updated.allow_signup);
    // org_name preserved.
    assert_eq!(updated.org_name, "soika");

    let read_back = repo.get().await.unwrap();
    assert!(read_back.allow_signup);
}

#[tokio::test]
async fn set_org_name_persists() {
    let repo = test_db().await.settings.clone();
    let updated = repo.set_org_name("Acme".to_string()).await.unwrap();
    assert_eq!(updated.org_name, "Acme");
    // allow_signup default preserved.
    assert!(!updated.allow_signup);

    assert_eq!(repo.get().await.unwrap().org_name, "Acme");
}

#[tokio::test]
async fn default_teams_replace_clear_and_ignore_duplicates() {
    let db = test_db().await;
    assert!(db.settings.default_team_ids().await.unwrap().is_empty());
    let a = insert_team(&db, "A").await;
    let b = insert_team(&db, "B").await;
    db.settings
        .set_default_team_ids(vec![b, a, a])
        .await
        .unwrap();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(db.settings.default_team_ids().await.unwrap(), expected);
    db.settings.set_default_team_ids(vec![b]).await.unwrap();
    assert_eq!(db.settings.default_team_ids().await.unwrap(), vec![b]);
    db.settings.set_default_team_ids(vec![]).await.unwrap();
    assert!(db.settings.default_team_ids().await.unwrap().is_empty());
    assert_eq!(db.settings.get().await.unwrap().org_name, "soika");
}

#[tokio::test]
async fn unknown_default_team_rejects_entire_replacement() {
    let db = test_db().await;
    let a = insert_team(&db, "A").await;
    let b = insert_team(&db, "B").await;
    db.settings.set_default_team_ids(vec![a]).await.unwrap();
    assert!(matches!(
        db.settings
            .set_default_team_ids(vec![b, Id::new_v4()])
            .await,
        Err(Error::Validation(_))
    ));
    assert_eq!(db.settings.default_team_ids().await.unwrap(), vec![a]);
}

#[tokio::test]
async fn deleting_default_team_removes_setting_and_does_not_block_new_users() {
    let db = test_db().await;
    let team = insert_team(&db, "A").await;
    db.settings.set_default_team_ids(vec![team]).await.unwrap();
    db.teams.delete(team, None).await.unwrap();
    assert!(db.settings.default_team_ids().await.unwrap().is_empty());
    let user = insert_user(&db, "new@example.com").await;
    assert!(db.teams.list_for_user(user).await.unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_default_replacements_never_merge_lists() {
    let db = test_db().await;
    let a = insert_team(&db, "A").await;
    let b = insert_team(&db, "B").await;
    let (first, second) = tokio::join!(
        db.settings.set_default_team_ids(vec![a]),
        db.settings.set_default_team_ids(vec![b]),
    );
    first.unwrap();
    second.unwrap();
    let defaults = db.settings.default_team_ids().await.unwrap();
    assert!(defaults == vec![a] || defaults == vec![b]);
}
