# Posthaste SPECial spec map

## Scope of scan

Read `special.conf.toml`, root `README.md`, all `docs/*.md` frontmatter, headings, and representative bodies for architecture-bearing docs. Also spot-checked implementation layout (`Cargo.toml`, `package.json`, `crates/*`, `apps/*`) to identify spec coverage gaps. No files were edited except this requested handoff.

## SPECial configuration and root health

- `special.conf.toml:1-2` sets `root = "README"` and `paths = ["docs"]`.
- SPECial requires every SPECial file, including the root, to have frontmatter with `scope`, `summary`, `modified`, `reviewed`, and uses root `dependents` to list top-level domains. Current `README.md` has no frontmatter and no dependency graph. It is product prose with TODO sections:
  - `README.md:15-18` promises modular/agent-first backend, OpenAPI API, MCP server, event stream, Rust backend, TypeScript Tauri frontend, smart mailboxes, action system, JMAP.
  - `README.md:20-42` has TODOs for installation, platform installation, development, and support.
- `docs/index.md` is MkDocs-facing, not valid SPECial metadata: only `title` and `description`, then a TODO (`docs/index.md:1-8`). Since `paths = ["docs"]`, it may be swept up by SPECial tools unless excluded or given valid frontmatter/lifecycle.

## Current major spec domains

### L0 domains (why/strategy)

- `docs/L0-accounts.md` — multi-account scoping, account-id invariant, credential storage deferred but designed for.
- `docs/L0-api.md` — Rust backend REST/SSE boundary. Key architecture statements:
  - REST/Axum localhost boundary (`docs/L0-api.md:15-25`).
  - Conversation-first list endpoints and cursor pagination (`docs/L0-api.md:27-40`).
  - Local SSE `/v1/events` with `afterSeq` resume (`docs/L0-api.md:42`).
  - Rust owns JMAP, SQLite, sync, event log, API; frontend is stateless API consumer (`docs/L0-api.md:44-48`).
- `docs/L0-branding.md` — brand identity, palette, typography, icon/shape systems.
- `docs/L0-compose.md` — Markdown compose, MIME strategy, `pulldown-cmark`, draft lifecycle.
- `docs/L0-jmap.md` — JMAP rationale, Fastmail first, `jmap-client`, protocol scope. Note it still emphasizes JMAP vs IMAP (`docs/L0-jmap.md:20`) while later provider spec has substantial IMAP support.
- `docs/L0-lab.md` — autonomous verification lab/control plane.
- `docs/L0-logging.md` — structured logging/tracing across Rust and React.
- `docs/L0-providers.md` — provider driver strategy. This is one of the most implementation-rich L0 docs:
  - Driver model with `jmap`, `imap_smtp`, `mock`; future Gmail/Graph native APIs (`docs/L0-providers.md:35-60`).
  - IMAP adapter strategy and `posthaste-imap` runtime (`docs/L0-providers.md:64-139`).
  - Implementation reference table starts at `docs/L0-providers.md:162` and ties behavior to local symbols/RFCs.
  - Invariants/assertions require local SQLite replica and no direct UI/provider reads (`docs/L0-providers.md:266-280`).
- `docs/L0-search.md` — custom query language, local SQLite execution, smart mailbox rationale.
- `docs/L0-sync.md` — local replica, JMAP delta/full reconciliation, online-first mutations, lazy body/attachment caching. Key lines: UI reads local SQLite via REST (`docs/L0-sync.md:21`), two push layers (`docs/L0-sync.md:31-35`), no offline mutation queue (`docs/L0-sync.md:39-42`).
- `docs/L0-testing.md` — executable behavior contracts, provider parity, red-first standard.
- `docs/L0-ui.md` — thin frontend principle, handoff-led React/Tauri shell, sanitized iframe rendering, keyboard/live updates. Key lines: React owns interaction state (`docs/L0-ui.md:15-20`), handoff reference source (`docs/L0-ui.md:24-40`), Rust sanitizes HTML and iframe disables scripts (`docs/L0-ui.md:84`), UI listens to SSE (`docs/L0-ui.md:102`).
- `docs/L0-website.md` — public showcase site and container deployment.

### L1 domains (interfaces/contracts)

- `docs/L1-accounts.md` — config directory, `ConfigRepository`, TOML schema, atomic writes, IDs, smart mailbox defaults.
- `docs/L1-api.md` — detailed `/v1` REST endpoints, request/response schemas, error mapping, SSE, accounts/settings/secret management, smart mailbox CRUD, sanitization. It is central but has stale dependents/metadata issues listed below.
- `docs/L1-compose.md` — Markdown subset, MIME structure, draft/session states, reply/forward/signatures/attachments.
- `docs/L1-jmap.md` — JMAP session/methods/types/push/auth/errors.
- `docs/L1-lab.md` — suite registry, command surface, profiles, artifacts, Tauri Playwright contracts.
- `docs/L1-logging.md` — crate layout/span conventions/config/event content/frontend logger. Its dependency section references headings that do not exist (see metadata issues).
- `docs/L1-search.md` — grammar/filter compilation/smart mailbox model/search UX/thread arcs. It states REST search compiles query text to smart mailbox rules and runs against local SQLite (`docs/L1-search.md:79-93`).
- `docs/L1-sync.md` — sync loop, cache planner, SQLite schema, events, automation actions, conflict/error model. Important implementation-driving coverage includes sync progress (`docs/L1-sync.md:52-56`), `cache_object` structural body row (`docs/L1-sync.md:103-107`, `docs/L1-sync.md:286-288`), UI source-of-truth invariant (`docs/L1-sync.md:340-341`).
- `docs/L1-ui.md` — component hierarchy, React Query, message list/detail, command search, overlays, keyboard, undo. Assertions include HTML iframe sandbox and Rust sanitization (`docs/L1-ui.md:289-291`).

### L2 domains (component/visual depth)

- `docs/L2-transport.md` — JMAP transport abstraction, WebSocket preferred with SSE fallback. Potential tension: L0 API’s frontend push is explicitly local SSE; L2 transport is about remote JMAP transport and should be clarified as a backend-provider layer, not browser API replacement.
- `docs/L2-ui-visual-reference.md` — precise visual contract for UI shell based on handoff.

## Metadata and dependency issues

### Invalid or incomplete metadata

- `README.md` is configured as SPECial root but has no frontmatter and no `dependents` graph.
- `docs/index.md` lacks required SPECial fields (`scope`, `summary`, `modified`, `reviewed`) and contains a TODO. Decide whether to exclude it from SPECial scan, mark it non-SPECial/ephemeral, or give valid metadata.
- `docs/L1-logging.md:8-11` depends on non-existent sections:
  - `docs/L1-accounts` section `"Config schema"` does not exist; closest headings are `TOML schema`, `app.toml`, etc.
  - `docs/L1-api` section `"Axum router"` does not exist; closest high-level headings are endpoint/SSE/error sections.

### Stale by SPECial date rules (`reviewed < dependency.modified`)

These should be reviewed before trusting downstream contracts:

- `docs/L0-sync` reviewed `2026-04-24` < `docs/L0-api` modified `2026-05-31`.
- `docs/L0-ui` reviewed `2026-05-26` < `docs/L0-api` modified `2026-05-31`.
- `docs/L0-testing` reviewed `2026-05-26` < `docs/L0-api` modified `2026-05-31`.
- `docs/L0-lab` reviewed `2026-05-26` < `docs/L0-api` modified `2026-05-31`.
- `docs/L0-logging` reviewed `2026-04-28` < `docs/L0-api` modified `2026-05-31`.
- `docs/L1-sync` reviewed `2026-05-25` < `docs/L0-testing` modified `2026-05-26` and `docs/L0-api` modified `2026-05-31`.
- `docs/L1-lab` reviewed `2026-05-27` < `docs/L1-api` modified `2026-06-02` and `docs/L1-ui` modified `2026-06-02`.
- `docs/L1-logging` reviewed `2026-05-26` < `docs/L1-accounts` modified `2026-06-01` and `docs/L1-api` modified `2026-06-02`.
- `docs/L2-transport` reviewed `2026-04-24` < `docs/L1-jmap`/`docs/L1-sync` modified `2026-05-25`.
- `docs/L2-ui-visual-reference` reviewed `2026-05-31` < `docs/L1-ui` modified `2026-06-02`, `docs/L1-search` modified `2026-06-01`, `docs/L1-compose` modified `2026-06-02`.

### Non-reciprocal dependency graph

`depends` is authoritative; `dependents` is only navigation, but the inverse graph is currently noisy. High-value examples to fix during revision:

- `docs/L0-logging` depends on `L0-api`, `L0-sync`, `L0-accounts`, but those docs do not list `L0-logging` as dependent.
- `docs/L0-providers` depends on `L0-accounts`, `L0-sync`, `L0-jmap`, but those docs do not list it as dependent.
- `docs/L0-testing` depends on `L0-providers`, `L0-sync`, `L0-api`, `L0-ui`, but inverse lists are missing.
- `docs/L1-api` depends on `L1-sync` and `L1-jmap`, but those docs do not list `L1-api`; also `L1-api` says `L1-ui` depends on it while `L1-ui` frontmatter does not include `L1-api`.
- `docs/L2-ui-visual-reference` depends on `L0-ui`, `L1-search`, `L1-compose`, but inverse lists are missing.

## Implementation coverage and high-level architecture gaps

The specs cover many domain behaviors, but the high-level architecture map is fragmented across L0 API/sync/providers/UI and not reflected in a single root/domain graph.

### Current implementation layout not explicitly mapped by spec

- Rust workspace members in `Cargo.toml` are `crates/*` and `apps/desktop`; key crates are `posthaste-config`, `posthaste-domain`, `posthaste-engine`, `posthaste-imap`, `posthaste-lab`, `posthaste-observability`, `posthaste-server`, `posthaste-store`.
- JS/Bun workspace apps in `package.json` are `apps/web`, `apps/site`, `apps/mcp`; root scripts expose web/site checks/builds and Tauri/Playwright lab.
- Specs mention some crates inline, but there is no architecture doc that defines crate/app responsibilities, process topology, and boundaries as a durable map.

### Gaps/under-covered domains

- **Architecture/root map:** Missing root SPECial frontmatter and a concise L0 architecture/domain index. The most important architectural invariant (`Rust owns backend; frontend API consumer`) lives in `L0-api`, while provider/store/sync/process boundaries are scattered.
- **Desktop/Tauri runtime:** There is substantial Tauri behavior not covered by a dedicated spec: default `embedded-server` feature (`apps/desktop/Cargo.toml:11-15`), backend injection tokens and client-only phase comments (`apps/desktop/src/lib.rs:327-331`), multi-surface windows (`apps/desktop/src/lib.rs:32-58`, `281`, `727-853`). UI/lab specs cover some Tauri testing, but not desktop packaging/runtime/window contracts.
- **MCP server / agent interface:** README promises a provided MCP server (`README.md:15`), and `apps/mcp/src/index.ts:21-23` builds `posthaste-mcp` with tools mapping to `/v1` operations (`apps/mcp/src/index.ts:61+`). No spec domain covers MCP capabilities, auth/connection model, tool naming, error behavior, or OpenAPI generation.
- **Security/auth/secrets:** Implementation has `auth.rs`, `authz.rs`, `oauth.rs`, `secret.rs`, `token.rs` under `posthaste-server`; specs mention credential storage/secret management, but no dedicated L0/L1 security domain for local API auth tokens, OAuth, secret storage, threat model, external-browser links, or desktop token injection.
- **Automation/action system:** README promises an action system; implementation has `posthaste-domain/src/service/automation.rs`, `posthaste-store/automation.rs`, and API preview endpoints in L1 API, but no dedicated automation/actions spec. Current coverage is scattered under API, search/UI, and sync.
- **Storage/cache as its own architecture:** L1 sync contains extensive SQLite schema/cache planner details, including cache-object and resource-governor assertions. If storage/cache will drive implementation, consider extracting L0/L1 storage/cache domain or adding L2 storage docs so sync does not own all persistence concerns.
- **OpenAPI/AsyncAPI/public contracts:** Root files `openapi.json` and `asyncapi.json` exist; MCP generates TS from `openapi.json`. Specs do not state generation ownership, drift checks, or whether API docs are source/derived artifacts.
- **Install/development/support docs:** README TODOs leave onboarding and release/platform promises unspecific despite website/desktop specs.
- **Calendar/JMAP future scope:** README mentions future JMAP calendars (`README.md:17`) but current spec is mail-focused. Either explicitly mark calendar out-of-scope or add roadmap-level mention.

## Suggested revision starting point

Start at the root, then stabilize the graph before rewriting individual domain bodies:

1. **Make `README.md` a valid SPECial root** with frontmatter (`scope: root`, summary, dates) and a `dependents` list of the major L0 domains. Also decide what public README content belongs here vs docs site.
2. **Add or create a high-level L0 architecture map** (could be `docs/L0-architecture.md` or a root section) that explains process topology and module boundaries: Rust backend/server/store/engine/domain/imap/config/observability/lab, React web, Tauri desktop, MCP adapter, site, generated OpenAPI/AsyncAPI, local SQLite/SSE/provider layers. This should become the dependency anchor for API/sync/UI/providers/desktop/MCP/security.
3. **Fix metadata hygiene**: handle `docs/index.md`, repair invalid section dependencies in `L1-logging`, review stale docs against newer dependencies, then update reciprocal `dependents` for navigation.
4. **Resolve missing or scattered domains** in priority order: security/auth/secrets, MCP/agent interface, desktop/Tauri runtime, automation/actions, storage/cache if it remains large.
5. **Then revise stale domain specs** starting with API → sync/UI/testing/logging/lab, because many stale edges originate from the 2026-05-31/06-02 API changes.

## Validation commands used / useful next checks

- Metadata/frontmatter/heading scan: custom Python over `docs/*.md`.
- Staleness/section/reciprocity check: custom Python comparing `reviewed` vs dependency `modified` and validating `section` headings.
- Implementation orientation: `Cargo.toml`, `package.json`, `crates/*/Cargo.toml`, `apps/mcp/src/index.ts`, `apps/desktop/src/lib.rs`.
- Useful next validation after edits: rerun the metadata/staleness script or a SPECial linter if available; search `spec: docs/` references to ensure renamed assertion IDs/paths do not break linked tests.
