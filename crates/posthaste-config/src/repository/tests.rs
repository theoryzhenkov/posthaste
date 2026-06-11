use super::*;
use std::path::PathBuf;

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
