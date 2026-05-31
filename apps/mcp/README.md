# posthaste-mcp

A thin [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server
that lets agents drive [Posthaste](../../README.md) over its documented `/v1`
REST API. It is a **downstream adapter over the OpenAPI contract**, not a
competing interface: tools map 1:1 to documented operations and the input/output
types are generated from the same `openapi.json` the web client uses.

See [`docs/eph/DESIGN-L1-mcp-adapter.md`](../../docs/eph/DESIGN-L1-mcp-adapter.md)
for the design.

## What it is

- TypeScript on [bun](https://bun.com), using `@modelcontextprotocol/sdk` over
  the **stdio** transport — the standard for a locally-launched MCP server that
  an agent host spawns as a subprocess.
- A client of the Posthaste **daemon**. It discovers the endpoint + token and
  forwards each tool call to the daemon's `/v1` API.

## Running

The server speaks MCP over stdio, so you normally configure it as a server in
your MCP host (Claude Desktop, etc.) rather than running it by hand. The command
is:

```sh
bun run apps/mcp/src/index.ts
```

(or, from inside `apps/mcp`, `bun run start`).

Example MCP host config entry:

```jsonc
{
  "mcpServers": {
    "posthaste": {
      "command": "bun",
      "args": ["run", "/abs/path/to/apps/mcp/src/index.ts"]
    }
  }
}
```

On startup it prints the resolved connection to stderr. If no daemon can be
found it exits non-zero with an actionable message telling you to start
`posthaste serve`.

## How it connects

The base URL + bearer token are resolved in this order:

1. **Env vars** — `POSTHASTE_API_URL` (used verbatim; include the `/v1` prefix)
   and `POSTHASTE_TOKEN`.
2. **Daemon port-file** — `<state_root>/daemon.json`, written by the daemon as
   `{ "port": number, "token": string }`. The base URL becomes
   `http://127.0.0.1:<port>/v1`.

`state_root` mirrors `crates/posthaste-server/src/config.rs`:

- `POSTHASTE_STATE_ROOT` if set, else
- `$XDG_DATA_HOME/posthaste`, else
- `~/.local/share/posthaste`.

(Note: the server uses `XDG_DATA_HOME` — **not** `XDG_STATE_HOME` — and applies
the same XDG fallback on every platform, including macOS; there is no
`~/Library/Application Support` special-case.)

Requests send `Authorization: Bearer <token>` when a token is resolved, so the
adapter works whether or not the daemon has `require_auth` enabled. Fetching
`127.0.0.1` sets a loopback `Host` implicitly.

## Tools

Each tool maps to one documented `/v1` operation. Inputs are validated with
`zod`; results are returned as JSON text content; API errors (the typed
`ApiErrorBody`, surfacing `code` + `message`) are reported as tool errors.

| Tool                 | Operation                                                          |
| -------------------- | ----------------------------------------------------------------- |
| `list_accounts`      | `GET /accounts`                                                   |
| `read_mail_navigation` | `POST /read` typed batch for accounts, mailboxes, smart mailboxes, and tags |
| `list_conversations` | `GET /views/conversations`                                        |
| `get_conversation`   | `GET /views/conversations/{id}`                                   |
| `search_messages`    | `GET /messages/search`                                            |
| `get_message`        | `GET /sources/{sourceId}/messages/{messageId}`                   |
| `set_keywords`       | `POST /sources/{sourceId}/commands/messages/{messageId}/set-keywords` |
| `move_to_mailbox`    | `POST /sources/{sourceId}/commands/messages/{messageId}/add-to-mailbox` |
| `send_message`       | `POST /sources/{sourceId}/commands/send`                         |

This is the initial, representative slice; additional tools are additive.

## Capability scoping caveat (important)

**The adapter uses the daemon token, which today grants _full access_.** The
capability-scoping model is designed but not yet implemented (PLAN P4 — see
`docs/eph/DESIGN-L1-trust-model.md`). This is the most important agent-facing
safety gap: a prompt-injected agent driving this server can do **anything the
token can**, including `send_message`. Until P4 lands and the MCP server can
request/carry a narrow scope, it is appropriate only for **trusted-local** use.

## Development

```sh
bun install              # from repo root or this dir (workspace member)
bun run api:generate     # regenerate src/schema.gen.ts from ../../openapi.json
bun run typecheck        # tsc --noEmit
bun run build            # tsc emit to dist/
```

`src/schema.gen.ts` is generated — do not hand-edit it (it is in
`.prettierignore`).
