//! Message-detail query: the provisional-`send-<id>` body-fetch guard.

use super::*;

fn draft_request(subject: &str) -> SendMessageRequest {
    SendMessageRequest {
        subject: subject.to_string(),
        body: "draft body".to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn get_message_detail_skips_body_fetch_for_a_provisional_send_row() {
    // A dispatched-but-unadopted send surfaces a provisional `send-<id>` Sent
    // row with no IMAP message behind it (the real copy lands under its own
    // provider id and is adopted by RFC-`Message-ID` prefix). Loading its
    // detail must NOT fetch its body — which would reject with "missing IMAP
    // location" — but return the detail without a body, available later under
    // the real id once adoption retires the provisional row.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    // A no-hold send-now is due immediately: the fold upserts the provisional
    // Sent row under the send op's entity id (`send-<id>`).
    let (send_op, _) = service
        .enqueue_send(&account_id, draft_request("Send now"))
        .await
        .expect("send queues");
    let send_row_id = MessageId::from(send_op.entity.id.as_str());
    assert!(
        posthaste_domain_model::is_provisional_sent_id(send_row_id.as_str()),
        "the send op's entity id is a provisional send-<id>"
    );

    // Wire the gateway to REJECT a body fetch — the bug's error. The fix must
    // never reach this call.
    let gateway = MutationGateway::with_fetch_body_result(Err(GatewayError::Rejected(
        "missing IMAP location for message".to_string(),
    )));

    let result = service
        .get_message_detail(&account_id, &send_row_id, Some(&gateway))
        .await
        .expect("detail for a provisional send-<id> must not error");

    // The detail is returned (the overlay row exists), without a body.
    assert!(
        result.detail.is_some(),
        "the provisional Sent row's detail is returned without a body fetch"
    );
    // The gateway's fetch_message_body was NEVER called.
    assert!(
        gateway
            .fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned")
            .is_empty(),
        "the body fetch must be skipped for a provisional send-<id>"
    );
}
