---
title: Scripting quickstart — a laptop script in five minutes
description: The five-minute technical version — discover the API, query your mail, write back safely, and watch for changes with a handler script.
sidebar:
  label: Scripting quickstart
  order: 4
---

Automate Posthaste from a shell script with **no protocol code**: read your
mail with typed queries, write back with typed commands, and run a handler
when something changes. Everything goes through one localhost API — the same
endpoints the app's own UI uses — driven by `posthastectl`.

The whole flow — discover, query, watch-and-act — takes about five minutes and
never requires reading Posthaste's source.

## Before you start

You need **the Posthaste desktop app running**. The app embeds its backend
in-process; on launch the backend binds a loopback port and writes a
**discovery file** — `connection-info.json`, containing the port and a session
token — into its state root:

- `$XDG_DATA_HOME/posthaste`, defaulting to `~/.local/share/posthaste` — the
  same path on Linux and macOS.
- `POSTHASTE_STATE_ROOT` overrides the location everywhere.

`posthastectl` reads that file automatically, so you pass **no URL and no
token**. If nothing is found it tells you to start the app. When you need to
override discovery (an unusual state root, a remote tunnel), set
`POSTHASTE_STATE_ROOT` or the `POSTHASTE_API_URL`/`POSTHASTE_TOKEN` environment
variables — the token is only ever read from the file or the environment,
**never from a command-line flag**, so it can't leak into shell history or
process listings.

### Install `posthastectl`

`posthastectl` is a single standalone binary. It ships two ways:

- **Bundled with the desktop app** — inside the app's resources (on macOS:
  `Posthaste.app/Contents/MacOS/posthastectl`). Symlink it onto your `PATH`.
- **As a release asset** — download the `posthastectl` binary for your
  platform (and `SHA256SUMS` to verify it) from the matching GitHub release,
  mark it executable, and place it on your `PATH`.

From a checkout, `bun --cwd=apps/tools run cli` runs the same code without a
build.

## Honest words about auth

The token in `connection-info.json` is the **session secret**: possession of
that file is the local trust boundary, equivalent to being logged in on the
machine. It grants the **full API surface** — every query, every command,
every account. The file is written owner-readable only (`0600`), and the app
rotates the secret each launch.

Scoped, auto-expiring capability tokens — "this script may only read", "this
agent may only touch account X" — are specified in the API design and are
staged work. **They are not implemented yet.** Until they land:

- anything you hand the session token can read _and_ write all of your mail;
- never copy the token off the machine, into a config file, or into a repo;
- treat every handler and agent you connect as fully trusted.

## 1. Read: queries and the search grammar

All reads go through `POST /query` — one typed query per request, answered
with the result and the store generation it was computed at. `posthastectl`
renders the query families as subcommands and prints JSON (pretty on a
terminal, compact when piped):

```sh
posthastectl accounts list
posthastectl mailboxes list --account-id work
posthastectl messages list --account-id work --mailbox-id <mailboxId> --limit 20
posthastectl messages search 'from:billing@myvendor.com is:unread' --limit 20
posthastectl messages get <messageId> --account-id work
posthastectl threads get <threadId> --account-id work
posthastectl operations list          # the outbox: pending writes + verdicts
```

Search is not a separate endpoint — it's a mail-list filter, in the **one
query grammar** shared with smart mailboxes and in-app search:

- **Prefixed tokens** become field conditions: `from:`, `is:` (`is:unread`,
  `is:flagged`), `tag:`, `conversation:`, and friends.
- **Bare words** search sender, subject, preview, and the cached body index.
- **Terms are ANDed**: `from:billing@myvendor.com invoice` means both.
- A string the grammar rejects fails the query as malformed (exit code `4`) —
  it never silently degrades to a bare-word search.

Mail lists are **windowed**: they take `--limit` and an opaque `--cursor` and
return a screenful, never the mailbox. Page by passing the previous answer's
`nextCursor` back:

```sh
posthastectl messages search 'is:unread' --limit 50 --cursor "$next"
```

The raw wire shape, for any HTTP client — read the port and token out of the
discovery file, then post one externally-tagged query object:

```sh
info="${XDG_DATA_HOME:-$HOME/.local/share}/posthaste/connection-info.json"
port=$(jq -r .port "$info"); token=$(jq -r .token "$info")

curl -sS "http://127.0.0.1:$port/query" \
  -H "Authorization: Bearer $token" \
  -H 'content-type: application/json' \
  -d '{"mailList":{"freeText":"from:billing@myvendor.com is:unread","limit":20}}'
```

Every answer is the envelope `{ "generation": 4182, "data": { "rows": [...],
"nextCursor": null } }`. The `generation` is the store generation the answer
was computed at — it's how you correlate reads with the event stream below.

## 2. Write: commands and idempotency ids

All writes go through `POST /command` — one typed intent with a
**client-generated idempotency id**. `posthastectl` mints a fresh ULID per
invocation; retrying the **same id** is safe (the command applies once and the
replay returns the original outcome). The write verbs:

```sh
posthastectl tag <messageId> --account-id work --add reviewed    # setKeywords
posthastectl tag <messageId> --account-id work --add '$flagged'  # flag = a keyword too
posthastectl move <messageId> --account-id work --mailbox-ids <mailboxId>
posthastectl reply <messageId> --account-id work --body "Got it, thanks!"
posthastectl send --account-id work --to a@example.com --subject "Heads up" --body "..."
posthastectl mailboxes create Receipts --account-id work
posthastectl mailboxes delete --account-id work --mailbox-id <mailboxId>
posthastectl sync work                                        # trigger a sync now
```

- Every write names its account (`--account-id`) and message explicitly — inside
  a `watch --exec` handler, that's the `$PH_ACCOUNT_ID`/`$PH_MESSAGE_ID` the
  watcher already exported (see section 4).
- `move` takes `--mailbox-ids` — the full new mailbox _membership_, as
  mailbox **ids** (get them from `mailboxes list`), usually one destination.
- Any command also accepts its whole argument object as JSON via
  `--input -` (stdin) or `--input @file`; explicit flags override it.
- `--id <id>` supplies the idempotency id yourself. Do this whenever a script
  might run twice for the same trigger — derive it from the trigger, e.g.
  `--id "summarize:$PH_MESSAGE_ID"` — and a re-run becomes a safe no-op.

On the wire it's one envelope:

```sh
curl -sS "http://127.0.0.1:$port/command" \
  -H "Authorization: Bearer $token" \
  -H 'content-type: application/json' \
  -d '{"id":"01JZX6Q0V8...","command":{"setKeywords":{"accountId":"work","messageId":"m9","change":{"add":["reviewed"],"remove":[]}}}}'
```

**Acceptance is not settlement.** A `2xx` means the command is recorded and
its effect is already visible in every query at or past the returned
generation — that's how you read your own writes. Delivery to the mail
provider is asynchronous: the eventual verdict (delivered, rejected, parked)
is _state_, observed through `posthastectl operations list` (the
pending-operations query) or the `operation.settled` event. A provider
failure is never an HTTP error on the original call.

## 3. The event stream

`GET /events` is one SSE broadcast. Every message carries the current **store
generation**; most also carry a domain event, and a generation-only heartbeat
fills silences. `posthastectl events` turns it into NDJSON, one object per
line:

```sh
posthastectl events
{"generation":4183,"event":{"kind":"message.updated","accountId":"work","messageId":"m9"}}
{"generation":4184}
```

- `--generation-only` prints just the generation numbers — a pure liveness
  tap for "did anything change since generation N?" scripts.
- Filters like `--account` and `--kind` are **client-side convenience**: the
  stream is one broadcast, and the filter only narrows what your terminal
  sees.

Two guarantees, honestly stated:

- **The generation is loss-proof.** Every message states current state, so a
  dropped message heals at the next one, and reconnecting is the same code
  path as connecting. A fresh `runId` on the first message means the backend
  restarted — treat everything you hold as stale.
- **Event payloads are prompts, not a ledger.** There is no replay of missed
  events. Anything that needs completeness reconciles through queries; the
  event only tells you _when_ to look.

## 4. Watch and act

`posthastectl watch --exec` is the tap wired to a handler. On each matching
event it **refetches the message through a query** and runs your command with
the fresh `messageDetail` JSON on **stdin** and these env vars set:

`PH_ACCOUNT_ID`, `PH_MESSAGE_ID`, `PH_GENERATION`, `PH_KIND` (the event
kind, e.g. `message.updated`), `PH_KEYWORDS`, `PH_MAILBOX_IDS`.

```sh
#!/bin/sh
# handler.sh — the message detail arrives as JSON on stdin.
printf '%s\t%s\n' "$PH_MESSAGE_ID" "$PH_KEYWORDS" >> ~/mail.log
posthastectl tag "$PH_MESSAGE_ID" --account-id "$PH_ACCOUNT_ID" \
  --add logged --id "logger:$PH_MESSAGE_ID"
```

```sh
posthastectl watch --exec 'sh ./handler.sh'
```

- By default the watch fires on genuine new-message arrivals;
  `--all-updates` fires on every message change.
- `--account <id>`, `--mailbox <id>`, `--keyword <tag>` narrow what your
  handler sees. They are conveniences, not a security fence — the guidance in
  [the security guide](scripting-security.md) applies to what your handler
  _does_, not to the filters.
- The payload reaches your handler **only** as stdin JSON and `PH_*` env
  vars — never interpolated into a command line — so a booby-trapped email
  cannot inject a command through the watcher.

### Restart semantics: at-most-once, reconcile via queries

A watcher only sees events while it is running. **There is no cursor and no
replay**: if the watcher (or the app) restarts, events that fired in the gap
are gone from the stream — this is at-most-once dispatch, by design, because
the event stream carries prompts, not a ledger.

The pattern that makes this robust: **reconcile on startup, then follow.**
Query for the work that accumulated while you were away, handle it, then let
the watch handle the live tail:

```sh
# Catch up: everything tagged todo but not yet done. Rows carry the
# account (sourceId) and message id the handler needs.
posthastectl messages search 'tag:todo -tag:done' --limit 100 |
  jq -r '.data.rows[] | "\(.sourceId) \(.id)"' |
  while read -r account id; do
    PH_ACCOUNT_ID="$account" PH_MESSAGE_ID="$id" sh ./handler.sh < /dev/null
  done

# Then follow live.
posthastectl watch --keyword todo --exec 'sh ./handler.sh'
```

Because your handler's writes carry deterministic `--id`s, handling the same
message in both phases is a safe no-op.

To keep a watcher alive across reboots, run it under your OS's user service
manager — a `systemd --user` unit on Linux, a launchd LaunchAgent on macOS —
with `Restart=on-failure`. It's a user service; nothing here needs `sudo`.

## 5. Binary resources

Message bodies and attachments are immutable blobs, fetched by id over plain
authenticated GETs. The `messageDetail` answer lists each attachment with its
`blobId`:

```sh
posthastectl blobs get <blobId> --output invoice.pdf
```

`--output` writes the bytes to a file; without it, small blobs come back as
JSON with a `base64` field (pipe through `jq -r .base64 | base64 -d`), and
anything large is refused with a pointer to `--output`.

On the wire that's `GET /blobs/{blobId}` with the bearer token; responses
carry long-lived caching headers because blobs never change. Compose
attachments travel the other way _inside_ the send command, base64-encoded —
there is no upload endpoint.

## Scriptability contract

- **Exit codes**: `0` success · `2` usage error · `3` no backend found /
  connection failed · `4` API error · `1` unexpected. Scripts branch on `$?`.
- **Output** is JSON on stdout — pretty on a TTY, compact when piped.
  Diagnostics go to stderr, never mixed into the payload.
- **Errors are classified by exit code.** An API failure exits `4` and prints
  one human-readable stderr line carrying the status and the API's error
  `kind` (e.g. `posthastectl: API 404 [unknownId]: ...`), plus a retryable
  hint when the same request may succeed shortly. Branch on `$?`; treat the
  stderr text as diagnostics, not a parse target.
- **Input**: every command accepts its argument object as JSON via
  `--input -` (stdin) or `--input @file`; explicit flags override the JSON.
- **Secrets stay off argv.** The token comes from the discovery file or the
  environment; there is no `--token` flag, and `posthastectl` never echoes it
  into logs or error output.

## Agents

`posthastectl mcp` starts the same operations as an MCP stdio server — one
binary, no separate install — so any MCP-capable agent host gets the mail
tools. That's the next page: **[Plug in an agent](agents.md)**.

## Reference

- Discovery: `connection-info.json` (`{ "port": ..., "token": ... }`) in the
  state root; `POSTHASTE_STATE_ROOT`, `POSTHASTE_API_URL`/`POSTHASTE_TOKEN`
  override.
- Reads: `POST /query`, one externally-tagged query object; answers are
  `{ generation, data }`. Families: mail list (search included, windowed by
  `limit`/`cursor`), thread, message detail, mailbox counts, accounts,
  pending operations, tags, smart mailboxes.
- Writes: `POST /command`, `{ id, command }` with a client-minted idempotency
  id; same-id retry is safe. Settlement is queried, never assumed.
- Events: `GET /events` (SSE), `{ generation, event? }`;
  `posthastectl events` for NDJSON, `--generation-only` for liveness.
- Blobs: `GET /blobs/{blobId}`; `posthastectl blobs get`.
- Write verbs: `tag`, `move`, `reply`, `send`, `messages destroy`,
  `mailboxes create|rename|delete`, `sync` — all accept
  `--id <idempotency-id>`, and all name their account and message explicitly
  (in a handler, pass `"$PH_ACCOUNT_ID"`/`"$PH_MESSAGE_ID"`).
- `watch --exec` env: `PH_ACCOUNT_ID`, `PH_MESSAGE_ID`, `PH_GENERATION`,
  `PH_KIND`, `PH_KEYWORDS`, `PH_MAILBOX_IDS`; message detail JSON on stdin.
- Full contract:
  [API (L1)](https://github.com/theoryzhenkov/posthaste/blob/main/docs/api/L1-api.md).
  Threat model: [scripting-security.md](scripting-security.md).
