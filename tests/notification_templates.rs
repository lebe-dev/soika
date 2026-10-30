use soika::domain::notifications::{AlertmanagerConfig, render_template, sample_variables};

#[test]
fn placeholders_preserve_untrusted_values_without_recursive_expansion() {
    let mut vars = sample_variables();
    vars.insert(
        "title".into(),
        "bad \"json\"\n{{ issue_id }} <script>".into(),
    );
    assert_eq!(
        render_template("{{ project_slug }}: {{ title }}", &vars).unwrap(),
        "example: bad \"json\"\n{{ issue_id }} <script>"
    );
}

#[test]
fn invalid_templates_and_unknown_variables_are_rejected() {
    for template in [
        "{{ secret }}",
        "{{title",
        "title }}",
        "{% for x %}",
        "{{ title | safe }}",
    ] {
        assert!(
            render_template(template, &sample_variables()).is_err(),
            "{template}"
        );
    }
}

#[test]
fn settings_validate_url_keys_and_templates_even_when_disabled() {
    let mut config = AlertmanagerConfig::default();
    assert!(config.validate().is_ok());
    config.enabled = true;
    assert!(config.validate().is_err());
    config.url = "http://alertmanager:9093/prefix".into();
    assert!(config.validate().is_ok());
    config.labels.insert("bad-key".into(), "value".into());
    assert!(config.validate().is_err());
    config.labels.remove("bad-key");
    config
        .annotations
        .insert("summary".into(), "{{ unknown }}".into());
    assert!(config.validate().is_err());
    config.annotations.clear();
    for url in [
        "file:///tmp/socket",
        "http://user:pass@host",
        "http://host/#secret",
    ] {
        config.url = url.into();
        assert!(config.validate().is_err());
    }
}

#[test]
fn defaults_use_stable_labels_and_put_changing_values_in_annotations() {
    let config = AlertmanagerConfig::default();
    assert_eq!(config.labels["alertname"], "SoikaIssue");
    assert!(config.annotations["summary"].contains("{{ title }}"));
    for value in config.labels.values() {
        assert!(!value.contains("trigger"));
        assert!(!value.contains("event_count"));
    }
}

#[test]
fn environment_url_is_a_default_and_explicit_settings_take_precedence() {
    use soika::domain::notifications::NotificationConfig;
    let mut config = NotificationConfig::default();
    config.alertmanager.enabled = true;
    assert!(config.validate_stored().is_ok());
    assert!(config.effective(None).is_err());
    assert_eq!(
        config
            .effective(Some("http://default:9093"))
            .unwrap()
            .alertmanager
            .url,
        "http://default:9093"
    );
    assert_eq!(
        config.alertmanager.url, "",
        "fallback must not be persisted as an override"
    );
    config.alertmanager.url = "http://override:9093".into();
    assert_eq!(
        config
            .effective(Some("http://default:9093"))
            .unwrap()
            .alertmanager
            .url,
        "http://override:9093"
    );
}
