---
scope: L1
type: DESIGN
lifecycle: ephemeral
summary: "Trust model for opening the API: authn, capability-scoped authz, boundary validation, rate limiting"
modified: 2026-05-29
reviewed: 2026-05-29
depends:
  - path: docs/eph/PLAN-L1-public-api-platform
  - path: docs/eph/DESIGN-L1-runtime-topology
dependents:
  - path: docs/eph/PLAN-L1-public-api-platform
---

# DESIGN: Trust model (PLAN P4)

## Status

**P4 design record + near-term implementation.** This doc specifies the whole
trust model but only part of it is implemented now (see Phasing). The
**capability-scoping model (§Authorization) is the load-bearing agent-facing
decision and is proposed here for sign-off, not yet implemented.**

## Why

Today the backend is localhost-only with **no auth** — every request is trusted
because the only client is the first-party app. Opening the API to other clients
and agents (the platform goal) makes that assumption false. The resolved plan
decision is **trusted-local now, full trust model as fast-follow**; this doc is
that fast-follow's design.

## Threat model (who can reach the port, and why auth ≠ "you already lost")

"Anyone who can reach the loopback port" is a much larger set than "code already
running as you". Auth defends the gap, and most of these need **no code execution**:

1. **Browser → localhost (no code-exec).** Any web page the user visits can issue
   requests to `http://127.0.0.1:<port>` via the browser. **CSRF** fires state-changing
   requests; **DNS rebinding** rebinds an attacker domain to `127.0.0.1`, defeating
   the same-origin assumption and letting the page *read* responses (exfiltrate mail).
   This is the primary near-term threat and the reason the `Origin`/`Host` check +
   token ship first.
2. **Multi-user hosts.** `127.0.0.1` is not per-user isolated; other local accounts
   can reach an unauthenticated daemon.
3. **Network exposure.** The moment the daemon binds beyond loopback (home server,
   VPS, container with a published port) "anyone can talk to it" is literal.
4. **Prompt-injected / runaway agents (blast radius, not perimeter).** Agents read
   attacker-controlled content (emails), so a crafted message can induce actions.
   Capability scoping bounds the worst case — this is *defense for trusted callers*,
   not keeping strangers out.

See [[runtime-topology]] for how these map to **embedded** (ephemeral port, injected
into the webview — secrecy is adequate) vs **daemon** (fixed reachable port — needs
real auth) modes.

## Controls (layered)

### 1. Authentication — loopback token (implemented, default-off)

- A **random per-process bearer token** is generated at server startup.
- **Daemon mode**: written to the state-dir port-file (`daemon.json`, see
  [[runtime-topology]]) as `{ port, token }` — the documented discovery mechanism
  for external clients. The file carries a live credential, so it is written with
  mode **`0600`** (owner-only) on unix and **only when `require_auth` is on** — an
  unused token is never persisted to disk.
- **Embedded mode**: injected into the webview alongside `window.__POSTHASTE_PORT__`
  as `window.__POSTHASTE_TOKEN__`; the web client sends it as `Authorization: Bearer`.
- Enforcement is gated by **`[daemon] require_auth` (default `false`)** so existing
  embedded/dogfood behavior is unchanged until explicitly enabled. When `true`, every
  `/v1` request except `GET /v1/health` requires the bearer token.
- **SSE caveat.** Browsers' `EventSource` cannot set request headers, so the
  `/v1/events` stream additionally accepts the token via an `?access_token=<token>`
  query param. The middleware honors `access_token` **only** for the `/events` path;
  all other routes require the `Authorization: Bearer` header. The web client appends
  the param automatically when `window.__POSTHASTE_TOKEN__` is present. (Query-param
  tokens can leak into logs/referrers; acceptable on loopback, revisit before any
  non-loopback bind.)

### 2. Origin / Host validation (implemented, default-off, same gate)

When `require_auth` is on, two checks run (a token alone can't stop a rebinding
attack that reads the token from the page context):

- **`Host` allowlist (mandatory, the real rebinding defense).** Every `/v1`
  request — *including* the otherwise-exempt `/health`, `/openapi.json`, and
  `/asyncapi.json`, and **before** any exemption short-circuit — must carry a
  `Host` header whose host portion is allowlisted: the loopback names
  (`localhost`, `127.0.0.1`, `::1`) plus the configured bind host. Comparison is
  host-only, case-insensitive, with an optional trailing dot stripped and the
  port ignored; a wildcard bind (`0.0.0.0`/`::`) does not widen the allowlist. A
  missing or non-allowlisted `Host` → **403**. This is what actually closes DNS
  rebinding, which arrives as `Host: attacker.com` with no `Origin` — a case the
  Origin check alone would wave through.
- **`Origin`/`Referer` allowlist (defense-in-depth, CSRF).** When the request
  carries an `Origin` (or `Referer`), its canonical origin (parsed via
  `url::Url`) is checked against the configured CORS origin + the Tauri webview
  origins; a mismatch → **403**. Requests with neither header (non-browser
  clients) pass on token alone.

Both checks are inert when `require_auth` is off, preserving the default-off
byte-identical invariant.

### 3. Boundary input validation (partially addressed; tracked)

With untrusted callers, the `as`-cast / trust-the-shape assumptions at the API
boundary become a real risk. The OpenAPI work (P1) already added typed request
bodies; remaining hardening (length/element caps, opaque-id format checks) is tracked
as follow-up — not a blocker for the localhost default, required before non-loopback
exposure.

### 4. Rate limiting (designed, not implemented)

A per-token / per-peer token-bucket layer, required before network exposure. Deferred
with the capability model; not needed while localhost-only.

## Authorization — capability scoping (PROPOSED, needs sign-off)

This is the most important agent-facing decision and is **not implemented**. The
goal: an agent (or any token) gets a *narrow* grant, so prompt injection or bugs
can't exceed it (e.g. read+tag, never delete/exfiltrate). Options:

- **Option A — Coarse scopes (recommended starting point).** A small fixed set —
  `read`, `write` (compose/keywords/mailbox moves), `admin` (accounts/settings),
  `events` (SSE subscribe). Tokens carry a scope set; each `#[utoipa::path]` declares
  its required scope; a middleware enforces. Simple, legible, ships fast. Maps cleanly
  onto the existing endpoint groups/tags.
- **Option B — Resource + verb capabilities.** Fine-grained grants like
  `messages:read`, `messages:tag`, `accounts:read`, per-account scoping. More precise
  least-privilege for agents, more surface to design/maintain.
- **Option C — Capability tokens (macaroons-style).** Attenuable tokens carrying
  caveats (account, mailbox, expiry, action). Most powerful for delegation, most
  complex; likely overkill near-term.

**Recommendation:** ship **A** with the token model now (once `require_auth` is
adopted), design the enum so B is an additive refinement. Open questions for sign-off:
scope granularity (A vs B), whether agents get per-account scoping, token issuance/
revocation UX, and expiry policy.

## Phasing

- **Now (this doc + impl):** token + `Origin`/`Host` guard + config gate (default off),
  webview/port-file token plumbing, tests. Capability model = design only.
- **After sign-off:** implement chosen capability model; enable `require_auth` for
  daemon mode by default; add rate limiting + remaining boundary validation before any
  non-loopback bind.

## Success criteria

- A documented auth + capability model exists **before** the API is exposed beyond
  localhost (plan success criterion).
- Enabling `require_auth` rejects un-tokened/cross-origin `/v1` calls without breaking
  the first-party app.
- Agents can be issued narrow grants; a compromised/injected agent cannot exceed them.

## Related

- [[runtime-topology]] — embedded vs daemon; the port-file the token rides in.
- `docs/eph/PLAN-L1-public-api-platform` — P4 phase; trusted-local-now decision.
