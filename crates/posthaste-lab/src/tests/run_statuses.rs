use super::*;

#[test]
fn maps_blocked_exit_code_to_blocked_summary() {
    let registry = SuiteRegistry::from_toml_str(
        r#"
[suite.blocked.demo]
level = "smoke"
targets = ["daemon"]
runners = ["runner.shell.test"]
tags = ["blocked"]
paths = []
command = "printf 'display unavailable\\n' >&2; exit 78"
timeout_seconds = 5
"#,
    )
    .unwrap();
    let temp_root =
        std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));

    let output = write_verify_run_with_env(
        &registry,
        VerifyOptions {
            run_root: temp_root.clone(),
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
    assert_eq!(output.status, LabStatus::Blocked);
    assert_eq!(summary["status"], "blocked");
    assert_eq!(summary["suiteResults"][0]["exitCode"], 78);
    assert_eq!(summary["firstFailure"], "suite.blocked.demo");
    assert!(summary["importantLogExcerpts"][0]
        .as_str()
        .unwrap()
        .contains("display unavailable"));

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn maps_skipped_exit_code_to_skipped_summary() {
    let registry = SuiteRegistry::from_toml_str(
        r#"
[suite.skipped.demo]
level = "smoke"
targets = ["daemon"]
runners = ["runner.shell.test"]
tags = ["skipped"]
paths = []
command = "printf 'unsupported platform\\n'; exit 77"
timeout_seconds = 5
"#,
    )
    .unwrap();
    let temp_root =
        std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));

    let output = write_verify_run_with_env(
        &registry,
        VerifyOptions {
            run_root: temp_root.clone(),
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
    assert_eq!(output.status, LabStatus::Skipped);
    assert_eq!(summary["status"], "skipped");
    assert_eq!(summary["reason"], "all selected suites were skipped");
    assert_eq!(summary["suiteResults"][0]["exitCode"], 77);
    assert_eq!(
        summary["suiteResults"][0]["reason"],
        "suite command reported skipped status 77"
    );

    let err = LabError::VerificationSkipped {
        summary_path: output.summary_path.display().to_string(),
    };
    assert_eq!(err.exit_code(), 77);

    fs::remove_dir_all(temp_root).ok();
}

#[test]
fn terminates_timed_out_suite_and_records_stdout_stderr() {
    let registry = SuiteRegistry::from_toml_str(
        r#"
[suite.timeout.demo]
level = "smoke"
targets = ["daemon"]
runners = ["runner.shell.test"]
tags = ["timeout"]
paths = []
command = "printf 'before sleep\\n'; printf 'still waiting\\n' >&2; sleep 2"
timeout_seconds = 1
"#,
    )
    .unwrap();
    let temp_root =
        std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));

    let output = write_verify_run_with_env(
        &registry,
        VerifyOptions {
            run_root: temp_root.clone(),
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
    assert_eq!(output.status, LabStatus::Failed);
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["suiteResults"][0]["timedOut"], true);
    assert_eq!(
        summary["suiteResults"][0]["reason"],
        "suite command timed out"
    );
    let stdout_path = summary["suiteResults"][0]["stdoutPath"].as_str().unwrap();
    let stderr_path = summary["suiteResults"][0]["stderrPath"].as_str().unwrap();
    assert_eq!(fs::read_to_string(stdout_path).unwrap(), "before sleep\n");
    assert_eq!(fs::read_to_string(stderr_path).unwrap(), "still waiting\n");

    fs::remove_dir_all(temp_root).ok();
}
