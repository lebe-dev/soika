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

#[test]
fn default_alert_description_includes_context_and_link_without_culprit() {
    let mut vars = sample_variables();
    vars.insert("culprit".into(), String::new());
    vars.insert("title".into(), "queue: claim pending".into());
    let alert = soika::alertmanager::render_alert(
        &AlertmanagerConfig::default(),
        &vars,
        chrono::Utc::now(),
    )
    .unwrap();
    assert_eq!(
        alert.annotations["summary"],
        "Example project: queue: claim pending"
    );
    let description = &alert.annotations["description"];
    for expected in [
        "queue: claim pending",
        "production",
        "1.0.0",
        "42",
        vars["issue_url"].as_str(),
    ] {
        assert!(
            description.contains(expected),
            "missing {expected}: {description}"
        );
    }
    assert_eq!(alert.annotations["service_name"], "Example project");
    assert_eq!(alert.annotations["issue_title"], "queue: claim pending");
}

#[test]
fn dates_use_local_timezone_and_allow_custom_formats() {
    use chrono::TimeZone;
    let mut vars = sample_variables();
    vars.insert(
        "last_seen".into(),
        chrono::Local
            .with_ymd_and_hms(2026, 11, 1, 13, 27, 0)
            .unwrap()
            .to_rfc3339(),
    );
    assert_eq!(
        render_template("{{ last_seen }}", &vars).unwrap(),
        "1 ноября 2026, 13:27"
    );
    vars.insert("last_seen".into(), "2026-11-01T13:27:00+03:00".into());
    let local = chrono::DateTime::parse_from_rfc3339(&vars["last_seen"])
        .unwrap()
        .with_timezone(&chrono::Local);
    assert_eq!(
        render_template("{{ last_seen }}", &vars).unwrap(),
        local
            .format_localized("%-d %B %Y, %H:%M", chrono::Locale::ru_RU)
            .to_string()
    );
    assert_eq!(
        render_template("{{ last_seen | date(\"%Y-%m-%d %H:%M %:z\") }}", &vars).unwrap(),
        local.format("%Y-%m-%d %H:%M %:z").to_string()
    );
    assert_eq!(
        render_template("{{ first_seen | date('%+') }}", &vars).unwrap(),
        chrono::DateTime::parse_from_rfc3339(&vars["first_seen"])
            .unwrap()
            .with_timezone(&chrono::Local)
            .to_rfc3339()
    );
}

#[test]
fn date_filters_reject_invalid_formats_and_non_date_variables() {
    for template in [
        "{{ title | date('%Y') }}",
        "{{ last_seen | date('%Q') }}",
        "{{ last_seen | date('%') }}",
        "{{ last_seen | date(%Y) }}",
        "{{ last_seen | date('%Y') | safe }}",
    ] {
        assert!(
            render_template(template, &sample_variables()).is_err(),
            "{template}"
        );
    }
    let mut vars = sample_variables();
    vars.insert("last_seen".into(), "invalid".into());
    assert!(render_template("{{ last_seen }}", &vars).is_err());
}
