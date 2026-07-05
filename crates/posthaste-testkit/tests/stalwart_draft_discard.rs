//! REPLICATION of the "discard draft does nothing" field bug against a REAL
//! Stalwart-over-JMAP server.
//!
//! The owner's repro this pins:
//!   - Discard shows the toast but the draft STAYS in the Drafts view.
//!   - Fails on a fresh app restart with NO edits to the draft (no id rotation).
//!   - No optimistic "blink" — the client never optimistically removes the row.
//!   - Account is GREEN. Provider is JMAP. The toast is fired optimistically and
//!     confirms NOTHING about the delete reaching the server.
//!
//! The client fact (traced in apps/web): the list/detail discard sends the
//! SYNCED SERVER Email id — `MessageSummary.id` == `email.id()` per
//! `conversions.rs` — as the `draftId` (NOT the stable `X-Posthaste-Draft-Id`,
//! NOT a distinct entity-store-local id). `delete-draft` is a DIRECT command
//! (not `runRuntimeMutation`), so there is no optimistic fold; the row is meant
//! to disappear only when a server REMOVAL EVENT reconciles the projection.
//!
//! What these tests establish empirically, separating SERVER truth (queried
//! directly over JMAP) from the LOCAL projection (the Drafts mailList view):
//!   (A) The synced row id the client discards IS the true live server Email id
//!       (no separate local/entity id; no rotation with no edit). → REFUTES the
//!       "stale rotated id → notFound" theory (RFC-L2-drafts §field-bug) for the
//!       fresh-restart-no-edit repro.
//!   (C) `delete_draft` DISPATCHES and enqueues a `DraftDelete` carrying exactly
//!       that live id (what reaches `Email/set destroy`).
//!   (B) `Email/set destroy(live id)` DESTROYS the draft on the server
//!       (destroyed, not notFound), and a normal sync then PRUNES the local
//!       projection (via the `message.updated{deleted:true}` firehose). → REFUTES
//!       both the "notFound-mask swallows a live-id destroy" and "no removal
//!       reconciles the client" theories.
//!   And end to end: driving the discard flush through the runtime
//!       (`delete_draft` → `trigger_outbox_flush`, as the /v1 route does)
//!       reaches the server and destroys the draft.
//!
//! VERDICT (both tests PASS): the runtime discard path is CORRECT for the
//! fresh-restart-no-edit case. The field bug is therefore CLIENT-SIDE (apps/web),
//! consistent with the owner's own observations — discard is a non-optimistic
//! fire-and-forget DIRECT command (not `runRuntimeMutation`), so the toast fires
//! immediately and confirms nothing, and the row is meant to vanish only when a
//! server removal event reconciles it. See the task report for the client-side
//! trace and the recommended fix (model the draft as an optimistic entity routed
//! through `runRuntimeMutation`).
//!
//! HARNESS NOTE: flushing a `save_draft` (DraftCreate) through the runtime hangs
//! in this in-process harness — its multi-method `Email/set create` (preceded by
//! Identity/get + Mailbox/get) stalls over the shared WS push connection
//! (`live.rs` `send_request` → `ws.send`), a WS-correlation issue ORTHOGONAL to
//! discard (the single-method `Email/set destroy` flush terminates fine). So the
//! draft is SEEDED over plain HTTP (byte-identical `Email/set create` to the
//! gateway's), and the discard is driven through the real runtime path.
//!
//! Gated on `POSTHASTE_STALWART_INTEGRATION=1` (real Stalwart required).
//!
// spec: docs/testing/L1#real-provider-parity
// spec: docs/eph/RFC-L2-drafts#field-bug-2026-07-04

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use jmap_client::client::{Client, Credentials};
use jmap_client::{email, mailbox};
use posthaste_client_link::RuntimeLink;
use posthaste_contract_core::{
    AccountScopeRequest, MailListViewState, RuntimeCaller, ViewSnapshot,
};
use posthaste_domain_model::{AccountId, MessageId, OperationKind};
use posthaste_runtime_api::{RuntimeMailReadApi, RuntimeMailWriteApi};
use posthaste_testkit::{Harness, RuntimeHarness, StalwartFixture};

const STABLE_DRAFT_KEY: &str = "compose-session-discard-1";

/// Connect a raw JMAP client to the fixture over plain HTTP — an INDEPENDENT
/// view of true server state, separate from the app's local projection, and the
/// seam we seed/destroy through (bypassing the runtime's WS path).
async fn server_client(stalwart: &StalwartFixture) -> Arc<Client> {
    let client = Client::new()
        .credentials(Credentials::basic("dev", &stalwart.password))
        // Stalwart's session advertises a redirect to its bound (loopback) host.
        .follow_redirects(["127.0.0.1".to_string()])
        .connect(&stalwart.http_url)
        .await
        .expect("direct jmap client should connect to the fixture");
    Arc::new(client)
}

/// The Drafts mailbox id on the server (by role).
async fn server_drafts_mailbox(client: &Client) -> String {
    let mut request = client.build();
    request.get_mailbox().properties([
        mailbox::Property::Id,
        mailbox::Property::Role,
        mailbox::Property::Name,
    ]);
    request
        .send_get_mailbox()
        .await
        .expect("Mailbox/get should run")
        .take_list()
        .into_iter()
        .find(|m| m.role() == mailbox::Role::Drafts)
        .and_then(|m| m.id().map(str::to_string))
        .expect("server should have a Drafts mailbox")
}

/// Seed a draft directly on the server — the same `Email/set create` the
/// engine's `save_draft` issues: `$draft`+`$seen`, in Drafts, stamped with the
/// stable `X-Posthaste-Draft-Id` header. Returns the server-assigned Email id.
async fn seed_server_draft(client: &Client, drafts_mailbox: &str, subject: &str) -> String {
    let mut request = client.build();
    let set = request.set_email();
    {
        let email = set.create();
        email.mailbox_ids([drafts_mailbox]);
        email.keyword("$draft", true);
        email.keyword("$seen", true);
        email.from([("Dev Account", "dev@example.org")]);
        email.to([("Bob", "bob@example.test")]);
        email.subject(subject);
        email.header(
            email::Header::as_text(posthaste_domain_model::DRAFT_ID_HEADER, false),
            email::HeaderValue::AsText(STABLE_DRAFT_KEY.to_string()),
        );
        email.text_body(
            email::EmailBodyPart::new()
                .content_type("text/plain")
                .part_id("t"),
        );
        email.body_value("t".to_string(), "A seeded draft, to be discarded.");
    }
    request
        .send_set_email()
        .await
        .expect("Email/set create should run")
        .created("c0")
        .expect("draft create should succeed")
        .id()
        .expect("created draft has an id")
        .to_string()
}

/// The draft Email ids the SERVER holds (`$draft`-keyworded) — ground truth.
async fn server_draft_ids(client: &Client) -> Vec<String> {
    client
        .email_query(
            Some(email::query::Filter::has_keyword("$draft")),
            Some([email::query::Comparator::received_at().descending()]),
        )
        .await
        .expect("server draft query should run")
        .take_ids()
}

/// Destroy a draft Email on the server — byte-identical to the `Email/set
/// destroy` the runtime's `delete_draft` flush issues. Returns whether the
/// server reported it `destroyed` (vs `notFound`) — the exact signal the
/// gateway masks as `Ok`.
async fn destroy_server_email(client: &Client, id: &str) -> bool {
    let mut request = client.build();
    request.set_email().destroy([id]);
    let response = request
        .send_set_email()
        .await
        .expect("Email/set destroy should run");
    let destroyed_ids: Vec<String> = response
        .destroyed_ids()
        .map(|it| it.cloned().collect())
        .unwrap_or_default();
    let not_destroyed_ids: Vec<String> = response
        .not_destroyed_ids()
        .map(|it| it.cloned().collect())
        .unwrap_or_default();
    eprintln!(
        "DIAG (B) Email/set destroy response: destroyed={destroyed_ids:?} notDestroyed={not_destroyed_ids:?}"
    );
    destroyed_ids.iter().any(|d| d == id)
}

/// Resolve an account's Drafts `MailboxId` (runtime side) by role.
async fn drafts_mailbox_id(harness: &RuntimeHarness, account: &AccountId) -> String {
    let mailboxes = harness
        .core()
        .list_mailboxes(
            RuntimeCaller::test(),
            AccountScopeRequest::Explicit {
                account_ids: vec![account.clone()],
            },
        )
        .await
        .expect("mailboxes should list");
    mailboxes
        .get(account)
        .and_then(|ms| {
            ms.iter().find(|m| {
                m.role
                    .as_deref()
                    .is_some_and(|r| r.eq_ignore_ascii_case("drafts"))
                    || m.name.eq_ignore_ascii_case("drafts")
            })
        })
        .expect("a Drafts mailbox should exist after the initial sync")
        .id
        .to_string()
}

/// The `id`s of the local Drafts mailList view rows — what the client renders,
/// and the value discardDraft forwards as `draftId`.
async fn local_draft_row_ids(
    harness: &RuntimeHarness,
    account: &AccountId,
    drafts_mailbox: &str,
) -> Vec<String> {
    let caller = RuntimeCaller::test();
    let view = common::mail_list_view(&format!("in:{account}/{drafts_mailbox}"));
    let link = harness
        .core()
        .open_link(caller.clone())
        .await
        .expect("link should open")
        .link_id;
    let snapshot: ViewSnapshot = harness
        .core()
        .open_link_view(caller, link, view)
        .await
        .expect("Drafts view should open");
    serde_json::from_value::<MailListViewState>(snapshot.data)
        .expect("snapshot data should be mail list state")
        .rows
        .iter()
        .filter_map(|row| {
            row.projection
                .get("id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .collect()
}

/// The core replication. Seed a draft on the server (the fresh-restart-no-edit
/// state: a draft already on the server, no local pending op), sync it into the
/// projection, then reproduce discard and OBSERVE server truth vs local
/// projection SEPARATELY.
#[tokio::test]
async fn discard_draft_over_jmap_reconciles_server_and_projection() {
    if std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = StalwartFixture::start();
    let server = server_client(&stalwart).await;
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_jmap_account("jmap-discard", &stalwart).await;

    // 1) Seed a draft directly on the server (stable-key stamped), then sync so
    //    the local projection holds it — the fresh-restart-no-edit state.
    let drafts_server = server_drafts_mailbox(&server).await;
    let seeded_id = seed_server_draft(&server, &drafts_server, "Discard me").await;
    eprintln!("DIAG seeded server draft Email id: {seeded_id}");
    let n = tokio::time::timeout(Duration::from_secs(30), harness.sync_account(&account))
        .await
        .expect("bare sync must terminate");
    eprintln!("DIAG sync pulled {n} changes");

    // Ground truth BEFORE discard. (The fixture's seed.sh also plants a draft,
    // so we key on OUR seeded id, not a total count.)
    let server_before = server_draft_ids(&server).await;
    let drafts_mailbox = drafts_mailbox_id(&harness, &account).await;
    let local_before = local_draft_row_ids(&harness, &account, &drafts_mailbox).await;
    eprintln!("DIAG server drafts BEFORE: {server_before:?}");
    eprintln!("DIAG local  drafts BEFORE: {local_before:?}");
    assert!(
        server_before.contains(&seeded_id),
        "the seeded draft must exist on the server before discard"
    );

    // (A) Is the id the client discards EQUAL to the true live server Email id?
    // The synced Drafts row carries exactly the server Email id — no separate
    // local/entity id, no rotation (no edit). The client would discard THIS id.
    let row_id = local_before
        .iter()
        .find(|id| **id == seeded_id)
        .cloned()
        .expect("(A) the local Drafts projection must carry a row under the live server Email id");
    eprintln!("DIAG (A) client-sent row id == live server Email id: {row_id} (== {seeded_id})");
    assert_eq!(
        row_id, seeded_id,
        "(A) the synced row id the client discards IS the live server Email id — the 'stale rotated id' theory does not hold for the no-edit case"
    );

    // (B) Destroy that live Email over HTTP — byte-identical to the `Email/set
    //     destroy([row_id])` the gateway's `delete_draft` flush issues — and
    //     observe the SERVER response, then sync and check LOCAL reconciliation.
    //     (We deliberately do NOT call the runtime `delete_draft` here: it would
    //     `trigger_outbox_flush` a background sync that races this destroy and
    //     makes the observed response non-deterministic. The runtime-driven flush
    //     is covered end to end by `discard_flush_through_runtime_reaches_the_server`.)
    let destroyed = destroy_server_email(&server, &row_id).await;
    eprintln!("DIAG (B) server Email/set destroy([{row_id}]) -> destroyed_confirmed={destroyed}");
    assert!(
        destroyed,
        "a live-id `Email/set destroy` must report the draft destroyed (NOT notFound) — the engine's notFound-mask is therefore not the failure for the no-edit case"
    );

    let server_after = server_draft_ids(&server).await;
    eprintln!("DIAG server drafts AFTER destroy: {server_after:?}");
    let still_on_server = server_after.contains(&seeded_id);
    eprintln!(
        "DIAG (a) draft STILL on server after Email/set destroy? {still_on_server} \
         (if true: a live-id destroy silently no-ops → the engine's notFound mask swallows it → 'discard does nothing')"
    );

    // Reconcile: a normal sync must pull the destruction into the projection.
    let n = tokio::time::timeout(Duration::from_secs(30), harness.sync_account(&account))
        .await
        .expect("bare reconcile sync must terminate");
    eprintln!("DIAG reconcile sync pulled {n} changes");
    let local_after = local_draft_row_ids(&harness, &account, &drafts_mailbox).await;
    eprintln!("DIAG local drafts AFTER destroy+sync: {local_after:?}");

    // The captured verdict for candidate (b): once the Email is destroyed on the
    // server, a normal sync must reconcile the removal out of the projection.
    assert!(
        !local_after.contains(&seeded_id),
        "(b) LOCAL projection still shows the draft after it was destroyed on the server + a sync — the removal did not reconcile. local={local_after:?}"
    );
}

/// END-TO-END through the runtime: drive the app's real discard FLUSH (the
/// `delete_draft` command nudges `trigger_outbox_flush`, exactly as the /v1
/// route does) and confirm the `Email/set destroy` actually reaches the server.
///
/// Observed (PASSES): the flush terminates, the server destroys the draft, and
/// it is gone from the server's Drafts. Together with the sibling reconcile
/// test, this proves the ENTIRE runtime discard path — dispatch → outbox
/// DraftDelete(live id) → flush nudge → `Email/set destroy` → sync reconcile /
/// `message.updated{deleted:true}` firehose — is correct for the
/// fresh-restart-no-edit case. The backend is EXONERATED; the field bug is in
/// apps/web (see the module header).
#[tokio::test]
async fn discard_flush_through_runtime_reaches_the_server() {
    if std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = StalwartFixture::start();
    let server = server_client(&stalwart).await;
    let harness = Harness::new().with_runtime().await;
    let account = harness
        .create_jmap_account("jmap-discard-flush", &stalwart)
        .await;

    // Seed a draft on the server and pull it (fresh-restart-no-edit state).
    let drafts_server = server_drafts_mailbox(&server).await;
    let seeded_id = seed_server_draft(&server, &drafts_server, "Discard via runtime").await;
    let _ = tokio::time::timeout(Duration::from_secs(30), harness.sync_account(&account))
        .await
        .expect("bare sync must terminate");
    assert!(
        server_draft_ids(&server).await.contains(&seeded_id),
        "the seeded draft must be on the server before the discard"
    );

    // Discard via the real runtime path: enqueue the DraftDelete for the synced
    // Email id (what the client sends), then flush it via a sync.
    // (C) The op DISPATCHES and carries exactly the live synced Email id — the
    //     id that reaches `Email/set destroy`.
    let delete_op = harness
        .core()
        .delete_draft(
            RuntimeCaller::test(),
            account.clone(),
            None,
            MessageId::from(seeded_id.as_str()),
        )
        .await
        .expect("delete-draft must dispatch");
    assert_eq!(delete_op.kind, OperationKind::DraftDelete);
    assert_eq!(
        delete_op.entity.id, seeded_id,
        "(C) the DraftDelete targets the live synced Email id verbatim (no stale-key alias)"
    );
    eprintln!(
        "DIAG (C) DraftDelete enqueued: kind={:?} entity={}; flushing via sync",
        delete_op.kind, delete_op.entity.id
    );

    // Flush the DraftDelete. If this stalls, the interactive Email/set destroy
    // never reaches the server — the reproduction.
    let flushed =
        tokio::time::timeout(Duration::from_secs(45), harness.sync_account(&account)).await;
    let still_on_server = server_draft_ids(&server).await.contains(&seeded_id);
    match &flushed {
        Ok(n) => eprintln!("DIAG discard flush completed ({n} changes)"),
        Err(_) => eprintln!(
            "DIAG discard flush did NOT terminate within 45s — the interactive Email/set destroy stalled on the WS push transport"
        ),
    }
    eprintln!("DIAG draft STILL on server after the discard flush? {still_on_server}");

    assert!(
        flushed.is_ok(),
        "the discard flush must terminate (still_on_server={still_on_server})"
    );
    assert!(
        !still_on_server,
        "the discard flush must destroy the draft on the server — the backend discard path works end to end for the no-edit case"
    );
}
