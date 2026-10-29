//! Contract tests for the `teams` repository port.

use crate::fixtures::*;

#[tokio::test]
async fn create_rename_list_delete() {
    let db = test_db().await;
    let repo = db.teams.clone();

    let t = repo.create("Backend".to_string()).await.unwrap();
    assert_eq!(repo.list().await.unwrap().len(), 1);

    let renamed = repo.rename(t.id, "Platform".to_string()).await.unwrap();
    assert_eq!(renamed.name, "Platform");
    assert_eq!(renamed.created_at, t.created_at);

    repo.delete(t.id, None).await.unwrap();
    assert!(repo.find_by_id(t.id).await.unwrap().is_none());
}

#[tokio::test]
async fn duplicate_name_is_conflict() {
    let db = test_db().await;
    let repo = db.teams.clone();
    repo.create("Dup".to_string()).await.unwrap();
    let err = repo.create("Dup".to_string()).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn membership_add_remove_and_listing() {
    let db = test_db().await;
    let repo = db.teams.clone();

    let team = repo.create("T".to_string()).await.unwrap();
    let u1 = insert_user(&db, "m1@example.com").await;
    let u2 = insert_user(&db, "m2@example.com").await;

    let m1 = repo.add_member(team.id, u1, TeamRole::Admin).await.unwrap();
    assert_eq!(m1.role, TeamRole::Admin);
    repo.add_member(team.id, u2, TeamRole::Admin).await.unwrap();
    // Idempotent re-add does not error or duplicate; it re-assigns the role.
    let re = repo
        .add_member(team.id, u1, TeamRole::Contributor)
        .await
        .unwrap();
    assert_eq!(re.role, TeamRole::Contributor);

    let members = repo.members(team.id).await.unwrap();
    assert_eq!(members.len(), 2);

    // member_role reflects the (re-)assigned role.
    assert_eq!(
        repo.member_role(team.id, u1).await.unwrap(),
        Some(TeamRole::Contributor)
    );
    assert_eq!(
        repo.member_role(team.id, u2).await.unwrap(),
        Some(TeamRole::Admin)
    );
    assert_eq!(repo.member_role(team.id, Id::new_v4()).await.unwrap(), None);

    // Promote u1 back to Admin via set_member_role.
    let promoted = repo
        .set_member_role(team.id, u1, TeamRole::Admin)
        .await
        .unwrap();
    assert_eq!(promoted.role, TeamRole::Admin);
    assert_eq!(
        repo.member_role(team.id, u1).await.unwrap(),
        Some(TeamRole::Admin)
    );
    // Setting the role of a non-member is a not-found error.
    let err = repo
        .set_member_role(team.id, Id::new_v4(), TeamRole::Admin)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");

    let teams_for_u1 = repo.list_for_user(u1).await.unwrap();
    assert_eq!(teams_for_u1.len(), 1);
    assert_eq!(teams_for_u1[0].id, team.id);

    // Membership probe mirrors the listing.
    assert!(repo.is_member(team.id, u1).await.unwrap());
    assert!(!repo.is_member(team.id, Id::new_v4()).await.unwrap());

    repo.remove_member(team.id, u1).await.unwrap();
    assert_eq!(repo.members(team.id).await.unwrap().len(), 1);
    assert!(repo.list_for_user(u1).await.unwrap().is_empty());
    assert!(!repo.is_member(team.id, u1).await.unwrap());
    assert_eq!(repo.member_role(team.id, u1).await.unwrap(), None);
}

#[tokio::test]
async fn deletion_preserves_projects_and_requires_explicit_transfer() {
    let db = test_db().await;
    let source = insert_team(&db, "source").await;
    let target = insert_team(&db, "target").await;
    let project = insert_project(&db, source, "keep", "stable-dsn").await;
    assert!(matches!(
        db.teams.delete(source, None).await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        db.teams.delete(source, Some(source)).await,
        Err(Error::Validation(_))
    ));
    assert!(db.teams.delete(source, Some(Id::new_v4())).await.is_err());
    assert_eq!(
        db.projects
            .find_by_id(project)
            .await
            .unwrap()
            .unwrap()
            .team_id,
        source
    );
    db.teams.delete(source, Some(target)).await.unwrap();
    let kept = db.projects.find_by_id(project).await.unwrap().unwrap();
    assert_eq!(kept.team_id, target);
    assert_eq!(kept.dsn_public_key, "stable-dsn");
    assert!(db.teams.find_by_id(source).await.unwrap().is_none());
}
