---
scope: L2
summary: "Design for posthastectl — a scriptable CLI over the existing API surface, built as a SECOND front-end alongside the MCP server over one shared operation registry + API client (in apps/mcp). No new API surface; a human (CLI) and an LLM (MCP) get identical capabilities by construction. Adds an event tap for reactive scripting; ships as a standalone binary via bun --compile."
modified: 2026-06-29
reviewed: 2026-06-29
state: implemented
depends:
  - path: docs/api/L1
    section: "OpenAPI surface"
    local: "3. The operation registry"
  - path: docs/eph/RFC-L2-configuration-surface
    local: "1. Goal & principles"
---

# DESIGN — posthastectl (scriptable CLI, unified with MCP)

Status: **implemented** (CLI + shared registry built 2026-06-29). Realizes the
"configurable + scriptable" goal of
[RFC-L2-configuration-surface](RFC-L2-configuration-surface.md) for *actions*
(the config RFC covered declarative settings).

## 0. Implementation status [::state implemented]

Built in `apps/mcp` (one registry, two front-ends):

- **Registry** — `src/operations/{read,commands,types}.ts`: each operation is a
  front-end-agnostic `Operation` (`mcpName`, `description`, `argSchema`,
  `handler`, `cli` binding). `src/index.ts` (MCP) and `src/cli.ts` (CLI) both
  render it. 11 operations (the original 9 MCP tools + additive `list_mailboxes`
  and `trigger_sync`).
- **MCP** — unchanged tool names (a documented contract); read ops now carry
  `readOnlyHint`. Verified by `test/operations.test.ts`.
- **CLI** — `posthastectl`: subcommands from the registry, kebab flags + a
  primary positional, `-i/--input` (inline / `-` stdin / `@file`), pretty-vs-
  compact JSON, exit codes `0/2/3/4`. Pure `run(argv, deps)` core, fully tested
  (`test/cli.test.ts`, stubbed fetch — no daemon needed). `just mcp build-cli`
  compiles a standalone binary via `bun build --compile`.
- **`watch`** — the turnkey "run a script on new mail" runner over the event tap
  (`cli/watch.ts`): arrival-gate → fetch detail → `--keyword`/`--mailbox` filter
  → `--exec` (detail on stdin + `PH_*` env) or emit JSON; `--cursor` resume.
  Tested in `test/watch.test.ts` + a live mock-daemon smoke. See §4.
- **`just mcp {check,test,build-cli}`** wired into the root `check`/`test`/`fmt`.

**Event tap — `GET /v1/events` restored (2026-06-29).** `posthastectl events`
streams the daemon's domain-event SSE as NDJSON. The route had been **removed**
as vestigial (commit `cce95402c`, 2026-06-25) while it had no consumer — three
days before this design was reviewed; the runtime machinery
(`runtime.subscribe_events` → replay + live broadcast) stayed intact, so only the
thin HTTP handler + route needed restoring. It is the **flat, view-less
projection of the same `DomainEvent` broadcast** the UI consumes in view-coupled
form (the runtime session stream's `Notification` frames), and `posthastectl` is
its consumer. Restored with two upgrades over a verbatim revert: (1) a
"why this exists" doc-comment at the handler so it is not GC'd again as "no
consumer"; (2) classified as a **read** capability in the authz read-route table
(it was previously under `commands.rs`, odd for a pure read). `openapi.json` +
both `schema.gen.ts` regenerated; the deleted capability-scoping tests restored.
This is the one place the design touches the Rust server (a deliberate, user-
ratified exception to principle §6.5 — it restores a documented contract, not a
new capability).

## 1. Goal & principles [::state implemented]

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

## 2. Architecture — one core, two front-ends [::state implemented]

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

## 3. The operation registry [::state implemented]

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

## 4. The CLI front-end — posthastectl [::state implemented]

**Commands** (mirror the registry; examples):
`posthastectl mailboxes list` · `messages search <query>` · `messages get <id>` ·
`messages move|flag|... <id>` · `send` (body via `--json -` / flags) ·
`sync <account>` · `events --follow`.

**`watch` — the turnkey scripting layer (implemented).** `posthastectl watch
[--account A] [--mailbox M] [--keyword TAG] [--all-updates] [--exec CMD]
[--cursor FILE]` is a thin runner over the `events` tap, for the most common
automation ("run a script on new mail"). Per genuine arrival it fetches the
full `MessageDetail` (one call → both the tag in `.keywords` and the body in
`.bodyText`), applies the client-side `--keyword`/`--mailbox` filters, then runs
`--exec` with the detail JSON on stdin + `PH_*` env (or emits JSON without
`--exec`). `--cursor` persists the last `seq` for at-least-once resume. Design
boundary (avoid a 2nd rules engine): the CLI owns the **plumbing** (loop, resume,
arrival-gate, fetch, dispatch); the script owns the **policy** (condition +
action). The ONE semantic filter is `--keyword` (tags are the app's first-class
label); `--from`/`--subject`/regex/multi-condition stay in the user's script.
Declarative tag/move/flag belongs in the app's native automation-rules, not here.

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

## 5. Distribution [::state implemented]

`bun build --compile` produces a standalone `posthastectl` executable — no Node
runtime to ship — so the "single static binary" benefit doesn't require Rust.
The MCP server continues to run under bun/node as today.

## 6. Decisions (settled) [::state implemented]

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

## 7. Refactor / migration plan [::state partial]

- **P1 — extract the registry. ✅ done.** `tools/{read,commands}.ts` lifted into
  `operations/` as an exported `Operation[]`; `index.ts` registers them. MCP tool
  names unchanged (no existing MCP tests; their names are pinned by
  `test/operations.test.ts`). `fetch` is now injectable via `Connection.fetch`
  (global by default) so handlers are testable.
- **P2 — the CLI front-end. ✅ done.** `cli.ts` + `cli/` render the registry as
  subcommands with `-i/--input` stdin, exit codes, and pretty/compact JSON.
  `bun build --compile` wired (`just mcp build-cli`).
- **P3 — the event tap. ✅ done.** `events` streams `GET /v1/events` (SSE → NDJSON,
  `--after-seq`/`--topic`/`--account`/`--mailbox`), tested against a stubbed
  stream. The daemon route (removed as vestigial) was restored with the two
  upgrades in §0; backend gate green (openapi/asyncapi contracts,
  capability-scoping, authz-completeness, clippy).
- **P4 — packaging. ◐ binary + docs done; release wiring + completions pending.**
  `bun --compile` binary, README, and the `just mcp` recipes exist; shipping the
  binary in the release artifacts and registry-derived shell-completion are not
  yet done.

## 8. Out of scope & open questions [::state partial]

- **Out of scope:** repair/factory-reset (local, not API); the desktop
  `connections.json` profile store (v1 uses discovery + flags).
- **Q1 — event stream: resolved — restored the domain-event SSE.** Confirmed the
  right feed: the flat view-less projection of the same `DomainEvent` broadcast
  the UI consumes view-coupled (one source, two renderings — the runtime
  view-frame stream stays view-internal). `GET /v1/events` restored (§0/P3).
- **Q2 — auth scope: noted in user docs.** The CLI inherits the token's full
  power; the README capability-scoping caveat now calls this out for both
  front-ends. Tightening waits on the trust-model scoping work.
- **Q3 — naming: resolved — keep `apps/mcp`.** The package name stays; the README
  reframes it as "one registry, two front-ends" rather than renaming (avoids
  churning the MCP host config + the published package identity).
