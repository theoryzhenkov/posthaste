use super::*;

#[test]
fn loads_docs_style_nested_suite_registry() {
    // spec: docs/L1-lab#registry-thin-orchestrator
    let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();

    let settings = registry.suites().get("suite.api.settings.dev").unwrap();
    assert_eq!(settings.level, "integration");
    assert_eq!(settings.targets, vec!["daemon"]);
    assert_eq!(settings.runners, vec!["runner.cargo.test.dev"]);
    assert!(registry.suites().contains_key("suite.dev.smoke.local"));
}

#[test]
fn real_suite_registry_paths_exist_and_smoke_stays_non_graphical() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry = SuiteRegistry::load(repo_root.join("tools/lab/suites.toml")).unwrap();
    let smoke_suite_ids = registry
        .suites()
        .iter()
        .filter_map(|(id, entry)| {
            entry
                .tags
                .iter()
                .any(|tag| tag == "lab-smoke")
                .then_some(id)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        smoke_suite_ids,
        vec!["suite.lab.core.rust.test", "suite.policy.no_telemetry.main",]
    );

    for (id, entry) in registry.suites() {
        assert!(!entry.command.trim().is_empty(), "{id} command is empty");
        assert!(
            entry
                .timeout_seconds
                .unwrap_or(DEFAULT_SUITE_TIMEOUT_SECONDS)
                > 0,
            "{id} timeout must be positive"
        );
        for path in &entry.paths {
            assert!(
                Path::new(path).is_relative() && !path.contains(".."),
                "{id} path {path:?} must stay relative to the repository root"
            );
            assert!(
                repo_root.join(path).exists(),
                "{id} path {path:?} does not exist"
            );
        }
        if entry.tags.iter().any(|tag| tag == "lab-smoke") {
            assert!(
                !entry.targets.iter().any(|target| target == "desktop")
                    && !entry.tags.iter().any(|tag| tag == "tauri"),
                "{id} lab-smoke suite must remain non-graphical"
            );
        }
    }
}

#[test]
fn selects_suites_by_explicit_id_tag_and_target() {
    let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();

    let explicit = registry
        .select(&SelectionCriteria {
            suite_id: Some("suite.api.settings.dev".to_string()),
            tags: vec!["settings".to_string()],
            targets: vec!["daemon".to_string()],
            changed: false,
            changed_paths: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        explicit
            .iter()
            .map(|suite| suite.id.as_str())
            .collect::<Vec<_>>(),
        vec!["suite.api.settings.dev"]
    );

    let filtered = registry
        .select(&SelectionCriteria {
            tags: vec!["fast".to_string()],
            targets: vec!["dev".to_string()],
            ..SelectionCriteria::default()
        })
        .unwrap();
    assert_eq!(
        filtered
            .iter()
            .map(|suite| suite.id.as_str())
            .collect::<Vec<_>>(),
        vec!["suite.dev.smoke.local"]
    );
}

#[test]
fn rejects_invalid_lab_ids() {
    assert!(validate_lab_id("runner:web.main.local").is_ok());
    assert!(validate_lab_id("profile.lab.upgrade.dev.from:v0.1.0-dogfood.17").is_ok());
    assert!(validate_lab_id("suite.api settings.dev").is_err());
    assert!(validate_lab_id("unknown.foo").is_err());
    assert!(validate_lab_id("suite").is_err());
}

#[test]
fn selects_suites_by_changed_paths() {
    let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();

    let selected = registry
        .select(&SelectionCriteria {
            changed: true,
            changed_paths: vec!["apps/client/backend/tests/settings_patch.rs".to_string()],
            ..SelectionCriteria::default()
        })
        .unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|suite| suite.id.as_str())
            .collect::<Vec<_>>(),
        vec!["suite.api.settings.dev"]
    );

    let selected = registry
        .select(&SelectionCriteria {
            changed: true,
            changed_paths: vec!["tools/dev".to_string()],
            ..SelectionCriteria::default()
        })
        .unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|suite| suite.id.as_str())
            .collect::<Vec<_>>(),
        vec!["suite.dev.smoke.local"]
    );

    let selected = registry
        .select(&SelectionCriteria {
            changed: true,
            tags: vec!["settings".to_string()],
            changed_paths: vec!["tools/lab/suites.toml".to_string()],
            ..SelectionCriteria::default()
        })
        .unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|suite| suite.id.as_str())
            .collect::<Vec<_>>(),
        vec!["suite.api.settings.dev"]
    );
}

#[test]
fn changed_selection_requires_changed_paths() {
    let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();

    let err = registry
        .select(&SelectionCriteria {
            changed: true,
            ..SelectionCriteria::default()
        })
        .unwrap_err();
    assert!(matches!(err, LabError::ChangedSelectionNeedsPaths));
}

#[test]
fn suite_list_json_shape_includes_selection_and_suites() {
    let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();
    let criteria = SelectionCriteria {
        tags: vec!["settings".to_string()],
        ..SelectionCriteria::default()
    };
    let suites = registry.select(&criteria).unwrap();
    let value = serde_json::to_value(SuiteListOutput {
        schema_version: 1,
        selection: SelectionRecord::from_criteria(&criteria),
        suites,
    })
    .unwrap();

    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["selection"]["tags"], serde_json::json!(["settings"]));
    assert_eq!(value["suites"][0]["id"], "suite.api.settings.dev");
}

#[test]
fn parses_changed_suite_list_options() {
    let options = parse_list_options(&[
        "--changed".to_string(),
        "--target".to_string(),
        "web".to_string(),
        "--tag=ui".to_string(),
        "--json".to_string(),
    ])
    .unwrap();

    assert!(options.changed);
    assert!(options.json);
    assert_eq!(options.targets, vec!["web"]);
    assert_eq!(options.tags, vec!["ui"]);
}
