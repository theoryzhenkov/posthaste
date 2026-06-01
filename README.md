---
scope: root
summary: "Posthaste — local-first, open-source mail you own and can build on"
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

Your email, delivered at Posthaste.

A local-first, open-source mail client: a Rust backend that keeps a local replica of your mail and puts a programmable local API in front of it. The desktop app is the first client — not the boundary of what you can build.

> **Pre-beta.** Posthaste runs daily for the people building it, but it is not production-ready. Expect sharp edges, providers still landing, and an API that is still moving. If you try it, keep another mail client available.

## What works today

- **Three-pane desktop mail client** — sidebar, tabular conversation list, conversation-first reader, keyboard-first actions, clear signal colors.
- **Smart mailboxes / saved views** — a boolean query language with field prefixes and date ranges, saved as living views.
- **Markdown compose** — write in Markdown, send clean multipart plain text plus HTML.
- **Local replica** — mail metadata syncs into SQLite and the interface reads from your machine; bodies are lazy-fetched and cached. Previously synced mail stays readable offline.
- **A programmable local API** — versioned JSON under `/v1` (`openapi.json`), an SSE event stream at `/v1/events` (`asyncapi.json`), and an initial MCP adapter in `apps/mcp/` (9 tools).
- **JMAP provider** — Fastmail and Stalwart are the initial targets.

## Planned / in progress

- **IMAP/SMTP adapter path** — to broaden provider support beyond JMAP.
- **Capability scoping for agents** — until it lands, the MCP/daemon token grants broad access (trusted-local only).
- **Hardening** — non-loopback exposure, rate limiting.
- **Multi-account UI** — the data model is account-scoped from day one; the UI is not built yet.

Explicitly **out of scope for now:** CalDAV/CardDAV, Sieve management UI, PGP/S-MIME, a plugin/extension store, and a production-stable agent authorization model.

## Built for builders

The same Rust backend that powers the app is a product surface in its own right.

- Versioned JSON API under `/v1`; `openapi.json` documents the REST surface.
- `asyncapi.json` documents the Server-Sent Events stream at `/v1/events`.
- `apps/mcp/` is an initial MCP server over the same API.
- Daemon mode gives external clients a stable local endpoint.

You can build a different client, subscribe to mail events and wire mail into local automations, or point a trusted local agent at the same backend the app uses to search, tag, move, and draft.

> The MCP adapter is early. Until capability scoping is complete, it is **trusted-local only**: the daemon token grants broad access.

## Privacy and security posture

Posthaste is not a hosted mail service. Your mail provider stays the source of truth; Posthaste keeps a local replica for the app, and there is no remote account to create.

The security model is built in concrete layers:

- HTML mail is sanitized in Rust (`ammonia`) and rendered in a sandboxed iframe with scripts disabled.
- The local API perimeter is **on by default**: bearer token plus Host/Origin checks.

Non-loopback exposure, rate limiting, and narrower agent capabilities are still early-stage work.


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
| Provider targets | Fastmail and Stalwart via JMAP today; IMAP/SMTP path planned/in progress | Starts with modern JMAP servers while keeping a path to mainstream providers |
| Backend | Rust + Axum | Owns protocol, sync, storage, API, event log, and sanitization |
| Storage | SQLite via rusqlite | Embedded, zero-config, portable |
| Frontend | React + TypeScript | Dense interactive mail UI, typed API contract, keyboard handling |
| Desktop | Tauri | Native shell around the local web client/backend model |
| Site | Astro + React island | Static public site with an interactive product mock |

## Architecture

Posthaste uses a hexagonal Rust core. The backend owns protocol drivers, sync reconciliation, SQLite storage, HTML sanitization, the REST API, and the SSE event stream. The frontend consumes paginated conversation endpoints and reacts to domain events from `/v1/events`.

That boundary is deliberate: the mail app, custom clients, scripts, and agents all speak the same local API instead of each owning a separate mail cache.

## Documentation

MkDocs serves and builds the Markdown content in `docs/` with the Material theme.

```sh
just mkdocs serve  # serve docs locally
just mkdocs build  # build docs into site/
```

The top-level SPECial domains are listed in the docs and tracked through the frontmatter above.

## License

Posthaste is open source under the MIT License.
