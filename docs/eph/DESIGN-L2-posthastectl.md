---
scope: L2
summary: "Design for posthastectl — a scriptable CLI over the existing API surface, built as a SECOND front-end alongside the MCP server over one shared operation registry + API client (in apps/mcp). No new API surface; a human (CLI) and an LLM (MCP) get identical capabilities by construction. Adds an event tap for reactive scripting; ships as a standalone binary via bun --compile."
modified: 2026-06-28
reviewed: 2026-06-28
state: planned
depends:
  - path: docs/api/L1
    section: "OpenAPI surface"
    local: "3. The operation registry"
  - path: docs/eph/RFC-L2-configuration-surface
    local: "1. Goal & principles"
---

# DESIGN — posthastectl (scriptable CLI, unified with MCP)

Status: **proposal** (`state: planned`). Realizes the "configurable + scriptable"
goal of [RFC-L2-configuration-surface](RFC-L2-configuration-surface.md) for
*actions* (the config RFC covered declarative settings).

## 1. Goal & principles [::state planned]

A CLI an LLM or a shell script can drive the app with — **no GUI, no harness**.
Four principles:

1. **Stateless API client** — it calls only the existing OpenAPI surface, so it
   can do exactly what the GUI can and nothing more.
2. **No new server endpoints** — *by construction*: the CLI is a rendering of the
   API operations, not a new capability layer.
3. **Scriptable I/O** — JSON in/out, meaningful exit codes, pipe-friendly,
   newline-delimited streams.
4. **Event tap** — subscribe to the event stream for reactive scripting.

Consequence: **repair / factory reset are out of scope** — those are local
desktop/renderer operations (Tauri commands + IndexedDB), not API endpoints, so
they stay GUI-only (see RFC §4). The CLI is a general API driver, not a repair
tool.

## 2. Architecture — one core, two front-ends [::state planned]

`apps/mcp` already holds both halves a CLI needs: `client.ts` (the stateless API
client + `daemon.json` discovery) and `tools/` (the operation definitions). MCP
is just *one front-end* over them. `posthastectl` is a *second front-end* over
the same core.

```
apps/mcp  (renamed conceptually to the "operations core")
├─ client.ts          API client + daemon discovery              [shared, exists]
├─ operations/        registry: { name, description, argSchema, handler }
│                     refactored from tools/{read,commands}.ts    [shared]
└─ front-ends
   ├─ index.ts        → MCP server (stdio)                        [exists]
   └─ cli.ts          → posthastectl (subcommands + `events`)     [NEW]
```

**One registry → two interfaces.** An MCP tool and a CLI subcommand are two
renderings of the same registry entry. "No new surface" and "CLI ≡ MCP" become
structural — neither front-end can drift from the other or from the API.

## 3. The operation registry [::state planned]

Today `registerReadTools`/`registerCommandTools` register tools *imperatively*
against the MCP `server` (`server.registerTool(name, schema, handler)`). The
refactor: lift each into a plain, SDK-agnostic descriptor —

```ts
interface Operation {
  name: string                 // e.g. "messages.search", "messages.move"
  description: string          // doubles as MCP tool desc + CLI help
  argSchema: ZodSchema         // validates CLI flags AND the MCP tool input
  handler: (conn: Connection, args) => Promise<unknown>  // the HTTP call
}
```

- The argument schemas are already contract-bound to `schema.gen.ts` (generated
  from `openapi.json`) — so the registry can't diverge from the API.
- `index.ts` registers each `Operation` as an MCP tool (today's behavior,
  unchanged for consumers).
- `cli.ts` renders each `Operation` as a subcommand: `name` → command path
  (`messages search`), `argSchema` → flags (with `--json` stdin for complex
  inputs), `handler` result → JSON on stdout.

The split between read and command operations is preserved (it maps to safe vs
mutating, useful for both MCP annotations and CLI confirmations).

## 4. The CLI front-end — posthastectl [::state planned]

**Commands** (mirror the registry; examples):
`posthastectl mailboxes list` · `messages search <query>` · `messages get <id>` ·
`messages move|flag|... <id>` · `send` (body via `--json -` / flags) ·
`sync <account>` · `events --follow`.

**Connection (stateless).** Reuses `client.ts::resolveConnection`: default
**auto-discovers the local daemon** via `daemon.json` (`{port, token}` under the
state root); `--base-url` + `--token` (or env) override for a remote daemon. No
stored profile of its own. (Reading the desktop's `connections.json` profiles is
a later option, not v1.)

**I/O.** JSON on stdout (`--json` for raw, default pretty for a TTY); errors +
diagnostics on stderr; non-zero exit on API error (carry `ApiErrorBody.code`).
Complex inputs via `--json -` (stdin) so an LLM/script can pipe a payload.

**Event tap.** `posthastectl events --follow` streams the **domain-event SSE**
(`GET /v1/events`, `after_seq`-cursored — "mail arrived / sync finished") as
newline-delimited JSON for `while read` / `jq` pipelines. `--after-seq N`
resumes; `--topic`/`--account` filter (the existing `EventFilter`). This is the
scriptable "what happened" feed; the lower-level runtime down-channel SSE
(`link.rs`, view-frame assertions) is **not** exposed (view-internal — see §8 Q1).

## 5. Distribution [::state planned]

`bun build --compile` produces a standalone `posthastectl` executable — no Node
runtime to ship — so the "single static binary" benefit doesn't require Rust.
The MCP server continues to run under bun/node as today.

## 6. Decisions (settled) [::state planned]

1. **TS, in `apps/mcp`, not a new Rust binary.** Reusing `client.ts` + the
   operation registry beats duplicating them in Rust; `bun --compile` still
   yields a single binary. (Revises the earlier Rust lean.)
2. **Unify, don't nest.** MCP and the CLI are sibling front-ends over one
   registry — not "MCP as a mode of the CLI" and not a Rust rewrite of the
   mature 5.7k-LOC SDK-based MCP server.
3. **Connection = discovery + flags** (stateless); profiles deferred.
4. **Event tap = domain-event SSE** (`/v1/events`); runtime frame stream not
   exposed.
5. **No new API surface; CLI ≡ MCP** — guaranteed by the shared registry.

## 7. Refactor / migration plan [::state planned]

- **P1 — extract the registry.** Convert `tools/{read,commands}.ts` from
  `server.registerTool(...)` calls into an exported `Operation[]`; make
  `index.ts` register them (no behavior change for MCP consumers; the existing
  MCP tests must stay green).
- **P2 — the CLI front-end.** `cli.ts`: arg-parser (e.g. a small command tree)
  that renders the registry as subcommands + `--json`/exit-code I/O; wire
  `bun build --compile`.
- **P3 — the event tap.** `events --follow` over the domain-event SSE
  (NDJSON, `--after-seq`/`--topic`/`--account`).
- **P4 — packaging.** Ship the compiled binary in the release; docs +
  shell-completion (registry-derived).

## 8. Out of scope & open questions [::state planned]

- **Out of scope:** repair/factory-reset (local, not API); the desktop
  `connections.json` profile store (v1 uses discovery + flags); any new endpoint.
- **Q1 — event stream:** domain-event SSE assumed for `events --follow`; confirm
  the runtime down-channel (view frames) is genuinely not wanted for scripting.
- **Q2 — auth scope:** the CLI inherits the token's full power (an LLM with
  `posthastectl` can do anything the GUI can, incl. send/delete) — acceptable for
  a power tool, but worth a note in user docs.
- **Q3 — naming:** keep `apps/mcp` as the package, or rename to reflect it now
  hosts both front-ends (e.g. `apps/cli` / `apps/agent-bridge`)?
