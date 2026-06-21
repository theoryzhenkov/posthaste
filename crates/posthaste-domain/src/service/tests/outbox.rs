use super::*;

fn draft_request(subject: &str) -> SendMessageRequest {
    SendMessageRequest {
        from: None,
        to: Vec::new(),
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: subject.to_string(),
        body: "draft body".to_string(),
        in_reply_to: None,
        references: None,
        attachments: Vec::new(),
    }
}

fn draft_entity(id: &str) -> OperationEntity {
    OperationEntity {
        kind: OperationEntityKind::Draft,
        id: id.to_string(),
    }
}

#[tokio::test]
async fn flush_create_then_update_reconciles_temp_id_and_settles() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    // Offline: a draft created then edited, both against the same temp id.
    let create = service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftCreate,
            serde_json::to_value(draft_request("Hello")).unwrap(),
            None,
        )
        .expect("queue create");
    service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftUpdate,
            serde_json::to_value(draft_request("Hello, edited")).unwrap(),
            None,
        )
        .expect("queue update");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush should succeed");

    // Both ops settled and were pruned.
    assert_eq!(events.len(), 2);
    assert!(events
        .iter()
        .all(|event| event.topic == "operation.settled"));
    assert!(service
        .list_pending_operations(&account)
        .expect("list pending")
        .is_empty());

    // The update flushed against the provider id assigned to the create, not the
    // temp id.
    let calls = gateway.save_draft_calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], None);
    assert_eq!(calls[1], Some(MessageId::from("provider-draft-1")));
    let _ = create;
}

#[tokio::test]
async fn transient_failure_keeps_op_pending_and_stops_draining() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    gateway
        .save_draft_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::Network("offline".to_string())));

    service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftCreate,
            serde_json::to_value(draft_request("Hello")).unwrap(),
            None,
        )
        .expect("queue create");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush returns ok");

    // No settlement emitted; the op remains pending with the error recorded.
    assert!(events.is_empty());
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, OperationState::Pending);
    assert_eq!(pending[0].attempts, 1);
    assert_eq!(pending[0].last_error.as_deref(), Some("offline"));
}

#[tokio::test]
async fn permanent_failure_marks_op_failed_and_settles() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    gateway
        .save_draft_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::Rejected("invalid draft".to_string())));

    service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftCreate,
            serde_json::to_value(draft_request("Hello")).unwrap(),
            None,
        )
        .expect("queue create");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush returns ok");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, "operation.settled");
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, OperationState::Failed);
    assert_eq!(pending[0].last_error.as_deref(), Some("invalid draft"));
}

#[tokio::test]
async fn delete_draft_flushes_and_settles() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    service
        .queue_operation(
            &account,
            draft_entity("provider-7"),
            OperationKind::DraftDelete,
            serde_json::json!({}),
            None,
        )
        .expect("queue delete");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush ok");

    assert_eq!(events.len(), 1);
    let deletes = gateway.delete_draft_calls.lock().unwrap();
    assert_eq!(deletes.as_slice(), &[MessageId::from("provider-7")]);
    assert!(service
        .list_pending_operations(&account)
        .unwrap()
        .is_empty());
}
