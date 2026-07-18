//! Declarative TOML fixture loader: a fixture declares accounts + messages
//! (with field overrides); loading it creates the accounts and seeds the
//! messages so the projection reflects the declared state.
//!
// spec: docs/testing/L1#declarative-fixtures

use posthaste_domain_model::AccountId;
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
async fn declarative_fixture_loads_accounts_and_messages_into_the_projection() {
    let harness = Harness::new();
    let accounts = harness
        .load_fixture_toml(FIXTURE_TOML)
        .expect("fixture should load");

    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].as_str(), "a");

    let messages = harness
        .service
        .list_messages(&AccountId::from("a"), None)
        .expect("messages should list");

    // Both declared messages are present.
    assert_eq!(messages.len(), 2);
    // Subject overrides landed: the declared subject for m-1, the default for m-2.
    let subjects: Vec<String> = messages.iter().filter_map(|m| m.subject.clone()).collect();
    assert!(
        subjects.contains(&"Welcome to Posthaste".to_string()),
        "declared subject override should be present; got {subjects:?}"
    );
    assert!(
        subjects.contains(&"Subject m-2".to_string()),
        "default subject should still apply when unset; got {subjects:?}"
    );
    // Keyword overrides landed: m-1 is flagged (declared), m-2 is not (default).
    let flagged = messages
        .iter()
        .find(|m| m.subject.as_deref() == Some("Welcome to Posthaste"))
        .expect("m-1 should be present");
    assert!(flagged.is_flagged, "declared $flagged keyword should apply");
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
    let harness = Harness::new();
    let accounts = harness
        .load_fixture_toml(UNREAD_FIXTURE_TOML)
        .expect("fixture should load");

    let mailboxes = harness
        .service
        .list_mailboxes(&accounts[0])
        .expect("mailboxes should list");
    // The seeded inbox is the only mailbox with 2 messages.
    let inbox = mailboxes
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
