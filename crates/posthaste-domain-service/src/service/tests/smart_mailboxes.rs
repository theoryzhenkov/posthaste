use super::*;

#[test]
fn list_smart_mailboxes_propagates_store_count_errors() {
    // Counts come from the store's indexed count query (S4), so a failing count
    // read must still surface (not be swallowed) from list_smart_mailboxes.
    let store = Arc::new(TestStore {
        smart_mailbox_counts_error: Some("counts failed".to_string()),
        ..Default::default()
    });
    let config = Arc::new(TestConfig {
        smart_mailboxes: vec![sample_smart_mailbox()],
        sources: vec![sample_source()],
        ..Default::default()
    });
    let service = MailService::new(store, config);

    let error = service
        .list_smart_mailboxes()
        .expect_err("count failures should not be swallowed");

    assert_eq!(error.code(), "storage_failure");
}
