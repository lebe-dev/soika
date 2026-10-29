//! Contract tests for the `users` repository port.

use crate::fixtures::*;

fn sample(email: &str) -> NewUser {
    NewUser {
        email: email.to_string(),
        display_name: "Test User".to_string(),
        password_hash: "$argon2id$v=19$m=4096,t=3,p=1$abc$def".to_string(),
        instance_role: InstanceRole::Member,
        auth_provider: AuthProvider::Local,
        status: UserStatus::Active,
    }
}

fn oidc_sample(email: &str) -> NewUser {
    NewUser {
        email: email.to_string(),
        display_name: "OIDC User".to_string(),
        // OIDC accounts carry an empty-string password sentinel.
        password_hash: String::new(),
        instance_role: InstanceRole::Member,
        auth_provider: AuthProvider::Oidc,
        status: UserStatus::Active,
    }
}

#[tokio::test]
async fn create_and_find_roundtrip() {
    let repo = test_db().await.users.clone();
    let created = repo.create(sample("a@example.com")).await.unwrap();

    assert_eq!(created.email, "a@example.com");
    assert_eq!(created.instance_role, InstanceRole::Member);
    assert!(created.notifications_enabled);
    assert_eq!(created.auth_provider, AuthProvider::Local);

    let by_id = repo.find_by_id(created.id).await.unwrap().unwrap();
    assert_eq!(by_id.id, created.id);
    assert_eq!(by_id.password_hash, created.password_hash);
    assert_eq!(by_id.auth_provider, AuthProvider::Local);

    let by_email = repo.find_by_email("a@example.com").await.unwrap().unwrap();
    assert_eq!(by_email.id, created.id);
    assert_eq!(by_email.auth_provider, AuthProvider::Local);

    assert!(
        repo.find_by_email("missing@example.com")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn create_oidc_user_with_empty_hash_roundtrips() {
    let repo = test_db().await.users.clone();
    let created = repo.create(oidc_sample("sso@example.com")).await.unwrap();

    assert_eq!(created.auth_provider, AuthProvider::Oidc);
    assert_eq!(created.password_hash, "");

    let by_id = repo.find_by_id(created.id).await.unwrap().unwrap();
    assert_eq!(by_id.auth_provider, AuthProvider::Oidc);
    assert_eq!(by_id.password_hash, "");
}

#[tokio::test]
async fn find_by_email_finds_local_and_oidc_users() {
    let repo = test_db().await.users.clone();
    repo.create(sample("local@example.com")).await.unwrap();
    repo.create(oidc_sample("oidc@example.com")).await.unwrap();

    let local = repo
        .find_by_email("local@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(local.auth_provider, AuthProvider::Local);

    let oidc = repo
        .find_by_email("oidc@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(oidc.auth_provider, AuthProvider::Oidc);
}

#[tokio::test]
async fn duplicate_email_is_conflict() {
    let repo = test_db().await.users.clone();
    repo.create(sample("dup@example.com")).await.unwrap();
    let err = repo.create(sample("dup@example.com")).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn partial_update_only_touches_given_fields() {
    let repo = test_db().await.users.clone();
    let created = repo.create(sample("u@example.com")).await.unwrap();

    let updated = repo
        .update(
            created.id,
            UserUpdate {
                display_name: Some("Renamed".to_string()),
                notifications_enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.display_name, "Renamed");
    assert!(!updated.notifications_enabled);
    // password_hash untouched
    assert_eq!(updated.password_hash, created.password_hash);
}

#[tokio::test]
async fn update_missing_user_is_not_found() {
    let repo = test_db().await.users.clone();
    let err = repo
        .update(Id::new_v4(), UserUpdate::default())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn create_defaults_status_active_and_approve_via_update() {
    let repo = test_db().await.users.clone();
    let created = repo.create(sample("status@example.com")).await.unwrap();
    // Local accounts are seeded active.
    assert_eq!(created.status, UserStatus::Active);

    // A pending OIDC account round-trips and can be approved via `update`.
    let mut pending = oidc_sample("pending@example.com");
    pending.status = UserStatus::Pending;
    let pending = repo.create(pending).await.unwrap();
    assert_eq!(pending.status, UserStatus::Pending);

    let reloaded = repo.find_by_id(pending.id).await.unwrap().unwrap();
    assert_eq!(reloaded.status, UserStatus::Pending);

    let approved = repo
        .update(
            pending.id,
            UserUpdate {
                status: Some(UserStatus::Active),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(approved.status, UserStatus::Active);
}

#[tokio::test]
async fn count_owners_counts_only_owners() {
    let repo = test_db().await.users.clone();
    assert_eq!(repo.count_owners().await.unwrap(), 0);

    // A non-owner account does not count toward initialization.
    repo.create(sample("member@example.com")).await.unwrap();
    assert_eq!(repo.count_owners().await.unwrap(), 0);

    // A Manager is not an Owner either.
    let mut manager = sample("manager@example.com");
    manager.instance_role = InstanceRole::Manager;
    repo.create(manager).await.unwrap();
    assert_eq!(repo.count_owners().await.unwrap(), 0);

    let mut owner = sample("owner@example.com");
    owner.instance_role = InstanceRole::Owner;
    repo.create(owner).await.unwrap();
    assert_eq!(repo.count_owners().await.unwrap(), 1);
}

#[tokio::test]
async fn set_instance_role_updates_and_roundtrips() {
    let repo = test_db().await.users.clone();
    let created = repo.create(sample("role@example.com")).await.unwrap();
    assert_eq!(created.instance_role, InstanceRole::Member);

    let promoted = repo
        .set_instance_role(created.id, InstanceRole::Owner)
        .await
        .unwrap();
    assert_eq!(promoted.instance_role, InstanceRole::Owner);

    let reloaded = repo.find_by_id(created.id).await.unwrap().unwrap();
    assert_eq!(reloaded.instance_role, InstanceRole::Owner);

    let err = repo
        .set_instance_role(Id::new_v4(), InstanceRole::Manager)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn list_count_and_delete() {
    let repo = test_db().await.users.clone();
    assert_eq!(repo.count().await.unwrap(), 0);

    let a = repo.create(sample("a@example.com")).await.unwrap();
    repo.create(sample("b@example.com")).await.unwrap();
    assert_eq!(repo.count().await.unwrap(), 2);
    assert_eq!(repo.list().await.unwrap().len(), 2);

    repo.delete(a.id).await.unwrap();
    assert_eq!(repo.count().await.unwrap(), 1);
    assert!(repo.find_by_id(a.id).await.unwrap().is_none());
    // Deleting a non-existent user is a no-op, not an error.
    repo.delete(Id::new_v4()).await.unwrap();
}

#[tokio::test]
async fn defaults_apply_only_at_creation_and_never_reconcile_existing_memberships() {
    let db = test_db().await;
    let a = insert_team(&db, "A").await;
    let b = insert_team(&db, "B").await;
    let existing = insert_user(&db, "existing@example.com").await;
    db.teams
        .add_member(a, existing, TeamRole::Admin)
        .await
        .unwrap();

    db.settings.set_default_team_ids(vec![a, b]).await.unwrap();
    assert_eq!(
        db.teams.member_role(a, existing).await.unwrap(),
        Some(TeamRole::Admin)
    );
    assert_eq!(db.teams.member_role(b, existing).await.unwrap(), None);

    let new = insert_user(&db, "new@example.com").await;
    for team in [a, b] {
        assert_eq!(
            db.teams.member_role(team, new).await.unwrap(),
            Some(TeamRole::Contributor)
        );
    }
    db.teams.remove_member(a, new).await.unwrap();
    db.users
        .update(
            new,
            UserUpdate {
                display_name: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    db.settings.set_default_team_ids(vec![a]).await.unwrap();
    assert_eq!(db.teams.member_role(a, new).await.unwrap(), None);
    assert_eq!(
        db.teams.member_role(b, new).await.unwrap(),
        Some(TeamRole::Contributor)
    );

    let later = insert_user(&db, "later@example.com").await;
    assert_eq!(
        db.teams.member_role(a, later).await.unwrap(),
        Some(TeamRole::Contributor)
    );
    assert_eq!(db.teams.member_role(b, later).await.unwrap(), None);
    db.settings.set_default_team_ids(vec![]).await.unwrap();
    let last = insert_user(&db, "last@example.com").await;
    assert!(db.teams.list_for_user(last).await.unwrap().is_empty());
    assert_eq!(
        db.teams.member_role(a, later).await.unwrap(),
        Some(TeamRole::Contributor)
    );
}

#[tokio::test]
async fn default_teams_apply_to_oidc_and_pending_accounts_without_approving_them() {
    let db = test_db().await;
    let team = insert_team(&db, "Default").await;
    db.settings.set_default_team_ids(vec![team]).await.unwrap();
    for status in [UserStatus::Active, UserStatus::Pending] {
        let user = db
            .users
            .create(NewUser {
                status,
                ..oidc_sample(&format!("{status:?}@example.com"))
            })
            .await
            .unwrap();
        assert_eq!(user.status, status);
        assert_eq!(
            db.teams.member_role(team, user.id).await.unwrap(),
            Some(TeamRole::Contributor)
        );
    }
}

#[tokio::test]
async fn duplicate_account_does_not_gain_new_default_memberships() {
    let db = test_db().await;
    let existing = db
        .users
        .create(sample("existing@example.com"))
        .await
        .unwrap();
    let team = insert_team(&db, "Default").await;
    db.settings.set_default_team_ids(vec![team]).await.unwrap();
    assert!(matches!(
        db.users.create(sample("existing@example.com")).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(db.teams.member_role(team, existing.id).await.unwrap(), None);
    assert_eq!(db.users.count().await.unwrap(), 1);
}

#[tokio::test]
async fn conversion_to_sso_revokes_local_authentication() {
    let db = test_db().await;
    let user = db
        .users
        .create(sample("convert@example.com"))
        .await
        .unwrap();
    db.sessions
        .create(
            "old-session".into(),
            user.id,
            chrono::Utc::now() + chrono::Duration::hours(1),
        )
        .await
        .unwrap();
    db.passkeys
        .create(NewPasskey {
            user_id: user.id,
            credential_id: "old-key".into(),
            name: "old".into(),
            credential: "{}".into(),
        })
        .await
        .unwrap();
    let converted = db.users.convert_to_sso(user.id).await.unwrap();
    assert!(db.passkeys.list_for_user(user.id).await.unwrap().is_empty());
    assert_eq!(converted.auth_provider, AuthProvider::Oidc);
    assert!(converted.password_hash.is_empty());
    assert!(db.sessions.find("old-session").await.unwrap().is_none());
    let owner = db
        .users
        .set_instance_role(user.id, InstanceRole::Owner)
        .await
        .unwrap();
    assert!(db.users.convert_to_sso(owner.id).await.is_err());
}
