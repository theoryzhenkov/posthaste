---
scope: L1
summary: "JMAP session, method calls, type system, push, error model"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: spec/L0-jmap
dependents:
  - path: spec/L1-sync
  - path: spec/L1-compose
---

# JMAP domain -- L1

## Session

Discovery starts with `GET /.well-known/jmap`, which returns a Session object containing `apiUrl`, `downloadUrl`, `uploadUrl`, `eventSourceUrl`, and a capabilities map. The client caches the Session and refreshes it on 401 responses or when a required capability is missing from the cached version.

## Method calls

All JMAP communication uses `Request` objects containing one or more method calls. Independent calls are batched into a single HTTP POST when possible. This is JMAP's key performance advantage over IMAP's one-command-at-a-time model.

Back-references (`#ref`) allow one call's result to feed into the next within the same request. For example, an `Email/query` result can feed email IDs directly into an `Email/get` call without a round trip.

## Core types

These come from `jmap-client` and are not reimplemented:

- `Id` -- opaque server-assigned string identifier
- `Email` -- message metadata: `id`, `threadId`, `mailboxIds`, `keywords` (flags), `from`, `to`, `subject`, `receivedAt`, `hasAttachment`, `preview`, `bodyStructure`
- `Mailbox` -- folder or label: `id`, `name`, `parentId`, `role` (inbox, drafts, sent, trash, etc.), `totalEmails`, `unreadEmails`
- `Thread` -- conversation: `id`, `emailIds` (ordered)
- `Identity` -- sender identity: `id`, `name`, `email`, `replyTo`, `bcc`
- `EmailSubmission` -- send operation: `identityId`, `emailId`, `envelope`

## Methods used

- `Email/get`, `Email/query`, `Email/queryChanges`, `Email/changes`, `Email/set`
- `Mailbox/get`, `Mailbox/changes`, `Mailbox/set`
- `Thread/get`, `Thread/changes`
- `Identity/get`
- `EmailSubmission/set`
- `SearchSnippet/get` (for search result highlighting)
- `Blob/upload`, `Blob/download` (attachments and body content)

## Push

An EventSource (SSE) connection to `eventSourceUrl` with type filters receives `StateChange` events containing updated state strings per object type. On receiving a state change, the sync engine triggers a delta sync for the affected type.

The SSE connection is maintained as long as the app is in the foreground. Reconnection uses the last known event ID for automatic catch-up.

## Authentication

Bearer token in the `Authorization` header. For Fastmail, this means app-specific passwords. The token is stored in macOS Keychain and never held in memory longer than the scope of a request. A 401 response triggers the re-authentication flow. There is no silent retry with stale credentials.

## Error model

All errors are represented as a typed enum, never as raw strings:

```
JmapError
  +-- NetworkError(cause)        -- HTTP or connection failure
  +-- AuthError                  -- 401, token expired or invalid
  +-- SessionError(detail)       -- discovery failed, capability missing
  +-- MethodError(type, detail)  -- server rejected a method call
  +-- StateMismatch              -- ifInState precondition failed
  +-- BlobError(blobId, cause)   -- upload or download failure
```

`NetworkError` and `MethodError` are distinct categories. A successful HTTP 200 response can still contain method-level errors in the response body. The error model makes this explicit at the type level.

## Invariants

All server communication goes through a single `JmapClient` instance per account. Method calls are batched when independent (e.g., `Email/get` + `Thread/get` in one request). State strings from responses are persisted before acting on the data they describe; this prevents the client from losing track of sync position if it crashes mid-processing.

No Fastmail-specific extensions appear in core types. The type system uses only properties defined in RFC 8620 and RFC 8621.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| session-discovery | MUST | Client discovers JMAP session via `.well-known/jmap` and caches capabilities |
| session-refresh | MUST | Client refreshes session on 401 or capability mismatch |
| batch-methods | SHOULD | Independent method calls are batched into a single HTTP request |
| state-persist | MUST | State strings are persisted atomically with the data they describe |
| error-typed | MUST | All JMAP errors are represented as typed enum variants, never raw strings |
| auth-keychain | MUST | Auth tokens stored only in macOS Keychain, never on disk or in memory beyond request scope |
| no-fastmail-ext | MUST | Core types use only RFC 8620/8621 standard properties |
