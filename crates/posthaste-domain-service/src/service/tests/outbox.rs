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

// DS3/D133: a `DraftUpdate` (create-new + destroy-old) threads the replace-
// destroy `notFound ⇒ Ok` mask by redelivery — `false` on the first delivery (so
// a failed replace-destroy surfaces rather than silently leaving a twin), `true`
// on a retry (whose earlier attempt may already have destroyed the old draft).
#[tokio::test]
async fn ds3_draft_update_redelivery_flag_tracks_attempts() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let key = MessageId::from("draft-stable");

    // Create + flush. A create carries no replace target → the flag is false.
    service
        .save_draft(&account, Some(key.clone()), draft_request("v1"))
        .expect("create");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("create flush");

    // Edit → a `DraftUpdate` (replace). Fail the first replace transiently so it
    // retries, making the second attempt a genuine redelivery.
    service
        .save_draft(&account, Some(key), draft_request("v2"))
        .expect("edit");
    gateway
        .save_draft_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::Unavailable("offline".to_string())));
    service
        .flush_account(&account, &gateway)
        .await
        .expect("first update flush stops on the transient");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("retry flush");

    // create=false, first update attempt=false (attempts 0), retry=true (attempts 1).
    let flags = gateway.save_draft_idempotent_calls.lock().unwrap();
    assert_eq!(flags.as_slice(), &[false, false, true]);
}

#[test]
fn delete_draft_enqueues_a_delete() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    let op = service
        .delete_draft(&account, MessageId::from("provider-7"), false)
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

// DS2: a `DraftCreate` whose create COMMITTED server-side but whose response was
// lost (Network → Transient) must not orphan a twin draft. The retry re-issues
// the create under the SAME deterministic create-id (derived from the stable op
// id), so the server no-ops the duplicate — exactly ONE provider draft results.
#[tokio::test]
async fn draft_create_lost_response_retry_yields_one_draft() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    // Attempt 1 commits the create server-side, then the response is lost.
    gateway
        .save_draft_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::Network("wifi dropped".to_string())));

    service
        .queue_operation(
            &account,
            draft_entity("draft-temp"),
            OperationKind::DraftCreate,
            serde_json::to_value(draft_request("Hello")).unwrap(),
        )
        .expect("queue create");

    // Attempt 1: transient, op re-Pends with the committed identity un-lost.
    service
        .flush_account(&account, &gateway)
        .await
        .expect("first flush stops on the transient");
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, OperationState::Pending);
    assert_eq!(pending[0].attempts, 1);

    // Attempt 2: the retry re-issues the SAME create-id → the mock dedups to the
    // id minted on attempt 1 → settles.
    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("retry flush settles");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, "operation.settled");
    assert!(service
        .list_pending_operations(&account)
        .expect("pending")
        .is_empty());

    // Two save attempts, but exactly ONE provider draft was minted (no twin).
    assert_eq!(gateway.save_draft_calls.lock().unwrap().len(), 2);
    assert_eq!(gateway.committed_draft_saves.lock().unwrap().len(), 1);
}

// DS2: the primary scenario — a `DraftUpdate` (create-new + destroy-old) whose
// create+destroy COMMITTED but whose response was lost. The retry re-issues the
// same deterministic create-id → one provider draft, not a twin (the orphaned P9
// + P10 the DS2 audit found).
#[tokio::test]
async fn draft_update_lost_response_retry_yields_one_draft() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let key = MessageId::from("draft-stable");

    // Create + flush → provider-draft-1.
    service
        .save_draft(&account, Some(key.clone()), draft_request("v1"))
        .expect("create");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("create flush");

    // Edit → a DraftUpdate. Attempt 1 commits the create+destroy, then loses the
    // response.
    service
        .save_draft(&account, Some(key), draft_request("v2"))
        .expect("edit");
    gateway
        .save_draft_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::Network("wifi dropped".to_string())));
    service
        .flush_account(&account, &gateway)
        .await
        .expect("first update flush stops on the transient");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("retry flush settles");

    assert!(service
        .list_pending_operations(&account)
        .expect("pending")
        .is_empty());
    // Three save attempts (create, update-1, update-2) but exactly TWO provider
    // drafts minted — one per distinct operation id. The lost-response update
    // retry re-used its committed id rather than orphaning a second draft.
    assert_eq!(gateway.save_draft_calls.lock().unwrap().len(), 3);
    assert_eq!(gateway.committed_draft_saves.lock().unwrap().len(), 2);
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
    assert!(
        gateway.delete_draft_calls.lock().unwrap().is_empty(),
        "a send without draft_id must not touch any draft (D126 is opt-in)"
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

/// A send request that names the saved draft it originates from (D126).
fn send_request_consuming(subject: &str, draft_key: &str) -> SendMessageRequest {
    SendMessageRequest {
        draft_id: Some(draft_key.to_string()),
        ..draft_request(subject)
    }
}

/// D126: a settled-successful send consumes its originating draft — the draft
/// delete is enqueued as a settlement effect and flushed by the follow-up pass
/// of the SAME `flush_account` call, against the provider-assigned draft id.
#[tokio::test]
async fn send_settlement_consumes_the_saved_draft_in_one_flush() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let key = MessageId::from("draft-local-stable");

    service
        .save_draft(&account, Some(key.clone()), draft_request("Hello"))
        .expect("save draft");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush draft");

    service
        .enqueue_send(&account, send_request_consuming("Outgoing", key.as_str()))
        .expect("send queues");
    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush send");

    assert_eq!(gateway.send_calls.lock().unwrap().len(), 1);
    assert_eq!(
        gateway.delete_draft_calls.lock().unwrap().as_slice(),
        &[MessageId::from("provider-draft-1")],
        "the settled send must destroy the provider draft it originated from"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.topic == "operation.settled")
            .count(),
        2,
        "both the send and its follow-up draft delete settle in one flush call"
    );
    assert!(
        service
            .list_pending_operations(&account)
            .unwrap()
            .is_empty(),
        "no operation is left behind"
    );
}

/// D125: a send parked `DispatchUncertain` KEEPS the draft — it is the user's
/// recovery artifact. Destruction happens only when an explicit retry settles
/// the send successfully.
#[tokio::test]
async fn parked_send_keeps_the_draft_until_settled_success() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let key = MessageId::from("draft-local-parked");

    service
        .save_draft(&account, Some(key.clone()), draft_request("Hello"))
        .expect("save draft");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush draft");

    gateway
        .send_results
        .lock()
        .unwrap()
        .push(Err(GatewayError::DispatchUncertain(
            "send timed out; delivery uncertain".to_string(),
        )));
    let send = service
        .enqueue_send(&account, send_request_consuming("Outgoing", key.as_str()))
        .expect("send queues");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush parks");

    // Parked: the draft is untouched — no destroy pushed, no delete enqueued.
    assert!(
        gateway.delete_draft_calls.lock().unwrap().is_empty(),
        "a parked send must not destroy its draft"
    );
    let pending = service.list_pending_operations(&account).expect("pending");
    assert_eq!(pending.len(), 1, "only the parked send remains queued");
    assert_eq!(pending[0].state, OperationState::DispatchUncertain);

    // Explicit user retry settles the send (deduplicated) — only now is the
    // draft consumed.
    assert!(service.retry_operation(&send.id).expect("retry"));
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush retry");
    assert_eq!(
        gateway.delete_draft_calls.lock().unwrap().as_slice(),
        &[MessageId::from("provider-draft-1")]
    );
    assert!(service
        .list_pending_operations(&account)
        .unwrap()
        .is_empty());
}

/// Ruling 24 / D126 idempotency: a redelivered send (same operation id) that
/// settles again must not double-destroy the already-consumed draft or error.
#[tokio::test]
async fn redelivered_send_settlement_does_not_double_destroy() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);
    let key = MessageId::from("draft-local-redelivered");

    service
        .save_draft(&account, Some(key.clone()), draft_request("Hello"))
        .expect("save draft");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush draft");
    let send = service
        .enqueue_send(&account, send_request_consuming("Outgoing", key.as_str()))
        .expect("send queues");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush send");
    assert_eq!(gateway.delete_draft_calls.lock().unwrap().len(), 1);

    // Redelivery: the same send operation re-enqueued under its original id
    // (the settled original was pruned). The gateway dedups the submission;
    // the settlement effect finds the draft already consumed (alias gone, no
    // projected message) and enqueues nothing.
    service.enqueue_operation(send).expect("redelivered send");
    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush redelivery");

    assert_eq!(
        gateway.committed_send_keys.lock().unwrap().len(),
        1,
        "the redelivered send is deduplicated, not resubmitted"
    );
    assert_eq!(
        gateway.delete_draft_calls.lock().unwrap().len(),
        1,
        "the consumed draft must not be destroyed a second time"
    );
    let pending = service.list_pending_operations(&account).expect("pending");
    assert!(
        pending.is_empty(),
        "the redelivered settlement leaves no failed/queued ops: {pending:?}"
    );
}

/// A send whose `draft_id` resolves to no alias and no projected message (a
/// never-saved compose) settles without enqueueing any destroy.
#[tokio::test]
async fn send_with_an_unknown_draft_id_settles_without_a_destroy() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    service
        .enqueue_send(
            &account,
            send_request_consuming("Outgoing", "draft-never-saved"),
        )
        .expect("send queues");
    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush send");

    assert_eq!(gateway.send_calls.lock().unwrap().len(), 1);
    assert!(gateway.delete_draft_calls.lock().unwrap().is_empty());
    assert_eq!(
        events
            .iter()
            .filter(|event| event.topic == "operation.settled")
            .count(),
        1
    );
    assert!(service
        .list_pending_operations(&account)
        .unwrap()
        .is_empty());
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
            serde_json::json!({ "idempotentRedelivery": true }),
        )
        .expect("queue delete");

    let events = service
        .flush_account(&account, &gateway)
        .await
        .expect("flush ok");

    // D132: the DraftDelete settlement emits BOTH operation.settled AND the
    // reconciling message.updated{deleted:true}, so the client's fold/prune
    // converges without a follow-up sync.
    assert_eq!(events.len(), 2);
    let reconciling = events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
        .expect("DraftDelete settlement emits message.updated");
    assert_eq!(reconciling.payload["messageId"], "provider-7");
    assert_eq!(reconciling.payload["deleted"], true);

    let deletes = gateway.delete_draft_calls.lock().unwrap();
    assert_eq!(deletes.as_slice(), &[MessageId::from("provider-7")]);
    // D133: the idempotent-redelivery flag reaches the gateway so the notFound
    // mask can be narrowed to that case only.
    assert_eq!(
        gateway
            .delete_draft_idempotent_calls
            .lock()
            .unwrap()
            .as_slice(),
        &[true]
    );
    assert!(service
        .list_pending_operations(&account)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn discard_draft_removes_the_row_emits_deleted_and_queues_a_non_idempotent_delete() {
    let account = AccountId::from("primary");
    // Seed a live draft row (in the Drafts mailbox) so the discard resolves it.
    let store = Arc::new(TestStore::with_message_state("draft-1", &["drafts"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    // A user-initiated discard (D130): emits the reconciling event immediately
    // and queues a non-idempotent provider delete (D133).
    let ack = service
        .discard_draft(&account, MessageId::from("draft-1"))
        .await
        .expect("discard ok");
    let reconciling = ack
        .events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
        .expect("discard emits message.updated");
    assert_eq!(reconciling.payload["messageId"], "draft-1");
    assert_eq!(reconciling.payload["deleted"], true);

    let pending = service.list_pending_operations(&account).unwrap();
    let delete = pending
        .iter()
        .find(|op| op.kind == OperationKind::DraftDelete)
        .expect("a DraftDelete op is queued");
    assert_eq!(delete.entity.id, "draft-1");
    assert_eq!(delete.payload["idempotentRedelivery"], false);
}

#[tokio::test]
async fn discard_of_a_synced_draft_resolves_via_the_sync_written_registry() {
    // The owner repro (DS2/D131), M69 shape: a draft synced from the server /
    // created on another device / surviving a restart was NOT saved in this
    // runtime, but sync's in-transaction write-through registered its stable
    // key ("draft-local-X") → live server Email id ("E1") in the draft
    // registry when the message row was projected (D135). A stable-id
    // list-row discard must resolve draft-local-X → E1 via the registry ALONE
    // (the projection fallback is deleted), NOT surface a spurious NotFound,
    // and target the LIVE Email id in the queued provider destroy.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("E1", &["drafts"]));
    store.draft_aliases.lock().unwrap().push((
        account.to_string(),
        "draft-local-X".to_string(),
        "E1".to_string(),
    ));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    let ack = service
        .discard_draft(&account, MessageId::from("draft-local-X"))
        .await
        .expect("synced-draft discard resolves via the registry, no NotFound");
    let reconciling = ack
        .events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_MESSAGE_UPDATED)
        .expect("discard emits message.updated");
    // The reconciling event names the resolved LIVE Email id.
    assert_eq!(reconciling.payload["messageId"], "E1");
    assert_eq!(reconciling.payload["deleted"], true);

    // The provider destroy is queued against the resolved live Email id (E1),
    // so the JMAP Email/set destroy actually removes the server draft.
    let pending = service.list_pending_operations(&account).unwrap();
    let delete = pending
        .iter()
        .find(|op| op.kind == OperationKind::DraftDelete)
        .expect("a DraftDelete op is queued");
    assert_eq!(delete.entity.id, "E1");
    assert_eq!(delete.payload["idempotentRedelivery"], false);
}

#[tokio::test]
async fn discard_of_an_unknown_draft_surfaces_not_found() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));

    // D133/D134: a discard of a draft that no longer resolves to a live row is a
    // surfaced failure (the client reverts the optimistic fold), not a silent
    // success.
    let result = service
        .discard_draft(&account, MessageId::from("ghost-draft"))
        .await;
    assert!(result.is_err(), "unknown-draft discard must surface an error");
}
