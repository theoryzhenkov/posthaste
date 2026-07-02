//! A managed mock Gmail IMAP server fixture for end-to-end provider-parity
//! tests.
//!
//! [`GmailImapFixture`] spawns a stateful, multi-connection mock Gmail IMAP
//! server on a loopback port. Unlike the focused single-shot protocol mock in
//! `posthaste-imap`'s discovery tests (which asserts capability negotiation and
//! FETCH parsing in isolation), this fixture answers the full discovery + sync
//! command set the real gateway drives across several connections, so a
//! `create_gmail_account` -> sync -> store -> mailList view test exercises the
//! Gmail IMAP path end to end (mirroring the JMAP live test).
//!
//! The server holds a shared [`InboxModel`] (behind a mutex) so a test can
//! mutate the mailbox between syncs — [`GmailImapFixture::vanish_inbox_and_deliver`]
//! advances the mailbox's MODSEQ, marks the current message vanished, and
//! delivers a new one. The second sync then exercises the real CONDSTORE /
//! QRESYNC delta path: `ENABLE QRESYNC` -> `UID FETCH 1:* (CHANGEDSINCE <modseq>
//! VANISHED)` -> changed `FETCH`es + `* VANISHED (EARLIER) <uids>`.
//!
//! Scope: the fixture advertises `X-GM-EXT-1` + `CONDSTORE` + `QRESYNC` but
//! **not** `IDLE`, so the gateway does not start a background IMAP IDLE push
//! stream and the explicit `sync_account` trigger drives a deterministic sync.
//! Per-message MODSEQ is omitted from FETCH responses (the parse defaults it;
//! the mailbox's new HIGHESTMODSEQ comes from the `EXAMINE`/`STATUS` response).

use std::sync::{Arc, Mutex};

use posthaste_domain_model::{AccountDriver, AccountId, ImapTransportSettings, SmtpTransportSettings};
use posthaste_domain_model::{ProviderAuthKind, ProviderHint, TransportSecurity};
use posthaste_contract_core::{
    AccountTransportMutation, CreateAccountMutation, RuntimeCaller, SecretWriteMode,
    SecretWriteMutation,
};
use posthaste_runtime_api::RuntimeAccountApi;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::RuntimeHarness;

/// Capabilities the mock advertises. Deliberately omits `IDLE` (see module
/// docs): with no IDLE the gateway skips the background push stream and syncs
/// only on the explicit trigger.
const CAPS: &str = "IMAP4rev1 CONDSTORE QRESYNC X-GM-EXT-1 UIDPLUS ENABLE";

/// The mailbox's stable UIDVALIDITY (never changes within a fixture's life).
const UID_VALIDITY: u32 = 7;

/// The baseline (sync 1) INBOX message's observable fields. Kept as constants so
/// tests can assert against the same values the mock serves.
pub const SEEDED_SUBJECT: &str = "Quarterly numbers";
pub const SEEDED_FROM_EMAIL: &str = "alice@example.test";
/// The Gmail labels served on every mock message (system + one custom).
pub const SEEDED_LABELS: &[&str] = &["\\Inbox", "\\Starred", "Project Alpha"];

/// One message the mock serves from INBOX.
#[derive(Clone)]
struct MockMessage {
    uid: u32,
    gmail_msgid: u64,
    gmail_thrid: u64,
    subject: String,
    /// The MODSEQ at which this message last changed (delivered).
    modseq: u64,
}

/// The mock INBOX's mutable state, shared across all connection handlers.
struct InboxModel {
    highest_modseq: u64,
    next_uid: u32,
    /// Messages currently present in INBOX.
    live: Vec<MockMessage>,
    /// Messages expunged since the baseline, with the MODSEQ at which they
    /// vanished (so a `CHANGEDSINCE` delta returns only the relevant removals).
    vanished: Vec<(u64, u32)>,
    /// How many `UID FETCH ... (CHANGEDSINCE ...)` commands the server has
    /// answered — lets a test prove the QRESYNC delta path was taken rather
    /// than a full re-snapshot.
    changedsince_fetches: usize,
}

impl InboxModel {
    /// The baseline: one message (UID 1, MODSEQ 100), HIGHESTMODSEQ 100.
    fn baseline() -> Self {
        Self {
            highest_modseq: 100,
            next_uid: 2,
            live: vec![MockMessage {
                uid: 1,
                gmail_msgid: 1278455344230334865,
                gmail_thrid: 1266894439832287888,
                subject: SEEDED_SUBJECT.to_string(),
                modseq: 100,
            }],
            vanished: Vec::new(),
            changedsince_fetches: 0,
        }
    }

    /// Expunge every live message and deliver one new message, advancing the
    /// mailbox MODSEQ. Returns the new message's UID.
    fn vanish_all_and_deliver(&mut self, subject: &str) -> u32 {
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        for message in self.live.drain(..) {
            self.vanished.push((modseq, message.uid));
        }
        let uid = self.next_uid;
        self.next_uid += 1;
        self.live.push(MockMessage {
            uid,
            gmail_msgid: 1278455344230334999,
            gmail_thrid: 1266894439832287888,
            subject: subject.to_string(),
            modseq,
        });
        uid
    }

    /// Deliver one new message (advancing MODSEQ) without expunging anything —
    /// the "a sibling arrived during sync" case. Returns the new UID.
    fn deliver(&mut self, subject: &str) -> u32 {
        self.highest_modseq += 1;
        let modseq = self.highest_modseq;
        let uid = self.next_uid;
        self.next_uid += 1;
        self.live.push(MockMessage {
            uid,
            gmail_msgid: 1278455344230330000 + u64::from(uid),
            gmail_thrid: 1266894439832280000 + u64::from(uid),
            subject: subject.to_string(),
            modseq,
        });
        uid
    }

    /// Messages and vanished UIDs changed strictly after `since_modseq` (the
    /// `CHANGEDSINCE` delta set).
    fn changed_since(&self, since_modseq: u64) -> (Vec<MockMessage>, Vec<u32>) {
        let changed = self
            .live
            .iter()
            .filter(|m| m.modseq > since_modseq)
            .cloned()
            .collect();
        let vanished = self
            .vanished
            .iter()
            .filter(|(modseq, _)| *modseq > since_modseq)
            .map(|(_, uid)| *uid)
            .collect();
        (changed, vanished)
    }
}

/// A disposable mock Gmail IMAP server bound to a loopback port.
///
/// The server task is aborted on drop. Use [`create_gmail_account`] to wire an
/// `ImapSmtp` account against it.
///
/// [`create_gmail_account`]: RuntimeHarness::create_gmail_account
pub struct GmailImapFixture {
    port: u16,
    server: JoinHandle<()>,
    state: Arc<Mutex<InboxModel>>,
}

impl GmailImapFixture {
    /// Bind a loopback port and start the mock server's accept loop with the
    /// baseline INBOX (one Gmail-labeled message).
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock gmail imap");
        let port = listener.local_addr().expect("mock gmail addr").port();
        let state = Arc::new(Mutex::new(InboxModel::baseline()));
        let server = {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(handle_connection(stream, Arc::clone(&state)));
                }
            })
        };
        Self {
            port,
            server,
            state,
        }
    }

    /// The loopback port the mock server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Expunge the current INBOX message(s) and deliver a new one with `subject`,
    /// advancing the mailbox MODSEQ. The next sync observes this as a QRESYNC
    /// delta (`VANISHED` + a changed `FETCH`). Returns the new message's UID.
    pub fn vanish_inbox_and_deliver(&self, subject: &str) -> u32 {
        self.state
            .lock()
            .expect("inbox model mutex")
            .vanish_all_and_deliver(subject)
    }

    /// Deliver one new message into INBOX (advancing MODSEQ) without expunging
    /// anything — the next sync observes it as a QRESYNC-delta sibling arrival.
    /// Returns the new message's UID.
    pub fn deliver_additional(&self, subject: &str) -> u32 {
        self.state
            .lock()
            .expect("inbox model mutex")
            .deliver(subject)
    }

    /// How many `CHANGEDSINCE` (QRESYNC-delta) fetches the server has answered.
    /// A test asserts this advanced to prove the delta path — not a full
    /// re-snapshot — drove a re-sync.
    pub fn changedsince_fetch_count(&self) -> usize {
        self.state
            .lock()
            .expect("inbox model mutex")
            .changedsince_fetches
    }

    /// The `ImapSmtp` account transport pointed at this mock. SMTP settings are
    /// required to build the gateway config, but the sync path never connects
    /// SMTP (only sends do).
    fn account_mutation(&self, id: &str) -> CreateAccountMutation {
        CreateAccountMutation {
            id: Some(id.to_string()),
            name: id.to_string(),
            driver: Some(AccountDriver::ImapSmtp),
            enabled: Some(true),
            full_name: Some("Gmail Dev".to_string()),
            signature: None,
            email_patterns: vec!["dev@gmail.example".to_string()],
            appearance: None,
            transport: AccountTransportMutation {
                provider: Some(ProviderHint::Gmail),
                auth: Some(ProviderAuthKind::Password),
                base_url: None,
                username: Some("dev@gmail.example".to_string()),
                imap: Some(ImapTransportSettings {
                    host: "127.0.0.1".to_string(),
                    port: self.port,
                    security: TransportSecurity::Plain,
                }),
                smtp: Some(SmtpTransportSettings {
                    host: "127.0.0.1".to_string(),
                    port: self.port,
                    security: TransportSecurity::Plain,
                }),
            },
            secret: SecretWriteMutation {
                mode: SecretWriteMode::Replace,
                password: Some("app-password".to_string()),
            },
        }
    }
}

impl Drop for GmailImapFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl RuntimeHarness {
    /// Create a Gmail `ImapSmtp` account against a [`GmailImapFixture`], enable
    /// it (which runs discovery), and run an initial sync (full-snapshot fetch
    /// that lands the baseline INBOX message in the store). Returns the account
    /// id; re-sync after mutating the fixture with
    /// [`RuntimeHarness::sync_account`].
    pub async fn create_gmail_account(&self, id: &str, gmail: &GmailImapFixture) -> AccountId {
        let account = self
            .core()
            .create_account(RuntimeCaller::test(), gmail.account_mutation(id))
            .await
            .expect("gmail account should create");
        self.sync_account(&account.id).await;
        account.id
    }
}

/// Handle one client connection for the full discovery + sync command set,
/// reading the shared [`InboxModel`] so SEARCH / FETCH / STATUS answer the
/// mailbox's current state, and tracking the selected mailbox per connection.
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
    let state = Arc::new(Mutex::new(InboxModel::baseline()));
    let imap = TcpListener::bind(("127.0.0.1", imap_port)).await?;
    let control = TcpListener::bind(("127.0.0.1", control_port)).await?;
    eprintln!(
        "mock-gmail: IMAP 127.0.0.1:{imap_port}  control http://127.0.0.1:{control_port} (POST /deliver?subject= , /vanish?subject=)"
    );
    let imap_state = Arc::clone(&state);
    let imap_loop = tokio::spawn(async move {
        while let Ok((stream, _)) = imap.accept().await {
            tokio::spawn(handle_connection(stream, Arc::clone(&imap_state)));
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

/// Minimal HTTP control surface: parse the request line, drive the inbox model,
/// reply 200. Just enough for `curl` to trigger a delivery or an expunge.
async fn handle_control(stream: tokio::net::TcpStream, state: Arc<Mutex<InboxModel>>) {
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
        let mut model = state.lock().expect("inbox model mutex");
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

async fn handle_connection(stream: tokio::net::TcpStream, state: Arc<Mutex<InboxModel>>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut selected_inbox = false;

    if !send(
        &mut writer,
        &format!("* OK [CAPABILITY {CAPS}] mock-gmail ready\r\n"),
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
        let upper = cmd.to_ascii_uppercase();
        let mut parts = cmd.split_whitespace();
        let tag = parts.next().unwrap_or("A1").to_string();
        let verb = parts.next().unwrap_or("").to_ascii_uppercase();

        let ok = match verb.as_str() {
            "CAPABILITY" => {
                send(&mut writer, &format!("* CAPABILITY {CAPS}\r\n")).await
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
                // RFC 7162: acknowledge QRESYNC so the gateway uses the VANISHED
                // delta path. Echo any requested capability the mock supports.
                let mut ok = true;
                if upper.contains("QRESYNC") {
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
                let is_inbox = upper.contains("INBOX");
                let name = if is_inbox { "INBOX" } else { mailbox_arg(&cmd) };
                let (messages, highest_modseq) = if is_inbox {
                    let model = state.lock().expect("inbox model mutex");
                    (model.live.len(), model.highest_modseq)
                } else {
                    (0, 1)
                };
                let next_uid = if is_inbox {
                    state.lock().expect("inbox model mutex").next_uid
                } else {
                    1
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
                selected_inbox = upper.contains("INBOX");
                let (exists, highest_modseq, next_uid) = if selected_inbox {
                    let model = state.lock().expect("inbox model mutex");
                    (
                        model.live.len() as u32,
                        model.highest_modseq,
                        model.next_uid,
                    )
                } else {
                    (0, 1, 1)
                };
                send_select(&mut writer, &tag, &verb, exists, highest_modseq, next_uid).await
            }
            "UID" => {
                let sub = parts.next().unwrap_or("").to_ascii_uppercase();
                match sub.as_str() {
                    "SEARCH" => {
                        let hits = if selected_inbox {
                            let model = state.lock().expect("inbox model mutex");
                            model
                                .live
                                .iter()
                                .map(|m| m.uid.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        } else {
                            String::new()
                        };
                        send(&mut writer, &format!("* SEARCH {hits}\r\n")).await
                            && send(&mut writer, &format!("{tag} OK SEARCH completed\r\n")).await
                    }
                    "FETCH" => send_fetch(&mut writer, &tag, &upper, selected_inbox, &state).await,
                    other => {
                        send(
                            &mut writer,
                            &format!("{tag} BAD unsupported UID {other}\r\n"),
                        )
                        .await
                    }
                }
            }
            "LOGOUT" => {
                let _ = send(&mut writer, "* BYE mock-gmail signing off\r\n").await;
                let _ = send(&mut writer, &format!("{tag} OK LOGOUT completed\r\n")).await;
                break;
            }
            other => send(&mut writer, &format!("{tag} BAD unsupported {other}\r\n")).await,
        };
        if !ok {
            break;
        }
    }
}

/// Answer a `UID FETCH`. A `CHANGEDSINCE` modifier (QRESYNC delta) returns the
/// messages changed after that MODSEQ plus a `* VANISHED (EARLIER)` line for
/// removals; otherwise (full-snapshot fetch after a SEARCH) it returns every
/// live message.
async fn send_fetch(
    writer: &mut (impl AsyncWriteExt + Unpin),
    tag: &str,
    upper_cmd: &str,
    selected_inbox: bool,
    state: &Arc<Mutex<InboxModel>>,
) -> bool {
    let (messages, vanished): (Vec<MockMessage>, Vec<u32>) = if !selected_inbox {
        (Vec::new(), Vec::new())
    } else if let Some(since) = parse_changedsince(upper_cmd) {
        let mut model = state.lock().expect("inbox model mutex");
        model.changedsince_fetches += 1;
        model.changed_since(since)
    } else {
        let model = state.lock().expect("inbox model mutex");
        (model.live.clone(), Vec::new())
    };

    if !vanished.is_empty() {
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
        if !write_bytes(writer, &encode_fetch((index + 1) as u32, message)).await {
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

/// Extract the mailbox argument from a `STATUS "<name>" (...)` command, falling
/// back to the first bare token after the verb.
fn mailbox_arg(cmd: &str) -> &str {
    if let Some(start) = cmd.find('"') {
        if let Some(end) = cmd[start + 1..].find('"') {
            return &cmd[start + 1..start + 1 + end];
        }
    }
    cmd.split_whitespace().nth(2).unwrap_or("INBOX")
}

async fn send_list(writer: &mut (impl AsyncWriteExt + Unpin), tag: &str) -> bool {
    for (attrs, name) in [
        ("\\Inbox", "INBOX"),
        ("\\HasChildren", "[Gmail]"),
        ("\\All \\HasNoChildren", "[Gmail]/All Mail"),
        ("\\Drafts \\HasNoChildren", "[Gmail]/Drafts"),
        ("\\Flagged \\HasNoChildren", "[Gmail]/Starred"),
        ("\\Junk \\HasNoChildren", "[Gmail]/Spam"),
        ("\\Trash \\HasNoChildren", "[Gmail]/Trash"),
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
    send(writer, "* FLAGS (\\Seen \\Flagged)\r\n").await
        && send(writer, "* OK [PERMANENTFLAGS (\\Seen \\Flagged \\*)]\r\n").await
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
/// RFC822.SIZE + RFC822.HEADER (literal) + X-GM-MSGID + X-GM-THRID +
/// X-GM-LABELS + MODSEQ. Multi-word labels are pre-quoted because the fork's
/// `Text` encode emits raw bytes with no quoting.
///
/// MODSEQ is **spliced in** rather than encoded: the fork's encoder emits
/// `MODSEQ <v>` but its own decoder requires `MODSEQ (<v>)` (RFC 7162), so
/// `MessageDataItem::ModSeq` does not round-trip. The mailbox's stored
/// HIGHESTMODSEQ watermark is derived from this per-message MODSEQ
/// (`imap_mailbox_state_from_header_snapshot`), so a correctly-parenthesized
/// value is required for the next sync to take the QRESYNC delta path. We reuse
/// the encoder for the error-prone literal + label parts and append
/// ` MODSEQ (<v>)` inside the FETCH item list by hand.
fn encode_fetch(seq: u32, message: &MockMessage) -> Vec<u8> {
    use imap_codec::encode::Encoder;
    use imap_codec::imap_types::core::{IString, Literal, NString, Text, Vec1};
    use imap_codec::imap_types::fetch::MessageDataItem;
    use imap_codec::imap_types::response::{Data, Response};
    use imap_codec::ResponseCodec;
    use std::num::NonZeroU32;

    let header = format!(
        "From: Alice <{SEEDED_FROM_EMAIL}>\r\nSubject: {}\r\nMessage-ID: <uid{}@example.test>\r\n\r\n",
        message.subject, message.uid
    );
    let items = Vec1::try_from(vec![
        MessageDataItem::Uid(NonZeroU32::new(message.uid).expect("nonzero uid")),
        MessageDataItem::Rfc822Size(512),
        MessageDataItem::Rfc822Header(NString(Some(IString::Literal(
            Literal::try_from(header.into_bytes()).expect("header literal"),
        )))),
        MessageDataItem::GmailMessageId(message.gmail_msgid),
        MessageDataItem::GmailThreadId(message.gmail_thrid),
        MessageDataItem::GmailLabels(vec![
            Text::try_from("\\Inbox").unwrap(),
            Text::try_from("\\Starred").unwrap(),
            Text::try_from("\"Project Alpha\"").unwrap(),
        ]),
    ])
    .expect("at least one fetch item");
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
