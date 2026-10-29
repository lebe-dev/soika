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
