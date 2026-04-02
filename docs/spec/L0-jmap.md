---
scope: L0
summary: "Why JMAP, protocol scope, target server decisions"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: README
dependents:
  - path: spec/L1-jmap
  - path: spec/L0-api
  - path: spec/L0-sync
  - path: spec/L0-accounts
  - path: spec/L0-compose
---

# JMAP domain -- L0

## Why JMAP over IMAP

JMAP (RFC 8620 + RFC 8621 for Mail) replaces IMAP's stateful connection model with stateless HTTP+JSON. The server maintains authoritative state and provides delta sync via `*/changes` endpoints. There is no connection pooling, no IDLE hacks, no UID validity tracking, no client-side MIME parsing. Threading is server-authoritative via `threadId`. Push notifications arrive over EventSource or WebSocket. Batch requests reduce round trips by combining multiple method calls into a single HTTP POST.

This eliminates roughly 60% of traditional mail client complexity. The client does not need to reconstruct conversations from References/In-Reply-To headers, does not need to manage persistent TCP connections, and does not need to handle the dozens of IMAP edge cases around flag sync, expunge notifications, and partial fetch.

## Why Fastmail first

Fastmail's JMAP implementation is the reference. The protocol was designed there by the same people who wrote the RFCs. Targeting a single well-tested server lets us avoid compatibility work during early development.

The client uses only RFC-standard JMAP with no Fastmail-specific extensions. Other compliant servers (Stalwart, Cyrus) can be supported later without protocol-layer changes.

## Borrowed component: stalwartlabs/jmap-client

The `jmap-client` crate (Apache-2.0) implements JMAP Core, Mail, WebSocket, and Sieve in Rust. Full async support, EventSource streams. We wrap it rather than implementing JMAP from scratch.

The wrapper adds reconnection logic, credential refresh, and state synchronization orchestration. These concerns sit outside what a protocol-level crate should own.

## Protocol scope

JMAP Mail only for MVP: Email, Mailbox, Thread, Identity, EmailSubmission, SearchSnippet. No JMAP Contacts, Calendars, or Sieve. Blob operations are in scope for attachments and body fetch.

## Risks

The `jmap-client` crate has low documentation coverage. This is mitigated by reading source directly (clean Rust, maps 1:1 to RFC method names), running integration tests against a Stalwart instance, and the crate being Apache-2.0 licensed -- forkable if abandoned.
