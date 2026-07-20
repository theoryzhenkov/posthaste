//! Adoption alias bridge for provisional sent messages: a state-assertion op
//! (Destroy/ReplaceMailboxes/SetKeywords) enqueued against a provisional
//! `send-<id>` (a not-yet-adopted sent message) retargets to the adopted real
//! id once adoption matches it, defers while the send is still in flight, and
//! no-ops when the send failed or was discarded.

use super::*;

fn send_request(subject: &str) -> SendMessageRequest {
    SendMessageRequest {
        subject: subject.to_string(),
        ..Default::default()
    }
}

fn send_entity(id: &str) -> OperationEntity {
    OperationEntity {
        kind: OperationEntityKind::Message,
        id: id.to_string(),
    }
}

/// A settled (Applied) send op for `send-<id>` — not flushable, so the flush
/// only touches the state-assertion op under test. Mirrors the real state after
/// a send settles but before adoption matches its provider copy.
fn settled_send(service: &MailService, account_id: &AccountId, send_entity_id: &str) -> Operation {
    let send = service
        .queue_operation(
            account_id,
            send_entity(send_entity_id),
            OperationKind::Send,
            serde_json::to_value(send_request("Outgoing")).unwrap(),
            None,
            None,
        )
        .expect("queue send");
    // Applied = settled, awaiting adoption. Not flushable, so the flush skips it.
    service
        .outbox
        .update_operation_state(&send.id, OperationState::Applied, 0, None)
        .expect("mark applied");
    send
}

#[tokio::test]
async fn state_assertion_on_a_provisional_send_retargets_to_the_adopted_id() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    let _send = settled_send(&service, &account, "send-retarget");
    // Adoption matched the provider copy: the alias is set.
    store
        .set_send_alias(&account, "send-retarget", "adopted-id")
        .expect("set alias");

    // A Destroy op against the provisional send-<id>.
    let destroy = service
        .queue_operation(
            &account,
            send_entity("send-retarget"),
            OperationKind::Destroy,
            serde_json::json!({}),
            None,
            None,
        )
        .expect("queue destroy");
    // The destroy readback: the adopted message was removed.
    gateway
        .readbacks
        .lock()
        .unwrap()
        .push(posthaste_domain_model::MessageReadback::Removed);

    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush ok");

    // The destroy retargeted to the adopted real id (not send-<id>).
    assert_eq!(
        gateway.message_mutation_targets.lock().unwrap().as_slice(),
        &[MessageId::from("adopted-id")],
        "the destroy retargeted to the adopted real id"
    );
    // The destroy settled + left the log (readback settlement).
    assert!(
        store.get_operation(&destroy.id).unwrap().is_none(),
        "the destroy op left the log"
    );
}

#[tokio::test]
async fn state_assertion_on_an_unadopted_send_defers_without_blocking_the_drain() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    // A settled send (Applied, awaiting adoption) — not flushable. No alias set.
    let _send = settled_send(&service, &account, "send-defer");

    // A Destroy op against the provisional send-<id> (alias absent → defer).
    let destroy = service
        .queue_operation(
            &account,
            send_entity("send-defer"),
            OperationKind::Destroy,
            serde_json::json!({}),
            None,
            None,
        )
        .expect("queue destroy");
    // A second state-assertion op on a REAL id — should flush (the deferred
    // destroy must not block the drain).
    let _flag = service
        .queue_operation(
            &account,
            send_entity("m-42"),
            OperationKind::SetKeywords,
            serde_json::to_value(SetKeywordsCommand {
                add: vec!["important".to_string()],
                remove: vec![],
            })
            .unwrap(),
            None,
            None,
        )
        .expect("queue setkeywords");

    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush ok");

    // The destroy deferred: re-queued Pending WITHOUT bumping attempts.
    let destroy_now = store
        .get_operation(&destroy.id)
        .unwrap()
        .expect("destroy still present");
    assert_eq!(destroy_now.state, OperationState::Pending);
    assert_eq!(destroy_now.attempts, 0, "defer does not bump attempts");
    // The destroy was never pushed to the gateway.
    assert!(
        !gateway
            .message_mutation_targets
            .lock()
            .unwrap()
            .iter()
            .any(|id| id.as_str() == "send-defer"),
        "the deferred destroy was never pushed"
    );
    // The unrelated setKeywords flushed — the drain continued past the defer.
    assert!(
        gateway
            .message_mutation_targets
            .lock()
            .unwrap()
            .iter()
            .any(|id| id.as_str() == "m-42"),
        "the unrelated setKeywords flushed despite the deferred destroy"
    );
}

#[tokio::test]
async fn state_assertion_on_a_failed_send_no_ops_without_a_gateway_call() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    // A FAILED send — terminal, no real copy landed.
    let send = service
        .queue_operation(
            &account,
            send_entity("send-failed"),
            OperationKind::Send,
            serde_json::to_value(send_request("Outgoing")).unwrap(),
            None,
            None,
        )
        .expect("queue send");
    store
        .update_operation_state(&send.id, OperationState::Failed, 1, Some("boom"))
        .expect("mark failed");

    // A Destroy op against the provisional send-<id>.
    let destroy = service
        .queue_operation(
            &account,
            send_entity("send-failed"),
            OperationKind::Destroy,
            serde_json::json!({}),
            None,
            None,
        )
        .expect("queue destroy");

    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush ok");

    // The destroy no-op'd: settled Applied + left the log, no gateway call.
    assert!(
        store.get_operation(&destroy.id).unwrap().is_none(),
        "the destroy op left the log"
    );
    assert!(
        gateway.message_mutation_targets.lock().unwrap().is_empty(),
        "no gateway call was made"
    );
}

#[tokio::test]
async fn state_assertion_on_a_discarded_send_no_ops() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_revision(1);

    // No send op at all — the send was discarded, its provisional row is gone.
    // A state-assertion op against the (now-gone) provisional send-<id>.
    let destroy = service
        .queue_operation(
            &account,
            send_entity("send-gone"),
            OperationKind::Destroy,
            serde_json::json!({}),
            None,
            None,
        )
        .expect("queue destroy");

    service
        .flush_account(&account, &gateway)
        .await
        .expect("flush ok");

    assert!(
        store.get_operation(&destroy.id).unwrap().is_none(),
        "the destroy op left the log"
    );
    assert!(
        gateway.message_mutation_targets.lock().unwrap().is_empty(),
        "no gateway call was made"
    );
}
