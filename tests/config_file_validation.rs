use soika::domain::config_file::*;

fn yaml_settings() -> ManagementSettings {
    ManagementSettings {
        project_source: ManagementSource::Yaml,
        team_source: ManagementSource::Yaml,
        ..Default::default()
    }
}

#[test]
fn rejects_invalid_projects_and_duplicate_slugs() {
    for project in [
        "{slug: '', name: API, team: Platform}",
        "{slug: Upper, name: API, team: Platform}",
        "{slug: api, name: ' ', team: Platform}",
        "{slug: api, name: API, team: ' '}",
        "{slug: api, name: API, team: Platform, retention_events: 0}",
        "{slug: api, name: API, team: Platform, retention_days: -1}",
        "{slug: api, name: API, team: Platform, webhook_url: 'file:///tmp/hook'}",
    ] {
        let text = format!("version: 1\nteams: []\nprojects: [{project}]");
        assert!(parse(&text, &yaml_settings()).is_err(), "{project}");
    }
    assert!(parse("version: 1\nteams: []\nprojects: [{slug: api, name: API, team: Platform}, {slug: api, name: Duplicate, team: Platform}]", &yaml_settings()).is_err());
}

#[test]
fn rejects_invalid_teams_and_case_insensitive_duplicate_grants() {
    for teams in [
        "[{name: '', members: []}]",
        "[{name: Platform, members: []}, {name: Platform, members: []}]",
        "[{name: Platform}]",
        "[{name: Platform, members: [{email: invalid, role: admin}]}]",
        "[{name: Platform, members: [{email: 'a b@example.com', role: admin}]}]",
        "[{name: Platform, members: [{email: A@example.com, role: admin}, {email: a@example.com, role: contributor}]}]",
        "[{name: Platform, sso_groups: [{group: ' ', role: admin}]}]",
        "[{name: Platform, sso_groups: [{group: Acme, role: admin}, {group: acme, role: contributor}]}]",
    ] {
        let text = format!("version: 1\nprojects: []\nteams: {teams}");
        assert!(parse(&text, &yaml_settings()).is_err(), "{teams}");
    }
    assert!(parse("version: 1\nprojects: []", &yaml_settings()).is_err());
}

#[test]
fn accepts_zero_retention_days_and_empty_explicit_grants() {
    let text = "version: 1\nteams: [{name: Platform, sso_groups: []}]\nprojects: [{slug: api, name: API, team: Platform, retention_events: 1, retention_days: 0, webhook_url: 'https://example.com/hook'}]";
    assert!(parse(text, &yaml_settings()).is_ok());
}

#[test]
fn deletion_plan_checks_final_ownership_and_aggregates_errors() {
    let settings = ManagementSettings {
        team_orphan_policy: OrphanPolicy::Delete,
        ..yaml_settings()
    };
    let file = parse("version: 1\nprojects: [{slug: api, name: API, team: New}]\nteams: [{name: New, members: []}]", &settings).unwrap();
    let teams = vec!["Old".into()];
    let projects = vec![("api".into(), "Old".into())];
    assert!(validate_plan(&file, &settings, &teams, &projects).is_ok());
    let mut removed = file.clone();
    removed.projects = Some(vec![]);
    assert!(validate_plan(&removed, &settings, &teams, &projects).is_err());
    let delete = ManagementSettings {
        project_orphan_policy: OrphanPolicy::Delete,
        ..settings.clone()
    };
    assert!(validate_plan(&removed, &delete, &teams, &projects).is_ok());
    removed.projects = None;
    assert!(validate_plan(&removed, &delete, &teams, &projects).is_err());
    let mut invalid = file;
    invalid.projects.as_mut().unwrap()[0].team = "Missing".into();
    let error = validate_plan(
        &invalid,
        &settings,
        &teams,
        &[("other".into(), "Old".into())],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown team Missing"));
    assert!(error.contains("still owns projects: other"));
}
