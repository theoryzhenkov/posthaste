# @posthaste/tools — `posthastectl` and the MCP server

One package, one operation registry, two front-ends over the integrated
app's localhost API:

- **`posthastectl`** (`src/cli.ts`) — the CLI. Every registry operation is a
  subcommand; `events` / `watch` stream the SSE feed.
- **`posthastectl mcp`** (`src/mcp.ts`) — the same operations served as MCP
  stdio tools for agent hosts.

Both discover the backend through `connection-info.json` (`{port, token}`)
in the app's state root (`POSTHASTE_STATE_ROOT` > `$XDG_DATA_HOME/posthaste`
> `~/.local/share/posthaste`), overridable via `POSTHASTE_API_URL` /
`POSTHASTE_TOKEN`.

## The trust model — read this before connecting anything

- **Possession of `connection-info.json` is the trust boundary.** The file
  is written owner-only (0600) and the secret rotates every app launch, but
  whoever holds the token holds your mail session.
- **The session secret grants the FULL surface.** Scoped capability tokens
  ("read only", "one account") are specified in the API design but **not
  implemented**. Anything you connect — a script, an MCP agent host — can
  read, write, and **send** across every account. The MCP server repeats
  this warning on stderr at startup. A prompt-injected agent holding this
  connection can send mail as you; host-side tool allow-lists and
  user-controlled triggers are the only mitigations today.
- **Secrets never travel on argv.** There is no `--token` flag; the token
  comes only from the discovery file or the environment, and is never echoed
  into logs or error output. Keep it that way in any change here.

## Contracts worth knowing

- **Events have no replay.** `GET /events` is one broadcast: every message
  carries the current store generation (level-triggered, loss-proof); event
  payloads are prompts, not a ledger. `watch` is at-most-once — reconcile
  through queries when completeness matters.
- **Writes are idempotent by client id.** Every command carries a
  client-minted ULID; retrying the same id is safe. `watch --exec` passes
  the payload only via stdin and `PH_*` env vars, never interpolated into a
  command line.
- **Wire types are generated.** Everything under `@posthaste/protocol/gen`
  comes from the models crate's `export-ts`; never hand-write wire shapes.

## Development

```sh
bun run cli -- accounts list   # run the CLI from source
bun run mcp                    # run the MCP server from source
bun run check                  # typecheck + tests (includes the guide smoke test)
bun run build:cli              # compile the standalone binary
```
