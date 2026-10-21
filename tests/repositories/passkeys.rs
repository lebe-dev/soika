//! Contract tests for the `passkeys` repository port.

use crate::fixtures::*;

fn new_key(user_id: Id, credential_id: &str) -> NewPasskey {
    NewPasskey {
        user_id,
        credential_id: credential_id.to_string(),
        name: "MacBook".to_string(),
        credential: "{\"cred\":true}".to_string(),
    }
}

#[tokio::test]
async fn create_list_find_delete() {
    let db = test_db().await;
    let user_id = insert_user(&db, "p@example.com").await;
    let repo = db.passkeys.clone();

    let created = repo.create(new_key(user_id, "cred-1")).await.unwrap();
    assert_eq!(created.user_id, user_id);
    assert!(created.last_used_at.is_none());

    let listed = repo.list_for_user(user_id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(repo.count_for_user(user_id).await.unwrap(), 1);

    let found = repo.find_by_credential_id("cred-1").await.unwrap().unwrap();
    assert_eq!(found.id, created.id);

    repo.delete(user_id, created.id).await.unwrap();
    assert!(repo.list_for_user(user_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn duplicate_credential_id_is_a_conflict() {
    let db = test_db().await;
    let first = insert_user(&db, "a@example.com").await;
    let second = insert_user(&db, "b@example.com").await;
    let repo = db.passkeys.clone();

    repo.create(new_key(first, "shared")).await.unwrap();
    let err = repo.create(new_key(second, "shared")).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)));
}

#[tokio::test]
async fn touch_persists_credential_and_last_used() {
    let db = test_db().await;
    let user_id = insert_user(&db, "t@example.com").await;
    let repo = db.passkeys.clone();
    let created = repo.create(new_key(user_id, "cred-2")).await.unwrap();

    let used_at = chrono::Utc::now();
    repo.touch(created.id, "{\"cred\":\"updated\"}", used_at)
        .await
        .unwrap();

    let found = repo
        .find_for_user(user_id, created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.credential, "{\"cred\":\"updated\"}");
    assert!(found.last_used_at.is_some());
}

#[tokio::test]
async fn rename_and_delete_are_scoped_to_the_owner() {
    let db = test_db().await;
    let owner = insert_user(&db, "owner@example.com").await;
    let other = insert_user(&db, "other@example.com").await;
    let repo = db.passkeys.clone();
    let created = repo.create(new_key(owner, "cred-3")).await.unwrap();

    let renamed = repo.rename(owner, created.id, "iPhone").await.unwrap();
    assert_eq!(renamed.name, "iPhone");

    let err = repo.rename(other, created.id, "stolen").await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)));

    let err = repo.delete(other, created.id).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)));
    assert_eq!(repo.count_for_user(owner).await.unwrap(), 1);
}

#[tokio::test]
async fn deleting_the_user_cascades_to_passkeys() {
    let db = test_db().await;
    let user_id = insert_user(&db, "cascade@example.com").await;
    let repo = db.passkeys.clone();
    repo.create(new_key(user_id, "cred-4")).await.unwrap();

    db.users.delete(user_id).await.unwrap();

    assert!(
        repo.find_by_credential_id("cred-4")
            .await
            .unwrap()
            .is_none()
    );
}
