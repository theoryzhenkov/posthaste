---
scope: root
summary: "JMAP-native macOS mail client with MailMate-grade search and threads"
modified: 2026-03-29
reviewed: 2026-03-29
dependents:
  - path: spec/L0-jmap
  - path: spec/L0-bridge
  - path: spec/L0-sync
  - path: spec/L0-accounts
  - path: spec/L0-search
  - path: spec/L0-compose
  - path: spec/L0-ui
---

# mail-client

A JMAP mail client for macOS that brings MailMate's power-user features to a modern native UI. Boolean search, smart mailboxes, threaded conversation view, Markdown composition.

## Setup

Two-phase setup. Requires `nix` and `direnv` installed.

```sh
./bootstrap.sh   # phase 1: creates .envrc, .env, allows direnv
# re-enter the directory so direnv activates the flake
just setup       # phase 2: generates age key, initializes jj
```

## Documentation

MkDocs serves and builds the Markdown content in `docs/` with the Material theme.

```sh
just mkdocs serve  # serve docs locally
just mkdocs build  # build docs into site/
```

## Stack

| Layer | Choice | Why |
|-------|--------|-----|
| Protocol | JMAP (RFC 8620/8621) | Stateless HTTP, server-side threading, push, batch requests |
| Target server | Fastmail (initially) | Reference JMAP implementation |
| Core language | Rust | Memory safety, `jmap-client` crate, `mail-parser` crate |
| Core storage | SQLite via GRDB (Swift) | Reactive UI via ValueObservation |
| FFI | Mozilla UniFFI | Production-grade Rust-to-Swift bindings |
| UI framework | SwiftUI | Modern macOS-native |
| HTML rendering | WKWebView (sandboxed) | System HTML renderer, JS disabled |
| HTML sanitization | ammonia (Rust) | Whitelist-based, built on html5ever |
| Markdown | pulldown-cmark (Rust) | CommonMark + GFM extensions |

## Scope

In scope for MVP:

- JMAP Mail objects: Email, Mailbox, Thread, Identity, EmailSubmission
- Boolean query language with field prefixes and date ranges
- Smart mailboxes (saved queries with auto-grouping)
- Threaded conversation view with thread arcs
- Markdown composition with multipart HTML+plain output
- Offline reading of synced mail

Out of scope (for now):

- CalDAV/CardDAV
- Sieve management UI
- PGP/S-MIME
- Multi-account UI (data model is account-scoped from day one)
- Plugins/extensions

## Architecture

Hexagonal core in Rust. Rust owns protocol logic and business rules, writing to Swift's GRDB cache via a UniFFI callback interface (`CacheWriter`). SwiftUI reads from GRDB for reactive updates through `ValueObservation`. Rust never reads from the cache for protocol decisions; it maintains its own in-memory state for sync orchestration.

This separation means the sync engine can be tested in pure Rust with a mock `CacheWriter`, and the UI can be tested against a pre-populated GRDB database with no network.

## Domains

- **jmap** -- JMAP protocol types, session, method calls, push. [L0](spec/L0-jmap.md)
- **sync** -- Bidirectional sync engine, local GRDB replica, state tokens
- **search** -- Query language, smart mailboxes, search execution. [L0](spec/L0-search.md)
- **compose** -- Markdown composition, MIME assembly, send/draft lifecycle
- **ui** -- SwiftUI shell, thread view, HTML rendering, keyboard model
- **accounts** -- Multi-account scoping (deferred, L0-only)
- **bridge** -- UniFFI boundary, callback interfaces, type projection. [L0](spec/L0-bridge.md)

## MVP acceptance criteria

1. Connect to Fastmail via JMAP, authenticate with app-specific password
2. Sync all mailboxes and email metadata, lazy-fetch bodies on demand
3. Search with boolean queries (AND/OR/NOT, field prefixes, date ranges)
4. Create and use smart mailboxes (saved queries with auto-grouping)
5. Read email in threaded conversation view
6. Compose in Markdown, send as multipart HTML+plain via JMAP
