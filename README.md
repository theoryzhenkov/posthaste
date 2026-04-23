---
scope: root
summary: "PostHaste — JMAP mail client with MailMate-grade search and conversation-first web UI"
modified: 2026-04-23
reviewed: 2026-04-23
dependents:
  - path: docs/L0-branding
  - path: docs/L0-jmap
  - path: docs/L0-api
  - path: docs/L0-sync
  - path: docs/L0-accounts
  - path: docs/L0-search
  - path: docs/L0-compose
  - path: docs/L0-ui
  - path: docs/L0-logging
---

# PostHaste

A JMAP mail client that brings MailMate's power-user features to a modern web UI. Boolean search, smart mailboxes, conversation-first reading, Markdown composition, and a Rust-owned local replica.

## Setup

Two-phase setup. Requires `nix` and `direnv` installed.

```sh
./bootstrap.sh   # phase 1: creates .envrc, .env, allows direnv
# re-enter the directory so direnv activates the flake
just setup       # phase 2: generates age key, initializes jj
```

## Development

The full local dev stacks run through Overmind in the Nix dev shell:

```sh
just dev-web       # Stalwart + seed + posthaste-daemon + Vite
just dev-desktop   # Stalwart + seed + Tauri dev shell
just dev-services  # Stalwart + seed + posthaste-daemon
just frontend dev  # Vite only, assumes the backend is already running
just desktop dev   # Tauri only, assumes Stalwart is already running if needed
```

The stacks use isolated config and state under `dev/posthaste/` and `dev/stalwart/`.
After changing `flake.nix`, reload the dev shell with `direnv reload` or `nix develop`.

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
| Backend | Rust + Axum | Owns protocol, sync, storage, and API |
| Storage | SQLite via rusqlite | Embedded, zero-config, portable |
| Frontend | React + TypeScript | Component model, React Query caching, keyboard handling |
| Build tool | Vite + npm scripts | Fast dev server and builds |
| HTML sanitization | ammonia (Rust) | Whitelist-based, built on html5ever |
| Markdown | pulldown-cmark (Rust) | CommonMark + GFM extensions |

## Scope

In scope for MVP:

- JMAP Mail objects: Email, Mailbox, Thread, Identity, EmailSubmission
- Boolean query language with field prefixes and date ranges
- Smart mailboxes (saved queries with auto-grouping)
- Conversation-first reading view with per-message thread switcher
- Markdown composition with multipart HTML+plain output
- Offline reading of synced mail

Out of scope (for now):

- CalDAV/CardDAV
- Sieve management UI
- PGP/S-MIME
- Multi-account UI (data model is account-scoped from day one)
- Plugins/extensions

## Architecture

Hexagonal core in Rust. The backend owns all business logic, JMAP protocol handling, SQLite storage, sync reconciliation, and HTML sanitization. It exposes a localhost JSON API plus a Server-Sent Events stream at `/v1/events`. The React frontend consumes paginated conversation endpoints, renders a virtualized middle pane, and reacts to domain events from the SSE stream. This keeps protocol and cache ownership in Rust while leaving the UI free to evolve independently.

## Domains

- **branding** -- Name, identity, palette, typography, logo. [L0](docs/L0-branding.md)
- **jmap** -- JMAP protocol types, session, method calls, push. [L0](docs/L0-jmap.md)
- **sync** -- Bidirectional sync engine, local SQLite replica, state tokens
- **search** -- Query language, smart mailboxes, search execution. [L0](docs/L0-search.md)
- **compose** -- Markdown composition, MIME assembly, send/draft lifecycle
- **ui** -- Web UI, React components, conversation list, HTML rendering, keyboard model
- **accounts** -- Multi-account scoping, config repository, TOML persistence. [L0](docs/L0-accounts.md) [L1](docs/L1-accounts.md)
- **api** -- REST API + SSE boundary, Axum handlers, pagination, error mapping. [L0](docs/L0-api.md) [L1](docs/L1-api.md)
- **logging** -- Structured tracing and logging across backend and frontend. [L0](docs/L0-logging.md) [L1](docs/L1-logging.md)

## MVP acceptance criteria

1. Connect to Fastmail via JMAP, authenticate with app-specific password
2. Sync all mailboxes and email metadata, lazy-fetch bodies on demand, and prune stale local mailboxes on authoritative full resyncs
3. Search with boolean queries (AND/OR/NOT, field prefixes, date ranges)
4. Create and use smart mailboxes (saved queries with auto-grouping)
5. Read email in a conversation-first view with paginated conversation list, anchored live updates, and on-demand message body fetch
6. Compose in Markdown, send as multipart HTML+plain via JMAP
