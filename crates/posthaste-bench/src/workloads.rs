//! Deterministic, offline workloads for profiling and benchmarking the
//! `posthaste-store` SQLite hot paths.
//!
//! Every workload drives the store through its public domain ports only, so the
//! profile binary (`posthaste-profile`), the Criterion timing benches, and the
//! iai-callgrind regression gate all share one source of truth. Fixtures are
//! fully synthetic and require no network or external services.

use posthaste_domain::{
    search, AccountId, MailboxId, MailboxRecord, MessageCommandStore, MessageId, MessageListStore,
    MessagePage, MessageRecord, MessageSortField, Recipient, SetKeywordsCommand, SmartMailboxStore,
    SortDirection, SourceProjectionStore, SyncBatch, SyncCursor, SyncObject, SyncWriteStore,
    ThreadId,
};
use posthaste_store::DatabaseStore;
use tempfile::TempDir;

/// Default mailbox population used by the standalone profile binary and the
/// Criterion benches. Large enough that query/search/ingest costs dominate the
/// per-iteration overhead, small enough to stay fast in CI.
pub const DEFAULT_MESSAGE_COUNT: usize = 5_000;

const ACCOUNT: &str = "primary";
const SUBJECT_WORDS: &[&str] = &[
    "invoice", "meeting", "report", "launch", "budget", "review", "proposal", "reminder", "update",
    "draft",
];
const SENDERS: &[&str] = &["alice", "bob", "carol", "dave", "erin", "frank"];

/// A ready-to-drive store fixture. Holds the backing temp dir so it is cleaned
/// up when the fixture is dropped.
pub struct Fixture {
    pub store: DatabaseStore,
    pub account: AccountId,
    _temp: TempDir,
}

/// Generate `count` deterministic synthetic messages with varied subjects,
/// senders, bodies, and mailboxes so search/sort paths see realistic shapes.
pub fn synthetic_messages(count: usize) -> Vec<MessageRecord> {
    let thread_span = (count / 8).max(1);
    (0..count)
        .map(|i| {
            let word = SUBJECT_WORDS[i % SUBJECT_WORDS.len()];
            let sender = SENDERS[i % SENDERS.len()];
            let mailbox = if i % 4 == 0 { "archive" } else { "inbox" };
            MessageRecord {
                id: MessageId::from(format!("msg-{i:06}")),
                source_thread_id: ThreadId::from(format!("thread-{}", i % thread_span)),
                remote_blob_id: None,
                subject: Some(format!("{word} for project {} ({i})", i % 50)),
                from_name: Some(format!("{sender} example")),
                from_email: Some(format!("{sender}@example.test")),
                to: vec![Recipient {
                    name: Some("Inbox Owner".to_string()),
                    email: "owner@example.test".to_string(),
                }],
                preview: Some(format!("Preview for {word} message {i}.")),
                received_at: format!(
                    "2026-{:02}-{:02}T{:02}:00:00Z",
                    (i % 12) + 1,
                    (i % 28) + 1,
                    i % 24
                ),
                has_attachment: i % 7 == 0,
                size: 1024 + (i as i64 % 4096),
                mailbox_ids: vec![MailboxId::from(mailbox)],
                keywords: if i % 3 == 0 {
                    vec!["$seen".to_string()]
                } else {
                    Vec::new()
                },
                body_html: Some(format!(
                    "<p>{word} body for message {i}. Lorem ipsum dolor sit amet.</p>"
                )),
                body_text: Some(format!(
                    "{word} body for message {i}. Lorem ipsum dolor sit amet."
                )),
                raw_mime: None,
                rfc_message_id: Some(format!("<{i}@example.test>")),
                in_reply_to: None,
                references: Vec::new(),
            }
        })
        .collect()
}

fn mailboxes() -> Vec<MailboxRecord> {
    vec![
        MailboxRecord {
            id: MailboxId::from("inbox"),
            name: "Inbox".to_string(),
            role: Some("inbox".to_string()),
            unread_emails: 0,
            total_emails: 0,
        },
        MailboxRecord {
            id: MailboxId::from("archive"),
            name: "Archive".to_string(),
            role: Some("archive".to_string()),
            unread_emails: 0,
            total_emails: 0,
        },
    ]
}

/// Wrap `messages` in a full `SyncBatch` with the standard mailbox set and a
/// single message cursor.
pub fn sync_batch(messages: Vec<MessageRecord>) -> SyncBatch {
    SyncBatch {
        mailboxes: mailboxes(),
        messages,
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: false,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Message,
            state: "state-1".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        }],
    }
}

/// Open a fresh, empty store in a throwaway temp dir with the source projection
/// already registered (apply/query both expect a known source).
pub fn open_empty() -> Fixture {
    let temp = TempDir::new().expect("create temp dir");
    let store = DatabaseStore::open(temp.path().join("mail.sqlite"), temp.path().join("data"))
        .expect("open store");
    let account = AccountId::from(ACCOUNT);
    store
        .upsert_source_projection(&account, "Primary")
        .expect("seed source projection");
    Fixture {
        store,
        account,
        _temp: temp,
    }
}

/// Open a store pre-seeded with `count` synthetic messages.
pub fn open_seeded(count: usize) -> Fixture {
    let fixture = open_empty();
    fixture
        .store
        .apply_sync_batch(&fixture.account, &sync_batch(synthetic_messages(count)))
        .expect("seed messages");
    fixture
}

// --- Operations under profile -------------------------------------------------

/// Apply a pre-built batch into an existing fixture. Lets benches keep batch
/// construction out of the measured section.
pub fn apply_batch(fixture: &Fixture, batch: &SyncBatch) {
    fixture
        .store
        .apply_sync_batch(&fixture.account, batch)
        .expect("apply sync batch");
}

/// Ingest path: apply a single sync batch of `count` messages into a fresh store.
pub fn ingest(count: usize) {
    let fixture = open_empty();
    apply_batch(&fixture, &sync_batch(synthetic_messages(count)));
}

/// Query path: first page of the inbox, newest first.
pub fn list_inbox(fixture: &Fixture) -> MessagePage {
    fixture
        .store
        .list_message_page(
            &fixture.account,
            Some(&MailboxId::from("inbox")),
            50,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )
        .expect("list inbox page")
}

/// Search path: a parsed smart-mailbox rule over the inbox.
pub fn search(fixture: &Fixture) -> MessagePage {
    let rule = search::parse_query("in:inbox subject:invoice").expect("parse query");
    fixture
        .store
        .query_message_page_by_rule(&rule, 50, None, MessageSortField::Date, SortDirection::Desc)
        .expect("rule query page")
}

/// Mutation path: toggle a keyword on a single message.
pub fn mutate(fixture: &Fixture, index: usize) {
    let id = MessageId::from(format!("msg-{index:06}"));
    fixture
        .store
        .set_keywords(
            &fixture.account,
            &id,
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .expect("set keywords");
}
