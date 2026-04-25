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

The driver prefers IMAP extensions when advertised:

- SPECIAL-USE for mailbox roles
- IDLE for low-latency change hints
- CONDSTORE/QRESYNC for efficient flag and expunge reconciliation
- MOVE and UIDPLUS for better mutation reconciliation

Every extension has a correctness-preserving fallback. IDLE is only a hint; the
periodic poll remains authoritative. If delta state cannot be trusted, the
driver performs a full mailbox snapshot and lets the store prune stale local
rows through the existing `replace_all_*` reconciliation contract.

SMTP sends do not return a synced message object. After a successful send, the
runtime triggers sync and reconciles Sent mail from the provider.

## Identity and threading

JMAP messages use the server `threadId` as authoritative. IMAP messages do not
have a portable thread identifier, so the IMAP driver derives conversation IDs
from RFC 5322 headers (`Message-ID`, `References`, `In-Reply-To`) and a stable
fallback for malformed messages. Provider-specific stable IDs may improve
deduplication when available, for example Gmail's `X-GM-MSGID`.

Message IDs stored in PostHaste remain opaque and driver-owned. IMAP IDs should
be stable across sessions and include enough server state to avoid UID reuse
bugs after `UIDVALIDITY` changes.

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
| imap-delta-fallback | MUST | IMAP sync falls back to full authoritative snapshots when delta state is unavailable or invalid |
| smtp-send-sync | MUST | SMTP send success triggers provider sync rather than inventing a local sent message as authoritative |
| jmapaccess-preferred | SHOULD | IMAP setup prefers JMAP when the server advertises JMAPACCESS for the same message store |
