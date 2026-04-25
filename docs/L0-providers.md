---
scope: L0
summary: "Provider driver strategy for JMAP, IMAP/SMTP, and future native APIs"
modified: 2026-04-25
reviewed: 2026-04-25
depends:
  - path: README
  - path: docs/L0-accounts
  - path: docs/L0-sync
  - path: docs/L0-jmap
dependents:
  - path: docs/L1-accounts
  - path: docs/L1-api
  - path: docs/L1-sync
---

# Provider drivers -- L0

## Problem

PostHaste began as a JMAP-first mail client. That is the best protocol shape
for the local replica model: JMAP has stateless HTTP calls, explicit state
strings, server-side thread IDs, and standard push transports. The deployment
problem is provider support. Fastmail and Stalwart are viable JMAP targets, but
many users need Gmail, Outlook/Microsoft 365, iCloud Mail, or a custom
IMAP/SMTP provider.

Traditional providers must be supported without weakening the existing
architecture. The UI should still read from SQLite through the local API, sync
should still reconcile into account-scoped records, and protocol details should
remain in backend adapters.

## Driver model

An account has a provider driver. The driver owns remote protocol behavior and
maps it into the common domain model consumed by sync, storage, automation, and
the API.

Initial drivers:

- `jmap` -- current JMAP Mail and EmailSubmission implementation.
- `imap_smtp` -- generic IMAP for mailbox/message sync and SMTP for
  submission.
- `mock` -- local test/development driver.

Future native drivers may be added when their provider APIs materially improve
correctness or setup:

- `gmail_api` -- Gmail history IDs, labels, threads, and Pub/Sub.
- `graph` -- Microsoft Graph message delta queries and webhook subscriptions.

Native provider APIs are optional optimizations, not a replacement for the
generic IMAP/SMTP path.

## IMAP/SMTP sync strategy

IMAP support is not a fake JMAP layer. It has its own sync state and then emits
the same local records as the JMAP driver.

The IMAP driver stores cursor state per account and mailbox. The minimum state
for a mailbox is:

- mailbox identifier/name selected on the server
- `UIDVALIDITY`
- highest seen UID or equivalent scan watermark
- `HIGHESTMODSEQ` when the server supports CONDSTORE/QRESYNC

The live implementation lives in the `posthaste-imap` adapter crate. Its first
runtime boundary performs connection, authentication, capability discovery, and
mailbox listing. Discovery results are synced as an authoritative mailbox
snapshot, and selectable mailboxes are fetched as an authoritative full message
snapshot. Lazy body fetches use stored IMAP locations and `BODY.PEEK[]`.
Implemented mutations use conservative IMAP command paths, and remaining
unsupported command surfaces are rejected explicitly.

Mailbox message sync starts by examining the mailbox and mapping SELECT/EXAMINE
state into `ImapSelectedMailbox`. `UIDVALIDITY` is required. `UIDNEXT` is used
when present. `HIGHESTMODSEQ` remains optional until the protocol client
exposes CONDSTORE/QRESYNC select metadata.

The first message mapping path consumes RFC 822 headers, not full message
bodies. Initial or invalidated snapshots use `UID SEARCH ALL` followed by
chunked `UID FETCH` for `FLAGS`, `RFC822.HEADER`, `RFC822.SIZE`, `UID`, and,
when CONDSTORE/QRESYNC is advertised, `MODSEQ`. Subsequent syncs with valid
per-mailbox `UIDVALIDITY` state reconcile current mailbox UID descriptors
against stored `ImapMessageLocation` rows, upsert current metadata, and delete
local messages whose stored mailbox UID is no longer returned by the server.
This follows RFC 4549's safe disconnected-client baseline until QRESYNC
`VANISHED` handling is implemented. Body text, HTML, raw MIME, and attachment
metadata are fetched lazily when a message is opened.

The driver prefers IMAP extensions when advertised:

- SPECIAL-USE for mailbox roles
- IDLE for low-latency change hints
- CONDSTORE/QRESYNC for efficient flag and expunge reconciliation
- MOVE and UIDPLUS for better mutation reconciliation

Every extension has a correctness-preserving fallback. IDLE is only a hint; the
periodic poll remains authoritative. If delta state cannot be trusted, the
driver performs a full mailbox snapshot and lets the store prune stale local
rows through the existing `replace_all_*` reconciliation contract.

The IMAP sync planner chooses the strongest safe mailbox strategy from server
capabilities and stored cursor state:

- QRESYNC delta when `QRESYNC`, `ENABLE`, stored MODSEQ, and selected
  `HIGHESTMODSEQ` are all present.
- CONDSTORE flag delta when MODSEQ state exists but QRESYNC is unavailable.
- UID range fetch inside the same `UIDVALIDITY` epoch when only UID state is
  available.
- Full authoritative snapshot on first sync, `UIDVALIDITY` changes, or missing
  watermarks.

Mutation planning follows the same rule: use the strongest server extension
when available, but resync after commands whose result cannot be mapped
authoritatively. Moves prefer `UID MOVE` plus UIDPLUS `COPYUID`; fall back to
`UID MOVE` plus destination resync; and finally fall back to `UID COPY`,
`\Deleted`, and resync when MOVE is not available.

SMTP sends do not return a synced message object. After a successful send, the
runtime triggers sync and reconciles Sent mail from the provider. The IMAP/SMTP
driver uses provider policy for Sent copies: Gmail and Outlook/Hotmail are
treated as provider-managed to avoid duplicate Sent messages, while generic and
iCloud IMAP/SMTP accounts append the exact submitted RFC 5322 message to the
discovered `\Sent` mailbox with `\Seen` before the follow-up sync.

## Implementation references

IMAP behavior must be traceable to protocol specifications or existing client
library behavior. When changing the IMAP adapter, update this table if a new
source informs the implementation.

| Posthaste area | Local implementation | Reference source |
|---|---|---|
| IMAP connection/auth/discovery | `posthaste-imap::discover_imap_account`, `LiveImapSmtpGateway::connect` | `imap-client` 0.3.0 constructors/auth/capability/list wrappers: <https://docs.rs/crate/imap-client/0.3.0/source/src/client/tokio.rs> |
| Capability normalization | `posthaste_domain::ImapCapabilities`, `normalize_imap_capabilities` | RFC 9051 capabilities and IMAP4rev2 baseline: <https://www.rfc-editor.org/rfc/rfc9051.html>; `imap-types` capability variants: <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/response.rs> |
| Mailbox LIST and roles | `map_imap_mailbox`, `imap_special_use_role` | RFC 6154 SPECIAL-USE attributes: <https://www.rfc-editor.org/rfc/rfc6154.html>; `imap-client` LIST task: <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/list.rs> |
| SELECT/EXAMINE state | `examine_imap_mailbox`, `ExamineStateTask`, `selected_mailbox_from_examine`, `ImapSelectedMailbox` | RFC 9051 SELECT/EXAMINE response codes including `UIDVALIDITY` and `UIDNEXT`: <https://www.rfc-editor.org/rfc/rfc9051.html>; RFC 7162 requires CONDSTORE servers to return `HIGHESTMODSEQ` for successful SELECT/EXAMINE unless `NOMODSEQ` applies: <https://datatracker.ietf.org/doc/html/rfc7162>; `imap-client` SELECT task provides the base response handling but does not expose `HIGHESTMODSEQ`, so Posthaste adds a local EXAMINE task over `imap-types` response codes: <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/select.rs>, <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/response.rs> |
| Mailbox sync planning and baseline delta reconciliation | `plan_imap_mailbox_sync`, `LiveImapSmtpGateway::sync`, `imap_delta_sync_batch` | RFC 4549 disconnected-client synchronization uses `UIDVALIDITY`, new UID discovery, and current UID/FLAGS descriptor comparison to remove local cache entries no longer returned by the server: <https://datatracker.ietf.org/doc/html/rfc4549>; RFC 9051 UID semantics and FETCH/SEARCH commands: <https://www.rfc-editor.org/rfc/rfc9051.html>; RFC 7162 CONDSTORE/QRESYNC `HIGHESTMODSEQ`, `CHANGEDSINCE`, `ENABLE QRESYNC`, and `VANISHED (EARLIER)` define the stronger MODSEQ delta path and the rule that `VANISHED` is only allowed on `UID FETCH` with `CHANGEDSINCE`: <https://datatracker.ietf.org/doc/html/rfc7162>; ImapFlow exposes QRESYNC/VANISHED and `changedSince`/modseq behavior for this stronger path: <https://imapflow.com/docs/>, <https://imapflow.com/docs/guides/fetching-messages>, <https://imapflow.com/docs/api/imapflow-client/>; node-imap exposes `changedsince`, `modseq`, and `highestmodseq` for CONDSTORE-style sync: <https://github.com/mscdex/node-imap> |
| Message identity and UID reuse | `imap_message_id`, `ImapMessageLocation` | RFC 9051 UID and UIDVALIDITY semantics: <https://www.rfc-editor.org/rfc/rfc9051.html> |
| Gmail identity/labels | `ImapProviderFeatures`, `gmail_message_id`, `gmail_thread_id` | Gmail IMAP extensions `X-GM-EXT-1`, `X-GM-MSGID`, `X-GM-THRID`, `X-GM-LABELS`: <https://developers.google.com/workspace/gmail/imap/imap-extensions> |
| Header-to-message mapping | `imap_header_message_record` | `mail-parser` message/header/body API: <https://docs.rs/mail-parser/0.11.2/mail_parser/>; source: <https://docs.rs/crate/mail-parser/0.11.2/source/src/core/message.rs> |
| Lazy body fetch/parsing | `fetch_message_body_by_location`, `fetched_body_from_items`, `imap_body_from_raw_mime` | RFC 9051 `BODY.PEEK[]` does not implicitly set `\Seen` and UIDs are valid only in a `UIDVALIDITY` epoch: <https://www.rfc-editor.org/rfc/rfc9051.html>; `imap-client` `uid_fetch_first`/FETCH task behavior: <https://docs.rs/crate/imap-client/0.3.0/source/src/client/tokio.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/fetch.rs>; `mail-parser` body and attachment APIs: <https://docs.rs/mail-parser/0.11.2/mail_parser/struct.Message.html>, <https://docs.rs/crate/mail-parser/0.11.2/source/src/core/message.rs> |
| Attachment blob download | `LiveImapSmtpGateway::download_blob`, `fetch_raw_message_by_location`, `imap_attachment_bytes_from_raw_mime`, `parse_imap_attachment_blob_id` | RFC 9051 `BODY.PEEK[]` fetches message bytes without the `\Seen` side effect and the stored UID is valid only within the selected mailbox `UIDVALIDITY` epoch: <https://www.rfc-editor.org/rfc/rfc9051.html>; `imap-client` `uid_fetch_first` uses UID FETCH and returns FETCH items from server responses: <https://docs.rs/crate/imap-client/0.3.0/source/src/client/tokio.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/fetch.rs>; `mail-parser` resolves attachment ordinals and decoded part contents: <https://docs.rs/crate/mail-parser/0.11.2/source/src/core/message.rs>, <https://docs.rs/crate/mail-parser/0.11.2/source/src/core/header.rs> |
| FETCH item extraction | `fetch_mailbox_header_records`, `fetch_mailbox_changed_since_snapshot`, `ChangedSinceFetchTask`, `fetched_header_from_items` | RFC 9051 SEARCH/UID/FETCH data items (`FLAGS`, `RFC822.HEADER`, `RFC822.SIZE`, `UID`): <https://www.rfc-editor.org/rfc/rfc9051.html>; RFC 7162 `MODSEQ`, `CHANGEDSINCE`, and `VANISHED` as CONDSTORE/QRESYNC FETCH items and modifiers: <https://datatracker.ietf.org/doc/html/rfc7162>; `imap-client` UID SEARCH/UID FETCH wrappers and FETCH task sequence-number collection, noting the stock task leaves `modifiers` empty so Posthaste adds a local task for `CHANGEDSINCE`: <https://docs.rs/crate/imap-client/0.3.0/source/src/client/tokio.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/fetch.rs>; `imap-types` command/fetch/sequence-set models provide typed `FetchModifier::ChangedSince`, `FetchModifier::Vanished`, `Data::Vanished`, and bounded UID sequence expansion: <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/command.rs>, <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/fetch.rs>, <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/response.rs>, <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/sequence.rs> |
| Keyword mutation | `LiveImapSmtpGateway::set_keywords`, `apply_imap_keyword_delta_by_location`, `imap_flags_for_keywords` | RFC 9051 `UID STORE`, `+FLAGS`, `-FLAGS`, `PERMANENTFLAGS`, and nonexistent UID no-op behavior: <https://www.rfc-editor.org/rfc/rfc9051.html>; `imap-client` `uid_store` and STORE task command construction: <https://docs.rs/crate/imap-client/0.3.0/source/src/client/tokio.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/store.rs>; `imap-types` flag and store models: <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/flag.rs> |
| Mailbox replacement and delete marking | `LiveImapSmtpGateway::replace_mailboxes`, `LiveImapSmtpGateway::destroy_message`, `copy_imap_message_to_mailbox_by_location`, `mark_imap_message_deleted_by_location`, `expunge_imap_message_by_location` | RFC 9051 COPY/MOVE behavior, `COPYUID`, `UID STORE`, nonexistent UID no-op behavior, and `\Deleted` semantics: <https://www.rfc-editor.org/rfc/rfc9051.html>; RFC 6851 explains why COPY/STORE/EXPUNGE fallback has side effects and why MOVE is preferred: <https://www.rfc-editor.org/rfc/rfc6851.html>; RFC 4315 UIDPLUS `UID EXPUNGE` avoids expunging other clients' `\Deleted` messages: <https://datatracker.ietf.org/doc/html/rfc4315>; `imap-client` COPY/MOVE/STORE wrappers expose command success but not COPYUID output, and the local adapter uses `imap-client`'s task API plus `imap-types` `ExpungeUid` command model for UID EXPUNGE: <https://docs.rs/crate/imap-client/0.3.0/source/src/client/tokio.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/copy.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/move.rs>, <https://docs.rs/crate/imap-types/2.0.0-alpha.6/source/src/command.rs> |
| Reply/forward context | `LiveImapSmtpGateway::fetch_reply_context`, `fetch_imap_reply_context_by_location`, `imap_reply_context_from_raw_mime` | RFC 9051 stored message identity and `BODY.PEEK[]` raw message fetch semantics: <https://www.rfc-editor.org/rfc/rfc9051.html>; `mail-parser` address, subject, message-id, references, and text body APIs: <https://docs.rs/mail-parser/0.11.2/mail_parser/struct.Message.html>, <https://docs.rs/mail-parser/0.11.2/mail_parser/struct.Addr.html>, <https://docs.rs/crate/mail-parser/0.11.2/source/src/core/message.rs>, <https://docs.rs/crate/mail-parser/0.11.2/source/src/core/address.rs> |
| SMTP message submission and Sent copy policy | `LiveImapSmtpGateway::send_message`, `build_smtp_message`, `submit_smtp_message`, `append_smtp_sent_copy`, `smtp_sent_copy_strategy` | RFC 6409 submission model: <https://www.rfc-editor.org/rfc/rfc6409.html>; RFC 5322 message headers, destination fields, and identification fields: <https://www.rfc-editor.org/rfc/rfc5322.html>; RFC 9051 `APPEND` stores a literal message in a mailbox with optional flags: <https://www.rfc-editor.org/rfc/rfc9051.html>; RFC 4315 `APPENDUID` allows the server to return the assigned UID after APPEND: <https://datatracker.ietf.org/doc/html/rfc4315>; `lettre` SMTP transport, async Tokio executor, typed message builder, Bcc envelope behavior, threading headers, and XOAUTH2 mechanism support: <https://docs.rs/lettre/latest/lettre/>, <https://docs.rs/lettre/latest/src/lettre/transport/smtp/async_transport.rs.html>, <https://docs.rs/lettre/latest/lettre/message/struct.MessageBuilder.html>, <https://docs.rs/lettre/latest/lettre/message/header/index.html>, <https://docs.rs/lettre/latest/lettre/transport/smtp/authentication/enum.Mechanism.html>; `imap-client` APPEND/APPENDUID wrappers and fallback behavior: <https://docs.rs/crate/imap-client/0.3.0/source/src/client/tokio.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/append.rs>, <https://docs.rs/crate/imap-client/0.3.0/source/src/tasks/tasks/appenduid.rs>; Gmail recommends not saving SMTP sent copies because it automatically copies sent messages to Gmail/Sent: <https://support.google.com/mail/answer/78892>; Microsoft documents duplicate Sent items when a provider creates a server copy and the client uploads another one: <https://support.microsoft.com/en-us/office/imap-sent-messages-in-outlook-for-windows-are-duplicated-and-unread-in-sent-items-folder-f9cb7d98-b6be-4740-89f8-3b7c2277a615>; iCloud documents standard IMAP/SMTP settings but no provider-managed Sent behavior, so Posthaste uses client APPEND for iCloud: <https://support.apple.com/en-us/102525>; provider XOAUTH2 references: <https://developers.google.com/workspace/gmail/imap/xoauth2-protocol>, <https://learn.microsoft.com/en-us/exchange/client-developer/legacy-protocols/how-to-authenticate-an-imap-pop-smtp-application-by-using-oauth> |
| Full IMAP metadata snapshot | `LiveImapSmtpGateway::sync`, `imap_full_sync_batch` | RFC 9051 UID/sequence-number and EXPUNGE behavior: <https://www.rfc-editor.org/rfc/rfc9051.html>; local store snapshot replacement contract: `docs/L1-sync` |
| Move/copy mutation planning | `plan_imap_move` | RFC 6851 MOVE: <https://datatracker.ietf.org/doc/html/rfc6851>; RFC 4315 UIDPLUS `COPYUID`/`APPENDUID`: <https://datatracker.ietf.org/doc/html/rfc4315> |

## Identity and threading

JMAP messages use the server `threadId` as authoritative. IMAP messages do not
have a portable thread identifier, so the IMAP driver derives conversation IDs
from RFC 5322 headers (`Message-ID`, `References`, `In-Reply-To`) and a stable
fallback for malformed messages. Provider-specific stable IDs may improve
deduplication when available, for example Gmail's `X-GM-MSGID`.

When a server advertises Gmail's `X-GM-EXT-1` capability, the IMAP driver uses
`X-GM-MSGID` as the message identity, `X-GM-THRID` as the provider thread
identity, and `X-GM-LABELS` as the label source. This avoids duplicating the
same Gmail message when it appears through multiple labels exposed as IMAP
mailboxes. Generic IMAP accounts continue to use `(mailbox, UIDVALIDITY, UID)`
for message identity and RFC 5322 headers for conversation projection.

Message IDs stored in PostHaste remain opaque and driver-owned. IMAP IDs should
be stable across sessions and include enough server state to avoid UID reuse
bugs after `UIDVALIDITY` changes.

IMAP command locations are persisted separately from message identity. A local
message may have one stable identity and multiple mailbox UID locations, which
is required for Gmail label deduplication and for generic IMAP commands such as
`UID STORE`, `UID MOVE`, and lazy body fetches.

## Authentication

The account model must distinguish protocol settings from secret material.
Secrets remain outside TOML and are referenced by `SecretRef`.

Expected provider behavior:

- Gmail IMAP/SMTP uses OAuth XOAUTH2 for distributed clients. App passwords are
  a possible personal-account fallback when available.
- Microsoft 365 and Outlook IMAP/SMTP use OAuth XOAUTH2.
- iCloud Mail uses IMAP/SMTP with an app-specific password.
- Custom providers may use username/password, app password, or OAuth depending
  on server support.

OAuth token refresh is part of the account runtime, not the UI data model.

## JMAPACCESS

If an IMAP server advertises JMAPACCESS and returns a JMAP Session URL for the
same message store, PostHaste should prefer the JMAP driver for that account.
This preserves the better sync model while keeping account setup compatible
with servers that expose both protocols.

## Invariants

- Provider drivers never bypass the local SQLite replica for UI reads.
- Protocol-specific state is hidden behind driver-owned cursors and records.
- Account config supports provider selection without exposing backend-only IDs
  as user-facing setup fields.
- IMAP/SMTP support must not add JMAP-specific assumptions to shared domain
  types.
- Full provider snapshots are authoritative when delta state is missing or
  invalid.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| driver-explicit | MUST | Each account declares an explicit provider driver |
| ui-uses-replica | MUST | Provider drivers feed the local SQLite replica; the UI never reads remote providers directly |
| imap-cursors-per-mailbox | MUST | IMAP sync state is tracked per mailbox, not only per account |
| imap-discovery-runtime | MUST | IMAP runtime setup connects, authenticates, discovers capabilities, and lists mailboxes before sync |
| imap-delta-fallback | MUST | IMAP sync falls back to full authoritative snapshots when delta state is unavailable or invalid |
| imap-plan-explicit | MUST | IMAP mailbox sync mode is selected from explicit capabilities and stored state |
| imap-mutation-plan-explicit | MUST | IMAP mutation strategy is selected from explicit capabilities and schedules resync when response state is insufficient |
| gmail-extension-identity | SHOULD | Gmail IMAP accounts use X-GM-MSGID, X-GM-THRID, and X-GM-LABELS when X-GM-EXT-1 is advertised |
| imap-location-map | MUST | IMAP command locations are persisted separately from local message IDs |
| smtp-send-sync | MUST | SMTP send success triggers provider sync rather than inventing a local sent message as authoritative |
| jmapaccess-preferred | SHOULD | IMAP setup prefers JMAP when the server advertises JMAPACCESS for the same message store |
