# posthaste-mcp + posthastectl

One operation registry, **two front-ends** over the [Posthaste](../../README.md)
daemon's documented `/v1` REST API:

- **`posthaste-mcp`** — a [Model Context Protocol](https://modelcontextprotocol.io)
  server (stdio) an agent host spawns.
- **`posthastectl`** — a scriptable CLI a human or a shell/LLM script drives.

Both are downstream adapters over the OpenAPI contract, not competing
interfaces. Each operation is defined once in `src/operations/` (name,
description, `zod` arg schema, handler); the MCP server renders it as a tool and
the CLI renders it as a subcommand. "No new API surface" and "CLI ≡ MCP" are
therefore _structural_ — neither front-end can drift from the other or from the
API. See [`docs/eph/DESIGN-L2-posthastectl.md`](../../docs/eph/DESIGN-L2-posthastectl.md).

```
src/
├─ client.ts          API client + daemon discovery        [shared]
├─ schema.gen.ts      types generated from openapi.json     [shared, generated]
├─ operations/        the registry: Operation[]             [shared]
│  ├─ read.ts            read (non-mutating) operations
│  ├─ commands.ts        mutating operations
│  └─ types.ts           Operation + defineOperation()
├─ index.ts           MCP front-end (registers operations as tools)
├─ cli.ts             CLI entry (thin: wires deps + process.exit)
└─ cli/               CLI core (registry → subcommands, I/O, events tap)
```

## posthastectl (CLI)

A no-GUI driver for the daemon: JSON in/out, pipe-friendly, meaningful exit
codes — for shell scripts and LLM-driven triage/automation.

```sh
# from the repo root
just mcp cli -- accounts list
just mcp cli -- messages search "from:boss is:unread" --limit 20

# or run the source directly
bun run apps/mcp/src/cli.ts mailboxes list acct_personal

# or build the standalone binary (no runtime to ship)
just mcp build-cli            # → apps/mcp/dist/posthastectl
./apps/mcp/dist/posthastectl --help
```

**Commands** mirror the registry (`posthastectl --help` lists them):

| Command                   | Operation                                                 |
| ------------------------- | --------------------------------------------------------- |
| `accounts list`           | `GET /accounts`                                           |
| `nav`                     | `POST /read` (accounts, mailboxes, smart mailboxes, tags) |
| `mailboxes list <src>`    | `GET /sources/{src}/mailboxes`                            |
| `conversations list`      | `GET /views/conversations`                                |
| `conversations get <id>`  | `GET /views/conversations/{id}`                           |
| `messages search <q>`     | `GET /messages/search`                                    |
| `messages get`            | `GET /sources/{src}/messages/{id}`                        |
| `messages set-keywords`   | `POST .../messages/{id}/set-keywords`                     |
| `messages add-to-mailbox` | `POST .../messages/{id}/add-to-mailbox`                   |
| `messages send`           | `POST /sources/{src}/commands/send`                       |
| `sync <src>`              | `POST /sources/{src}/commands/sync`                       |
| `events`                  | `GET /v1/events` (SSE → NDJSON; see below)                |

### I/O contract (scriptable)

- **Output:** JSON on stdout — pretty on a TTY, compact (one line) when piped.
  `--pretty` / `--compact` force either.
- **Input:** scalar args as `--kebab-flag value`; a single positional fills the
  command's primary arg (e.g. `messages search "hello"` → `--q`). Complex
  (array/object) args are JSON — pass the flag value directly
  (`--to '[{"email":"a@b.com"}]'`) or seed the whole args object with
  `-i/--input` (inline JSON, `-` for stdin, or `@file`). Explicit flags override
  `--input`.
- **Exit codes:** `0` ok · `2` usage error · `3` connection error · `4` API
  error (the message carries `ApiErrorBody.code`) · `1` unexpected.

```sh
# pipe a payload from an LLM/script
echo '{"sourceId":"acct","messageId":"m1","add":["\\Seen"],"remove":[]}' \
  | posthastectl messages set-keywords -i -
```

### Event tap

`posthastectl events` streams the daemon's domain-event SSE (`GET /v1/events`,
`afterSeq`-cursored) as newline-delimited JSON — the "what happened" feed for
reactive scripting:

```sh
posthastectl events --topic sync.completed | while read -r e; do
  echo "$e" | jq -r '.accountId'
done
```

`--after-seq N` resumes (the server replays matching backlog, then goes live);
`--topic` / `--account` / `--mailbox` filter. The lower-level runtime view-frame
stream is intentionally **not** exposed (view-internal).

> `events` consumes the daemon's `GET /v1/events` SSE — the flat, view-less
> projection of the same `DomainEvent` broadcast the UI consumes in view-coupled
> form (the runtime session stream's `Notification` frames). See
> DESIGN-L2-posthastectl §0/§3.

## posthaste-mcp (MCP server)

Speaks MCP over **stdio**, so you configure it as a server in your MCP host
rather than running it by hand:

```sh
bun run apps/mcp/src/index.ts      # (or, in apps/mcp, `bun run start`)
```

```jsonc
{
  "mcpServers": {
    "posthaste": {
      "command": "bun",
      "args": ["run", "/abs/path/to/apps/mcp/src/index.ts"],
    },
  },
}
```

Each registry operation is a tool (the MCP tool names — `list_accounts`,
`search_messages`, … — are a stable documented contract). Read operations carry
the `readOnlyHint` annotation. On startup it prints the resolved connection to
stderr and exits non-zero with an actionable message if no daemon is found.

## How both connect

The base URL + bearer token resolve in this order (CLI flags override; MCP uses
discovery only):

1. **CLI flags** — `--base-url` (used verbatim; include `/v1`) and `--token`.
2. **Env vars** — `POSTHASTE_API_URL`, `POSTHASTE_TOKEN`.
3. **Daemon port-file** — `<state_root>/daemon.json` (`{ port, token }`); base
   URL becomes `http://127.0.0.1:<port>/v1`.

`state_root` mirrors `crates/posthaste-server/src/config.rs`:
`POSTHASTE_STATE_ROOT`, else `$XDG_DATA_HOME/posthaste`, else
`~/.local/share/posthaste` (the server uses `XDG_DATA_HOME` — **not**
`XDG_STATE_HOME` — on every platform, including macOS).

## Capability scoping caveat (important)

Both front-ends use the daemon token, which today grants **full access**. The
capability-scoping model is designed but not yet implemented (see
`docs/eph/DESIGN-L1-trust-model.md`). A prompt-injected agent — via MCP _or_
`posthastectl` — can do anything the token can, including `send_message`. Until
scoping lands, this is appropriate only for **trusted-local** use.

## Development

```sh
just mcp check          # typecheck + bun test + prettier --check
just mcp test           # registry + CLI tests (bun test)
just mcp typecheck      # tsc --noEmit
just mcp build-cli      # compile dist/posthastectl
just mcp api-generate   # regenerate src/schema.gen.ts from ../../openapi.json
```

`src/schema.gen.ts` is generated — do not hand-edit it (it is in
`.prettierignore`).
