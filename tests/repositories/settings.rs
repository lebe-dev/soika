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
