use super::*;

use std::num::NonZeroU32;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::fetch::fetch_selected_mailbox_headers;
use crate::mailbox::examine_selected_mailbox;

#[test]
fn normalizes_capability_tokens_case_insensitively() {
    let capabilities = normalize_imap_capabilities(["imap4rev1", "idle", "x-gm-ext-1", "uidplus"]);

    assert!(capabilities.supports_idle());
    assert!(capabilities.supports_uidplus());
    assert!(capabilities.supports_gmail_extensions());
}

#[test]
fn maps_special_use_mailbox_roles() {
    let mailbox = map_imap_mailbox("Sent Items", ["\\HasNoChildren", "\\Sent"]);

    assert_eq!(
        mailbox.id,
        MailboxId::from("imap:mailbox:53656e74204974656d73")
    );
    assert_eq!(mailbox.role, Some(MailboxRole::Sent.as_str()));
    assert!(mailbox.selectable);
}

#[test]
fn maps_noselect_mailboxes_without_role_loss() {
    let mailbox = map_imap_mailbox("[Gmail]", ["\\Noselect"]);

    assert_eq!(mailbox.role, None);
    assert!(!mailbox.selectable);
}

#[test]
fn maps_gmail_role_aliases_only_with_gmail_provider_policy() {
    let generic = map_imap_mailbox("[Gmail]/All Mail", ["\\All", "\\HasNoChildren"]);
    let gmail = map_imap_mailbox_with_provider(
        ProviderProfile::from_kind(posthaste_domain_model::ProviderKind::Gmail),
        "[Gmail]/All Mail",
        ["\\All", "\\HasNoChildren"],
    );

    assert_eq!(generic.role, None);
    assert_eq!(gmail.role, Some(MailboxRole::Archive.as_str()));
}

// --- mock Gmail IMAP server (increment 1: capability negotiation + LIST) ---
//
// A hand-rolled TCP IMAP server that advertises X-GM-EXT-1 + CONDSTORE +
// QRESYNC, handles the discovery command set (greeting + CAPABILITY +
// AUTHENTICATE PLAIN + LIST), and serves Gmail-shaped mailboxes. The real
// `imap-client` connects to it over TCP, so this exercises the actual
// capability negotiation and mailbox-list decoding end-to-end. (Increment 2
// will add SELECT + UID FETCH with X-GM-* via `imap-codec` encode.)

/// Capabilities the mock advertises (a Gmail-shaped server).
const MOCK_GMAIL_CAPS: &str = "IMAP4rev1 CONDSTORE QRESYNC X-GM-EXT-1 IDLE UIDPLUS ENABLE";

// spec: docs/testing/L1#provider-observation-matrix
#[tokio::test]
async fn mock_gmail_imap_negotiates_condstore_qresync_and_gmail_extensions() {
    let addr = spawn_mock_gmail_imap().await;
    let discovered = discover_imap_account(&ImapConnectionConfig {
        host: "127.0.0.1".to_string(),
        port: addr.port(),
        security: TransportSecurity::Plain,
        username: "user@gmail.example".to_string(),
        secret: "secret".to_string(),
        auth: ProviderAuthKind::Password,
    })
    .await
    .expect("discover imap account against mock");

    assert!(
        discovered.capabilities.supports_gmail_extensions(),
        "X-GM-EXT-1 should be advertised and picked up"
    );
    assert!(
        discovered.capabilities.supports_condstore(),
        "CONDSTORE should be advertised and picked up"
    );
    assert!(
        discovered.capabilities.supports_qresync(),
        "QRESYNC should be advertised and picked up"
    );
    assert_eq!(
        discovered.provider_profile().kind(),
        posthaste_domain_model::ProviderKind::Gmail,
        "X-GM-EXT-1 should select the Gmail provider profile"
    );

    let by_name: std::collections::HashMap<&str, &DiscoveredImapMailbox> = discovered
        .mailboxes
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect();
    let inbox = by_name.get("INBOX").expect("INBOX listed");
    assert_eq!(inbox.role, Some(MailboxRole::Inbox.as_str()));
    let all_mail = by_name
        .get("[Gmail]/All Mail")
        .expect("[Gmail]/All Mail listed");
    assert_eq!(all_mail.role, Some(MailboxRole::Archive.as_str()));
    assert!(by_name.contains_key("[Gmail]/Starred"));
    assert!(by_name.contains_key("[Gmail]/Sent Mail"));
}

/// Spawn a minimal mock Gmail IMAP server on a free loopback port. Handles the
/// discovery command set and advertises X-GM-EXT-1 + CONDSTORE + QRESYNC.
async fn spawn_mock_gmail_imap() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock imap");
    let addr = listener.local_addr().expect("mock imap addr");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept mock imap client");
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        writer
            .write_all(
                format!("* OK [CAPABILITY {MOCK_GMAIL_CAPS}] mock-gmail ready\r\n").as_bytes(),
            )
            .await
            .unwrap();

        loop {
            line.clear();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let cmd = line.trim_end_matches(['\r', '\n']).to_string();
            let mut parts = cmd.split_whitespace();
            let tag = parts.next().unwrap_or("A1").to_string();
            let verb = parts.next().unwrap_or("").to_ascii_uppercase();

            match verb.as_str() {
                "CAPABILITY" => {
                    writer
                        .write_all(format!("* CAPABILITY {MOCK_GMAIL_CAPS}\r\n").as_bytes())
                        .await
                        .unwrap();
                    writer
                        .write_all(format!("{tag} OK CAPABILITY completed\r\n").as_bytes())
                        .await
                        .unwrap();
                }
                "AUTHENTICATE" => {
                    // SASL (PLAIN/XOAUTH2): accept any mechanism. If the client
                    // gave an initial response inline, succeed directly;
                    // otherwise issue a continuation and read the response.
                    let _mechanism = parts.next();
                    let inline = parts.next().is_some();
                    if !inline {
                        writer.write_all(b"+ \r\n").await.unwrap();
                        let mut resp = String::new();
                        reader.read_line(&mut resp).await.unwrap();
                    }
                    writer
                        .write_all(format!("{tag} OK AUTHENTICATE completed\r\n").as_bytes())
                        .await
                        .unwrap();
                }
                "LOGIN" => {
                    writer
                        .write_all(format!("{tag} OK LOGIN completed\r\n").as_bytes())
                        .await
                        .unwrap();
                }
                "LIST" => {
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
                        writer
                            .write_all(format!("* LIST ({attrs}) \"/\" \"{name}\"\r\n").as_bytes())
                            .await
                            .unwrap();
                    }
                    writer
                        .write_all(format!("{tag} OK LIST completed\r\n").as_bytes())
                        .await
                        .unwrap();
                }
                "SELECT" | "EXAMINE" => {
                    // A Gmail INBOX with one message; advertise CONDSTORE/QRESYNC
                    // state (UIDVALIDITY + HIGHESTMODSEQ) so delta sync can plan.
                    writer
                        .write_all(b"* FLAGS (\\Seen \\Flagged)\r\n")
                        .await
                        .unwrap();
                    writer
                        .write_all(b"* OK [PERMANENTFLAGS (\\Seen \\Flagged \\*)]\r\n")
                        .await
                        .unwrap();
                    writer.write_all(b"* 1 EXISTS\r\n").await.unwrap();
                    writer.write_all(b"* 0 RECENT\r\n").await.unwrap();
                    writer.write_all(b"* OK [UIDVALIDITY 7]\r\n").await.unwrap();
                    writer.write_all(b"* OK [UIDNEXT 2]\r\n").await.unwrap();
                    writer
                        .write_all(b"* OK [HIGHESTMODSEQ 100]\r\n")
                        .await
                        .unwrap();
                    writer
                        .write_all(format!("{tag} OK [READ-WRITE] {verb} completed\r\n").as_bytes())
                        .await
                        .unwrap();
                }
                "UID" => {
                    let sub = parts.next().unwrap_or("").to_ascii_uppercase();
                    if sub == "FETCH" {
                        writer
                            .write_all(&encode_gmail_fetch_response())
                            .await
                            .unwrap();
                        writer
                            .write_all(format!("{tag} OK FETCH completed\r\n").as_bytes())
                            .await
                            .unwrap();
                    } else {
                        writer
                            .write_all(format!("{tag} BAD unsupported UID {sub}\r\n").as_bytes())
                            .await
                            .unwrap();
                    }
                }
                "NOOP" | "ENABLE" | "ID" => {
                    writer
                        .write_all(format!("{tag} OK {verb} completed\r\n").as_bytes())
                        .await
                        .unwrap();
                }
                "LOGOUT" => {
                    writer
                        .write_all(
                            format!(
                                "* BYE mock-gmail logging out\r\n{tag} OK LOGOUT completed\r\n"
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                    break;
                }
                _ => {
                    writer
                        .write_all(format!("{tag} BAD unknown command\r\n").as_bytes())
                        .await
                        .unwrap();
                }
            }
        }
    });
    addr
}

/// Encode one Gmail-shaped FETCH response via `imap-codec`: UID + MODSEQ +
/// RFC822.SIZE + RFC822.HEADER (literal) + X-GM-MSGID + X-GM-THRID +
/// X-GM-LABELS. The real `imap-client` decodes this and posthaste's parse
/// extracts the Gmail labels. (BodyStructure/Flags omitted -- the parse
/// defaults them.) MODSEQ is now included: the fork encodes it as the
/// parenthesized RFC 7162 form `MODSEQ (<v>)` that its decoder expects, so it
/// round-trips through the real client. This is where the forked `imap-codec`
/// earns its keep: the literal + label-list encoding is error-prone by hand.
fn encode_gmail_fetch_response() -> Vec<u8> {
    use imap_client::imap_types::core::{IString, Literal, NString, Text, Vec1};
    use imap_client::imap_types::fetch::MessageDataItem;
    use imap_client::imap_types::response::{Data, Response};
    use imap_codec::encode::Encoder;
    use imap_codec::ResponseCodec;
    use std::num::{NonZeroU32, NonZeroU64};

    let header_bytes =
        b"From: Alice <alice@example.test>\r\nSubject: Labels\r\nMessage-ID: <gmail-labels@example.test>\r\n\r\n";
    // Note: the fork's `Text` encode writes raw bytes with no quoting, so a
    // label containing a space must be pre-quoted here (system labels stay
    // bare, matching real Gmail). See `EncodeIntoContext for Text`.
    let items = Vec1::try_from(vec![
        MessageDataItem::Uid(NonZeroU32::new(1).unwrap()),
        MessageDataItem::ModSeq(NonZeroU64::new(320162350).unwrap()),
        MessageDataItem::Rfc822Size(512),
        MessageDataItem::Rfc822Header(NString(Some(IString::Literal(
            Literal::try_from(header_bytes.to_vec()).expect("header literal"),
        )))),
        MessageDataItem::GmailMessageId(1278455344230334865),
        MessageDataItem::GmailThreadId(1266894439832287888),
        MessageDataItem::GmailLabels(vec![
            Text::try_from("\\Inbox").unwrap(),
            Text::try_from("\\Starred").unwrap(),
            // Pre-quoted: the bytes include the `"` so the encoded form is a
            // valid quoted astring.
            Text::try_from("\"Project Alpha\"").unwrap(),
        ]),
    ])
    .expect("at least one fetch item");
    let response = Response::Data(Data::Fetch {
        seq: NonZeroU32::new(1).unwrap(),
        items,
    });
    ResponseCodec::default().encode(&response).dump()
}

// spec: docs/testing/L1#provider-observation-matrix
#[tokio::test]
async fn mock_gmail_imap_uid_fetch_decodes_x_gm_labels_through_real_client() {
    let addr = spawn_mock_gmail_imap().await;
    let config = ImapConnectionConfig {
        host: "127.0.0.1".to_string(),
        port: addr.port(),
        security: TransportSecurity::Plain,
        username: "user@gmail.example".to_string(),
        secret: "secret".to_string(),
        auth: ProviderAuthKind::Password,
    };

    let mut client = connect_authenticated_client(&config)
        .await
        .expect("connect + authenticate");
    client.refresh_capabilities().await.expect("refresh caps");
    let selected = examine_selected_mailbox(&mut client, "INBOX")
        .await
        .expect("examine INBOX");
    let headers = fetch_selected_mailbox_headers(
        &mut client,
        &selected,
        &[NonZeroU32::new(1).expect("uid")],
        true,
        true,
        "2026-06-27T00:00:00Z".to_string(),
    )
    .await
    .expect("fetch headers");

    let header = headers.first().expect("one fetched header");
    // X-GM-LABELS round-tripped: the real imap-client decoded the FETCH
    // response (literal header + quoted multi-word label) and posthaste's
    // parse extracted the Gmail labels onto the mapped header.
    let labels: Vec<&str> = header
        .gmail_labels
        .as_ref()
        .map(|labels| {
            labels
                .iter()
                .map(posthaste_domain_model::GmailLabel::as_str)
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(labels, ["\\Inbox", "\\Starred", "Project Alpha"]);
}
