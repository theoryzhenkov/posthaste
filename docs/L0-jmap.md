---
scope: L0
summary: "Why JMAP, protocol scope, target server decisions"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: README
dependents:
  - path: docs/L1-jmap
  - path: docs/L0-api
  - path: docs/L0-sync
  - path: docs/L0-accounts
  - path: docs/L0-compose
---

# JMAP domain -- L0

## Why JMAP over IMAP

JMAP (RFC 8620 + RFC 8621 for Mail) replaces IMAP's stateful connection model with stateless HTTP+JSON. The server maintains authoritative state and provides delta sync via `*/changes` endpoints. There is no connection pooling, no IDLE hacks, no UID validity tracking, no client-side MIME parsing. Threading is server-authoritative via `threadId`. Push notifications arrive over EventSource or WebSocket. Batch requests reduce round trips by combining multiple method calls into a single HTTP POST.

This eliminates roughly 60% of traditional mail client complexity. The client does not need to reconstruct conversations from References/In-Reply-To headers, does not need to manage persistent TCP connections, and does not need to handle the dozens of IMAP edge cases around flag sync, expunge notifications, and partial fetch.

## Why Fastmail first

Fastmail's JMAP implementation is the reference. The protocol was designed there by the same people who wrote the RFCs. Targeting a single well-tested server lets us avoid compatibility work during early development.

The client uses only RFC-standard JMAP with no Fastmail-specific extensions. Other compliant servers (Stalwart, Cyrus) can be supported later without protocol-layer changes.

Fastmail's current JMAP access model is OAuth for distributed clients and API tokens for personal/testing use. App-specific passwords are for non-JMAP protocols and must not be treated as the normal JMAP credential path.

## Borrowed component: stalwartlabs/jmap-client

The `jmap-client` crate (Apache-2.0) implements JMAP Core, Mail, WebSocket, and Sieve in Rust. Full async support, EventSource streams. We wrap it rather than implementing JMAP from scratch.

The wrapper adds reconnection logic, credential refresh, and state synchronization orchestration. These concerns sit outside what a protocol-level crate should own.

## Protocol scope

JMAP Mail only for MVP: Email, Mailbox, Thread, Identity, EmailSubmission, SearchSnippet. No JMAP Contacts, Calendars, or Sieve. Blob operations are in scope for attachments and body fetch.

JMAP account IDs are server-assigned IDs from the Session object. They are distinct from PostHaste's local account IDs, even when there is only one configured account. All JMAP method calls use the server account ID; local storage, API routes, and UI state use the local account ID and store the mapping explicitly.

JMAP `threadId` is authoritative for server-side mail threading. PostHaste may derive local conversation projections for UI pagination and multi-source presentation, but it must not reconstruct JMAP conversations from `Message-ID`, `References`, `In-Reply-To`, or normalized subject when a JMAP `threadId` is available.

Core upload/download uses the Session `uploadUrl` and `downloadUrl` templates. RFC 9404 `Blob/upload` and related blob-management methods are optional extension methods, not part of the RFC 8620/8621 baseline.

## Risks

The `jmap-client` crate has low documentation coverage. This is mitigated by reading source directly (clean Rust, maps 1:1 to RFC method names), running integration tests against a Stalwart instance, and the crate being Apache-2.0 licensed -- forkable if abandoned.
