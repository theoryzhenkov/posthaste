# Scripting quickstart — a laptop script in five minutes

Automate Posthaste from a shell script with **no protocol code**: subscribe to
new mail, run a handler, and write back safely. This is the slice-1 scripting
surface (RFC-L2-scripting): the fact-carrying **tap** (`/v1/events`) plus the
one-vocabulary **apply** action path, driven by `posthastectl`.

The whole flow — discover, mint a token, watch-and-act — takes about five
minutes and never requires reading Posthaste's source.

## Before you start

You need **the Posthaste desktop app running** (PosthasteNightly) *or* the
standalone daemon (`posthaste serve`). Either way an HTTP server is listening on
loopback:

- The **desktop app embeds the exact same server** in-process. On launch it
  writes a **discovery file** — `daemon.json` (port + a bootstrap capability
  token) — into its state dir. The standalone daemon writes the identical file.
- `posthastectl` reads that file automatically, so you pass **no `--url` and no
  `--token`**. If nothing is found it tells you to start the app.

`posthastectl` ships with the `posthaste-mcp` package (bun). Run it with
`bun apps/mcp/src/cli.ts …`, or via the packaged `posthastectl` bin.

## 1. Mint a least-privilege token (30 seconds)

Never hand a script the full-scope bootstrap token. Attenuate it down to exactly
what the script needs — and give it an expiry:

```sh
TOKEN=$(posthastectl token mint --grant tap:read,apply,read --expiry 1h)
```

- `--grant` scopes (comma-separated, repeatable):
  - `tap:read` — subscribe to the event tap (`/v1/events`)
  - `read` — bootstrap reads (mail list, message detail, …)
  - `apply` — write back (set keywords, move/replace mailboxes, destroy)
  - or a raw verb: `read, send, tag, move, delete, manage`
- `--expiry` — a human duration: `3600`, `90m`, `1h`, `7d`.
- Narrow further with `--account <id>` / `--mailbox <id>` / `--message <id>`.

The token is printed to **stdout** (so `TOKEN=$(…)` captures exactly the
credential); a ready-to-paste `export POSTHASTE_TOKEN=…` line goes to stderr.
Attenuation happens server-side and can only *narrow* authority — a minted token
can never do more than the bootstrap token it came from.

## 2. Write a handler (2 minutes)

A handler is any program. `watch --exec` runs it **once per matching message**,
with the full `MessageDetail` JSON on **stdin** and these env vars set:

`PH_ACCOUNT_ID`, `PH_MESSAGE_ID`, `PH_SEQ`, `PH_TOPIC`, `PH_KEYWORDS`,
`PH_MAILBOX_IDS`.

Here is a handler that tags every matched message `$processed` — writing back
through `apply`, **idempotently**:

```sh
#!/bin/sh
# write_back.sh — receives a MessageDetail on stdin; tags it $processed.
curl -sS -X POST \
  "$POSTHASTE_API_URL/sources/$PH_ACCOUNT_ID/commands/messages/$PH_MESSAGE_ID/set-keywords" \
  -H "Authorization: Bearer $POSTHASTE_TOKEN" \
  -H "Idempotency-Key: rule:tagger:$PH_MESSAGE_ID" \
  -H "content-type: application/json" \
  -d '{"add":["$processed"],"remove":[]}'
```

### Safe write-back: the idempotency key

The tap is **at-least-once**: after a reconnect or a crash-and-resume, your
handler can see the same message twice. That is safe *only if your write-back is
idempotent*. Pass an **`Idempotency-Key`** header on any `apply` (message
command) request:

- A redelivery under the **same key** returns the **first outcome** instead of
  re-executing — no double-apply.
- Reusing a key with a **different operation** is rejected (`409 Conflict`).
- Make the key a deterministic function of the trigger (e.g.
  `rule:<name>:<messageId>`) so a redelivery reproduces it exactly.

This is the runtime-side dedup ledger (RFC-L2-scripting D53, resolving P8); no
setup is required beyond sending the header.

## 3. Watch and act (30 seconds)

```sh
# The handler runs as a child of `watch` and inherits its environment, so export
# what the handler's curl needs. POSTHASTE_API_URL is the loopback /v1 URL from
# the discovery file (jq it out of daemon.json, or read it from the app's UI).
export POSTHASTE_TOKEN="$TOKEN"          # the minted token from step 1
export POSTHASTE_API_URL="http://127.0.0.1:$(jq .port "${XDG_DATA_HOME:-$HOME/.local/share}/posthaste/daemon.json")/v1"

posthastectl watch --exec 'sh ./write_back.sh' --cursor ./cursor
```

`watch` itself needs no `--url`/`--token` — it auto-discovers from `daemon.json`
(and here also honors `POSTHASTE_TOKEN`, the minted token).

- `--exec <command>` — run per matching message (detail JSON on stdin).
- `--cursor <file>` — persist the last-processed `seq`; on restart it resumes
  from there. The cursor is your **only** state — durable across both script and
  app restarts.
- Filters: `--account <id>`, `--mailbox <id>`, `--keyword <tag>`, or
  `--all-updates` to fire on every change (not just genuine arrivals).

> The `--exec` handler runs on attacker-influenced input (email). Treat stdin as
> untrusted; `--keyword` is convenience, not an auth boundary.

If the tap's durable history is truncated past your cursor, `watch` receives an
explicit **gap frame** (never a silent drop) and resumes from the live head.

## Snapshot-attach: read state, then tail from that point

For a level-triggered script (reconcile current state, then follow changes),
mail-list reads return an **`asOfSeq`** — the event seq as-of that read:

```sh
# Read the current mail list AND the consistency token in one call.
resp=$(curl -sS "$POSTHASTE_API_URL/sources/$ACCOUNT/messages" \
  -H "Authorization: Bearer $POSTHASTE_TOKEN")
seq=$(printf '%s' "$resp" | jq .asOfSeq)

# Then tail the tap from exactly that point — gap-free, no per-consumer state.
posthastectl events --after-seq "$seq"
```

`asOfSeq` appears on the message-list and conversation-list reads (the
`GET /v1/sources/{id}/messages`, `/v1/messages/search`, `/v1/views/conversations`
and their smart-mailbox variants).

## The ladder

This quickstart is level 2 of the scripting ladder (`watch --exec` — the CLI
owns the cursor, reconnect, and auth). Lower-code options (declarative rules,
webhook/exec rule actions) and the agent-native MCP tool surface land in
slice 2.

## Reference

- Tap: `GET /v1/events` (SSE) — `--after-seq`, `--topic`, `--account`,
  `--mailbox`.
- Apply (write-back) commands: `POST /v1/sources/{id}/commands/messages/{mid}/…`
  (`set-keywords`, `add-to-mailbox`, `remove-from-mailbox`, `replace-mailboxes`,
  `destroy`) — all accept the `Idempotency-Key` header.
- Token mint: `posthastectl token mint` → `POST /v1/auth/tokens`.
- Design: `docs/eph/RFC-L2-scripting.md`.
