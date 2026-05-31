---
scope: L1
type: DESIGN
lifecycle: ephemeral
summary: "Macaroon capability tokens: resource+action+expiry caveats, a per-route authz map, capability-URL-ready"
modified: 2026-05-31
reviewed: 2026-05-31
status_note: "Stage B implemented: per-route caveat enforcement + authz map live"
depends:
  - path: docs/eph/DESIGN-L1-trust-model
  - path: docs/eph/PLAN-L1-public-api-platform
  - path: docs/L1-api
dependents:
  - path: docs/eph/DESIGN-L1-trust-model
---

# DESIGN: Capability tokens (macaroons)

## Status

**Design for sign-off — no code yet.** Implements the P4 capability-scoping model
(macaroons-style, the user's choice) on top of the shipped P4 perimeter
([[trust-model]]: token + Host/Origin, `require_auth` on by default). Designed to
be **capability-URL-ready** so the future "export a scoped link" feature is a UX
layer, not a redesign.

## Goal

Replace the single opaque bearer token with **macaroons**: bearer tokens carrying
**caveats** (restrictions) checked at verification. This gives:
- **Narrow agent grants now** — an agent gets a token scoped to e.g. read+tag,
  account X, expiring in 1h; prompt injection can't exceed the caveats.
- **Capability URLs later** — a macaroon is a compact string; embed it in
  `?access_token=…` and the URL *is* the grant ("read-only account X inbox",
  "send one email"). Staged behind exposure hardening (see Phasing).

## Why macaroons (vs coarse scopes)

Macaroons are **attenuable**: anyone holding a token can mint a *narrower* one by
appending caveats, with **no server round-trip**. That is exactly "take my
full-access token and derive a link scoped to this one message." Caveats are
self-describing and verified statelessly via an HMAC chain against a root key.

## Caveat vocabulary (first-party caveats)

Each caveat is a predicate the server evaluates against the request. Phase-1 set,
resource-scoped per the user's direction:

| Caveat | Form | Meaning |
|---|---|---|
| `action` | `action in {read,send,tag,move,delete,manage}` | permitted verb(s) |
| `account` | `account = <source_id>` | restrict to one account/source |
| `mailbox` | `mailbox = <mailbox_id>` | restrict to one mailbox |
| `message` | `message = <message_id>` | restrict to one message |
| `expires` | `expires < <rfc3339>` | hard expiry (always recommended) |

**Implemented wire format (Stage B).** Every caveat is an ASCII `key = value`
predicate (single spaces around `=`); mint/attenuate and verify agree on this
exact form (documented in `authz.rs`):
- `action = <verb>[,<verb>...]` — comma-separated set; satisfied iff the route's
  required verb is in the set.
- `account = <source_id>`, `mailbox = <mailbox_id>`, `message = <message_id>` —
  satisfied iff the request's value on that axis equals the value.
- `expires = <rfc3339-utc>` — satisfied iff `now` < the timestamp (the design's
  `expires < <rfc3339>` shorthand; the wire key is `expires =`).
A malformed or unknown-key caveat fails closed (denied).

Absent caveat = unrestricted on that axis. A token with **no** caveats is
full-access (what the embedded app/daemon use). Multiple `action`s allowed.
Verbs map to endpoint groups (below). `once`/nonce (one-time-use) is **deferred**
to the capability-URL phase (needs server state; see Revocation).

## Per-route authorization map

Enforcement is a **mapping + check** layer, not an endpoint rewrite — the REST
surface already encodes resource identity in the path. Each operation declares a
`RouteAuthz` descriptor:

```
RouteAuthz {
  action: Action,                 // read | send | tag | move | manage
  resource: ResourceExtractor,    // which path params identify account/mailbox/message
  scope_mode: Gate | Filter,      // see below
}
```

- `action` — the verb the operation represents (GET → read; send command → send;
  set-keywords → tag; mailbox commands → move; accounts/settings → manage).
- `resource` — extracts `(account?, mailbox?, message?)` from the matched route's
  path params (e.g. `/sources/{source_id}/messages/{message_id}` → account, message).
- `scope_mode` — Gate vs Filter (below).

**Representation:** a central `authz_map` (one entry per `operationId`), kept
next to the route table, cross-checked in a test against the OpenAPI `paths` so a
new route without an authz entry fails CI (the same drift-test taste as
`openapi_contract`). Considered embedding it in the `#[utoipa::path]` macro;
keeping it a separate table is cleaner to review as a security artifact.

## Gate vs Filter (the list-endpoint question)

- **Gate** (most endpoints): allow/deny the whole request. The resource is in the
  path, so checking `caveat.account == path.source_id` etc. is exact. Applies to
  all `/sources/{source_id}/…`, single-resource GETs, and command endpoints.
- **Filter** (aggregate endpoints): `GET /views/conversations`,
  `GET /messages/search`, `GET /sidebar` return cross-account data. **Phase-1
  policy: require-matching-filter** — if the token is scoped to account X, the
  request must carry the matching filter (`?source_id=X` / equivalent) or it's
  rejected; the endpoint already supports these filters. Result-side filtering
  (server injects the scope) is a later enhancement. Truly global endpoints
  (`PATCH /settings`, `/sidebar` whole-tree) require the `manage` action and are
  **not** finely grantable — a product limitation, documented, not a rewrite.

This is the only place endpoints may need work: ensuring each aggregate endpoint
*has* the filter a caveat needs (account today; mailbox may need adding).

**Stage-B mapping decisions (judgment calls, confirmed against `openapi.json`):**
- `GET /sidebar` carries **no** query filter, so it is a **global Gate read**,
  not a Filter — an account-scoped token is (correctly) rejected on it. (The
  design listed `/sidebar` as Filter; with no filter param available it can only
  be a global read in Phase 1.)
- `GET /messages/search` has no `sourceId` filter param, so it is also a global
  read: an account-scoped token cannot be satisfied there.
- `GET /sources/{source_id}/messages` is a **Gate** (account from the path), even
  though it accepts an optional `mailboxId` query filter — the account axis is
  exact from the path, not the query.
- The SSE `GET /events` stream is a **Filter** route keyed on `accountId`
  (+ `mailboxId`): the handler result-side filters by these, so an `account=X`
  token is accepted with a matching `?accountId=X` and rejected otherwise.
- **Conversation lists are GLOBAL reads in Phase 1.** `GET /views/conversations`
  and `GET /smart-mailboxes/{id}/conversations` were initially mapped as Filter
  routes keyed on `sourceId`/`mailboxId`, but a security review found their
  handlers do **not** result-side filter by source/mailbox in every branch
  (`/views/conversations`' search `q` branch drops the filter;
  `/smart-mailboxes/{id}/conversations` ignores it entirely), so declaring a
  query axis as the satisfier would let an `account=X` token read **all**
  accounts' conversations. The safe, design-aligned fix (result-side scoping is
  a Phase-2 item with capability URLs) is to map both with **no resource axis**
  (`ResourceShape::empty()`): an `account`/`mailbox` caveat is unsatisfiable →
  account-scoped tokens get **403**, while a full-scope token (no caveats, fast
  path) and an `action=read` token (no resource caveat) still read them. This
  matches how `GET /messages/search` and `GET /smart-mailboxes/{id}/messages` are
  already mapped (global reads, no source filter exposed). The handlers' query
  logic is unchanged; proper per-account conversation scoping is deferred to
  Phase 2. SECURITY: do not re-add a query axis to these routes until the handler
  enforces source/mailbox scoping in every branch.
- Global management/read endpoints with no scopable resource axis
  (`/accounts` list + create, `PATCH /settings`, `/automation-rules:preview`,
  `/sender-addresses`, smart-mailbox definitions, `/config:reload`, oauth) take a
  resource caveat as **unsatisfiable** → such tokens are rejected, as intended.

## Verification flow

Replaces the `constant_time_eq` token check in `auth.rs`:
1. Perimeter unchanged: `require_auth`, Host allowlist, Origin allowlist run first.
2. Extract the macaroon from `Authorization: Bearer`. (The `?access_token=`
   query-param transport was removed — the SSE stream and browser-loadable reads
   now `fetch()` with the header. **Capability URLs** will deliberately
   re-introduce a query-param token, but as a distinct, narrowly-scoped,
   short-lived share grant rather than the full-scope session token; gate it on
   the same exposure hardening.)
3. Verify the HMAC chain against the **root key**; reject if invalid.
4. Resolve the request's `(action, account?, mailbox?, message?)` from the
   `authz_map` entry for the matched route + path params.
5. Evaluate every caveat against that context + `now`; reject (403) on any
   unsatisfied caveat (expired, wrong account, action not permitted, …).
6. For Filter routes, additionally require the matching query filter.

## Root key & minting

- **Root key**: a per-install HMAC secret. Stored in the **OS keyring** (reusing
  `SystemSecretStore`) so it isn't a plaintext file; generated once on first run.
- **Full-scope macaroon** (embedded app / daemon): minted from the root key with
  only an `expires` caveat (long, refreshed). Replaces today's random token:
  injected into the webview as `__POSTHASTE_TOKEN__`, written to `daemon.json`.
- **Attenuation**: holders add caveats client-side (no key needed) — the macaroon
  superpower. Minting *new* root macaroons needs the root key (server-side only).
- **Agent/share issuance** (IMPLEMENTED): `POST /v1/auth/tokens`, `Manage`-gated
  with no resource axis (so only a full-scope / unscoped-`manage` caller reaches
  it). Request body: `actions?`, `account?`, `mailbox?`, `message?`,
  `expiresInSeconds?` (all narrowing); response `{ token, expiresAt? }`. The
  handler **attenuates the CALLER's own presented token** (plumbed via the
  `PresentedToken` request extension set in `auth.rs`) rather than minting fresh
  from the root key — so a minted token can only narrow, never widen, the
  caller's authority, regardless of what scope is requested. When `require_auth`
  is off there is no caller token, so it mints from the root key with the
  requested caveats. (CLI `posthaste token attenuate` remains the offline
  convenience; same caveat format.)

## Migration from the shipped random token

The random per-process token shipped in dogfood.23. Two options (see sign-off):
the embedded app + daemon switch to a full-scope macaroon. Since everything is
pre-release dogfood, a **hard cutover** (macaroon only) is viable and simplest;
**accept-both** (legacy token OR macaroon during a transition) is safer but
carries two code paths.

## Revocation

Macaroons are stateless → no per-token revocation without server state. Phase-1:
**expiry only** (short caveats for anything shared). Later (capability-URL phase):
an optional revocation list (revoke by macaroon identifier) and/or root-key
rotation (revokes everything). Documented as a known tradeoff.

## Phasing

- **Phase 1 (this effort):** macaroon root key (keyring) + full-scope mint +
  verify + the caveat model + the per-route authz map + Gate/Filter for the ~3
  aggregate endpoints + migrate the perimeter token → macaroon. Enforced for the
  **local/trusted** case. Security-reviewed + live-verified. NO capability-URL UX,
  NO non-loopback exposure yet.
- **Phase 2 (later milestone):** agent/share issuance UX + capability URLs
  (token-in-URL beyond `/events`) + exposure hardening (TLS/Tailscale, rate
  limiting, boundary validation, Host allowlist for the tailnet, revocation list).

## Resolved decisions (2026-05-30, signed off)

1. **Migration: hard cutover** — macaroon only; remove the legacy random token.
   Pre-release dogfood, one code path.
2. **Issuance: defer the `POST /v1/auth/tokens` mint endpoint to Phase 2.** Phase 1
   mints the full-scope token internally; narrow tokens are derived by **client-side
   attenuation** via a small CLI (`posthaste token attenuate …`) — no root key
   needed, enough to test and hand-issue scoped tokens.
3. **Authz map: a separate central table**, CI-cross-checked against the OpenAPI
   `paths` (missing entry fails CI). Reviewable as one security artifact.
4. **Action verbs: `{read, send, tag, move, delete, manage}`** — `delete`
   (destroy) split out from `manage` for least privilege.

## Success criteria

- A full-scope macaroon drives the app exactly as the random token did (no UX
  regression); verified live.
- An attenuated token (account X, read-only, short expiry) is accepted for
  in-scope requests and **rejected** (403) for out-of-scope ones — proven by tests.
- Every `/v1` operation has an `authz_map` entry (CI-enforced); aggregate
  endpoints honor the matching-filter rule.
- Security review confirms caveat enforcement is sound before any exposure.

## Related

- [[trust-model]] — the perimeter this builds on; capability scoping was its
  deferred piece.
- `docs/L1-api` — the endpoint/resource model the authz map maps onto.
