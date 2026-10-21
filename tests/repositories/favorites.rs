//! Contract tests for the `favorites` repository port.

use crate::fixtures::*;

#[tokio::test]
async fn add_is_idempotent_and_listed_per_user() {
    let db = test_db().await;
    let team = insert_team(&db, "t").await;
    let alpha = insert_project(&db, team, "alpha", "dsn-alpha").await;
    let beta = insert_project(&db, team, "beta", "dsn-beta").await;
    let user = insert_user(&db, "fav@example.com").await;
    let other = insert_user(&db, "other@example.com").await;

    db.favorites.add(user, alpha).await.unwrap();
    // Favoriting twice is a no-op, not a conflict.
    db.favorites.add(user, alpha).await.unwrap();
    db.favorites.add(user, beta).await.unwrap();

    let mut mine = db.favorites.list_for_user(user).await.unwrap();
    mine.sort();
    let mut expected = vec![alpha, beta];
    expected.sort();
    assert_eq!(mine, expected);

    // Favorites are per user.
    assert!(db.favorites.list_for_user(other).await.unwrap().is_empty());
}

#[tokio::test]
async fn remove_unfavorites_and_tolerates_missing_rows() {
    let db = test_db().await;
    let project = seed_project(&db).await;
    let user = insert_user(&db, "fav@example.com").await;

    db.favorites.add(user, project).await.unwrap();
    db.favorites.remove(user, project).await.unwrap();
    assert!(db.favorites.list_for_user(user).await.unwrap().is_empty());

    // Removing something that is not favorited is a no-op.
    db.favorites.remove(user, project).await.unwrap();
}

#[tokio::test]
async fn deleting_the_project_drops_its_favorites() {
    let db = test_db().await;
    let project = seed_project(&db).await;
    let user = insert_user(&db, "fav@example.com").await;

    db.favorites.add(user, project).await.unwrap();
    db.projects.delete(project).await.unwrap();

    assert!(db.favorites.list_for_user(user).await.unwrap().is_empty());
}
