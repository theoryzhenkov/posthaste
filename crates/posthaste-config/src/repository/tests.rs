use super::*;
use std::{fs, path::PathBuf};

use posthaste_domain::{AccountId, AccountSettings, AppSettings, ConfigRepository};

fn temp_root() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "posthaste-config-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        n
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn mock_source(id: &str, name: &str) -> AccountSettings {
    AccountSettings {
        id: AccountId::from(id),
        name: name.to_string(),
        full_name: None,
        email_patterns: Vec::new(),
        driver: posthaste_domain::AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: Default::default(),
        created_at: "2026-03-31T00:00:00Z".to_string(),
        updated_at: "2026-03-31T00:00:00Z".to_string(),
    }
}

#[test]
fn empty_config_root_creates_empty_snapshot() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();
    let snapshot = repo.load_snapshot().unwrap();

    assert!(snapshot.sources.is_empty());
    assert!(snapshot.smart_mailboxes.is_empty());
    assert_eq!(snapshot.app_settings, AppSettings::default());
}

#[test]
fn initialize_defaults_creates_smart_mailbox_files() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();
    repo.initialize_defaults().unwrap();

    assert!(root.join("app.toml").exists());
    assert!(root.join("smart-mailboxes/default-inbox.toml").exists());
    assert!(root.join("smart-mailboxes/default-all-mail.toml").exists());

    let snapshot = repo.load_snapshot().unwrap();
    assert_eq!(snapshot.smart_mailboxes.len(), 7);
}

#[test]
fn source_crud_round_trips() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();

    let source = AccountSettings {
        id: AccountId::from("test"),
        name: "Test".to_string(),
        full_name: None,
        email_patterns: Vec::new(),
        driver: posthaste_domain::AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: Default::default(),
        created_at: "2026-03-31T00:00:00Z".to_string(),
        updated_at: "2026-03-31T00:00:00Z".to_string(),
    };

    repo.save_source(&source).unwrap();
    assert!(root.join("sources/test.toml").exists());

    let loaded = repo.get_source(&AccountId::from("test")).unwrap().unwrap();
    assert_eq!(loaded.name, "Test");

    repo.delete_source(&AccountId::from("test")).unwrap();
    assert!(!root.join("sources/test.toml").exists());
    assert!(repo.get_source(&AccountId::from("test")).unwrap().is_none());
}

#[test]
fn insert_source_rejects_duplicate_without_overwriting() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();

    let source = AccountSettings {
        id: AccountId::from("test"),
        name: "Test".to_string(),
        full_name: None,
        email_patterns: Vec::new(),
        driver: posthaste_domain::AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: Default::default(),
        created_at: "2026-03-31T00:00:00Z".to_string(),
        updated_at: "2026-03-31T00:00:00Z".to_string(),
    };
    repo.insert_source(&source).unwrap();

    let mut duplicate = source.clone();
    duplicate.name = "Updated".to_string();
    let error = repo
        .insert_source(&duplicate)
        .expect_err("duplicate source insert should fail");

    assert!(matches!(error, ConfigError::Conflict(_)));
    let loaded = repo.get_source(&AccountId::from("test")).unwrap().unwrap();
    assert_eq!(loaded.name, "Test");
}

// spec: docs/backend/L1#config-snapshot-validation
#[test]
fn reload_rejects_semantic_validation_errors_and_preserves_snapshot() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();
    repo.save_source(&mock_source("primary", "Primary"))
        .unwrap();

    fs::write(
        root.join("app.toml"),
        "schema_version = 1\ndefault_source_id = \"missing\"\n",
    )
    .unwrap();
    fs::write(
        root.join("sources/broken.toml"),
        r#"
id = "broken"
name = " "
driver = "mock"
enabled = true
"#,
    )
    .unwrap();

    let error = repo
        .reload()
        .expect_err("semantic validation errors should reject reload")
        .to_string();

    assert!(
        error.contains("default account 'missing' does not exist"),
        "error should mention dangling default account: {error}"
    );
    assert!(
        error.contains("account name is required"),
        "error should mention invalid source: {error}"
    );
    let snapshot = repo.load_snapshot().unwrap();
    assert_eq!(snapshot.sources, vec![mock_source("primary", "Primary")]);
    assert_eq!(snapshot.app_settings.default_account_id, None);
}

#[test]
fn unsafe_file_id_is_rejected() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();

    let bad_content = r#"
id = "bad..id"
name = "Test"
driver = "mock"
enabled = true
"#;
    fs::write(root.join("sources/bad..id.toml"), bad_content).unwrap();

    let error = repo
        .reload()
        .expect_err("unsafe ids should reject config reload")
        .to_string();
    assert!(
        error.contains("unsafe characters"),
        "error should mention unsafe id: {error}"
    );
}

#[test]
fn filename_id_mismatch_is_rejected() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();

    // Write a source file with mismatched filename/id
    let bad_content = r#"
id = "real-id"
name = "Test"
driver = "mock"
enabled = true
"#;
    fs::write(root.join("sources/wrong-name.toml"), bad_content).unwrap();

    let result = repo.reload();
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("wrong-name"),
        "error should mention filename: {err}"
    );
    assert!(err.contains("real-id"), "error should mention id: {err}");
}

#[test]
fn reload_detects_added_source() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();

    // Externally write a source file
    let content = r#"
id = "new-source"
name = "New Source"
driver = "mock"
enabled = true
"#;
    fs::write(root.join("sources/new-source.toml"), content).unwrap();

    let diff = repo.reload().unwrap();
    assert_eq!(diff.added_sources, vec![AccountId::from("new-source")]);
    assert!(diff.removed_sources.is_empty());
    assert!(diff.changed_sources.is_empty());
}

#[test]
fn malformed_app_toml_is_rejected() {
    let root = temp_root();
    fs::write(root.join("app.toml"), "not = [valid").unwrap();

    let err = match TomlConfigRepository::open(&root) {
        Ok(_) => panic!("repository open should fail for malformed app.toml"),
        Err(err) => err.to_string(),
    };

    assert!(err.contains("app.toml"), "error should mention file: {err}");
}

#[test]
fn smart_mailbox_crud_round_trips() {
    let root = temp_root();
    let repo = TomlConfigRepository::open(&root).unwrap();

    let mailbox = default_smart_mailboxes().into_iter().next().unwrap();
    repo.save_smart_mailbox(&mailbox).unwrap();

    let loaded = repo.get_smart_mailbox(&mailbox.id).unwrap().unwrap();
    assert_eq!(loaded.name, mailbox.name);
    assert_eq!(loaded.rule, mailbox.rule);

    repo.delete_smart_mailbox(&mailbox.id).unwrap();
    assert!(repo.get_smart_mailbox(&mailbox.id).unwrap().is_none());
}
