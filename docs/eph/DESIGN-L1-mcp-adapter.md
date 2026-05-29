---
scope: L1
type: DESIGN
lifecycle: ephemeral
summary: "MCP adapter: a thin stdio MCP server over the documented /v1 API for agents"
modified: 2026-05-29
reviewed: 2026-05-29
depends:
  - path: docs/eph/PLAN-L1-public-api-platform
  - path: docs/eph/DESIGN-L1-trust-model
  - path: docs/eph/DESIGN-L1-runtime-topology
dependents:
  - path: docs/eph/PLAN-L1-public-api-platform
---

# DESIGN: MCP adapter (PLAN P5)

## Why

The platform goal includes letting agents drive Posthaste. MCP (Model Context
Protocol) is the standard surface for that. Per the plan, the MCP server is a
**downstream adapter over the OpenAPI'd `/v1` API**, not a competing interface —
it scaffolds from the same contract, so it inherits the typed surface for free.

## Decisions

- **Location:** `apps/mcp` — a new bun workspace member (TypeScript), alongside
  `apps/web`/`apps/site`.
- **Runtime/SDK:** TypeScript on bun, `@modelcontextprotocol/sdk`, **stdio**
  transport (the standard for a locally-launched MCP server an agent host spawns).
- **Types from the spec:** generate `schema.gen.ts` from the committed
  `openapi.json` (same `openapi-typescript` as the web client) — the adapter does
  not hand-mirror the contract.
- **Connection (ties to P4 + topology):** the adapter is a client of the
  **daemon**. It discovers the endpoint + token from the `daemon.json` port-file
  (`<state_root>/daemon.json` = `{ port, token }`) written by the daemon, with env
  overrides (`POSTHASTE_API_URL`, `POSTHASTE_TOKEN`) for flexibility. It sends
  `Authorization: Bearer` and a valid loopback `Host`, so it works whether or not
  `require_auth` is enabled. Programmatic access implies daemon mode (see
  [[runtime-topology]]).
- **Tools (initial, representative — not exhaustive):** read-oriented + core
  actions: `list_accounts`, `get_sidebar`, `list_conversations`, `get_conversation`,
  `search_messages`, `get_message`, `set_keywords`, `move_to_mailbox`,
  `send_message`. Each maps 1:1 to a documented operation, with input/output typed
  from the generated schema. Additional tools are additive.

## Capability scoping (deferred — depends on P4 sign-off)

The adapter currently uses the daemon token, which today grants **full access**
(capability scoping is designed but unimplemented — see [[trust-model]]). This is
the most important agent-facing safety gap: a prompt-injected agent driving the
MCP server can do anything the token can. Once the P4 capability model is chosen
and built, the MCP server must request/carry a **narrow scope** (e.g. read+tag,
not delete/send) and surface the granted scope to the agent. Until then, the MCP
server is appropriate only for **trusted-local** use. This is flagged, not solved.

## Phasing

- **Now:** `apps/mcp` package, stdio server, daemon.json discovery, the ~9 core
  tools above, generated types, README. Typechecks/builds; a smoke that it starts
  and lists tools.
- **After P4 sign-off:** capability-scoped tokens; full tool coverage; tool-level
  scope declarations mirroring the chosen authz model.

## Related

- [[trust-model]] — the token the adapter carries; the capability gap.
- [[runtime-topology]] — daemon vs embedded; the port-file.
- `docs/eph/PLAN-L1-public-api-platform` — P5 phase.
