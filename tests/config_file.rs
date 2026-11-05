use soika::domain::config_file::*;
#[test]
fn strict_config_validation() {
    let settings = ManagementSettings {
        project_source: ManagementSource::Yaml,
        ..Default::default()
    };
    assert!(parse("version: 1\nprojects: []", &settings).is_ok());
    for text in [
        "version: 2\nprojects: []",
        "version: 1",
        "version: 1\nprojects: []\nunknown: true",
        "version: 1\nversion: 1\nprojects: []",
    ] {
        assert!(parse(text, &settings).is_err(), "{text}");
    }
}

#[test]
fn generic_yaml_keeps_duplicate_key_rejection_before_environment_expansion() {
    assert!(
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>("version: 1\nversion: 1\nprojects: []")
            .is_err()
    );
}

#[test]
fn ui_sections_are_ignored_even_when_their_content_is_not_a_valid_entity() {
    let settings = ManagementSettings {
        project_source: ManagementSource::Yaml,
        ..Default::default()
    };
    assert!(parse("version: 1\nprojects: []\nteams: ignored", &settings).is_ok());
}

#[test]
fn notification_source_is_independent_and_requires_valid_yaml_section() {
    use soika::domain::config_file::*;
    let mut settings = ManagementSettings::default();
    let invalid = "version: 1\nnotifications: {alertmanager: {labels: {bad: '{{ secret }}'}}}\n";
    assert!(parse(invalid, &settings).unwrap().notifications.is_none());
    settings.notification_source = ManagementSource::Yaml;
    assert!(parse("version: 1", &settings).is_err());
    assert!(parse(invalid, &settings).is_err());
    let file = parse("version: 1\nnotifications:\n  alertmanager:\n    enabled: true\n    url: http://alertmanager:9093\n    labels: {alertname: SoikaIssue, project: '{{ project_slug }}'}", &settings).unwrap();
    assert!(file.notifications.unwrap().alertmanager.enabled);
    assert!(file.projects.is_none());
}

#[test]
fn env_project_labels_are_validated_even_for_ui_projects() {
    let settings = ManagementSettings {
        service_source: ManagementSource::Env,
        ..Default::default()
    };
    let file = parse("version: 1\nnotifications:\n  alertmanager:\n    project_labels:\n      api: {team: platform, severity: '{{ level }}'}\n", &settings).unwrap();
    assert_eq!(file.alertmanager_project_labels["api"]["team"], "platform");
    for labels in [
        "{issue_id: x}",
        "{bad-name: x}",
        "{team: '{{ secret }}'}",
        "{alertname: ''}",
    ] {
        let yaml = format!(
            "version: 1\nnotifications:\n  alertmanager:\n    project_labels:\n      api: {labels}\n"
        );
        assert!(parse(&yaml, &settings).is_err(), "{yaml}");
    }
}

#[test]
fn alertmanager_intervals_follow_project_yaml_ownership() {
    let mut settings = ManagementSettings::default();
    let invalid = "version: 1\nalertmanager_interval_seconds: 0\nprojects: []\n";
    assert!(
        parse(invalid, &settings).is_ok(),
        "UI-owned values are ignored"
    );
    settings.project_source = ManagementSource::Yaml;
    assert!(parse(invalid, &settings).is_err());
    let text = "version: 1\nalertmanager_interval_seconds: 300\nprojects:\n  - {slug: api, name: API, team: platform, alertmanager_interval_seconds: 120}\n";
    let file = parse(text, &settings).unwrap();
    assert_eq!(file.alertmanager_interval_seconds, Some(300));
    assert_eq!(
        file.projects.unwrap()[0].alertmanager_interval_seconds,
        Some(120)
    );
    assert!(parse(&text.replace("seconds: 120", "seconds: 0"), &settings).is_err());
}

#[test]
fn project_alertmanager_severity_follows_yaml_ownership_and_rejects_unknown_classes() {
    let mut settings = ManagementSettings::default();
    let yaml = "version: 1\nprojects:\n  - {slug: api, name: API, team: platform, alertmanager_severity: critical}\n";
    assert!(parse(yaml, &settings).unwrap().projects.is_none());
    settings.project_source = ManagementSource::Yaml;
    let file = parse(yaml, &settings).unwrap();
    assert_eq!(
        file.projects.unwrap()[0]
            .alertmanager_severity
            .unwrap()
            .as_str(),
        "critical"
    );
    assert!(parse(&yaml.replace("critical", "urgent"), &settings).is_err());
    assert!(
        parse(&yaml.replace("critical", "null"), &settings)
            .unwrap()
            .projects
            .unwrap()[0]
            .alertmanager_severity
            .is_none()
    );
}
