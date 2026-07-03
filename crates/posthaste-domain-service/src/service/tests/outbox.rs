use super::*;

fn draft_request(subject: &str) -> SendMessageRequest {
    SendMessageRequest {
        subject: subject.to_string(),
        body: "draft body".to_string(),
        ..Default::default()
    }
}

fn draft_entity(id: &str) -> OperationEntity {
    OperationEntity {
        kind: OperationEntityKind::Draft,
        id: id.to_string(),
    }
}

#[test]
fn save_draft_without_id_enqueues_a_create_with_a_temp_id() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    let op = service
        .save_draft(&account, None, draft_request("Hello"))
        .expect("save draft");

    assert_eq!(op.kind, OperationKind::DraftCreate);
    assert_eq!(op.entity.kind, OperationEntityKind::Draft);
    assert!(op.entity.id.starts_with("draft-local-"));
    assert_eq!(op.state, OperationState::Pending);
}

#[test]
fn save_draft_with_id_enqueues_an_update_ordered_after_pending_ops() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    let create = service
        .save_draft(&account, None, draft_request("Hello"))
        .expect("create");
    let draft_id = MessageId::from(create.entity.id.as_str());
    let update = service
        .save_draft(&account, Some(draft_id), draft_request("Edited"))
        .expect("update");

    assert_eq!(update.kind, OperationKind::DraftUpdate);
    assert_eq!(update.entity.id, create.entity.id);
    assert_eq!(update.depends_on.as_ref(), Some(&create.id));
}

#[test]
fn save_draft_resuming_an_existing_provider_draft_updates_in_place() {
    // A draft resumed by its provider id (no alias — a legacy draft saved before
    // stable ids, or one created elsewhere) must edit in place, not duplicate.
    // `mailbox_ids` non-empty makes the draft "exist" in the projection.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("state-1", &["drafts"]));
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    let op = service
        .save_draft(
            &account,
            Some(MessageId::from("provider-draft-42")),
            draft_request("Edited"),
        )
        .expect("save draft");

    assert_eq!(op.kind, OperationKind::DraftUpdate);
    assert_eq!(op.entity.id, "provider-draft-42");
}

#[test]
fn save_draft_for_a_brand_new_key_still_creates() {
    // The same path with no existing message (empty mailbox set) is a genuine
    // new draft, so it creates rather than trying to replace a non-existent id.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    let op = service
        .save_draft(
            &account,
            Some(MessageId::from("draft-local-new")),
            draft_request("Hello"),
        )
        .expect("save draft");

    assert_eq!(op.kind, OperationKind::DraftCreate);
}

#[test]
fn save_draft_stamps_the_stable_id_into_the_payload() {
    // The stable key is injected into the request payload so the gateway writes
    // it as the X-Posthaste-Draft-Id header.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    let op = service
        .save_draft(
            &account,
            Some(MessageId::from("draft-local-xyz")),
            draft_request("Hello"),
        )
        .expect("save draft");

    let request: SendMessageRequest =
        serde_json::from_value(op.payload).expect("payload is a SendMessageRequest");
    assert_eq!(request.draft_id.as_deref(), Some("draft-local-xyz"));
}

#[tokio::test]
async fn stable_draft_key_reuses_provider_id_across_flush() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let key = MessageId::from("draft-local-stable");

    // First save creates; flushing assigns a provider id and updates the alias.
    service
        .save_draft(&account, Some(key.clone()), draft_request("Hello"))
        .expect("create");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("first flush");

    // Editing with the SAME stable key targets the provider draft (an update),
    // not a brand-new create -- this is what prevents duplicate drafts.
    let edit = service
        .save_draft(&account, Some(key), draft_request("Edited"))
        .expect("edit");
    assert_eq!(edit.kind, OperationKind::DraftUpdate);
    assert_eq!(edit.entity.id, "provider-draft-1");

    service
        .flush_account(&account, &gateway)
        .await
        .expect("second flush");

    // Provider saw exactly one create then one replace of the assigned id.
    let calls = gateway.save_draft_calls.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        &[None, Some(MessageId::from("provider-draft-1"))]
    );
}

#[test]
fn delete_draft_enqueues_a_delete() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    let op = service
        .delete_draft(&account, MessageId::from("provider-7"))
        .expect("delete draft");

    assert_eq!(op.kind, OperationKind::DraftDelete);
    assert_eq!(op.entity.id, "provider-7");
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
        )
        .expect("queue create");
    service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftUpdate,
            serde_json::to_value(draft_request("Hello, edited")).unwrap(),
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
async fn permanent_failure_of_a_message_assertion_emits_a_base_correction() {
    // Tier 1: a failed message state-assertion leaves the read overlay, but the
    // served views need a recompute trigger to revert. The failure must emit a
    // message.updated base correction (in addition to operation.settled).
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    gateway
        .set_keywords_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::Rejected("provider rejected".to_string())));

    service
        .queue_operation(
            &account,
            OperationEntity {
                kind: OperationEntityKind::Message,
                id: "m1".to_string(),
            },
            OperationKind::SetKeywords,
            serde_json::json!({ "add": ["$flagged"], "remove": [] }),
        )
        .expect("queue setKeywords");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush returns ok");

    assert!(
        events
            .iter()
            .any(|event| event.topic == "operation.settled"),
        "failure still settles the operation"
    );
    let correction = events
        .iter()
        .find(|event| event.topic == "message.updated")
        .expect("a message.updated base correction is emitted on failure");
    assert_eq!(
        correction.message_id.as_ref().map(MessageId::as_str),
        Some("m1")
    );
    assert_eq!(correction.payload["reverted"], serde_json::json!(true));
    assert_eq!(
        correction.payload["changes"]["keywords"],
        serde_json::json!(true)
    );
}

#[tokio::test]
async fn permanent_failure_of_a_draft_emits_no_base_correction() {
    // Drafts/sends don't fold into message reads, so a failure surfaces only via
    // operation.settled — no spurious message.updated.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
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
        )
        .expect("queue create");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush");

    assert!(events.iter().all(|event| event.topic != "message.updated"));
}

async fn queue_and_fail_one(
    service: &MailService,
    account: &AccountId,
    gateway: &MutationGateway,
) -> OperationId {
    gateway
        .save_draft_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::Rejected("invalid draft".to_string())));
    service
        .queue_operation(
            account,
            draft_entity("draft-temp"),
            OperationKind::DraftCreate,
            serde_json::to_value(draft_request("Hello")).unwrap(),
        )
        .expect("queue create");
    service
        .flush_account(account, gateway)
        .await
        .expect("flush");
    let failed = service.list_pending_operations(account).expect("pending");
    assert_eq!(failed[0].state, OperationState::Failed);
    failed[0].id.clone()
}

#[tokio::test]
async fn discard_removes_a_failed_operation() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let id = queue_and_fail_one(&service, &account, &gateway).await;

    assert!(service.discard_operation(&id).expect("discard"));
    assert!(service
        .list_pending_operations(&account)
        .expect("pending")
        .is_empty());
}

#[tokio::test]
async fn retry_re_arms_a_failed_operation_to_pending() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let id = queue_and_fail_one(&service, &account, &gateway).await;

    assert!(service.retry_operation(&id).expect("retry"));
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending[0].state, OperationState::Pending);
    assert_eq!(pending[0].last_error, None);
}

#[tokio::test]
async fn retry_rejects_a_non_failed_operation() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftCreate,
            serde_json::to_value(draft_request("Hello")).unwrap(),
        )
        .expect("queue create");
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending[0].state, OperationState::Pending);

    assert!(service.retry_operation(&pending[0].id).is_err());
}

#[tokio::test]
async fn failed_draft_predecessor_cancels_dependent_update() {
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
        )
        .expect("queue create");
    service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftUpdate,
            serde_json::to_value(draft_request("Hello again")).unwrap(),
        )
        .expect("queue update");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush returns ok");

    assert_eq!(events.len(), 2);
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending.len(), 2);
    assert!(pending
        .iter()
        .all(|operation| operation.state == OperationState::Failed));
    assert_eq!(
        gateway.save_draft_calls.lock().unwrap().len(),
        1,
        "dependent update must not flush after create failure",
    );
}

#[tokio::test]
async fn enqueue_send_queues_then_flushes_once() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    let send = service
        .enqueue_send(&account, draft_request("Outgoing"))
        .expect("send queues");
    assert_eq!(send.kind, OperationKind::Send);
    assert_eq!(send.state, OperationState::Pending);
    assert!(send.entity.id.starts_with("send-"));

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush ok");

    assert_eq!(events.len(), 1);
    assert_eq!(
        *gateway.send_calls.lock().unwrap(),
        vec!["Outgoing".to_string()]
    );
    assert!(service
        .list_pending_operations(&account)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn interrupted_inflight_send_parks_dispatch_uncertain_not_resent() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    // Simulate a send that began flushing and was interrupted by a crash: the
    // op is durably `inflight` before the process restarts. It may already have
    // left the provider, so it must park as dispatch-uncertain — never resent.
    let send = service
        .queue_operation(
            &account,
            draft_entity("send-1"),
            OperationKind::Send,
            serde_json::to_value(draft_request("Outgoing")).unwrap(),
        )
        .expect("queue send");
    store
        .update_operation_state(&send.id, OperationState::Inflight, 1, None)
        .expect("mark inflight");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush returns ok");

    // The interrupted send is parked, never pushed to the gateway.
    assert!(gateway.send_calls.lock().unwrap().is_empty());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, "operation.dispatch_uncertain");
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, OperationState::DispatchUncertain);
    assert!(pending[0]
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("uncertain")));
}

/// The S1 regression (the M32 gate): a send that times out after the server
/// already committed the submission must produce **exactly one** submission, and
/// the op must sit `DispatchUncertain` — never blind-resent on the next flush.
/// An explicit user retry re-dispatches under the same idempotency identity and
/// still produces exactly one submission (the JMAP/SMTP dedup — D84/D85 —
/// modeled at the gateway boundary).
///
/// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
#[tokio::test]
async fn s1_dispatch_uncertain_send_never_duplicates_across_reflush_and_retry() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    // The first submission commits server-side, but the response is lost and the
    // send times out: the gateway returns `DispatchUncertain`.
    gateway
        .send_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::DispatchUncertain(
            "send timed out; delivery uncertain".to_string(),
        )));

    let send = service
        .enqueue_send(&account, draft_request("Outgoing"))
        .expect("send queues");

    // Flush 1: times out after commit -> parked; exactly one submission.
    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush 1");
    assert_eq!(
        gateway.committed_send_keys.lock().unwrap().len(),
        1,
        "exactly one submission after the timeout"
    );
    assert!(events.iter().any(|e| e.topic == "operation.dispatch_uncertain"));
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, send.id);
    assert_eq!(pending[0].state, OperationState::DispatchUncertain);

    // Flush 2 (an ordinary auto-flush): the parked send is removed from the
    // flush set, so it is NOT resent — still exactly one submission.
    let events2 = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush 2");
    assert!(events2.is_empty(), "parked send emits nothing on re-flush");
    assert_eq!(
        gateway.committed_send_keys.lock().unwrap().len(),
        1,
        "auto re-flush does not resend a parked send"
    );

    // Explicit user retry: re-dispatches under the same identity. The re-forward
    // of an already-committed send is deduplicated -> still one submission, and
    // the op settles Applied and is removed.
    assert!(service.retry_operation(&send.id).expect("retry"));
    let events3 = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush 3");
    assert_eq!(
        gateway.committed_send_keys.lock().unwrap().len(),
        1,
        "explicit retry under the same identity does not duplicate"
    );
    assert!(events3.iter().any(|e| e.topic == "operation.settled"));
    assert!(
        service
            .list_pending_operations(&account)
            .unwrap()
            .is_empty(),
        "the settled send is removed"
    );
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
