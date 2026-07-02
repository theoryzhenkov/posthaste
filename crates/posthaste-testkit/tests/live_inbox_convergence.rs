//! Live convergence scenario: inject 20 messages into a disposable Stalwart and
//! assert the app's real sync path converges them into the inbox view. Gated on
//! `POSTHASTE_STALWART_INTEGRATION=1` (real Stalwart required).
//!
//! The view is queried by the inbox's actual `MailboxId` (resolved via
//! `list_mailboxes`), matching how the app drives it
//! (`apps/web/src/runtime/httpAdapter.ts` `scopeQuery`: `in:<source>/<mailboxId>`,
//! not a bare name): a live-synced mailbox's id is namespaced (e.g. `jmap:<blob>`),
//! never the bare "inbox" the mock path seeds, so querying by name returns 0.
//!
// spec: docs/testing/L1#real-provider-parity

#[path = "common/mod.rs"]
mod common;

use std::time::Duration;

use posthaste_contract_core::{AccountScopeRequest, MailListViewState, RuntimeCaller};
use posthaste_runtime_api::RuntimeMailReadApi;
use posthaste_testkit::{Harness, StalwartFixture};

fn mail_list_rows(snapshot: &posthaste_contract_core::ViewSnapshot) -> usize {
    serde_json::from_value::<MailListViewState>(snapshot.data.clone())
        .expect("snapshot data should be mail list state")
        .rows
        .len()
}

#[tokio::test]
async fn twenty_injected_messages_converge_into_inbox_view() {
    if std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = StalwartFixture::start();
    let harness = Harness::new().with_runtime().await;
    let account = harness
        .create_jmap_account("jmap-stalwart", &stalwart)
        .await;

    // The app queries a mailbox by its actual MailboxId, not its name — see
    // apps/web/src/runtime/httpAdapter.ts `scopeQuery` (`in:<source>/<mailboxId>`).
    // A live-synced mailbox's id is namespaced (e.g. `jmap:<blob>`), never the
    // bare "inbox" the mock path seeds, so resolve the inbox's real id here.
    let caller = RuntimeCaller::test();
    let mailboxes = harness
        .core()
        .list_mailboxes(
            caller.clone(),
            AccountScopeRequest::Explicit {
                account_ids: vec![account.clone()],
            },
        )
        .await
        .expect("mailboxes should list");
    let inbox_mailbox = mailboxes
        .get(&account)
        .and_then(|ms| {
            ms.iter().find(|m| {
                m.role
                    .as_deref()
                    .is_some_and(|r| r.eq_ignore_ascii_case("inbox"))
                    || m.name.eq_ignore_ascii_case("inbox")
            })
        })
        .expect("inbox mailbox should be present after the initial sync");
    let inbox = common::mail_list_view(&format!("in:{account}/{}", inbox_mailbox.id.as_str()));
    let mut watch = harness.watch_view(inbox).await;
    let initial = mail_list_rows(watch.snapshot());
    eprintln!("DIAG initial inbox rows: {initial}");

    stalwart.inject(20).await;

    let reached = watch
        .wait_until(
            |snapshot| mail_list_rows(snapshot) >= initial + 20,
            Duration::from_secs(20),
        )
        .await;
    assert!(
        reached,
        "inbox never reached {} messages (started at {initial})",
        initial + 20
    );
    watch.assert_no_view_errors();
    watch.assert_seq_monotonic();
}
