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
fn slug_errors_name_every_offending_slug_and_the_rule() {
    let long = "a".repeat(49);
    let text = format!(
        "version: 1\nteams: []\nprojects: [\
         {{slug: Web_API, name: A, team: P}}, \
         {{slug: -edge, name: B, team: P}}, \
         {{slug: {long}, name: C, team: P}}, \
         {{slug: api, name: D, team: P}}, \
         {{slug: api, name: E, team: P}}]"
    );
    let err = parse(&text, &yaml_settings()).unwrap_err().to_string();
    assert!(err.contains("\"Web_API\""), "{err}");
    assert!(
        err.contains("lowercase latin letters, digits and '-'"),
        "{err}"
    );
    assert!(err.contains("\"-edge\""), "{err}");
    assert!(err.contains("at most 48 characters"), "{err}");
    assert!(err.contains("duplicate project slug \"api\""), "{err}");
}

#[test]
fn alertmanager_label_keys_follow_the_slug_rule() {
    let settings = ManagementSettings {
        service_source: ManagementSource::Env,
        ..Default::default()
    };
    let text = "version: 1\nnotifications:\n  alertmanager:\n    project_labels:\n      Bad--Slug: {team: x}";
    let err = parse(text, &settings).unwrap_err().to_string();
    assert!(err.contains("\"Bad--Slug\""), "{err}");
}

#[test]
fn rejects_invalid_teams_and_duplicate_grants() {
    for teams in [
        "[{name: '', members: []}]",
        "[{name: Platform, members: []}, {name: Platform, members: []}]",
        "[{name: Platform}]",
        "[{name: Platform, members: [{email: invalid, role: admin}]}]",
        "[{name: Platform, members: [{email: 'a b@example.com', role: admin}]}]",
        "[{name: Platform, members: [{email: A@example.com, role: admin}, {email: a@example.com, role: contributor}]}]",
        "[{name: Platform, sso_groups: [{group: ' ', role: admin}]}]",
        "[{name: Platform, sso_groups: [{group: Acme, role: admin}, {group: Acme, role: contributor}]}]",
    ] {
        let text = format!("version: 1\nprojects: []\nteams: {teams}");
        assert!(parse(&text, &yaml_settings()).is_err(), "{teams}");
    }
    assert!(parse("version: 1\nprojects: []", &yaml_settings()).is_err());
}

#[test]
fn treats_sso_groups_differing_only_in_case_as_distinct() {
    let text = "version: 1\nprojects: []\nteams: [{name: Platform, sso_groups: [{group: Acme, role: admin}, {group: acme, role: contributor}]}]";
    assert!(parse(text, &yaml_settings()).is_ok());
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

#[test]
fn example_configuration_file_is_valid() {
    let text = include_str!("../config.example.yaml");
    let settings = ManagementSettings {
        service_source: ManagementSource::Env,
        ..yaml_settings()
    };
    let file = parse(text, &settings).expect("config.example.yaml must parse");
    validate_references(&file, &[]).expect("example projects must reference example teams");
    assert_eq!(file.projects.as_ref().map(Vec::len), Some(2));
    assert_eq!(file.teams.as_ref().map(Vec::len), Some(2));
    assert_eq!(file.alertmanager_project_labels.len(), 2);
    parse(text, &ManagementSettings::default()).expect("UI-managed sections are ignored");
}
