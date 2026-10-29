//! Contract tests for the `invites` repository port.

use crate::fixtures::*;

use chrono::Duration;

async fn fixtures(db: &AppState) -> (Id, Id) {
    let team_id = insert_team(db, "T").await;
    let user_id = insert_user(db, "admin@example.com").await;
    (team_id, user_id)
}

fn new_invite(team_id: Id, created_by: Id, token: &str) -> NewInvite {
    NewInvite {
        token: token.to_string(),
        team_id,
        role: TeamRole::Contributor,
        email: Some("invitee@example.com".to_string()),
        created_by: Some(created_by),
        expires_at: chrono::Utc::now() + Duration::days(7),
    }
}

#[tokio::test]
async fn create_find_roundtrip() {
    let db = test_db().await;
    let (team_id, user_id) = fixtures(&db).await;
    let repo = db.invites.clone();

    let created = repo
        .create(new_invite(team_id, user_id, "tok"))
        .await
        .unwrap();
    assert_eq!(created.role, TeamRole::Contributor);
    assert_eq!(created.email.as_deref(), Some("invitee@example.com"));
    assert_eq!(created.created_by, Some(user_id));
    assert!(created.accepted_at.is_none());

    let found = repo.find_by_token("tok").await.unwrap().unwrap();
    assert_eq!(found.token, "tok");
    assert_eq!(found.team_id, team_id);

    assert!(repo.find_by_token("missing").await.unwrap().is_none());
}

#[tokio::test]
async fn create_without_email_or_creator() {
    let db = test_db().await;
    let (team_id, _user_id) = fixtures(&db).await;
    let repo = db.invites.clone();

    let invite = NewInvite {
        token: "anon".to_string(),
        team_id,
        role: TeamRole::Admin,
        email: None,
        created_by: None,
        expires_at: chrono::Utc::now() + Duration::days(1),
    };
    repo.create(invite).await.unwrap();

    let found = repo.find_by_token("anon").await.unwrap().unwrap();
    assert_eq!(found.role, TeamRole::Admin);
    assert!(found.email.is_none());
    assert!(found.created_by.is_none());
}

#[tokio::test]
async fn mark_accepted_and_delete() {
    let db = test_db().await;
    let (team_id, user_id) = fixtures(&db).await;
    let repo = db.invites.clone();

    repo.create(new_invite(team_id, user_id, "tok"))
        .await
        .unwrap();

    let when = chrono::Utc::now();
    repo.mark_accepted("tok", when).await.unwrap();
    let found = repo.find_by_token("tok").await.unwrap().unwrap();
    assert!(found.accepted_at.is_some());

    assert_eq!(repo.list_for_team(team_id).await.unwrap().len(), 1);

    repo.delete("tok").await.unwrap();
    assert!(repo.find_by_token("tok").await.unwrap().is_none());
}

#[tokio::test]
async fn invite_accept_is_single_use_and_does_not_demote_an_admin() {
    let db = test_db().await;
    let (team, creator) = fixtures(&db).await;
    let first = insert_user(&db, "first@example.com").await;
    let second = insert_user(&db, "second@example.com").await;
    let mut invite = new_invite(team, creator, "once");
    invite.email = None;
    db.invites.create(invite).await.unwrap();
    let now = chrono::Utc::now();
    let (a, b) = tokio::join!(
        db.invites.accept("once", first, now),
        db.invites.accept("once", second, now)
    );
    assert_eq!(
        [a.is_ok(), b.is_ok()].into_iter().filter(|ok| *ok).count(),
        1
    );
    let members = db.teams.members(team).await.unwrap();
    assert_eq!(members.len(), 1);
    let winner = members[0].0.id;
    db.teams
        .add_member(team, winner, TeamRole::Admin)
        .await
        .unwrap();
    let mut invite = new_invite(team, creator, "contributor");
    invite.email = None;
    db.invites.create(invite).await.unwrap();
    db.invites.accept("contributor", winner, now).await.unwrap();
    assert_eq!(
        db.teams.member_role(team, winner).await.unwrap(),
        Some(TeamRole::Admin)
    );
}

#[tokio::test]
async fn wrong_recipient_does_not_consume_invite_or_gain_membership() {
    let db = test_db().await;
    let (team, creator) = fixtures(&db).await;
    db.invites
        .create(new_invite(team, creator, "targeted"))
        .await
        .unwrap();
    assert!(
        db.invites
            .accept("targeted", creator, chrono::Utc::now())
            .await
            .is_err()
    );
    assert!(
        db.invites
            .find_by_token("targeted")
            .await
            .unwrap()
            .unwrap()
            .accepted_at
            .is_none()
    );
    assert!(!db.teams.is_member(team, creator).await.unwrap());
}
