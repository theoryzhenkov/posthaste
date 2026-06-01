---
scope: root
summary: "Posthaste — open-source, local-first mail workstation with power-user search and a documented API"
modified: 2026-06-01
reviewed: 2026-06-01
dependents:
  - path: docs/L0-branding
  - path: docs/L0-providers
  - path: docs/L0-jmap
  - path: docs/L0-api
  - path: docs/L0-sync
  - path: docs/L0-accounts
  - path: docs/L0-search
  - path: docs/L0-compose
  - path: docs/L0-ui
  - path: docs/L0-logging
  - path: docs/L0-testing
  - path: docs/L0-lab
  - path: docs/L0-website
---

# Posthaste

Your mail, delivered to you at Posthaste.

Posthaste turns email into a fast, local, programmable workspace. It gives power users the parts they miss from classic mail clients: precise search, smart mailboxes, keyboard-first triage, and Markdown compose. The same Rust backend is exposed through a documented API for custom clients, scripts, and agents.

This is an early-stage build, not a conservative production recommendation. If you try it today, expect sharp edges and keep another mail client available.

## Why Posthaste

Email should be more than a feed you clear. It is where work arrives, receipts live, projects move, and agents can help — if the mail client gives you the right handles.

Posthaste is built around those handles:

- **Search you can keep:** boolean queries, field prefixes, date ranges, and smart mailboxes turn recurring searches into reusable mail views.
- **A local replica:** mail metadata syncs into SQLite, so the interface reads from your machine. Previously synced mail remains readable offline.
- **A serious desktop shell:** three panes, compact rows, keyboard-first actions, and a reader that treats conversations as the main unit.
- **Markdown compose:** write mail as plain text that becomes clean multipart plain text plus HTML.
- **Protocol adapters:** JMAP is the first-class path, with IMAP/SMTP behind backend provider adapters so the UI can stay mail-native without becoming protocol-shaped.

## Built for builders

Posthaste is a local mail platform with a real contract.

- The Rust backend exposes a versioned JSON API under `/v1`.
- `openapi.json` documents the REST surface.
- `asyncapi.json` documents the Server-Sent Events stream at `/v1/events`.
- `apps/mcp/` contains an initial MCP server over the same API.
- Daemon mode gives external clients a stable local endpoint for custom clients, scripts, and trusted local agents.

That means the bundled UI is only one way to use Posthaste. You can build a different client, subscribe to mail events, wire mail into local automations, or let an agent search, tag, move, and draft through the same backend the app uses.

The MCP adapter is still early. Until capability scoping is complete, it is for trusted-local use only: the daemon token grants broad access.

## Privacy and security posture

Posthaste is not a hosted mail service. Your mail provider remains the source of truth, and Posthaste keeps a local replica for the app. Posthaste does not collect product telemetry.

The security model is being built in concrete layers: HTML mail is sanitized in Rust and rendered with scripts disabled, and the local API perimeter uses bearer-token plus Host/Origin checks when auth is enabled. Non-loopback exposure, rate limiting, and narrower agent capabilities are still early-stage work.

## Try it

Builds are published from GitHub releases and listed on the site:

- <https://posthaste.theor.net/releases>
- <https://github.com/theoryzhenkov/posthaste/releases>

## Development setup

Requires `nix` and `direnv`. Create local environment files if they are missing, allow direnv, then run setup inside the flake dev shell:

```sh
cp -n .env.example .env
cp -n .envrc.example .envrc
direnv allow
just setup       # generate age key, init jj
```

The full local dev stacks run through Overmind in the Nix dev shell:

```sh
just dev web       # Stalwart + seed + posthaste serve --api-only + Vite
just dev desktop   # Stalwart + seed + Tauri dev shell
just dev services  # Stalwart + seed + posthaste serve --api-only
just dev smoke     # validate dev-stack path wiring without starting services
just web dev       # Vite only, assumes the backend is already running
just desktop dev   # Tauri only, assumes Stalwart is already running if needed
just desktop test  # desktop Rust tests with constrained Cargo parallelism
just build-serve   # build web assets plus the browser-localhost server binary
just package-serve # create target/distribute/posthaste-serve-*.tar.gz
just serve         # run `posthaste serve` against apps/web/dist
```

Rust backend validation intentionally excludes the Tauri desktop shell from normal workspace checks. Use `just test` or `just backend check` for routine backend/frontend validation. On constrained Linux VMs, avoid raw `cargo test --workspace` or `cargo clippy --workspace`: they include `apps/desktop` and can compile the GTK/WebKit stack alongside every backend test target. Run desktop tests and builds explicitly with `just desktop test` or `just desktop build`.

## Repository layout

| Path | Purpose |
|------|---------|
| `crates/` | Rust domain, engine, store, config, and server crates |
| `apps/web/` | React/Vite mail client |
| `apps/site/` | Static public product site |
| `apps/desktop/` | Tauri desktop shell |
| `apps/mcp/` | MCP adapter over the local `/v1` API |
| `docs/` | SPECial project documentation and MkDocs content |
| `tools/dev/` | Local development utilities, Procfiles, Stalwart config, and fixtures |
| `var/dev/` | Ignored local runtime data generated by dev stacks |
| `target/`, `apps/web/dist/`, `apps/site/dist/`, `site/` | Ignored build artifacts |

## Stack

| Layer | Choice | Why |
|-------|--------|-----|
| Provider drivers | JMAP first; IMAP/SMTP adapter path | JMAP fits the local replica model; IMAP/SMTP keeps common providers reachable |
| Target servers | Fastmail and Stalwart initially; Gmail/iCloud/Outlook through IMAP/SMTP | Covers modern JMAP servers without excluding mainstream providers |
| Backend | Rust + Axum | Owns protocol, sync, storage, API, event log, and sanitization |
| Storage | SQLite via rusqlite | Embedded, zero-config, portable |
| Frontend | React + TypeScript | Dense interactive mail UI, typed API contract, keyboard handling |
| Desktop | Tauri | Native shell around the local web client/backend model |
| Site | Astro + React island | Static public site with an interactive product mock |

## Scope

In scope for MVP:

- JMAP Mail objects: Email, Mailbox, Thread, Identity, EmailSubmission
- Boolean query language with field prefixes and date ranges
- Smart mailboxes: saved queries with display metadata
- Conversation-first reading view with paginated conversation list
- Markdown composition with multipart HTML+plain output
- Offline reading of synced mail
- Local `/v1` API, OpenAPI spec, SSE event stream, and initial MCP adapter

Out of scope for now:

- CalDAV/CardDAV
- Sieve management UI
- PGP/S-MIME
- Multi-account UI, though the data model is account-scoped from day one
- Plugin or extension store
- Production-stable agent authorization model

## Architecture

Posthaste uses a hexagonal Rust core. The backend owns protocol drivers, sync reconciliation, SQLite storage, HTML sanitization, the REST API, and the SSE event stream. The frontend consumes paginated conversation endpoints and reacts to domain events from `/v1/events`.

That boundary is deliberate: the mail app, custom clients, scripts, and agents can all speak the same local API instead of each owning a separate mail cache.

## Documentation

MkDocs serves and builds the Markdown content in `docs/` with the Material theme.

```sh
just mkdocs serve  # serve docs locally
just mkdocs build  # build docs into site/
```

The top-level SPECial domains are listed in the docs and tracked through the frontmatter above.

## License

Posthaste is open source under the MIT License.
