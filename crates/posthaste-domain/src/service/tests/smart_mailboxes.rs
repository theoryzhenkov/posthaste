use super::*;

#[test]
fn list_smart_mailboxes_propagates_store_count_errors() {
    // Counts now fold lazily over the message read, so a failing message read
    // must still surface (not be swallowed) from the count computation.
    let store = Arc::new(TestStore {
        messages_error: Some("messages failed".to_string()),
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
