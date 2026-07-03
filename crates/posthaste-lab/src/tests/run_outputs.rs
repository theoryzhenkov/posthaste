use super::*;

#[test]
fn writes_manifest_and_summary_under_disposable_run_root() {
    // spec: docs/L1-lab#disposable-run-roots
    // spec: docs/L1-lab#artifact-manifest
    let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();
    let temp_root = crate::test_support::temp_root();
    let options = VerifyOptions {
        run_root: temp_root.path().to_path_buf(),
        registry_path: PathBuf::from("tools/lab/suites.toml"),
        argv: vec![
            "posthaste-lab".to_string(),
            "verify".to_string(),
            "suite.api.settings.dev".to_string(),
        ],
        criteria: SelectionCriteria {
            suite_id: Some("suite.api.settings.dev".to_string()),
            ..SelectionCriteria::default()
        },
    };

    let output = write_verify_run_with_env(
        &registry,
        options,
        [
            ("POSTHASTE_CONFIG_ROOT", "/tmp/posthaste/config"),
            ("POSTHASTE_TEST_SECRET_TOKEN", "super-secret-token"),
            ("PATH", "/private/local/bin:/other/local/bin"),
            ("CARGO_MANIFEST_PATH", "/private/local/Cargo.toml"),
            ("SSH_AUTH_SOCK", "/private/local/agent.sock"),
            ("UNRELATED_SECRET", "should-not-be-recorded"),
        ],
    )
    .unwrap();

    assert!(output.run_dir.join("state.config").is_dir());
    assert!(output.run_dir.join("state.data").is_dir());
    assert!(output.run_dir.join("state.secrets").is_dir());

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&output.manifest_path).expect("manifest should be readable"),
    )
    .unwrap();
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(
        manifest["selectedSuites"][0]["id"],
        "suite.api.settings.dev"
    );
    assert_eq!(
        manifest["selection"]["rationale"],
        "explicit suite suite.api.settings.dev"
    );
    assert_eq!(manifest["profiles"][0], "profile.lab.empty.dev");
    assert_eq!(manifest["fixtures"][0], "fixture.mail.basic.test");
    assert_eq!(
        manifest["env"]["POSTHASTE_CONFIG_ROOT"],
        "/tmp/posthaste/config"
    );
    assert_eq!(manifest["env"]["POSTHASTE_TEST_SECRET_TOKEN"], REDACTED);
    assert!(manifest["env"].get("PATH").is_none());
    assert!(manifest["env"].get("CARGO_MANIFEST_PATH").is_none());
    assert!(manifest["env"].get("SSH_AUTH_SOCK").is_none());
    assert!(manifest["env"].get("UNRELATED_SECRET").is_none());

    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&output.summary_path).expect("summary should be readable"),
    )
    .unwrap();
    assert_eq!(summary["status"], "passed");
    assert_eq!(summary["reason"], "all selected suites passed");
    assert_eq!(summary["selectedSuiteCount"], 1);
    assert_eq!(
        summary["selection"]["rationale"],
        "explicit suite suite.api.settings.dev"
    );
    assert_eq!(summary["suiteResults"][0]["exitCode"], 0);
    assert_eq!(summary["suiteResults"][0]["timedOut"], false);
    let stdout_path = summary["suiteResults"][0]["stdoutPath"].as_str().unwrap();
    let stderr_path = summary["suiteResults"][0]["stderrPath"].as_str().unwrap();
    assert_eq!(
        fs::read_to_string(stdout_path).unwrap(),
        "settings stdout\n"
    );
    assert_eq!(
        fs::read_to_string(stderr_path).unwrap(),
        "settings stderr\n"
    );
}

#[test]
fn propagates_marked_nested_artifacts_and_excerpts() {
    let registry = SuiteRegistry::from_toml_str(
        r#"
[suite.nested.artifacts.demo]
level = "smoke"
targets = ["desktop"]
runners = ["runner.shell.test"]
tags = ["nested"]
paths = []
command = '''
mkdir -p "$POSTHASTE_LAB_RUN_DIR/nested/artifacts"
printf '{"status":"failed","reason":"nested smoke failed","exitCode":1,"runDir":"%s"}\n' "$POSTHASTE_LAB_RUN_DIR/nested" > "$POSTHASTE_LAB_RUN_DIR/nested/summary.json"
printf 'nested playwright failure detail\n' > "$POSTHASTE_LAB_RUN_DIR/nested/artifacts/playwright.log"
printf 'TOKEN=should-not-be-in-summary\n' > "$POSTHASTE_SECRETS_ROOT/leak.log"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_LAB_RUN_DIR/nested/../nested/summary.json"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_LAB_RUN_DIR/nested/artifacts/playwright.log"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_LAB_RUN_DIR/nested/missing.json"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_SECRETS_ROOT/leak.log"
exit 1
'''
timeout_seconds = 5
"#,
    )
    .unwrap();
    let temp_root = crate::test_support::temp_root();

    let output = write_verify_run_with_env(
        &registry,
        VerifyOptions {
            run_root: temp_root.path().to_path_buf(),
            registry_path: PathBuf::from("tools/lab/suites.toml"),
            argv: vec!["posthaste-lab".to_string(), "verify".to_string()],
            criteria: SelectionCriteria::default(),
        },
        std::iter::empty::<(String, String)>(),
    )
    .unwrap();

    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&output.summary_path).expect("summary should be readable"),
    )
    .unwrap();
    let summary_path = output
        .run_dir
        .join("nested")
        .join("summary.json")
        .display()
        .to_string();
    let log_path = output
        .run_dir
        .join("nested")
        .join("artifacts")
        .join("playwright.log")
        .display()
        .to_string();
    let missing_path = output
        .run_dir
        .join("nested")
        .join("missing.json")
        .display()
        .to_string();
    let secret_path = output
        .run_dir
        .join("state.secrets")
        .join("leak.log")
        .display()
        .to_string();

    assert_eq!(summary["status"], "failed");
    let artifact_paths = summary["suiteResults"][0]["artifactPaths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(artifact_paths.contains(&summary_path.as_str()));
    assert!(artifact_paths.contains(&log_path.as_str()));
    assert!(!artifact_paths.contains(&missing_path.as_str()));
    assert!(!artifact_paths.contains(&secret_path.as_str()));
    let artifacts = summary["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(artifacts.contains(&summary_path.as_str()));
    assert!(artifacts.contains(&log_path.as_str()));
    assert!(summary["importantLogExcerpts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str().unwrap().contains("nested smoke failed")));
    assert!(summary["importantLogExcerpts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value
            .as_str()
            .unwrap()
            .contains("nested playwright failure detail")));
    assert!(!summary["importantLogExcerpts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value.as_str().unwrap().contains("should-not-be-in-summary")));

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&output.manifest_path).expect("manifest should be readable"),
    )
    .unwrap();
    let manifest_artifacts = manifest["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(manifest_artifacts.contains(&summary_path.as_str()));
    assert!(manifest_artifacts.contains(&log_path.as_str()));
}
