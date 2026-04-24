---
scope: L1
summary: "JMAP session, method calls, type system, push, error model"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: docs/L0-jmap
dependents:
  - path: docs/L1-sync
  - path: docs/L1-compose
  - path: docs/L2-transport
---

# JMAP domain -- L1

## Session

Discovery starts from a configured JMAP Session URL or provider origin. Generic providers may support `GET /.well-known/jmap`; Fastmail documents `https://api.fastmail.com/jmap/session` as the Session resource. The Session object contains `apiUrl`, `downloadUrl`, `uploadUrl`, `eventSourceUrl`, `accounts`, `primaryAccounts`, `capabilities`, and a Session `state` string.

The client caches the Session per local account, including the server-assigned account ID selected from `primaryAccounts`. The local `AccountId` is never sent in JMAP method calls unless it happens to equal the server account ID. A 401 response refreshes credentials and Session state. A missing required capability triggers Session refresh before the operation is rejected.

## Method calls

All JMAP communication uses `Request` objects containing one or more method calls. Independent calls are batched into a single HTTP POST when possible. This is JMAP's key performance advantage over IMAP's one-command-at-a-time model.

Back-references (`#ref`) allow one call's result to feed into the next within the same request. For example, an `Email/query` result can feed email IDs directly into an `Email/get` call without a round trip.

Requests include the RFC capability URIs required by the called methods in `using`. Mail read/manage operations require `urn:ietf:params:jmap:mail`; submission operations require the mail and submission capabilities. The client preserves method-level error codes because an HTTP 200 response can contain failed method responses.

## Core types

These come from `jmap-client` and are not reimplemented:

- `Id` -- opaque server-assigned string identifier
- `Email` -- message metadata: `id`, `threadId`, `mailboxIds`, `keywords` (flags), `from`, `to`, `subject`, `receivedAt`, `hasAttachment`, `preview`, `bodyStructure`
- `Mailbox` -- folder or label: `id`, `name`, `parentId`, `role` (inbox, drafts, sent, trash, etc.), `totalEmails`, `unreadEmails`
- `Thread` -- server-authoritative thread: `id`, `emailIds` (ordered)
- `Identity` -- sender identity: `id`, `name`, `email`, `replyTo`, `bcc`
- `EmailSubmission` -- send operation: `identityId`, `emailId`, `envelope`

## Methods used

- `Email/get`, `Email/query`, `Email/queryChanges`, `Email/changes`, `Email/set`
- `Mailbox/get`, `Mailbox/changes`, `Mailbox/set`
- `Thread/get`, `Thread/changes`
- `Identity/get`
- `EmailSubmission/set`
- `SearchSnippet/get` (for search result highlighting)
- Session `uploadUrl` and `downloadUrl` HTTP endpoints for attachments and raw blobs
- Optional RFC 9404 blob-management methods only when the server advertises `urn:ietf:params:jmap:blob`

`Mailbox/set` is used for user-controlled mailbox metadata, starting with role assignment. Role changes use the mailbox sync state as `ifInState`, and clearing a role sends an explicit JSON `null` patch. Because JMAP servers reject duplicate system roles, role reassignment clears the previous role owner before assigning the new owner with the returned mailbox state.

## Push

Push notifications arrive over either WebSocket or EventSource (SSE), depending on server capabilities. The client prefers WebSocket only when the server advertises `urn:ietf:params:jmap:websocket` with `supportsPush: true`; otherwise it falls back to SSE. Both transports deliver `StateChange` events containing updated state strings per object type, keyed by server account ID. On receiving a state change for the mapped server account, the sync engine triggers a delta sync for the affected type.

The push connection is maintained as long as the app is in the foreground. SSE reconnection uses the last known event ID. WebSocket request/response frames use client-supplied request IDs for correlation; push catch-up still relies on the next delta sync from stored type state. Transport details are documented in the transport layer spec.

## Authentication

JMAP requests authenticate with the configured HTTP authentication scheme. For Fastmail, distributed clients use OAuth bearer tokens and personal/testing clients use API tokens with `Authorization: Bearer ...`. The secret material is stored in the OS keyring and never written to project config. A 401 response triggers token refresh when possible or an explicit re-authentication flow. There is no silent retry with stale credentials.

## Error model

All errors are represented as a typed enum, never as raw strings:

```
JmapError
  +-- NetworkError(cause)        -- HTTP or connection failure
  +-- AuthError                  -- 401, token expired or invalid
  +-- SessionError(detail)       -- discovery failed, capability missing
  +-- MethodError(type, detail)  -- server rejected a method call; type preserved
  +-- StateMismatch              -- ifInState precondition failed
  +-- BlobError(blobId, cause)   -- upload or download failure
```

`NetworkError` and `MethodError` are distinct categories. A successful HTTP 200 response can still contain method-level errors in the response body. The error model makes this explicit at the type level.

## Invariants

All server communication goes through a single `JmapClient` instance per local account, configured with the mapped server account ID. Method calls are batched when independent (e.g., `Email/query` + `Email/get` via result references). State strings from responses are persisted atomically with the data they describe; this prevents the client from losing track of sync position if it crashes mid-processing.

Property lists are explicit. If a sync path omits a property from `Email/get`, the store must either preserve the old local value for that property or treat the response as a partial update. An omitted property is not the same as a server `null` value.

No Fastmail-specific extensions appear in core types. The type system uses only properties defined in RFC 8620 and RFC 8621.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| session-discovery | MUST | Client discovers or loads the JMAP Session resource and caches capabilities |
| session-refresh | MUST | Client refreshes session on 401 or capability mismatch |
| server-account-id | MUST | JMAP method calls and push filtering use the server account ID from Session, not the local account ID |
| batch-methods | SHOULD | Independent method calls are batched into a single HTTP request |
| state-persist | MUST | State strings are persisted atomically with the data they describe |
| error-typed | MUST | All JMAP errors are represented as typed enum variants, never raw strings |
| auth-keychain | MUST | JMAP auth secrets are stored only in the OS keyring, never in config files |
| thread-authoritative | MUST | JMAP conversation grouping uses server `threadId` as authoritative |
| partial-properties | MUST | Partial `Email/get` responses never erase locally stored properties that were not requested |
| upload-download-urls | MUST | Baseline attachment upload/download uses Session URL templates, not non-baseline blob methods |
| no-fastmail-ext | MUST | Core types use only RFC 8620/8621 standard properties |
