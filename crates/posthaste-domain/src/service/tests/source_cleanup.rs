use super::*;

#[test]
fn delete_source_clears_default_account_before_removing_it() {
    let account = sample_source();
    let config = Arc::new(TestConfig {
        sources: vec![account.clone()],
        app_settings: Mutex::new(AppSettings {
            default_account_id: Some(account.id.clone()),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), config.clone());

    service
        .delete_source(&account.id)
        .expect("deleting the account should succeed");

    assert_eq!(
        config
            .get_app_settings()
            .expect("settings lookup should succeed")
            .default_account_id,
        None
    );
    assert_eq!(
        config
            .deleted_sources
            .lock()
            .expect("deleted sources lock poisoned")
            .as_slice(),
        std::slice::from_ref(&account.id)
    );
    assert_eq!(
        store
            .projection_deletes
            .lock()
            .expect("projection deletes lock poisoned")
            .as_slice(),
        &[account.id.to_string()]
    );
    assert_eq!(
        store
            .source_data_deletes
            .lock()
            .expect("source data deletes lock poisoned")
            .as_slice(),
        &[account.id.to_string()]
    );
}

#[test]
fn reload_config_cleans_up_removed_sources_before_resyncing_projections() {
    let removed = AccountId::from("removed");
    let remaining = sample_source();
    let config = Arc::new(TestConfig {
        sources: vec![remaining.clone()],
        reload_diff: ConfigDiff {
            added_sources: Vec::new(),
            changed_sources: Vec::new(),
            removed_sources: vec![removed.clone()],
        },
        ..Default::default()
    });
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), config);

    let diff = service
        .reload_config()
        .expect("reloading config should succeed");

    assert_eq!(diff.removed_sources, vec![removed.clone()]);
    assert_eq!(
        store
            .projection_deletes
            .lock()
            .expect("projection deletes lock poisoned")
            .as_slice(),
        &[removed.to_string()]
    );
    assert_eq!(
        store
            .source_data_deletes
            .lock()
            .expect("source data deletes lock poisoned")
            .as_slice(),
        &[removed.to_string()]
    );
    assert_eq!(
        store
            .projection_calls
            .lock()
            .expect("projection lock poisoned")
            .as_slice(),
        &[remaining.id.to_string()]
    );
}
