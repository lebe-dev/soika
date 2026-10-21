//! Contract tests for the `mute_rules` repository port.

use crate::fixtures::*;

fn tag(k: &str, v: &str) -> TagMatch {
    TagMatch {
        key: k.to_string(),
        value: v.to_string(),
    }
}

#[tokio::test]
async fn create_list_and_delete_roundtrip() {
    let db = test_db().await;
    let team = insert_team(&db, "t").await;
    let project = insert_project(&db, team, "p", "dsn-p").await;
    let user = insert_user(&db, "u@example.com").await;
    let repo = db.mute_rules.clone();

    let created = repo
        .create(NewTagMuteRule {
            project_id: project,
            name: Some("staging noise".into()),
            created_by: Some(user),
            tags: vec![tag("environment", "staging"), tag("server", "ci")],
        })
        .await
        .expect("create");

    assert_eq!(created.tags.len(), 2);

    let listed = repo.list_for_project(project).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
    assert_eq!(listed[0].name.as_deref(), Some("staging noise"));
    assert_eq!(listed[0].created_by, Some(user));
    assert_eq!(listed[0].tags.len(), 2);

    // Wrong project id does not delete.
    assert!(
        !repo
            .delete(Id::new_v4(), created.id)
            .await
            .expect("delete other")
    );
    assert!(repo.delete(project, created.id).await.expect("delete"));
    assert!(
        repo.list_for_project(project)
            .await
            .expect("list")
            .is_empty()
    );
}

#[tokio::test]
async fn deleting_project_cascades_rules() {
    let db = test_db().await;
    let team = insert_team(&db, "t").await;
    let project = insert_project(&db, team, "p", "dsn-p").await;
    let repo = db.mute_rules.clone();

    repo.create(NewTagMuteRule {
        project_id: project,
        name: None,
        created_by: None,
        tags: vec![tag("environment", "staging")],
    })
    .await
    .expect("create");

    db.projects.delete(project).await.expect("delete project");

    let rules = repo.list_for_project(project).await.expect("list");
    assert!(rules.is_empty(), "rules cascade when the project is gone");
}
