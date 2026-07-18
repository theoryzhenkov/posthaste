//! A managed mock Gmail IMAP server fixture for end-to-end provider-parity
//! tests.
//!
//! [`GmailImapFixture`] spawns a stateful, multi-connection mock Gmail IMAP
//! server on a loopback port. Unlike the focused single-shot protocol mock in
//! `posthaste-imap`'s discovery tests (which asserts capability negotiation and
//! FETCH parsing in isolation), this fixture answers the full discovery + sync +
//! mutation command set the real gateway drives across several connections,
//! so an account-against-the-fixture -> sync -> store -> projection test
//! exercises the Gmail IMAP path end to end (mirroring the JMAP live test).
//!
//! The server holds a shared [`MailModel`] (behind a mutex) modeling Gmail's
//! **label** semantics: one message store where each message carries a label
//! set, and mailboxes are views over labels — `INBOX` is the `\Inbox` label,
//! `[Gmail]/Starred` is `\Starred`, `[Gmail]/All Mail` is every live message
//! not in Trash/Spam. Mutation commands are modeled Gmail-faithfully:
//!
//! - `UID STORE +FLAGS (\Deleted)` marks the message deleted *in the selected
//!   mailbox only* (no visible effect on its own — exactly real Gmail).
//! - `UID EXPUNGE` of a `\Deleted`-marked message removes the selected
//!   mailbox's label (expunge-from-INBOX == archive); expunging from All Mail,
//!   Trash, or Spam deletes the message permanently.
//! - `UID COPY`/`UID MOVE` add the target mailbox's label; copying or moving
//!   into Trash/Spam **strips every other label** (real Gmail does this), so a
//!   trashed message leaves INBOX/All Mail/Starred immediately.
//!
//! Every label change advances the mailbox-shared MODSEQ and records
//! per-mailbox vanished UIDs, so the next sync observes mutations through the
//! real CONDSTORE / QRESYNC delta path (`ENABLE QRESYNC` -> `UID FETCH 1:*
//! (CHANGEDSINCE <modseq> VANISHED)` -> changed `FETCH`es + `* VANISHED
//! (EARLIER) <uids>`), and tests can assert the exact wire commands the
//! gateway issued via [`GmailImapFixture::commands`].
//!
//! Scope: [`GmailImapFixture::start`] advertises `X-GM-EXT-1` + `CONDSTORE` +
//! `QRESYNC`, while [`GmailImapFixture::start_condstore_only`] is
//! Gmail-faithful — real Gmail advertises **only** `CONDSTORE` — so it drives
//! the executor's CONDSTORE-only delta path (`UID FETCH ... (CHANGEDSINCE ...)`
//! without `VANISHED`, plus a header-free `UID SEARCH UNDELETED` for deletion
//! reconciliation). [`GmailImapFixture::start_generic_uidplus`] and
//! [`GmailImapFixture::start_generic_without_uidplus`] are plain-IMAP variants
//! (no Gmail extensions, no label stripping) for the generic move/expunge
//! wire tests. Per RFC 7162 the mock only emits `* VANISHED` when the client
//! used the `VANISHED` fetch modifier, and rejects that modifier with `BAD`
//! when QRESYNC is not advertised. No variant advertises `IDLE`, so the
//! gateway does not start a background IMAP IDLE push stream and the explicit
//! `sync_account` trigger drives a deterministic sync. Plain `EXPUNGE` and
//! `CLOSE` are answered with `BAD` on purpose: the adapter must never issue
//! the RFC 4315 mailbox-wide expunge, and a regression fails loudly here.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use posthaste_domain_model::{
    AccountTransportSettings, ImapTransportSettings, ProviderAuthKind, ProviderHint, SecretKind,
    SecretRef, SmtpTransportSettings, TransportSecurity,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Capabilities the QRESYNC-capable Gmail mock advertises. Deliberately omits
/// `IDLE` (see module docs): with no IDLE the gateway skips the background push
/// stream and syncs only on the explicit trigger.
const GMAIL_QRESYNC_CAPS: &str = "IMAP4rev1 CONDSTORE QRESYNC X-GM-EXT-1 UIDPLUS MOVE ENABLE";

/// Gmail-faithful capabilities: real Gmail advertises `CONDSTORE` (never
/// QRESYNC), `UIDPLUS`, and `MOVE`.
const GMAIL_CONDSTORE_ONLY_CAPS: &str = "IMAP4rev1 CONDSTORE X-GM-EXT-1 UIDPLUS MOVE ENABLE";

/// A plain IMAP server with UIDPLUS but no MOVE and no Gmail extensions — the
/// copy+`UID EXPUNGE` move path.
const GENERIC_UIDPLUS_CAPS: &str = "IMAP4rev1 UIDPLUS ENABLE";

/// A minimal IMAP server without UIDPLUS — pins the mark-`\Deleted`-only
/// removal fallback (no `UID EXPUNGE` available, plain EXPUNGE forbidden).
const GENERIC_BASIC_CAPS: &str = "IMAP4rev1 ENABLE";

/// Which server personality a fixture models.
#[derive(Clone, Copy)]
struct ServerFlavor {
    caps: &'static str,
    /// Gmail label semantics: X-GM-LABELS served on FETCH, Trash/Spam strips
    /// other labels on COPY/MOVE.
    gmail: bool,
}

impl ServerFlavor {
    fn qresync(&self) -> bool {
        self.caps.contains("QRESYNC")
    }
}

/// The mailbox's stable UIDVALIDITY (never changes within a fixture's life).
const UID_VALIDITY: u32 = 7;

/// The baseline (sync 1) INBOX message's observable fields. Kept as constants so
/// tests can assert against the same values the mock serves.
pub const SEEDED_SUBJECT: &str = "Quarterly numbers";
pub const SEEDED_FROM_EMAIL: &str = "alice@example.test";
/// The Gmail labels served on the seeded message (system + one custom).
pub const SEEDED_LABELS: &[&str] = &["\\Inbox", "\\Starred", "Project Alpha"];

/// The mailbox names the mock LISTs and models membership for.
pub const MAILBOX_INBOX: &str = "INBOX";
pub const MAILBOX_ALL_MAIL: &str = "[Gmail]/All Mail";
pub const MAILBOX_STARRED: &str = "[Gmail]/Starred";
pub const MAILBOX_TRASH: &str = "[Gmail]/Trash";
pub const MAILBOX_SPAM: &str = "[Gmail]/Spam";
pub const MAILBOX_DRAFTS: &str = "[Gmail]/Drafts";
pub const MAILBOX_SENT: &str = "[Gmail]/Sent Mail";

/// Mailboxes whose membership the model tracks (the rest are always empty).
const MODELED_MAILBOXES: &[&str] = &[
    MAILBOX_INBOX,
    MAILBOX_ALL_MAIL,
    MAILBOX_STARRED,
    MAILBOX_TRASH,
    MAILBOX_SPAM,
    MAILBOX_DRAFTS,
    MAILBOX_SENT,
];

/// The Gmail label whose presence puts a message in a mailbox view. All Mail
/// has no label — membership there is implicit (every live non-Trash/Spam
/// message).
fn label_for_mailbox(mailbox: &str) -> Option<&'static str> {
    if mailbox.eq_ignore_ascii_case(MAILBOX_INBOX) {
        return Some("\\Inbox");
    }
    match mailbox {
        MAILBOX_STARRED => Some("\\Starred"),
        MAILBOX_TRASH => Some("\\Trash"),
        MAILBOX_SPAM => Some("\\Spam"),
        MAILBOX_DRAFTS => Some("\\Draft"),
        MAILBOX_SENT => Some("\\Sent"),
        _ => None,
    }
}

/// One message in the mock server's single (Gmail-style) message store.
#[derive(Clone)]
struct MockMessage {
    uid: u32,
    gmail_msgid: u64,
    gmail_thrid: u64,
    subject: String,
    /// The MODSEQ at which this message last changed (delivered or relabeled).
    modseq: u64,
    /// The Gmail label set (drives mailbox-view membership).
    labels: BTreeSet<String>,
    /// Mailboxes in which `\Deleted` is currently set (per-mailbox, like real
    /// IMAP: the flag is folder-scoped on Gmail).
    deleted_in: BTreeSet<String>,
    /// The RFC822 header block served on header FETCHes. Synthesized for
    /// seeded/delivered messages; the literal client bytes for APPENDed and
    /// SMTP-submitted messages (so e.g. `X-Posthaste-Draft-Id` round-trips).
    header: String,
}

impl MockMessage {
    fn in_mailbox(&self, mailbox: &str) -> bool {
        if mailbox == MAILBOX_ALL_MAIL {
            // Gmail's All Mail: every live message except Trash/Spam — and
            // drafts, which live only in the Drafts view.
            return !self.labels.contains("\\Trash")
                && !self.labels.contains("\\Spam")
                && !self.labels.contains("\\Draft");
        }
        label_for_mailbox(mailbox)
            .map(|label| self.labels.contains(label))
            .unwrap_or(false)
    }
}

/// The synthesized header block for seeded/delivered messages (the shape the
/// fixture always served before APPEND support carried real client bytes).
fn synthesized_header(subject: &str, uid: u32) -> String {
    format!(
        "From: Alice <{SEEDED_FROM_EMAIL}>\r\nSubject: {subject}\r\nMessage-ID: <uid{uid}@example.test>\r\n\r\n"
    )
}

/// The header block (up to and including the blank line) of raw RFC822 bytes.
fn raw_header_block(raw: &[u8]) -> String {
    let end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// The (unfolded, single-line) value of `name:` in a raw header block.
fn header_value(header: &str, name: &str) -> Option<String> {
    header.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

/// The mock server's mutable state, shared across all connection handlers.
struct MailModel {
    highest_modseq: u64,
    next_uid: u32,
    /// Messages currently present anywhere on the server.
    live: Vec<MockMessage>,
    /// Per-mailbox (mailbox name, modseq, uid) removals since the baseline, so
    /// a `CHANGEDSINCE` delta returns only the relevant removals per mailbox.
    vanished: Vec<(String, u64, u32)>,
    /// How many `UID FETCH ... (CHANGEDSINCE ...)` commands the server has
    /// answered — lets a test prove the CONDSTORE/QRESYNC delta path was taken
    /// rather than a full re-snapshot.
    changedsince_fetches: usize,
    /// How many header-bearing FETCH responses the server has served (one per
    /// message per mailbox) — lets a test prove a delta sync fetched exactly
    /// the changed messages' headers and nothing else.
    header_fetches: usize,
    /// Every command line received, prefixed with the connection's selected
    /// mailbox (`-` before any SELECT) — the wire-assertion log.
    commands: Vec<String>,
    /// Raw RFC5322 payloads accepted over the mock SMTP endpoint, in order.
    smtp_submissions: Vec<Vec<u8>>,
}

impl MailModel {
    /// The baseline: one message (UID 1, MODSEQ 100), HIGHESTMODSEQ 100. The
    /// Gmail flavor seeds it with [`SEEDED_LABELS`] (INBOX + Starred + a custom
    /// label — so it also lives in All Mail and [Gmail]/Starred); the generic
    /// flavor seeds `\Inbox` only.
    fn baseline(gmail: bool) -> Self {
        let labels: BTreeSet<String> = if gmail {
            SEEDED_LABELS
                .iter()
                .map(|label| label.to_string())
                .collect()
        } else {
            BTreeSet::from(["\\Inbox".to_string()])
        };
        Self {
            highest_modseq: 100,
            next_uid: 2,
            live: vec![MockMessage {
                uid: 1,
                gmail_msgid: 1278455344230334865,
                gmail_thrid: 1266894439832287888,
                subject: SEEDED_SUBJECT.to_string(),
                modseq: 100,
                labels,
                deleted_in: BTreeSet::new(),
                header: synthesized_header(SEEDED_SUBJECT, 1),
            }],
            vanished: Vec::new(),
            changedsince_fetches: 0,
            header_fetches: 0,
            commands: Vec::new(),
            smtp_submissions: Vec::new(),
        }
    }

    fn members(&self, mailbox: &str) -> Vec<&MockMessage> {
        self.live
            .iter()
            .filter(|message| message.in_mailbox(mailbox))
            .collect()
    }

    /// Record per-mailbox vanished entries for every modeled mailbox `message`
    /// currently belongs to (used when a message leaves the server entirely).
    fn record_vanished_everywhere(&mut self, index: usize, modseq: u64) {
        let uid = self.live[index].uid;
        let mailboxes: Vec<String> = MODELED_MAILBOXES
            .iter()
            .filter(|mailbox| self.live[index].in_mailbox(mailbox))
            .map(|mailbox| mailbox.to_string())
            .collect();
        for mailbox in mailboxes {
            self.vanished.push((mailbox, modseq, uid));
        }
    }

    /// Expunge every live message and deliver one new message (labels
    /// `\Inbox`), advancing the mailbox MODSEQ. Returns the new message's UID.
    fn vanish_all_and_deliver(&mut self, subject: &str) -> u32 {
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        for index in 0..self.live.len() {
            self.record_vanished_everywhere(index, modseq);
        }
        self.live.clear();
        self.push_message(subject, modseq)
    }

    /// Deliver one new message (advancing MODSEQ) without expunging anything —
    /// the "a sibling arrived during sync" case. Returns the new UID.
    fn deliver(&mut self, subject: &str) -> u32 {
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        self.push_message(subject, modseq)
    }

    fn push_message(&mut self, subject: &str, modseq: u64) -> u32 {
        let uid = self.next_uid;
        self.next_uid += 1;
        self.live.push(MockMessage {
            uid,
            gmail_msgid: 1278455344230330000 + u64::from(uid),
            gmail_thrid: 1266894439832280000 + u64::from(uid),
            subject: subject.to_string(),
            modseq,
            labels: BTreeSet::from(["\\Inbox".to_string()]),
            deleted_in: BTreeSet::new(),
            header: synthesized_header(subject, uid),
        });
        uid
    }

    /// Store raw client-supplied RFC5322 bytes as a new message labeled into
    /// `mailbox` (IMAP `APPEND`, and the Gmail-SMTP auto-placed Sent copy),
    /// advancing the MODSEQ. Returns the new UID.
    fn append_raw(&mut self, mailbox: &str, raw: &[u8]) -> u32 {
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        let uid = self.next_uid;
        self.next_uid += 1;
        let header = raw_header_block(raw);
        let subject = header_value(&header, "Subject").unwrap_or_default();
        let labels: BTreeSet<String> = label_for_mailbox(mailbox)
            .into_iter()
            .map(|label| label.to_string())
            .collect();
        self.live.push(MockMessage {
            uid,
            gmail_msgid: 1278455344230330000 + u64::from(uid),
            gmail_thrid: 1266894439832280000 + u64::from(uid),
            subject,
            modseq,
            labels,
            deleted_in: BTreeSet::new(),
            header,
        });
        uid
    }

    /// Expunge every live message (advancing the mailbox MODSEQ) without
    /// delivering anything — the pure-removal case a CONDSTORE-only delta must
    /// detect through UID reconciliation (zero header bytes).
    fn expunge_all(&mut self) {
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        for index in 0..self.live.len() {
            self.record_vanished_everywhere(index, modseq);
        }
        self.live.clear();
    }

    /// Messages and vanished UIDs in `mailbox` changed strictly after
    /// `since_modseq` (the `CHANGEDSINCE` delta set).
    fn changed_since(&self, mailbox: &str, since_modseq: u64) -> (Vec<MockMessage>, Vec<u32>) {
        let changed = self
            .members(mailbox)
            .into_iter()
            .filter(|m| m.modseq > since_modseq)
            .cloned()
            .collect();
        let vanished = self
            .vanished
            .iter()
            .filter(|(name, modseq, _)| name == mailbox && *modseq > since_modseq)
            .map(|(_, _, uid)| *uid)
            .collect();
        (changed, vanished)
    }

    /// `UID STORE <uids> ±FLAGS (\Deleted)` in `mailbox`: track the
    /// folder-scoped `\Deleted` mark. Other flags are accepted and ignored
    /// (the mock does not model them).
    fn store_deleted(&mut self, mailbox: &str, uids: &[u32], add: bool) {
        for message in &mut self.live {
            if uids.contains(&message.uid) && message.in_mailbox(mailbox) {
                if add {
                    message.deleted_in.insert(mailbox.to_string());
                } else {
                    message.deleted_in.remove(mailbox);
                }
            }
        }
    }

    /// `UID EXPUNGE <uids>` in `mailbox`, Gmail-faithfully: expunging a
    /// `\Deleted`-marked message from a label mailbox removes that label only
    /// (expunge-from-INBOX == archive; the message stays in All Mail);
    /// expunging from All Mail, Trash, Spam, or Drafts deletes it permanently
    /// (a Gmail draft exists only as a draft — discarding it removes the
    /// message everywhere). Returns the (sequence, uid) pairs expunged.
    fn uid_expunge(&mut self, mailbox: &str, uids: &[u32]) -> Vec<(u32, u32)> {
        let targets: Vec<u32> = self
            .members(mailbox)
            .into_iter()
            .filter(|m| uids.contains(&m.uid) && m.deleted_in.contains(mailbox))
            .map(|m| m.uid)
            .collect();
        if targets.is_empty() {
            return Vec::new();
        }
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        let mut expunged = Vec::new();
        for uid in targets {
            let seq = self
                .members(mailbox)
                .iter()
                .position(|m| m.uid == uid)
                .map(|index| (index + 1) as u32)
                .unwrap_or(1);
            let index = self
                .live
                .iter()
                .position(|m| m.uid == uid)
                .expect("expunge target is live");
            let permanent = matches!(
                mailbox,
                MAILBOX_ALL_MAIL | MAILBOX_TRASH | MAILBOX_SPAM | MAILBOX_DRAFTS
            );
            if permanent {
                self.record_vanished_everywhere(index, modseq);
                self.live.remove(index);
            } else {
                let message = &mut self.live[index];
                if let Some(label) = label_for_mailbox(mailbox) {
                    message.labels.remove(label);
                }
                message.deleted_in.remove(mailbox);
                message.modseq = modseq;
                self.vanished.push((mailbox.to_string(), modseq, uid));
            }
            expunged.push((seq, uid));
        }
        expunged
    }

    /// `UID COPY`-into semantics: add the target mailbox's label. With Gmail
    /// semantics, copying into Trash or Spam strips every other label (real
    /// Gmail removes a trashed message from INBOX/All Mail/Starred itself).
    fn add_to_mailbox(&mut self, uids: &[u32], target: &str, gmail: bool) {
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        let strip = gmail && matches!(target, MAILBOX_TRASH | MAILBOX_SPAM);
        for index in 0..self.live.len() {
            if !uids.contains(&self.live[index].uid) {
                continue;
            }
            if strip {
                let uid = self.live[index].uid;
                let left: Vec<String> = MODELED_MAILBOXES
                    .iter()
                    .filter(|mailbox| **mailbox != target && self.live[index].in_mailbox(mailbox))
                    .map(|mailbox| mailbox.to_string())
                    .collect();
                for mailbox in left {
                    self.vanished.push((mailbox, modseq, uid));
                }
                self.live[index].labels.clear();
                self.live[index].deleted_in.clear();
            }
            if let Some(label) = label_for_mailbox(target) {
                self.live[index].labels.insert(label.to_string());
            }
            self.live[index].modseq = modseq;
        }
    }

    /// `UID MOVE` semantics: [`MailModel::add_to_mailbox`] plus removal from
    /// the source mailbox (already implied when Gmail stripping applied).
    fn move_to_mailbox(&mut self, source: &str, uids: &[u32], target: &str, gmail: bool) {
        self.add_to_mailbox(uids, target, gmail);
        let modseq = self.highest_modseq;
        for message in &mut self.live {
            if uids.contains(&message.uid) && message.in_mailbox(source) {
                if let Some(label) = label_for_mailbox(source) {
                    message.labels.remove(label);
                    message.modseq = modseq;
                    self.vanished
                        .push((source.to_string(), modseq, message.uid));
                }
            }
        }
    }
}

/// A disposable mock Gmail IMAP server bound to a loopback port.
///
/// The server task is aborted on drop. Use [`GmailImapFixture::imap_transport`]
/// to wire an `ImapSmtp` account against it.
pub struct GmailImapFixture {
    port: u16,
    smtp_port: u16,
    server: JoinHandle<()>,
    smtp_server: JoinHandle<()>,
    state: Arc<Mutex<MailModel>>,
    provider: ProviderHint,
}

impl GmailImapFixture {
    /// Bind a loopback port and start the mock server's accept loop with the
    /// baseline mailboxes (one Gmail-labeled message), advertising CONDSTORE +
    /// QRESYNC (the QRESYNC-delta coverage variant).
    pub async fn start() -> Self {
        Self::start_with_flavor(
            ServerFlavor {
                caps: GMAIL_QRESYNC_CAPS,
                gmail: true,
            },
            ProviderHint::Gmail,
        )
        .await
    }

    /// Like [`GmailImapFixture::start`], but Gmail-faithful: advertises
    /// CONDSTORE **without** QRESYNC (real Gmail never advertises QRESYNC), so
    /// re-syncs must take the executor's CONDSTORE-only delta path.
    pub async fn start_condstore_only() -> Self {
        Self::start_with_flavor(
            ServerFlavor {
                caps: GMAIL_CONDSTORE_ONLY_CAPS,
                gmail: true,
            },
            ProviderHint::Gmail,
        )
        .await
    }

    /// A plain IMAP server (no Gmail extensions, no label stripping) with
    /// UIDPLUS but without MOVE — drives the generic copy + `UID EXPUNGE`
    /// non-simple move path.
    pub async fn start_generic_uidplus() -> Self {
        Self::start_with_flavor(
            ServerFlavor {
                caps: GENERIC_UIDPLUS_CAPS,
                gmail: false,
            },
            ProviderHint::Generic,
        )
        .await
    }

    /// A plain IMAP server without UIDPLUS — pins the removal fallback
    /// (mark `\Deleted` only; no `UID EXPUNGE`, and never plain EXPUNGE).
    pub async fn start_generic_without_uidplus() -> Self {
        Self::start_with_flavor(
            ServerFlavor {
                caps: GENERIC_BASIC_CAPS,
                gmail: false,
            },
            ProviderHint::Generic,
        )
        .await
    }

    async fn start_with_flavor(flavor: ServerFlavor, provider: ProviderHint) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock gmail imap");
        let port = listener.local_addr().expect("mock gmail addr").port();
        let smtp_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock gmail smtp");
        let smtp_port = smtp_listener.local_addr().expect("mock smtp addr").port();
        let state = Arc::new(Mutex::new(MailModel::baseline(flavor.gmail)));
        let server = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(handle_connection(stream, Arc::clone(&state), flavor));
                }
            })
        };
        let smtp_server = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                while let Ok((stream, _)) = smtp_listener.accept().await {
                    tokio::spawn(handle_smtp_connection(stream, Arc::clone(&state), flavor));
                }
            })
        };
        Self {
            port,
            smtp_port,
            server,
            smtp_server,
            state,
            provider,
        }
    }

    /// The loopback port the mock IMAP server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The loopback port the mock SMTP submission endpoint is listening on.
    pub fn smtp_port(&self) -> u16 {
        self.smtp_port
    }

    /// How many messages `mailbox` currently holds on the mock server
    /// (label-model membership) — e.g. the exactly-one-Sent-copy assertion.
    pub fn mailbox_message_count(&self, mailbox: &str) -> usize {
        self.state
            .lock()
            .expect("mail model mutex")
            .members(mailbox)
            .len()
    }

    /// The subjects of the messages `mailbox` currently holds, in UID order —
    /// e.g. asserting the single Sent copy is the message that was sent.
    pub fn mailbox_subjects(&self, mailbox: &str) -> Vec<String> {
        self.state
            .lock()
            .expect("mail model mutex")
            .members(mailbox)
            .iter()
            .map(|message| message.subject.clone())
            .collect()
    }

    /// How many raw messages the mock SMTP endpoint has accepted.
    pub fn smtp_submission_count(&self) -> usize {
        self.state
            .lock()
            .expect("mail model mutex")
            .smtp_submissions
            .len()
    }

    /// Expunge the current message(s) and deliver a new one with `subject`,
    /// advancing the mailbox MODSEQ. The next sync observes this as a QRESYNC
    /// delta (`VANISHED` + a changed `FETCH`). Returns the new message's UID.
    pub fn vanish_inbox_and_deliver(&self, subject: &str) -> u32 {
        self.state
            .lock()
            .expect("mail model mutex")
            .vanish_all_and_deliver(subject)
    }

    /// Deliver one new message into INBOX (advancing MODSEQ) without expunging
    /// anything — the next sync observes it as a QRESYNC-delta sibling arrival.
    /// Returns the new message's UID.
    pub fn deliver_additional(&self, subject: &str) -> u32 {
        self.state
            .lock()
            .expect("mail model mutex")
            .deliver(subject)
    }

    /// Expunge every live message (advancing MODSEQ) without delivering
    /// anything — the next CONDSTORE-only sync must observe the removal through
    /// UID reconciliation while fetching zero headers.
    pub fn expunge_inbox(&self) {
        self.state.lock().expect("mail model mutex").expunge_all();
    }

    /// How many `CHANGEDSINCE` (CONDSTORE/QRESYNC-delta) fetches the server has
    /// answered. A test asserts this advanced to prove the delta path — not a
    /// full re-snapshot — drove a re-sync.
    pub fn changedsince_fetch_count(&self) -> usize {
        self.state
            .lock()
            .expect("mail model mutex")
            .changedsince_fetches
    }

    /// How many header-bearing FETCH responses (one per message per mailbox)
    /// the server has served across all syncs. The zero-refetch gate: a
    /// no-change re-sync must leave this untouched, and a delta must advance it
    /// by exactly the number of changed (message, mailbox) pairs.
    pub fn header_fetch_count(&self) -> usize {
        self.state.lock().expect("mail model mutex").header_fetches
    }

    /// Every command line the server received, prefixed with the connection's
    /// selected mailbox at the time (`-` before any SELECT), e.g.
    /// `INBOX: a5 UID EXPUNGE 1`. The wire-assertion log for mutation tests.
    pub fn commands(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("mail model mutex")
            .commands
            .clone()
    }

    /// Whether the message with `uid` is currently a member of `mailbox` on
    /// the mock server (label-model membership).
    pub fn mailbox_contains_uid(&self, mailbox: &str, uid: u32) -> bool {
        self.state
            .lock()
            .expect("mail model mutex")
            .members(mailbox)
            .iter()
            .any(|message| message.uid == uid)
    }

    /// Whether the message with `uid` is marked `\Deleted` in `mailbox` (the
    /// unexpunged-residual observation for the non-UIDPLUS fallback).
    pub fn is_marked_deleted_in(&self, mailbox: &str, uid: u32) -> bool {
        self.state
            .lock()
            .expect("mail model mutex")
            .live
            .iter()
            .any(|message| message.uid == uid && message.deleted_in.contains(mailbox))
    }

    /// The provider personality this fixture models (`Gmail` for the Gmail
    /// flavors, `Generic` for the plain-IMAP flavors) — the hint a gateway
    /// config built against this fixture should carry.
    pub fn provider(&self) -> ProviderHint {
        self.provider.clone()
    }

    /// The account username the mock authenticates (any password accepted;
    /// tests conventionally use [`GmailImapFixture::password`]).
    pub fn username(&self) -> String {
        "dev@gmail.example".to_string()
    }

    /// The password tests use against this mock (it accepts any).
    pub fn password(&self) -> String {
        "app-password".to_string()
    }

    /// The `ImapSmtp` account transport pointed at this mock. SMTP settings are
    /// required to build the gateway config, but the sync path never connects
    /// SMTP (only sends do).
    pub fn imap_transport(&self) -> AccountTransportSettings {
        AccountTransportSettings {
            provider: self.provider.clone(),
            auth: ProviderAuthKind::Password,
            base_url: None,
            username: Some(self.username()),
            secret_ref: Some(SecretRef {
                kind: SecretKind::Env,
                key: "POSTHASTE_UNUSED".to_string(),
            }),
            imap: Some(ImapTransportSettings {
                host: "127.0.0.1".to_string(),
                port: self.port,
                security: TransportSecurity::Plain,
            }),
            smtp: Some(SmtpTransportSettings {
                host: "127.0.0.1".to_string(),
                port: self.smtp_port,
                security: TransportSecurity::Plain,
            }),
        }
    }
}

impl Drop for GmailImapFixture {
    fn drop(&mut self) {
        self.server.abort();
        self.smtp_server.abort();
    }
}

/// Run the mock Gmail IMAP server on fixed ports indefinitely — the
/// long-running dev-provider counterpart to [`GmailImapFixture::start`]. Serves
/// IMAP on `imap_port` and a tiny HTTP control surface on `control_port`:
///
/// ```text
/// curl -XPOST 'http://127.0.0.1:<control_port>/deliver?subject=Hello'
/// curl -XPOST 'http://127.0.0.1:<control_port>/vanish?subject=Replaced'
/// ```
///
/// so a developer can drive deliveries/expunges against a live account and watch
/// the next sync take the QRESYNC delta path (`VANISHED` + a changed `FETCH`).
pub async fn serve(imap_port: u16, control_port: u16) -> std::io::Result<()> {
    let flavor = ServerFlavor {
        caps: GMAIL_QRESYNC_CAPS,
        gmail: true,
    };
    let state = Arc::new(Mutex::new(MailModel::baseline(true)));
    let imap = TcpListener::bind(("127.0.0.1", imap_port)).await?;
    let control = TcpListener::bind(("127.0.0.1", control_port)).await?;
    eprintln!(
        "mock-gmail: IMAP 127.0.0.1:{imap_port}  control http://127.0.0.1:{control_port} (POST /deliver?subject= , /vanish?subject=)"
    );
    let imap_state = Arc::clone(&state);
    let imap_loop = tokio::spawn(async move {
        while let Ok((stream, _)) = imap.accept().await {
            tokio::spawn(handle_connection(stream, Arc::clone(&imap_state), flavor));
        }
    });
    let control_loop = tokio::spawn(async move {
        while let Ok((stream, _)) = control.accept().await {
            tokio::spawn(handle_control(stream, Arc::clone(&state)));
        }
    });
    let _ = tokio::join!(imap_loop, control_loop);
    Ok(())
}

/// Minimal HTTP control surface: parse the request line, drive the mail model,
/// reply 200. Just enough for `curl` to trigger a delivery or an expunge.
async fn handle_control(stream: tokio::net::TcpStream, state: Arc<Mutex<MailModel>>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
        return;
    }
    // e.g. "POST /deliver?subject=Hello HTTP/1.1"
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let subject = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("subject="))
        .map(|value| value.replace('+', " "))
        .unwrap_or_else(|| "Dev message".to_string());
    let body = {
        let mut model = state.lock().expect("mail model mutex");
        match route {
            "/deliver" => format!("delivered uid {}\n", model.deliver(&subject)),
            "/vanish" => format!(
                "vanished + delivered uid {}\n",
                model.vanish_all_and_deliver(&subject)
            ),
            _ => "routes: POST /deliver?subject= , POST /vanish?subject=\n".to_string(),
        }
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = writer.write_all(response.as_bytes()).await;
}

/// Handle one client connection for the full discovery + sync + mutation
/// command set, reading the shared [`MailModel`] so SEARCH / FETCH / STATUS /
/// STORE / EXPUNGE / COPY / MOVE answer the model's current state, and tracking
/// the selected mailbox per connection.
async fn handle_connection(
    stream: tokio::net::TcpStream,
    state: Arc<Mutex<MailModel>>,
    flavor: ServerFlavor,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut selected: Option<String> = None;
    let caps = flavor.caps;

    if !send(
        &mut writer,
        &format!("* OK [CAPABILITY {caps}] mock-gmail ready\r\n"),
    )
    .await
    {
        return;
    }

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let cmd = line.trim_end_matches(['\r', '\n']).to_string();
        {
            let mut model = state.lock().expect("mail model mutex");
            model
                .commands
                .push(format!("{}: {}", selected.as_deref().unwrap_or("-"), cmd));
        }
        let upper = cmd.to_ascii_uppercase();
        let mut parts = cmd.split_whitespace();
        let tag = parts.next().unwrap_or("A1").to_string();
        let verb = parts.next().unwrap_or("").to_ascii_uppercase();

        let ok = match verb.as_str() {
            "CAPABILITY" => {
                send(&mut writer, &format!("* CAPABILITY {caps}\r\n")).await
                    && send(&mut writer, &format!("{tag} OK CAPABILITY completed\r\n")).await
            }
            "AUTHENTICATE" => {
                // SASL: accept any mechanism. Issue a continuation if no inline
                // initial response, then read+discard the client's response.
                let _mechanism = parts.next();
                let inline = parts.next().is_some();
                if !inline {
                    if !send(&mut writer, "+ \r\n").await {
                        break;
                    }
                    let mut resp = String::new();
                    if reader.read_line(&mut resp).await.is_err() {
                        break;
                    }
                }
                send(&mut writer, &format!("{tag} OK AUTHENTICATE completed\r\n")).await
            }
            "LOGIN" => send(&mut writer, &format!("{tag} OK LOGIN completed\r\n")).await,
            "ENABLE" => {
                // RFC 7162/5161: acknowledge QRESYNC (so the gateway uses the
                // VANISHED delta path) only when this server variant supports
                // it; an unsupported capability is silently absent from the
                // ENABLED response.
                let mut ok = true;
                if flavor.qresync() && upper.contains("QRESYNC") {
                    ok = send(&mut writer, "* ENABLED QRESYNC\r\n").await;
                }
                ok && send(&mut writer, &format!("{tag} OK ENABLE completed\r\n")).await
            }
            "NOOP" | "ID" => send(&mut writer, &format!("{tag} OK {verb} completed\r\n")).await,
            "LIST" => send_list(&mut writer, &tag).await,
            "STATUS" => {
                // Preflight the gateway issues on a re-sync to decide
                // skip-unchanged. Report the current MODSEQ/count so a mailbox
                // that changed since the last sync is correctly re-fetched.
                let name = normalized_mailbox(mailbox_arg(&cmd));
                let (messages, highest_modseq, next_uid) = {
                    let model = state.lock().expect("mail model mutex");
                    mailbox_status(&model, &name)
                };
                send(
                    &mut writer,
                    &format!(
                        "* STATUS \"{name}\" (MESSAGES {messages} UIDNEXT {next_uid} UIDVALIDITY {UID_VALIDITY} HIGHESTMODSEQ {highest_modseq})\r\n"
                    ),
                )
                .await
                    && send(&mut writer, &format!("{tag} OK STATUS completed\r\n")).await
            }
            "SELECT" | "EXAMINE" => {
                let name = normalized_mailbox(mailbox_arg(&cmd));
                let (exists, highest_modseq, next_uid) = {
                    let model = state.lock().expect("mail model mutex");
                    mailbox_status(&model, &name)
                };
                selected = Some(name);
                send_select(&mut writer, &tag, &verb, exists, highest_modseq, next_uid).await
            }
            "UID" => {
                let sub = parts.next().unwrap_or("").to_ascii_uppercase();
                let mailbox = selected.clone().unwrap_or_default();
                match sub.as_str() {
                    "SEARCH" => {
                        // The executor's deletion reconciliation sends `UID
                        // SEARCH UNDELETED`; honor the criterion by excluding
                        // `\Deleted`-marked members.
                        let hits = {
                            let model = state.lock().expect("mail model mutex");
                            model
                                .members(&mailbox)
                                .iter()
                                .filter(|m| {
                                    !(upper.contains("UNDELETED")
                                        && m.deleted_in.contains(&mailbox))
                                })
                                .map(|m| m.uid.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        };
                        send(&mut writer, &format!("* SEARCH {hits}\r\n")).await
                            && send(&mut writer, &format!("{tag} OK SEARCH completed\r\n")).await
                    }
                    "FETCH" => send_fetch(&mut writer, &tag, &cmd, &mailbox, &state, flavor).await,
                    "STORE" => {
                        let uids = uid_set_arg(&cmd, &state);
                        if upper.contains("\\DELETED") {
                            let add = !cmd.split_whitespace().any(|t| t.starts_with("-FLAGS"));
                            state
                                .lock()
                                .expect("mail model mutex")
                                .store_deleted(&mailbox, &uids, add);
                        }
                        send(&mut writer, &format!("{tag} OK STORE completed\r\n")).await
                    }
                    "EXPUNGE" => {
                        let uids = uid_set_arg(&cmd, &state);
                        let expunged = state
                            .lock()
                            .expect("mail model mutex")
                            .uid_expunge(&mailbox, &uids);
                        let mut ok = true;
                        for (seq, _uid) in expunged {
                            ok = ok && send(&mut writer, &format!("* {seq} EXPUNGE\r\n")).await;
                        }
                        ok && send(&mut writer, &format!("{tag} OK EXPUNGE completed\r\n")).await
                    }
                    "COPY" | "MOVE" => {
                        let uids = uid_set_arg(&cmd, &state);
                        let target = normalized_mailbox(trailing_mailbox_arg(&cmd));
                        {
                            let mut model = state.lock().expect("mail model mutex");
                            if sub == "MOVE" {
                                model.move_to_mailbox(&mailbox, &uids, &target, flavor.gmail);
                            } else {
                                model.add_to_mailbox(&uids, &target, flavor.gmail);
                            }
                        }
                        send(&mut writer, &format!("{tag} OK {sub} completed\r\n")).await
                    }
                    other => {
                        send(
                            &mut writer,
                            &format!("{tag} BAD unsupported UID {other}\r\n"),
                        )
                        .await
                    }
                }
            }
            "APPEND" => {
                // `tag APPEND <mailbox> (<flags>) {N}` — reply with the literal
                // continuation, read exactly N raw bytes (+ the trailing CRLF),
                // store the message, and (under UIDPLUS) report APPENDUID. Both
                // the draft-save path and the generic-provider Sent copy land
                // here.
                let mailbox = normalized_mailbox(mailbox_arg(&cmd));
                let Some(size) = literal_size_arg(&cmd) else {
                    let _ = send(
                        &mut writer,
                        &format!("{tag} BAD APPEND without a literal\r\n"),
                    )
                    .await;
                    continue;
                };
                // The caps never advertise LITERAL+, so the literal is always
                // synchronizing: issue the continuation before reading.
                if !send(&mut writer, "+ Ready for literal data\r\n").await {
                    break;
                }
                let mut raw = vec![0_u8; size];
                if reader.read_exact(&mut raw).await.is_err() {
                    break;
                }
                let mut trailer = String::new();
                if reader.read_line(&mut trailer).await.is_err() {
                    break;
                }
                let uid = state
                    .lock()
                    .expect("mail model mutex")
                    .append_raw(&mailbox, &raw);
                let status = if caps.contains("UIDPLUS") {
                    format!("{tag} OK [APPENDUID {UID_VALIDITY} {uid}] APPEND completed\r\n")
                } else {
                    format!("{tag} OK APPEND completed\r\n")
                };
                send(&mut writer, &status).await
            }
            "LOGOUT" => {
                let _ = send(&mut writer, "* BYE mock-gmail signing off\r\n").await;
                let _ = send(&mut writer, &format!("{tag} OK LOGOUT completed\r\n")).await;
                break;
            }
            // Deliberately BAD: the adapter must never issue the RFC 4315
            // mailbox-wide expunge (plain EXPUNGE / CLOSE) — it would sweep
            // other clients' `\Deleted` messages. A regression fails loudly.
            other => send(&mut writer, &format!("{tag} BAD unsupported {other}\r\n")).await,
        };
        if !ok {
            break;
        }
    }
}

/// Handle one mock SMTP submission session: a permissive ESMTP endpoint that
/// accepts any AUTH and one or more MAIL/RCPT/DATA transactions. Accepted
/// payloads are recorded on the model; with Gmail semantics the message is
/// additionally auto-placed into the Sent mailbox (real Gmail SMTP does this —
/// the client APPENDing its own copy is exactly the classic duplicate the
/// per-provider Sent-copy gate must prevent). Generic flavors record only, so
/// the Sent copy exists solely if the client APPENDs it.
async fn handle_smtp_connection(
    stream: tokio::net::TcpStream,
    state: Arc<Mutex<MailModel>>,
    flavor: ServerFlavor,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    if !send(&mut writer, "220 mock-gmail-smtp ESMTP ready\r\n").await {
        return;
    }
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let cmd = line.trim_end_matches(['\r', '\n']);
        let upper = cmd.to_ascii_uppercase();
        let ok = if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            send(
                &mut writer,
                "250-mock-gmail-smtp\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n",
            )
            .await
        } else if upper.starts_with("AUTH") {
            // Accept anything. A mechanism without an inline initial response
            // gets one continuation whose reply is read and discarded.
            if cmd.split_whitespace().nth(2).is_none() {
                if !send(&mut writer, "334 \r\n").await {
                    break;
                }
                let mut response = String::new();
                if reader.read_line(&mut response).await.is_err() {
                    break;
                }
            }
            send(&mut writer, "235 2.7.0 Authentication successful\r\n").await
        } else if upper.starts_with("MAIL") || upper.starts_with("RCPT") {
            send(&mut writer, "250 2.1.0 Ok\r\n").await
        } else if upper.starts_with("DATA") {
            if !send(&mut writer, "354 End data with <CR><LF>.<CR><LF>\r\n").await {
                break;
            }
            let mut raw: Vec<u8> = Vec::new();
            loop {
                let mut data_line = String::new();
                match reader.read_line(&mut data_line).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                let trimmed = data_line.trim_end_matches(['\r', '\n']);
                if trimmed == "." {
                    break;
                }
                // Undo SMTP dot-stuffing (RFC 5321 §4.5.2).
                let unstuffed = trimmed
                    .strip_prefix('.')
                    .filter(|_| trimmed.starts_with(".."));
                raw.extend_from_slice(unstuffed.unwrap_or(trimmed).as_bytes());
                raw.extend_from_slice(b"\r\n");
            }
            {
                let mut model = state.lock().expect("mail model mutex");
                if flavor.gmail {
                    // Real Gmail auto-places SMTP-submitted mail in Sent.
                    model.append_raw(MAILBOX_SENT, &raw);
                }
                model.smtp_submissions.push(raw);
            }
            send(&mut writer, "250 2.0.0 Ok: accepted\r\n").await
        } else if upper.starts_with("QUIT") {
            let _ = send(&mut writer, "221 2.0.0 Bye\r\n").await;
            break;
        } else if upper.starts_with("RSET") || upper.starts_with("NOOP") {
            send(&mut writer, "250 2.0.0 Ok\r\n").await
        } else {
            send(&mut writer, "502 5.5.2 Command not implemented\r\n").await
        };
        if !ok {
            break;
        }
    }
}

/// The mock's per-mailbox STATUS/SELECT numbers. Modeled mailboxes share the
/// server-wide HIGHESTMODSEQ (Gmail's modseq is account-global); unmodeled
/// mailboxes are permanently empty at MODSEQ 1 so skip-unchanged applies.
fn mailbox_status(model: &MailModel, mailbox: &str) -> (u32, u64, u32) {
    if MODELED_MAILBOXES
        .iter()
        .any(|name| name.eq_ignore_ascii_case(mailbox))
    {
        (
            model.members(mailbox).len() as u32,
            model.highest_modseq,
            model.next_uid,
        )
    } else {
        (0, 1, 1)
    }
}

/// Answer a `UID FETCH`. A `CHANGEDSINCE` modifier (CONDSTORE/QRESYNC delta)
/// returns the messages changed after that MODSEQ; a header-bearing fetch
/// (RFC822.HEADER requested) returns the selected mailbox's members filtered
/// by the UID set; a UID-only fetch (the mutation path's pre-flight probe)
/// returns bare `UID` items without counting header fetches.
///
/// VANISHED fidelity (RFC 7162): `* VANISHED (EARLIER)` responses are emitted
/// ONLY when the client used the `VANISHED` fetch modifier — a plain
/// CONDSTORE `CHANGEDSINCE` fetch never carries unsolicited VANISHED data —
/// and the modifier itself is rejected with `BAD` when the server variant does
/// not advertise QRESYNC.
async fn send_fetch(
    writer: &mut (impl AsyncWriteExt + Unpin),
    tag: &str,
    cmd: &str,
    mailbox: &str,
    state: &Arc<Mutex<MailModel>>,
    flavor: ServerFlavor,
) -> bool {
    let upper_cmd = cmd.to_ascii_uppercase();
    let wants_vanished = upper_cmd.contains("VANISHED");
    if wants_vanished && !flavor.qresync() {
        // RFC 7162: the VANISHED fetch modifier requires QRESYNC to be
        // enabled; a CONDSTORE-only server rejects it.
        return send(writer, &format!("{tag} BAD VANISHED requires QRESYNC\r\n")).await;
    }
    let wants_header = upper_cmd.contains("RFC822.HEADER");
    let requested = uid_set_arg(cmd, state);

    if !wants_header {
        // The mutation path's `UID FETCH <uid> (UID)` existence probe.
        let members: Vec<(u32, u32)> = {
            let model = state.lock().expect("mail model mutex");
            model
                .members(mailbox)
                .iter()
                .enumerate()
                .filter(|(_, m)| requested.contains(&m.uid))
                .map(|(index, m)| ((index + 1) as u32, m.uid))
                .collect()
        };
        for (seq, uid) in members {
            if !send(writer, &format!("* {seq} FETCH (UID {uid})\r\n")).await {
                return false;
            }
        }
        return send(writer, &format!("{tag} OK FETCH completed\r\n")).await;
    }

    let (messages, vanished): (Vec<MockMessage>, Vec<u32>) =
        if let Some(since) = parse_changedsince(&upper_cmd) {
            let mut model = state.lock().expect("mail model mutex");
            model.changedsince_fetches += 1;
            let (changed, vanished) = model.changed_since(mailbox, since);
            model.header_fetches += changed.len();
            (changed, vanished)
        } else {
            let mut model = state.lock().expect("mail model mutex");
            let members: Vec<MockMessage> = model
                .members(mailbox)
                .into_iter()
                .filter(|m| requested.contains(&m.uid))
                .cloned()
                .collect();
            model.header_fetches += members.len();
            (members, Vec::new())
        };

    if wants_vanished && !vanished.is_empty() {
        let uids = vanished
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        // Real Gmail (RFC 7162) sends VANISHED with NO leading sequence
        // number. The forked imap-codec now decodes this standard form (a
        // top-level `vanished` response-data parser), so the mock emits it
        // faithfully + the delta e2e exercises the real wire path.
        if !send(writer, &format!("* VANISHED (EARLIER) {uids}\r\n")).await {
            return false;
        }
    }
    for (index, message) in messages.iter().enumerate() {
        if !write_bytes(
            writer,
            &encode_fetch((index + 1) as u32, message, flavor.gmail),
        )
        .await
        {
            return false;
        }
    }
    send(writer, &format!("{tag} OK FETCH completed\r\n")).await
}

/// Extract the `CHANGEDSINCE <n>` value from an uppercased FETCH command, if any.
fn parse_changedsince(upper_cmd: &str) -> Option<u64> {
    let after = upper_cmd.split("CHANGEDSINCE").nth(1)?;
    after
        .split_whitespace()
        .next()?
        .trim_matches([')', '('])
        .parse()
        .ok()
}

async fn send(writer: &mut (impl AsyncWriteExt + Unpin), line: &str) -> bool {
    writer.write_all(line.as_bytes()).await.is_ok()
}

async fn write_bytes(writer: &mut (impl AsyncWriteExt + Unpin), bytes: &[u8]) -> bool {
    writer.write_all(bytes).await.is_ok()
}

/// Extract the mailbox argument from a `STATUS "<name>" (...)` / `SELECT
/// <name>` command, falling back to the first bare token after the verb.
fn mailbox_arg(cmd: &str) -> &str {
    if let Some(start) = cmd.find('"') {
        if let Some(end) = cmd[start + 1..].find('"') {
            return &cmd[start + 1..start + 1 + end];
        }
    }
    cmd.split_whitespace().nth(2).unwrap_or("INBOX")
}

/// Extract the trailing mailbox argument of `UID COPY <set> <mailbox>` /
/// `UID MOVE <set> <mailbox>` (quoted or bare).
fn trailing_mailbox_arg(cmd: &str) -> &str {
    if let Some(start) = cmd.find('"') {
        if let Some(end) = cmd[start + 1..].find('"') {
            return &cmd[start + 1..start + 1 + end];
        }
    }
    cmd.split_whitespace().nth(4).unwrap_or("INBOX")
}

/// Extract the trailing literal size of an `APPEND ... {N}` / `{N+}` command.
fn literal_size_arg(cmd: &str) -> Option<usize> {
    cmd.rsplit('{')
        .next()?
        .trim_end()
        .trim_end_matches('}')
        .trim_end_matches('+')
        .parse()
        .ok()
}

/// Fold mailbox-name case for INBOX (case-insensitive per RFC 3501); other
/// names are matched verbatim as LISTed.
fn normalized_mailbox(name: &str) -> String {
    if name.eq_ignore_ascii_case("INBOX") {
        MAILBOX_INBOX.to_string()
    } else {
        name.to_string()
    }
}

/// Parse a UID sequence set (`1`, `1:3`, `1,4:*`, `1:*`) against the model's
/// current UID space.
fn uid_set_arg(cmd: &str, state: &Arc<Mutex<MailModel>>) -> Vec<u32> {
    let max_uid = {
        let model = state.lock().expect("mail model mutex");
        model.next_uid.saturating_sub(1)
    };
    let Some(spec) = cmd.split_whitespace().nth(3) else {
        return Vec::new();
    };
    let mut uids = Vec::new();
    for part in spec.split(',') {
        let (from, to) = match part.split_once(':') {
            Some((from, to)) => (from, to),
            None => (part, part),
        };
        let from: u32 = if from == "*" {
            max_uid
        } else {
            from.parse().unwrap_or(0)
        };
        let to: u32 = if to == "*" {
            max_uid
        } else {
            to.parse().unwrap_or(0)
        };
        for uid in from.min(to)..=from.max(to) {
            if uid > 0 && !uids.contains(&uid) {
                uids.push(uid);
            }
        }
    }
    uids
}

async fn send_list(writer: &mut (impl AsyncWriteExt + Unpin), tag: &str) -> bool {
    for (attrs, name) in [
        ("\\Inbox", "INBOX"),
        ("\\HasChildren", "[Gmail]"),
        ("\\All \\HasNoChildren", MAILBOX_ALL_MAIL),
        ("\\Drafts \\HasNoChildren", "[Gmail]/Drafts"),
        ("\\Flagged \\HasNoChildren", MAILBOX_STARRED),
        ("\\Junk \\HasNoChildren", MAILBOX_SPAM),
        ("\\Trash \\HasNoChildren", MAILBOX_TRASH),
        ("\\Sent \\HasNoChildren", "[Gmail]/Sent Mail"),
    ] {
        if !send(writer, &format!("* LIST ({attrs}) \"/\" \"{name}\"\r\n")).await {
            return false;
        }
    }
    send(writer, &format!("{tag} OK LIST completed\r\n")).await
}

async fn send_select(
    writer: &mut (impl AsyncWriteExt + Unpin),
    tag: &str,
    verb: &str,
    exists: u32,
    highest_modseq: u64,
    next_uid: u32,
) -> bool {
    send(writer, "* FLAGS (\\Seen \\Flagged \\Deleted)\r\n").await
        && send(
            writer,
            "* OK [PERMANENTFLAGS (\\Seen \\Flagged \\Deleted \\*)]\r\n",
        )
        .await
        && send(writer, &format!("* {exists} EXISTS\r\n")).await
        && send(writer, "* 0 RECENT\r\n").await
        && send(writer, &format!("* OK [UIDVALIDITY {UID_VALIDITY}]\r\n")).await
        && send(writer, &format!("* OK [UIDNEXT {next_uid}]\r\n")).await
        && send(
            writer,
            &format!("* OK [HIGHESTMODSEQ {highest_modseq}]\r\n"),
        )
        .await
        && send(
            writer,
            &format!("{tag} OK [READ-WRITE] {verb} completed\r\n"),
        )
        .await
}

/// Encode one message's FETCH response via the forked `imap-codec`: UID +
/// RFC822.SIZE + RFC822.HEADER (literal) and, for the Gmail flavor, X-GM-MSGID +
/// X-GM-THRID + X-GM-LABELS + MODSEQ. Multi-word labels are pre-quoted
/// because the fork's `Text` encode emits raw bytes with no quoting.
///
/// MODSEQ is **spliced in** rather than encoded: the fork's encoder emits
/// `MODSEQ <v>` but its own decoder requires `MODSEQ (<v>)` (RFC 7162), so
/// `MessageDataItem::ModSeq` does not round-trip. The mailbox's stored
/// HIGHESTMODSEQ watermark is derived from this per-message MODSEQ
/// (`imap_mailbox_state_from_header_snapshot`), so a correctly-parenthesized
/// value is required for the next sync to take the QRESYNC delta path. We reuse
/// the encoder for the error-prone literal + label parts and append
/// ` MODSEQ (<v>)` inside the FETCH item list by hand.
fn encode_fetch(seq: u32, message: &MockMessage, gmail: bool) -> Vec<u8> {
    use imap_codec::encode::Encoder;
    use imap_codec::imap_types::core::{IString, Literal, NString, Vec1};
    use imap_codec::imap_types::fetch::MessageDataItem;
    use imap_codec::imap_types::response::{Data, Response};
    use imap_codec::ResponseCodec;
    use std::num::NonZeroU32;

    let mut items = vec![
        MessageDataItem::Uid(NonZeroU32::new(message.uid).expect("nonzero uid")),
        MessageDataItem::Rfc822Size(512),
        MessageDataItem::Rfc822Header(NString(Some(IString::Literal(
            Literal::try_from(message.header.clone().into_bytes()).expect("header literal"),
        )))),
    ];
    if gmail {
        items.push(MessageDataItem::GmailMessageId(message.gmail_msgid));
        items.push(MessageDataItem::GmailThreadId(message.gmail_thrid));
        items.push(MessageDataItem::GmailLabels(
            message
                .labels
                .iter()
                .map(|label| {
                    // Multi-word labels must be pre-quoted (see doc comment).
                    let encoded = if label.contains(' ') {
                        format!("\"{label}\"")
                    } else {
                        label.clone()
                    };
                    std::borrow::Cow::from(encoded)
                })
                .collect(),
        ));
    }
    let items = Vec1::try_from(items).expect("at least one fetch item");
    let response = Response::Data(Data::Fetch {
        seq: NonZeroU32::new(seq).expect("nonzero seq"),
        items,
    });
    let mut bytes = ResponseCodec::default().encode(&response).dump();
    // Splice ` MODSEQ (<v>)` before the FETCH item-list's closing paren. The
    // encoded line ends with `)\r\n`; replace that outer `)` with
    // ` MODSEQ (<v>))`.
    debug_assert!(bytes.ends_with(b")\r\n"));
    bytes.truncate(bytes.len() - 3);
    bytes.extend_from_slice(format!(" MODSEQ ({}))\r\n", message.modseq).as_bytes());
    bytes
}
