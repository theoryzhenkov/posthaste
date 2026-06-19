use super::*;

#[test]
fn routes_command_specific_help() {
    let args = |parts: &[&str]| {
        parts
            .iter()
            .map(|part| (*part).to_string())
            .collect::<Vec<_>>()
    };

    assert_eq!(usage_kind_for_args(&args(&[])), Some(UsageKind::TopLevel));
    assert_eq!(
        usage_kind_for_args(&args(&["--help"])),
        Some(UsageKind::TopLevel)
    );
    assert_eq!(
        usage_kind_for_args(&args(&["suite", "list", "--help"])),
        Some(UsageKind::SuiteList)
    );
    assert_eq!(
        usage_kind_for_args(&args(&["verify", "--help"])),
        Some(UsageKind::Verify)
    );
    assert_eq!(
        usage_kind_for_args(&args(&["config", "validate", "--help"])),
        Some(UsageKind::ConfigValidate)
    );
    assert_eq!(usage_kind_for_args(&args(&["suite", "list"])), None);
}

#[test]
fn parses_config_validate_config_dir() {
    let options =
        parse_config_validate_options(&["--config-dir=var/dev/posthaste/config".to_string()])
            .expect("config validate options should parse");

    assert_eq!(
        options.config_dir,
        PathBuf::from("var/dev/posthaste/config")
    );
}

fn temp_root() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "posthaste-lab-config-validate-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn config_validate_command_accepts_valid_config_dir() {
    let root = temp_root();

    run_config_validate_command(
        "posthaste-lab",
        &[format!("--config-dir={}", root.display())],
    )
    .expect("empty config dir should validate as an empty snapshot");

    assert!(
        !root.join("sources").exists() && !root.join("smart-mailboxes").exists(),
        "validation should not create repository subdirectories"
    );
}

#[test]
fn config_validate_command_reports_semantic_errors() {
    let root = temp_root();
    std::fs::write(
        root.join("app.toml"),
        "schema_version = 1\ndefault_source_id = \"missing\"\n",
    )
    .unwrap();

    let error = run_config_validate_command(
        "posthaste-lab",
        &[format!("--config-dir={}", root.display())],
    )
    .expect_err("dangling default account should fail validation");

    assert!(matches!(error, LabError::ConfigValidation { .. }));
    assert!(
        error
            .to_string()
            .contains("default account 'missing' does not exist"),
        "error should mention dangling default account: {error}"
    );
}
