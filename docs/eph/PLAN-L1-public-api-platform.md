---
scope: L1
type: PLAN
lifecycle: ephemeral
summary: "Adopt a documented API standard (OpenAPI + AsyncAPI) so the backend is a standalone, integrable platform"
modified: 2026-05-29
reviewed: 2026-05-29
depends:
  - path: docs/L0-api
  - path: docs/L1-api
  - path: docs/L2-transport
  - path: docs/L1-sync
---

# PLAN: Public API platform

## Why

Posthaste's backend is intended to be a **standalone, documented API** — not just
a private channel for the bundled web/desktop UI. The goal is a foundation others
build on: run the daemon separately, write any client in any language, let agents
drive it, expose an **MCP** server, and **subscribe to daemon-emitted events** to
trigger local automation. That only works if the frontend↔backend contract is a
first-class, documented, versioned product surface rather than a hand-rolled
implementation detail.

This plan is large and cross-cutting (touches the wire contract, type generation,
and the security/trust model). **Orchestrate it in a dedicated jj workspace**, not
the cleanup loop. This file is the design record; implementation is phased below.

## Current state (2026-05-29)

- `axum` HTTP server, **38 routes under `/v1`** (already versioned — good), plus an
  **SSE `/events`** stream (`crates/posthaste-server/src/lib.rs`, `api.rs`).
- **Hand-mirrored types**: `apps/web/src/api/types.ts` is maintained by hand against
  the Rust domain types — no codegen. This is the root cause of recurring drift
  (mailbox roles, error codes).
- **Stringly-typed errors at the boundary**: backend has a typed `ServiceErrorKind`
  (`model/mod.rs:1601`, ~14 kinds) **plus ad-hoc validator code strings** in the API
  layer (`invalid_query`, `invalid_mailbox`, `invalid_compose`, `invalid_oauth_request`,
  …). Frontend `ApiError.code` is `string | undefined` (`api/errors.ts`), so all type
  information is discarded at the wire.
- **Trust model: localhost-only, no auth.** Responses are not runtime-validated
  because the only client is first-party. Opening the API changes this fundamentally.

## Decision

Adopt **schema-as-contract**, **code-first** (Rust stays the single source of truth):

1. **OpenAPI for the REST surface** via `utoipa` (axum-native; `aide` is the
   alternative). Annotate existing handlers + response structs → emit `openapi.json`.
   No rewrite — annotate what exists.
2. **AsyncAPI for the `/events` SSE stream** — document the event-driven contract
   (event topics, payloads) so automation clients can subscribe against a spec.
3. **Generate, don't hand-mirror**: `openapi-typescript` (or the chosen generator)
   produces `api/types.ts` + a typed client from the spec. Retire the hand-written
   mirror. This resolves the type-drift and error-code threads in one move.
4. **MCP server is a downstream adapter** over the OpenAPI'd API (can be scaffolded
   from the spec) — complementary to OpenAPI, not a competing standard.

Rejected: gRPC/protobuf (not curl/agent-friendly, proto toolchain, browser shims),
GraphQL (resource model maps cleanly to REST; query flexibility not needed here).
Both are ceremony for a first-party-plus-integrators HTTP+JSON API.

## Typed error model (folds in the error-typing thread)

1. Consolidate the backend code space into **one canonical enum** — fold the ad-hoc
   API-layer validator strings into `ServiceErrorKind` (or a dedicated `ApiErrorCode`
   enum). The backend should stop being half-stringly-typed too.
2. Surface that enum in the OpenAPI error schema.
3. Generated TS gives `ApiError.code: ApiErrorCode` (a discriminated union), so
   clients `switch` exhaustively instead of matching magic strings. NB: prefer a
   union on `code` over a TS class hierarchy — errors cross JSON and class instances
   don't survive `response.json()`.

## Trust model / security — REQUIRED, not optional

Opening the API to arbitrary clients/agents is a security shift, not just docs:

- **Authentication**: API tokens / keys / OAuth — who is calling?
- **Authorization + capability scoping**: agents must get *narrow* grants (e.g.
  read + tag, not blanket delete). This is the most important agent-facing decision.
- **Boundary input validation**: the `as`-cast trust becomes a real risk once
  callers are untrusted.
- **Rate limiting** and a **stability/versioning policy** (build on `/v1`).

## Phases

- **P0 — Design & decisions** (this doc + open questions below resolved).
- **P1 — OpenAPI emission**: `utoipa` annotations across the 38 routes; serve
  Swagger UI / Redoc; commit `openapi.json` as a contract artifact.
- **P2 — Generated TS client**: generate `api/types.ts` + client from the spec;
  retire the hand-mirror; ship the typed `ApiErrorCode` union (with backend code
  consolidation).
- **P3 — AsyncAPI for `/events`**: document the SSE event contract.
- **P4 — Trust model**: authn + capability-scoped authz + boundary validation +
  rate limiting.
- **P5 — MCP adapter**: thin MCP server over the documented API.

## Open questions (resolve before P1)

- **Auth scope now or later?** Near-term: documented API for *trusted local*
  clients + own MCP, with P4 as fast-follow — OR external multi-client auth in scope
  from the start? (This single answer sizes the whole effort.)
- **Generated artifacts**: commit `openapi.json` + generated TS, or build them in
  CI? (Recommend: commit the spec, generate TS in build, contract-test agreement —
  consistent with the existing `check-logging-contract.ts` taste.)
- **Generator choice**: `utoipa` vs `aide`; `openapi-typescript` vs alternatives.

## Success criteria

- `openapi.json` is the single source; `api/types.ts` is generated, not hand-written.
- A third party can generate a working client in another language from the spec alone.
- The event stream is documented well enough to subscribe without reading server code.
- Error codes are a typed union end-to-end; no magic-string matching in clients.
- A documented auth + capability model exists before the API is exposed beyond localhost.

## Related

- Drift precedent fixed in cleanup: mailbox-role single-source (commit on the
  overnight-cleanup branch). Same lesson, applied small.
- Frontend robustness: React error boundaries already added (same branch).
