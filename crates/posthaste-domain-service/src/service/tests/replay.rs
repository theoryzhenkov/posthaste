//! Rebuild-equivalence suite for the replay engine: the derived override
//! rows are a function of (op log, base) — wiping them and running the full
//! rebuild must reproduce the exact same overlay plane and effective views.

use super::*;

fn draft_request(subject: &str) -> SendMessageRequest {
    SendMessageRequest {
        subject: subject.to_string(),
        body: "draft body".to_string(),
        ..Default::default()
    }
}

/// Snapshot of the whole overlay plane (id → folded row / tombstone), as a
/// JSON value so wholesale before/after equality can be asserted.
fn overlay_snapshot(store: &TestStore) -> serde_json::Value {
    serde_json::to_value(&*store.overlay_rows.lock().expect("overlay rows lock"))
        .expect("overlay plane serializes")
}

/// One effective summary as a JSON value, for exact before/after comparison.
fn summary_value(
    store: &TestStore,
    account_id: &AccountId,
    message_id: &MessageId,
) -> serde_json::Value {
    serde_json::to_value(
        store
            .get_message_summary(account_id, message_id)
            .expect("effective read"),
    )
    .expect("summary serializes")
}

fn wipe_overlay(store: &TestStore) {
    store
        .overlay_rows
        .lock()
        .expect("overlay rows lock")
        .clear();
}

#[tokio::test]
async fn rebuild_reproduces_pending_assertion_rows() {
    // Three pending message assertions (flag, move, destroy) each derived an
    // override row at command time. Wipe the derived plane entirely and run
    // the full rebuild: every row must reappear identically from (log, base).
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("state-1", &["inbox"]));
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    service
        .set_keywords(
            &account,
            &MessageId::from("m-flag"),
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("flag queues");
    service
        .replace_mailboxes(
            &account,
            &MessageId::from("m-move"),
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![MailboxId::from("archive")],
            },
        )
        .await
        .expect("move queues");
    service
        .destroy_message(&account, &MessageId::from("m-gone"))
        .await
        .expect("destroy queues");

    let before = overlay_snapshot(&store);
    assert_eq!(
        before.as_object().expect("a map").len(),
        3,
        "each assertion derived one override row"
    );
    let flag_summary = summary_value(&store, &account, &MessageId::from("m-flag"));
    let move_summary = summary_value(&store, &account, &MessageId::from("m-move"));

    wipe_overlay(&store);
    assert!(
        store
            .get_message_summary(&account, &MessageId::from("m-gone"))
            .expect("effective read")
            .is_some(),
        "the wipe dropped the destroy tombstone: base shows through"
    );

    service
        .replay_account_overrides(&account)
        .await
        .expect("full rebuild");

    assert_eq!(
        overlay_snapshot(&store),
        before,
        "the rebuilt overlay plane is identical to the command-time derivation"
    );
    assert_eq!(
        summary_value(&store, &account, &MessageId::from("m-flag")),
        flag_summary,
        "the folded keyword survives the wipe"
    );
    assert_eq!(
        summary_value(&store, &account, &MessageId::from("m-move")),
        move_summary,
        "the folded membership survives the wipe"
    );
    assert!(
        store
            .get_message_summary(&account, &MessageId::from("m-gone"))
            .expect("effective read")
            .is_none(),
        "the destroy tombstone reappears and hides its base row"
    );
}

#[tokio::test]
async fn rebuild_reproduces_send_and_draft_intent_rows() {
    // Content-carrying ops while unsettled: a queued draft save and a due
    // send consuming a synced draft. Their derived rows — the instant draft
    // row, the provisional Sent row, and the consumed-draft tombstone — are
    // all op-owned, so a wipe plus full rebuild reproduces every one.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("state-1", &["drafts"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    let (save_op, _) = service
        .save_draft(&account_id, None, draft_request("Standalone draft"))
        .await
        .expect("save draft");
    let draft_live_id = save_op.entity.id.clone();
    let (send_op, _) = service
        .enqueue_send(
            &account_id,
            SendMessageRequest {
                draft_id: Some("provider-draft-42".to_string()),
                ..draft_request("Send now")
            },
        )
        .await
        .expect("send queues");

    let before = overlay_snapshot(&store);
    {
        let overlay = store.overlay_rows.lock().expect("overlay rows lock");
        assert!(
            matches!(overlay.get(&draft_live_id), Some(Some(record)) if record.draft_id.is_some()),
            "the queued save derived an instant draft row"
        );
        assert!(
            matches!(overlay.get("provider-draft-42"), Some(None)),
            "the due send tombstoned its consumed draft"
        );
        assert!(
            matches!(
                overlay.get(send_op.entity.id.as_str()),
                Some(Some(record))
                    if record.rfc_message_id.as_deref().is_some_and(|rfc| rfc.starts_with("phsend-"))
            ),
            "the due send derived its provisional Sent row"
        );
    }

    wipe_overlay(&store);
    service
        .replay_account_overrides(&account_id)
        .await
        .expect("full rebuild");

    assert_eq!(
        overlay_snapshot(&store),
        before,
        "the draft row, the consumed-draft tombstone, and the provisional \
         Sent row all reappear identically from (log, base)"
    );
}

#[tokio::test]
async fn rebuild_passes_pinned_rows_through() {
    // Pinned rows with NO owning op — a draft pin awaiting its provider copy
    // and a phsend- provisional row awaiting adoption — are not derivable
    // from the log; replay must pass them through byte-identically.
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    let mut draft_pin = sample_message_record("pinned-draft", 64, false);
    draft_pin.draft_id = Some("draft-local-pin".to_string());
    store
        .upsert_overlay_message(&account, &draft_pin)
        .expect("place draft pin");
    let mut sent_pin = sample_message_record("send-pin", 64, false);
    sent_pin.rfc_message_id = Some("phsend-token@posthaste.local".to_string());
    store
        .upsert_overlay_message(&account, &sent_pin)
        .expect("place provisional sent pin");

    let before = overlay_snapshot(&store);
    service
        .replay_account_overrides(&account)
        .await
        .expect("full rebuild");

    assert_eq!(
        overlay_snapshot(&store),
        before,
        "ownerless pinned rows survive the rebuild unchanged"
    );
}

#[tokio::test]
async fn sync_base_write_rederives_under_pending_op() {
    // A base row rewritten by sync UNDER a pending op re-derives: the fresh
    // base fields show through with the folded intent on top — and the
    // re-derivation does not depend on the override row still existing (a
    // wiped derived row reappears because the sweep's inventory is
    // log-derived, not overlay-derived).
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let message_id = MessageId::from("m-sync");

    let mut original = sample_message_record("m-sync", 128, false);
    original.subject = Some("Old subject".to_string());
    let seed_gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            messages: vec![original],
            ..Default::default()
        },
    );
    service
        .flush_and_observe(&account_id, &seed_gateway)
        .await
        .expect("seed base");

    service
        .set_keywords(
            &account_id,
            &message_id,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("flag queues");

    let mut rewritten = sample_message_record("m-sync", 128, false);
    rewritten.subject = Some("New subject".to_string());
    let rewrite_batch = SyncBatch {
        messages: vec![rewritten],
        ..Default::default()
    };
    // The flush leg must NOT settle the keyword op (a settled op leaves the
    // log; the pending-op re-derivation is what this test pins): every push
    // attempt fails transiently, so the op stays pending across both syncs.
    let rewrite_gateway = MutationGateway::with_sync_batch(2, rewrite_batch.clone());
    rewrite_gateway
        .set_keywords_results
        .lock()
        .expect("results lock")
        .push(Err(GatewayError::Network("offline".to_string())));
    service
        .flush_and_observe(&account_id, &rewrite_gateway)
        .await
        .expect("sync rewrites base");

    let summary = store
        .get_message_summary(&account_id, &message_id)
        .expect("effective read")
        .expect("visible row");
    assert_eq!(
        summary.subject.as_deref(),
        Some("New subject"),
        "the fresh base field shows through the re-derived row"
    );
    assert!(
        summary.keywords.iter().any(|keyword| keyword == "$flagged"),
        "the pending intent still folds on top of the rewritten base"
    );

    // Wipe the derived row: the next sweep must re-derive it from the log
    // alone (the row is no longer in the overlay inventory).
    store
        .overlay_rows
        .lock()
        .expect("overlay rows lock")
        .remove(message_id.as_str());
    let resweep_gateway = MutationGateway::with_sync_batch(3, rewrite_batch);
    resweep_gateway
        .set_keywords_results
        .lock()
        .expect("results lock")
        .push(Err(GatewayError::Network("offline".to_string())));
    service
        .flush_and_observe(&account_id, &resweep_gateway)
        .await
        .expect("sweep re-derives");

    let rederived = store
        .get_message_summary(&account_id, &message_id)
        .expect("effective read")
        .expect("visible row");
    assert!(
        rederived
            .keywords
            .iter()
            .any(|keyword| keyword == "$flagged"),
        "a wiped override row reappears from the op-derived sweep inventory"
    );
    assert_eq!(rederived.subject.as_deref(), Some("New subject"));
}

#[tokio::test]
async fn assertion_over_vanished_base_row_stays_derivable() {
    // A pending flag whose base row a sync DELETES: the fold has nothing to
    // fold over, so the remote removal wins — the override row is dropped by
    // the incremental replay, and a wipe plus full rebuild reproduces the
    // same (empty) state. The visible row is never a write-once artifact
    // that replay(log, base) cannot recompute.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let message_id = MessageId::from("m-vanish");

    let seed_gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            messages: vec![sample_message_record("m-vanish", 128, false)],
            ..Default::default()
        },
    );
    service
        .flush_and_observe(&account_id, &seed_gateway)
        .await
        .expect("seed base");

    service
        .set_keywords(
            &account_id,
            &message_id,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("flag queues");
    assert!(
        store
            .overlay_rows
            .lock()
            .expect("overlay rows lock")
            .contains_key("m-vanish"),
        "the pending flag derived an override row over the live base row"
    );

    // The remote delete arrives; the flag push keeps failing transiently so
    // the op stays pending across the sync.
    let delete_gateway = MutationGateway::with_sync_batch(
        2,
        SyncBatch {
            deleted_message_ids: vec![message_id.clone()],
            ..Default::default()
        },
    );
    delete_gateway
        .set_keywords_results
        .lock()
        .expect("results lock")
        .push(Err(GatewayError::Network("offline".to_string())));
    service
        .flush_and_observe(&account_id, &delete_gateway)
        .await
        .expect("sync deletes base row");

    assert!(
        store
            .get_message_summary(&account_id, &message_id)
            .expect("effective read")
            .is_none(),
        "the remote delete wins: the pending flag folds over nothing"
    );
    let before = overlay_snapshot(&store);
    assert!(
        !before.as_object().expect("a map").contains_key("m-vanish"),
        "the incremental replay dropped the override row with its base row"
    );

    wipe_overlay(&store);
    service
        .replay_account_overrides(&account_id)
        .await
        .expect("full rebuild");
    assert_eq!(
        overlay_snapshot(&store),
        before,
        "replay(log, base) reproduces the post-delete state exactly"
    );
}

#[tokio::test]
async fn aborted_sync_cycle_still_rederives_applied_chunks() {
    // Each applied chunk is a base write and triggers replay on its own: a
    // stream that aborts AFTER a chunk rewrote base under a pending op must
    // leave the override row re-derived over the fresh base — not a stale
    // fold surviving until some later cycle completes.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::default());
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);
    let message_id = MessageId::from("m-abort");

    let mut original = sample_message_record("m-abort", 128, false);
    original.subject = Some("Old subject".to_string());
    let seed_gateway = MutationGateway::with_sync_batch(
        1,
        SyncBatch {
            messages: vec![original],
            ..Default::default()
        },
    );
    service
        .flush_and_observe(&account_id, &seed_gateway)
        .await
        .expect("seed base");

    service
        .set_keywords(
            &account_id,
            &message_id,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("flag queues");

    let mut rewritten = sample_message_record("m-abort", 128, false);
    rewritten.subject = Some("New subject".to_string());
    let gateway = MutationGateway::with_stream(
        vec![SyncBatch {
            messages: vec![rewritten],
            ..Default::default()
        }],
        posthaste_domain_model::SyncReconciliation {
            remote_message_ids: vec![message_id.clone()],
            remote_mailbox_ids: Vec::new(),
            prune_messages: false,
            prune_mailboxes: false,
            cursors: Vec::new(),
        },
    );
    // The flush leg must not settle the flag, and the stream aborts after
    // the chunk applied — the end-of-cycle sweep never runs.
    gateway
        .set_keywords_results
        .lock()
        .expect("results lock")
        .push(Err(GatewayError::Network("offline".to_string())));
    *gateway.stream_error.lock().expect("stream error lock") =
        Some(GatewayError::Network("stream dropped".to_string()));
    service
        .sync_account(&account_id, SyncTrigger::Manual, &gateway, None)
        .await
        .expect_err("the aborted cycle surfaces its error");

    let summary = store
        .get_message_summary(&account_id, &message_id)
        .expect("effective read")
        .expect("visible row");
    assert_eq!(
        summary.subject.as_deref(),
        Some("New subject"),
        "the chunk's base write re-derived the override before the abort"
    );
    assert!(
        summary.keywords.iter().any(|keyword| keyword == "$flagged"),
        "the pending intent still folds on top of the rewritten base"
    );
}

#[tokio::test]
async fn op_touched_row_ids_matches_refresh_relevance() {
    // The op→rows mapping is the incremental-replay key: for every op shape
    // it must name exactly the live rows whose refresh folds that op —
    // assertions by entity id, draft ops by their registry-resolved live id,
    // sends by their own row plus the consumed draft's live row.
    let account = sample_source();
    let account_id = account.id.clone();
    let store = Arc::new(TestStore::with_message_state("state-1", &["inbox"]));
    let config = Arc::new(TestConfig {
        sources: vec![account],
        ..Default::default()
    });
    let service = MailService::new(store.clone(), config);

    service
        .set_keywords(
            &account_id,
            &MessageId::from("m-assert"),
            &SetKeywordsCommand {
                add: vec!["$seen".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("flag queues");
    let (save_op, _) = service
        .save_draft(&account_id, None, draft_request("Draft"))
        .await
        .expect("save draft");
    let (send_op, _) = service
        .enqueue_send(
            &account_id,
            SendMessageRequest {
                draft_id: Some("provider-draft-42".to_string()),
                ..draft_request("Send")
            },
        )
        .await
        .expect("send queues");

    let pending = service
        .list_pending_operations(&account_id)
        .expect("pending");
    for op in &pending {
        let touched = service
            .op_touched_row_ids(&account_id, op)
            .expect("touched rows");
        match op.kind {
            OperationKind::SetKeywords => {
                assert_eq!(touched, vec![MessageId::from("m-assert")]);
            }
            OperationKind::DraftCreate | OperationKind::DraftUpdate => {
                assert_eq!(
                    touched,
                    vec![MessageId::from(save_op.entity.id.as_str())],
                    "a draft op touches its key's registry-resolved live row"
                );
            }
            OperationKind::Send => {
                assert_eq!(
                    touched,
                    vec![
                        MessageId::from(send_op.entity.id.as_str()),
                        MessageId::from("provider-draft-42"),
                    ],
                    "a send touches its own provisional row and the consumed \
                     draft's live row"
                );
            }
            other => panic!("unexpected queued op kind: {other:?}"),
        }
    }
    assert_eq!(pending.len(), 3, "one op per shape under test");
}
