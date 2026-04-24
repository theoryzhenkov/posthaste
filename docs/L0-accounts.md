---
scope: L0
summary: "Multi-account scoping invariant and deferral rationale"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: README
  - path: docs/L0-jmap
  - path: docs/L0-sync
dependents:
  - path: docs/L1-accounts
---

# Accounts domain -- L0

## Deferred but designed for

Multi-account UI is out of scope for MVP. The implementation targets a single Fastmail account. However, every data structure in the system is account-scoped from day one because retrofitting account isolation after the fact is expensive and error-prone. Tables need composite keys, queries need scoping predicates, state tokens need namespacing. Doing this later means migrating production data and auditing every query. Doing it now costs almost nothing.

## The invariant

All SQLite tables use `(account_id, ...)` composite primary keys. All Rust-side state (sync state strings, session objects) is keyed by account ID. All API endpoints that return account-scoped data filter by account ID internally. The UI may hardcode a single account ID for v1, but no code path assumes there is only one account.

The local account ID is a hidden PostHaste identifier used for storage, routes, and secret references. User-facing account identity is the account name, full name, and owned email address/pattern list. The JMAP server account ID comes from the Session object's `accounts` and `primaryAccounts` fields. These IDs are mapped explicitly and are not interchangeable.

## Credential storage

JMAP auth secrets are stored in the OS keyring as generic secrets, keyed by local account ID. For Fastmail, distributed clients use OAuth access/refresh tokens and personal/testing clients use JMAP API tokens. App-specific passwords are for non-JMAP protocols and are not the normal JMAP credential. Secrets are read at session creation and not written to TOML config. On authentication failure (HTTP 401), the UI refreshes OAuth credentials when possible or presents a re-authentication flow.

## JMAP discovery

Account setup starts from a configured Session URL or provider origin. Generic JMAP providers may support `GET /.well-known/jmap` on their domain. Fastmail documents `https://api.fastmail.com/jmap/session` as the Session resource. The Session response reveals the JMAP API URL, upload/download URLs, capabilities, and server account IDs. If discovery fails, the user can manually provide the Session URL.

## What "deferred" means concretely

The account settings UI can add, edit, verify, enable/disable, and delete configured accounts, but the primary mail shell still treats the configured sources as one workspace rather than a full account-picker product. This L0 exists so that other domain specs reference the account-scoping invariant and do not accidentally introduce single-account assumptions into data models, interfaces, or queries.
