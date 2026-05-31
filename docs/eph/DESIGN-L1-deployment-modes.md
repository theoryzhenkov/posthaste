---
scope: L1
type: DESIGN
lifecycle: ephemeral
summary: "Decouple daemon/client state to support bundled, daemon-only, and client-only deployments; unified with capability-token Phase 2"
modified: 2026-05-31
reviewed: 2026-05-31
depends:
  - path: docs/eph/DESIGN-L1-runtime-topology
  - path: docs/eph/DESIGN-L1-trust-model
  - path: docs/eph/DESIGN-L1-capability-tokens
  - path: docs/eph/PLAN-L1-capability-urls
  - path: docs/L1-api
dependents:
  - path: docs/eph/PLAN-L1-capability-urls
---

# DESIGN: Deployment modes & backend/client state decoupling

## Status

Design for sign-off — the boundary + open decisions below need your call before
implementation. Grounded in a codebase mapping (workflow `map-state-decoupling`).
Combined with capability-token **Phase 2** (see [[capability-urls]]) where they
converge on the "remote client connects to a daemon" layer.

## Why

Today backend (daemon) and client (frontend) are fused: the desktop app embeds the
server, `client.ts` freezes `baseUrl`/`token` at module load from the injected
`__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__`, and `AppSettings` mixes daemon data
(automation, cache) with pure presentation (theme). That blocks the deployment
modes we want:
- **Bundled app** (`.dmg`) — backend + frontend together (today's default).
- **CLI / brew daemon** — the `posthaste` binary as a daemon manager, no UI.
- **Client-only `.dmg`** — the frontend with no embedded server, connecting to a
  remote/local daemon (e.g. one on your server, reached over Tailscale).

The enabling move: **split state by ownership — the daemon owns data + behavior;
each client owns presentation + which daemon it talks to.**

## Ownership boundary

| State / config | Owner | Notes |
|---|---|---|
| `appearance` (theme/palette/density/accent/glass) | **client** | Pure presentation; `ThemeProvider` still has a localStorage fallback → migration is reversible. **Carve out of API `AppSettings`.** |
| `cachePolicy` | **backend** | Daemon memory/fetch budget (`service/cache.rs`), not a preference. |
| `automationRules` | **backend** | Daemon executes them per sync batch. |
| `automationDrafts` | **backend** | Server-persisted so a draft started on one device continues on another (shared across clients). |
| `defaultAccountId` | **client** | Confirmed by code: it's only a stored preference + the `is_default` display flag + account-deleted cleanup — the backend never defaults a query to it. Account-specific ops always carry `source_id`; the only account-less endpoints are genuine cross-account aggregates. So the client owns "which account is pre-selected"; `is_default` becomes client-derived. |
| Connection `baseUrl` + token | **client** | Property of the active connection profile; token in OS keyring. |
| Panel/column/floating-layout, SSE resume cursor | **client** | Already client-local (localStorage/sessionStorage) — correct. |
| `app.toml`, `sources/*`, `smart-mailboxes/*`, `account-assets/` (CONFIG_ROOT) | **backend** | Daemon is sole writer; clients read via `/v1`. |
| `mail.sqlite`, raw bodies, `logs/` (STATE_ROOT) | **backend** | Single-writer daemon artifacts. |
| `daemon.json` `{port, token}` | **backend writes / client reads** | Discovery + auth handoff — the shared contract between both efforts. |
| `macaroon.key` root key | **backend** | Never leaves the daemon; clients attenuate, never mint from root. |
| Connection-profile store (which daemon) | **client** | New layer — the keystone of the decoupling. |

`__POSTHASTE_PORT__/__POSTHASTE_TOKEN__` injection and Tauri window/menu IPC stay,
but become **embedded-mode-only** (feature-gated off in the client-only build).

## Connection profiles (the keystone)

Promote the pattern the MCP adapter already proves (`apps/mcp/src/client.ts`
`resolveConnection`: env → `daemon.json` → base URL) into a first-class,
**multi-profile** client store shared by web + desktop.

- **`connections.json`** (client-owned dir, never in the daemon roots):
  `{ version: 1, activeProfileId, profiles: [{ id, name, baseUrl, hostHeader?, mode, tokenRef }] }`
  where `mode ∈ { embedded, local-daemon, remote }` and `tokenRef` is a **pointer**
  (the secret lives in the OS keyring, keyed by profile id — never plaintext, never
  a URL for long-term access).
- **Per-profile resolution**: `embedded` → in-process `ServerHandle`; `local-daemon`
  → read `STATE_ROOT/daemon.json`; `remote` → explicit `baseUrl` + keyring token.
  Preserves the runtime-topology auto-detect (embedded default, daemon opt-in,
  single-writer lock via `daemon.json`) while making **remote** a real third mode.
- **Critical refactor**: `client.ts` `resolveBaseUrl()`/`resolveAuthToken()` are
  module-load `const`s today; they must become **functions of the active profile**,
  re-resolvable on switch — including `authHeaders()` and `buildEventsUrl()` (the SSE
  `?access_token=` path).

## Client state layout

A client-owned dir, **distinct from** `STATE_ROOT` (`~/.local/share/posthaste`,
daemon-exclusive) and `CONFIG_ROOT` (`~/.config/posthaste`, daemon config) — e.g.
`~/.config/posthaste/client/`:
- `connections.json` (profiles), `appearance.json` (carved from `AppSettings`),
  `layout.json` (panel/column geometry), + OS-keyring entries for per-profile tokens.
- A small **client-store abstraction** with two backends — localStorage (web, no
  filesystem) and file+keyring (desktop `.dmg`) — so the same React code serves both.

## Build modes (one server + one contract, two client lifecycles)

A single cargo feature **`embedded-server`** (default on) is the switch:
1. **Bundled `.dmg`** (`embedded-server` on): spawns the in-process server, becomes
   an implicit auto-discovered profile; honors the single-writer lock.
2. **CLI / brew daemon**: the standalone `posthaste` binary, fixed port, writes
   `daemon.json`, optional `frontend_dist` browser mode. The only mode needing the
   full Phase-2 trust perimeter.
3. **Client-only `.dmg`** (`embedded-server` off): gate OUT `start_server` + the
   port/token injection (Tauri IPC degrades via the existing `desktop.ts` guards);
   gate IN the connection-profile UI as the entry point (a profile must exist before
   any API call). Keep the Tauri shell (native windows + keyring).

## Unified plan with capability-token Phase 2

Four shared surfaces — do them together to avoid double-touching the connection layer:
1. **`daemon.json` schema** — version it now (`{version:1,…}`); reader/writer tolerate
   unknown fields, so Phase 2 can add a macaroon id (revocation) + tailnet hostname.
2. **Token transport classes** — long-term per-daemon token (keyring, `Authorization`
   header) vs short-lived capability share-links (query param, expiring). Bake the
   distinction into the profile store so Phase-2 share-links slot in.
3. **Mint endpoint `POST /v1/auth/tokens`** — so a remote/agent client gets a *narrow*
   token instead of carrying the full-scope `daemon.json` token. (`manage`-scoped.)
4. **Host allowlist** — loopback-only today; Phase 2 admits tailnet hosts; the
   profile's `hostHeader` is the client side. Land together.

**Recommended order** (extends [[capability-urls]] staging):
- **(A)** version `daemon.json` + dynamic per-profile `client.ts` + carve `appearance`
  out of `AppSettings` — *non-exposing, no security surface.*
- **(B)** build the client profile store (keyring tokens; local-daemon + remote modes)
  against the existing full-scope token.
- **(C)** Phase 2: result-side scoping of aggregate endpoints + the mint endpoint, so
  profiles can hold narrow tokens.
- **(D)** Phase 2: revocation/once + exposure hardening (TLS/Tailscale, rate limit,
  Host allowlist) — **last, gated behind security review.**

## Near-term actions (lock the boundary cheaply now)

1. **Carve `appearance` out of API `AppSettings`** — the cleanest contract change
   available (sparse-mergeable; `ThemeProvider` localStorage fallback exists). Shrinks
   server `AppSettings` to genuinely daemon-owned fields.
2. **Version `daemon.json`** (`{version:1,port,token}`); readers ignore unknown fields.
3. **Make `client.ts` resolution dynamic** (per-active-profile accessors; default the
   active profile to today's injection). Removes the module-load freeze.
4. **Client-store abstraction** (localStorage + file/keyring backends) behind the
   existing layout/column keys — a durable home for client-only state.
5. **Add the `embedded-server` cargo feature** (default on), gating `start_server` +
   injection — proves the seam compiles both ways.

## Resolved decisions (2026-05-31, signed off)

1. **`defaultAccountId` → client-owned.** Code confirms it's only a stored preference
   (no server-side query-defaulting); the client pre-selects an account and always
   passes `source_id` for account-specific ops. Account-less endpoints are the genuine
   cross-account aggregates. `is_default` becomes client-derived; account-deleted
   cleanup moves client-side. *Sharpens the contract: no endpoint silently defaults to
   "the default account" server-side.*
2. **Remote exposure → Tailscale-only for v1.** No in-daemon TLS. **Posthaste is not
   tailnet-aware:** the client profile holds a generic `baseUrl`; the daemon gains a
   generic, operator-configured **`allowed_hosts`** list (extending the loopback Host
   check). Tailscale is the external transport/ACL layer — Posthaste only sees HTTP +
   a `Host` header checked against config.
3. **Per-daemon token storage → OS keyring** keyed by profile id (encrypted-blob
   fallback; never plaintext/URL).
4. **Client-only build → keep the Tauri shell**, gate only the embedded server via the
   `embedded-server` cargo feature (native windows + keyring; IPC degrades gracefully).
5. **`automationDrafts` → backend (server-persisted).** Drafts should sync across
   devices (start on laptop, continue on phone) → shared → backend. (Reinforces the
   boundary rule: per-device → client, shared → backend.)
6. **Revocation → expiry + once/nonce + revocation-list fast-follow** (root-key
   rotation as break-glass).

## Success criteria

- The same codebase builds three targets (bundled / daemon CLI / client-only) from
  the `embedded-server` feature.
- A client can hold multiple connection profiles (work/personal/remote) and switch
  daemons at runtime; tokens never sit in plaintext.
- `appearance` is client-local; the daemon `AppSettings` is genuinely daemon-owned.
- A remote/self-hosted daemon (over Tailscale) is reachable by a client-only build
  with the Phase-2 perimeter + a narrow-scoped token.

## Related

- [[runtime-topology]] — embedded vs daemon lifecycle; the single-writer lock.
- [[trust-model]] / [[capability-tokens]] — the perimeter + macaroon caveats.
- [[capability-urls]] — Phase 2; sequenced together here.
