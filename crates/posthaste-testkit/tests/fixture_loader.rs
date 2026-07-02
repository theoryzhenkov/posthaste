//! Declarative TOML fixture loader: a fixture declares accounts + messages
//! (with field overrides); loading it creates the accounts and seeds the
//! messages so a view reflects the declared state.
//!
// spec: docs/testing/L1#declarative-fixtures

#[path = "common/mod.rs"]
mod common;

use posthaste_client_link::RuntimeLink;
use posthaste_contract_core::{AccountScopeRequest, MailListViewState, RuntimeCaller};
use posthaste_runtime_api::RuntimeMailReadApi;
use posthaste_testkit::Harness;

const FIXTURE_TOML: &str = r#"
[[accounts]]
id = "a"

  [[accounts.messages]]
  id = "m-1"
  mailbox = "inbox"
  subject = "Welcome to Posthaste"
  keywords = ["$seen", "$flagged"]

  [[accounts.messages]]
  id = "m-2"
  mailbox = "inbox"
"#;

#[tokio::test]
async fn declarative_fixture_loads_accounts_and_messages_into_the_view() {
    let harness = Harness::new().with_runtime().await;
    let accounts = harness
        .load_fixture_toml(FIXTURE_TOML)
        .await
        .expect("fixture should load");

    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].as_str(), "a");

    let snapshot = harness
        .core()
        .open_view(RuntimeCaller::test(), common::mail_list_view("in:a/inbox"))
        .await
        .expect("mail list view should open");
    let state: MailListViewState =
        serde_json::from_value(snapshot.data).expect("snapshot should be mail list state");

    // Both declared messages are present.
    assert_eq!(state.rows.len(), 2);
    // Subject overrides landed: the declared subject for m-1, the default for m-2.
    let subjects: Vec<String> = state
        .rows
        .iter()
        .filter_map(|row| {
            row.projection
                .get("subject")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    assert!(
        subjects.contains(&"Welcome to Posthaste".to_string()),
        "declared subject override should be present; got {subjects:?}"
    );
    assert!(
        subjects.contains(&"Subject m-2".to_string()),
        "default subject should still apply when unset; got {subjects:?}"
    );
}

const UNREAD_FIXTURE_TOML: &str = r#"
[[accounts]]
id = "a"

  [[accounts.messages]]
  id = "m-read"
  mailbox = "inbox"
  keywords = ["$seen"]

  [[accounts.messages]]
  id = "m-unread"
  mailbox = "inbox"
  keywords = []
"#;

#[tokio::test]
async fn fixture_unread_message_is_counted_in_mailbox_summary() {
    // A message without `$seen` is unread; the mailbox summary's unread count
    // must reflect it (the store derives unread from message keywords on read).
    let harness = Harness::new().with_runtime().await;
    let accounts = harness
        .load_fixture_toml(UNREAD_FIXTURE_TOML)
        .await
        .expect("fixture should load");

    let mailboxes = harness
        .core()
        .list_mailboxes(
            RuntimeCaller::test(),
            AccountScopeRequest::Explicit {
                account_ids: accounts.clone(),
            },
        )
        .await
        .expect("mailboxes should list");
    // The seeded inbox is the only mailbox with 2 messages; any system
    // mailboxes created by the mock-account sync are empty.
    let inbox = mailboxes
        .get(&accounts[0])
        .expect("account should have mailboxes")
        .iter()
        .find(|m| m.total_emails == 2)
        .expect("seeded inbox with 2 messages should be present");
    assert_eq!(inbox.total_emails, 2);
    assert_eq!(
        inbox.unread_emails, 1,
        "the unread (no $seen) message should count; got unread={} on {:?}",
        inbox.unread_emails, inbox
    );
}
