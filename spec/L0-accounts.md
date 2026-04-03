---
scope: L0
summary: "Multi-account scoping invariant and deferral rationale"
modified: 2026-03-31
reviewed: 2026-03-31
depends:
  - path: README
  - path: spec/L0-jmap
  - path: spec/L0-sync
dependents:
  - path: spec/L1-accounts
---

# Accounts domain -- L0

## Deferred but designed for

Multi-account UI is out of scope for MVP. The implementation targets a single Fastmail account. However, every data structure in the system is account-scoped from day one because retrofitting account isolation after the fact is expensive and error-prone. Tables need composite keys, queries need scoping predicates, state tokens need namespacing. Doing this later means migrating production data and auditing every query. Doing it now costs almost nothing.

## The invariant

All SQLite tables use `(account_id, ...)` composite primary keys. All Rust-side state (sync state strings, session objects) is keyed by account ID. All API endpoints that return account-scoped data filter by account ID internally. The UI may hardcode a single account ID for v1, but no code path assumes there is only one account.

## Credential storage

Auth tokens (Fastmail app-specific passwords) are stored in macOS Keychain as generic passwords, keyed by account ID. Tokens are read from Keychain at session creation and not held in memory beyond the active session. On authentication failure (HTTP 401), the UI presents a re-authentication flow.

## JMAP discovery

Account setup uses `GET /.well-known/jmap` on the provider's domain. The Session response reveals the JMAP API URL, capabilities, and account IDs. For Fastmail, the domain is `fastmail.com`. For future providers, the user enters their email domain and discovery is attempted automatically. If discovery fails, the user can manually provide the JMAP endpoint URL.

## What "deferred" means concretely

No account picker UI, no add/remove account flow, no per-account settings screen, no universal mailbox aggregating mail across accounts. This L0 exists so that other domain specs reference the account-scoping invariant and do not accidentally introduce single-account assumptions into data models, interfaces, or queries.
