---
title: PostHaste
description: JMAP mail client with MailMate-grade search and conversation-first web UI
---

# PostHaste

A JMAP mail client that brings MailMate's power-user features to a modern web UI. Boolean search, smart mailboxes, conversation-first reading, Markdown composition, and a Rust-owned local replica.

## Stack

| Layer | Choice | Why |
|-------|--------|-----|
| Protocol | JMAP (RFC 8620/8621) | Stateless HTTP, server-side threading, push, batch requests |
| Target server | Fastmail (initially) | Reference JMAP implementation |
| Backend | Rust + Axum | Owns protocol, sync, storage, and API |
| Storage | SQLite via rusqlite | Embedded, zero-config, portable |
| Frontend | React + TypeScript | Component model, React Query caching, keyboard handling |
| Build tool | Vite + Bun scripts | Fast dev server and builds |
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

- **[Branding](L0-branding.md)** -- Name, identity, palette, typography, logo
- **[JMAP](L0-jmap.md)** -- JMAP protocol types, session, method calls, push
- **[Sync](L0-sync.md)** -- Bidirectional sync engine, local SQLite replica, state tokens
- **[Search](L0-search.md)** -- Query language, smart mailboxes, search execution
- **[Compose](L0-compose.md)** -- Markdown composition, MIME assembly, send/draft lifecycle
- **[UI](L0-ui.md)** -- Web UI, React components, conversation list, HTML rendering, keyboard model
- **[Website](L0-website.md)** -- Public product showcase site and static Docker deployment
- **[Accounts](L0-accounts.md)** -- Multi-account scoping (deferred, L0-only)
- **[API](L0-api.md)** -- REST API + SSE boundary, Axum handlers, pagination, error mapping

## MVP acceptance criteria

1. Connect to Fastmail via JMAP, authenticate with an OAuth bearer token or JMAP API token
2. Sync all mailboxes and email metadata, lazy-fetch bodies on demand, and prune stale local mailboxes and messages on authoritative full resyncs
3. Search with boolean queries (AND/OR/NOT, field prefixes, date ranges)
4. Create and use smart mailboxes (saved queries with auto-grouping)
5. Read email in a conversation-first view with paginated conversation list, anchored live updates, and on-demand message body fetch
6. Compose in Markdown, send as multipart HTML+plain via JMAP
