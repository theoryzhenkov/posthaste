---
scope: root
summary: "JMAP mail client with MailMate-grade search and threads"
modified: 2026-03-31
reviewed: 2026-03-31
dependents:
  - path: spec/L0-jmap
  - path: spec/L0-api
  - path: spec/L0-sync
  - path: spec/L0-accounts
  - path: spec/L0-search
  - path: spec/L0-compose
  - path: spec/L0-ui
---

# mail-client

A JMAP mail client that brings MailMate's power-user features to a modern web UI. Boolean search, smart mailboxes, threaded conversation view, Markdown composition.

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
| Backend | Rust + Axum | Owns protocol, storage, and API |
| Storage | SQLite via rusqlite | Embedded, zero-config, portable |
| Frontend | React + TypeScript | Component model, large ecosystem, keyboard handling |
| Build tool | Bun + Vite | Fast dev server and builds |
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

Hexagonal core in Rust. The backend owns all business logic, JMAP protocol handling, and SQLite storage. It exposes a REST API + WebSocket on localhost. The React frontend is a stateless view layer that fetches data via the API and renders it. This separation means any frontend (web, desktop via Tauri, native mobile) can consume the same API.

## Domains

- **jmap** -- JMAP protocol types, session, method calls, push. [L0](spec/L0-jmap.md)
- **sync** -- Bidirectional sync engine, local SQLite replica, state tokens
- **search** -- Query language, smart mailboxes, search execution. [L0](spec/L0-search.md)
- **compose** -- Markdown composition, MIME assembly, send/draft lifecycle
- **ui** -- Web UI, React components, thread view, HTML rendering, keyboard model
- **accounts** -- Multi-account scoping (deferred, L0-only)
- **api** -- REST API + WebSocket boundary, Axum handlers, error mapping. [L0](spec/L0-api.md)

## MVP acceptance criteria

1. Connect to Fastmail via JMAP, authenticate with app-specific password
2. Sync all mailboxes and email metadata, lazy-fetch bodies on demand
3. Search with boolean queries (AND/OR/NOT, field prefixes, date ranges)
4. Create and use smart mailboxes (saved queries with auto-grouping)
5. Read email in threaded conversation view
6. Compose in Markdown, send as multipart HTML+plain via JMAP
