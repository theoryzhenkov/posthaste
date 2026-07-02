//! Declarative TOML fixtures.
//!
//! A [`Fixture`] declares accounts and their messages (with optional field
//! overrides) in TOML. [`RuntimeHarness::load_fixture_toml`](crate::RuntimeHarness::load_fixture_toml)
//! parses it and drives the programmatic `create_mock_account` /
//! `seed_messages_typed` helpers, so a scenario is one declarative block
//! instead of imperative setup.
//!
//! Only `driver = "mock"` is supported today; JMAP / provider-state fixtures
//! land with the live read-path (see `docs/eph/PLAN-L2-testkit-roadmap`).

use posthaste_domain_service::{MailboxId, MessageId, MessageRecord, SystemKeyword, ThreadId};

/// A parsed fixture: a set of accounts with their messages.
#[derive(Debug, serde::Deserialize)]
pub struct Fixture {
    #[serde(default)]
    pub accounts: Vec<FixtureAccount>,
}

impl Fixture {
    /// Parse a fixture from a TOML string.
    pub fn parse(toml: &str) -> Result<Self, FixtureError> {
        Ok(toml::from_str(toml)?)
    }
}

/// An account declared in a fixture.
#[derive(Debug, serde::Deserialize)]
pub struct FixtureAccount {
    pub id: String,
    #[serde(default)]
    pub driver: FixtureDriver,
    #[serde(default)]
    pub messages: Vec<FixtureMessage>,
}

/// A message declared in a fixture. Every field except `id` and `mailbox`
/// defaults to the seeded-message baseline ([`default_message`]); declaring a
/// field overrides just that field.
#[derive(Debug, serde::Deserialize)]
pub struct FixtureMessage {
    pub id: String,
    pub mailbox: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub from_name: Option<String>,
    #[serde(default)]
    pub from_email: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub received_at: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    /// Replaces the default `["$seen"]` keywords. `[]` clears them (unread).
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub rfc_message_id: Option<String>,
}

impl FixtureMessage {
    /// Build a [`MessageRecord`] from this spec, layering overrides on the
    /// [`default_message`] baseline.
    pub(crate) fn into_record(self) -> MessageRecord {
        let mut record = default_message(&self.id, &self.mailbox);
        if let Some(subject) = self.subject {
            record.subject = Some(subject);
        }
        if let Some(from_name) = self.from_name {
            record.from_name = Some(from_name);
        }
        if let Some(from_email) = self.from_email {
            record.from_email = Some(from_email);
        }
        if let Some(preview) = self.preview {
            record.preview = Some(preview);
        }
        if let Some(received_at) = self.received_at {
            record.received_at = received_at;
        }
        if let Some(size) = self.size {
            record.size = size;
        }
        if let Some(keywords) = self.keywords {
            record.keywords = keywords;
        }
        if let Some(thread_id) = self.thread_id {
            record.source_thread_id = ThreadId::from(thread_id);
        }
        if let Some(rfc_message_id) = self.rfc_message_id {
            record.rfc_message_id = Some(rfc_message_id);
        }
        record
    }
}

/// The account driver a fixture requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FixtureDriver {
    /// Mock driver — the working in-process path (default).
    #[default]
    Mock,
    /// JMAP over a `StalwartFixture` — not yet supported (live read-path).
    Jmap,
}

/// Errors raised while parsing or loading a fixture.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    /// TOML parse failure.
    #[error("fixture parse failed: {0}")]
    Parse(#[from] toml::de::Error),
    /// File read failure.
    #[error("fixture read failed: {0}")]
    Io(#[from] std::io::Error),
    /// A driver the loader does not (yet) drive.
    #[error("unsupported driver `{driver}`; only `mock` is supported (jmap/provider fixtures land with the live read-path)")]
    UnsupportedDriver { driver: &'static str },
}

/// The default keyword set for a seeded message: `[$seen]` (read). Single
/// source of truth for "a message with no declared keywords is read".
fn default_keywords() -> Vec<String> {
    vec![SystemKeyword::Seen.as_str().to_string()]
}

/// The seeded-message baseline: a `MessageRecord` with stable defaults that
/// every fixture message starts from before overrides apply. Single source of
/// truth shared by [`FixtureMessage::into_record`] and the programmatic
/// `seed_messages` tuple helper.
pub(crate) fn default_message(message_id: &str, mailbox_id: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(message_id),
        source_thread_id: ThreadId::from(format!("thread-{message_id}")),
        subject: Some(format!("Subject {message_id}")),
        from_name: Some("Alice".to_string()),
        from_email: Some("alice@example.com".to_string()),
        preview: Some("Preview".to_string()),
        received_at: "2026-03-31T10:00:00Z".to_string(),
        size: 42,
        mailbox_ids: vec![MailboxId::from(mailbox_id)],
        keywords: default_keywords(),
        rfc_message_id: Some(format!("<{message_id}@example.test>")),
        ..Default::default()
    }
}
