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

### Install `posthastectl`: run the wizard

The fastest path — and the one this quickstart assumes — is
`posthaste-wizard`, the same tool that sets up a Posthaste node:

```sh
posthaste-wizard ctl install
```

This locates a `posthastectl` binary (an explicit `--from` path, the desktop
app's bundled sidecar if present, or a checksum-verified download from the
matching GitHub release), installs it to `~/.local/bin/posthastectl`
(`--to <dir>` overrides), and — only ever for a verified download, and only on
macOS — clears the quarantine flag so Gatekeeper does not block the first run.
It never escalates to `sudo`: a permission error explains itself instead.

Right after installing, it runs the same checks `posthaste-wizard ctl status`
re-runs any time, and prints a ✓/✗ table:

```
posthastectl setup:
  ✓ binary       /home/you/.local/bin/posthastectl
  ✗ PATH         /home/you/.local/bin is not on PATH — add to ~/.bashrc: export PATH="/home/you/.local/bin:$PATH"
  ✓ app running  daemon.json found at ~/.local/share/posthaste/daemon.json
  ✓ discovery    version 1, http://127.0.0.1:3001/v1
  ✓ probe        http://127.0.0.1:3001/v1/openapi.json -> 200
```

The wizard only ever prints a PATH hint — it never edits your shell's rc file
for you. If a row fails, fix it (start the app, add the directory to `PATH`)
and re-run `posthaste-wizard ctl status`.

If you'd rather install `posthastectl` by hand, see the
[manual install appendix](#appendix-manual-posthastectl-install) below.

## 1. Mint a least-privilege token (30 seconds)

The discovery bootstrap itself can read and tail the tap, but it cannot write —
every write goes through an explicitly minted token, so mint one scoped to
exactly what the script needs, with an expiry:

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
Minting happens server-side, scoped to exactly what you asked for — including
write grants the bootstrap itself doesn't have, since minting is precisely how
a script trades the read-only bootstrap for a working, least-privilege token.

## 2. Write a handler (2 minutes)

A handler is any program. `watch --exec` runs it **once per matching message**,
with the full `MessageDetail` JSON on **stdin** and these env vars set:

`PH_ACCOUNT_ID`, `PH_MESSAGE_ID`, `PH_SEQ`, `PH_TOPIC`, `PH_RULE`,
`PH_KEYWORDS`, `PH_MAILBOX_IDS` (`PH_RULE` is empty except on a `--topic
rule.fired` watch — see [Central evaluate, edge
execute](#central-evaluate-edge-execute-emit) below).

### Writing a handler: the two-line form

**`posthastectl` IS the SDK** (RFC-L2-scripting ruling 21): its write verbs —
`tag`, `move`, `reply`, `send`, `apply` — construct the typed request body,
attach auth, and derive a safe `Idempotency-Key` for you. A handler that tags
every matched message `reviewed` is two lines, with **no JSON parsing, no
REST, and no idempotency math**:

```sh
#!/bin/sh
# write_back.sh — the env vars above are already in scope; nothing else needed.
posthastectl tag --message "$PH_MESSAGE_ID" --add reviewed
```

`--account` is optional here too — it falls back to `$PH_ACCOUNT_ID` /
`$PH_ACCOUNT` when omitted, so the common "act on the message that triggered
this handler" case never needs it spelled out. The other verbs follow the same
shape:

```sh
posthastectl move --message "$PH_MESSAGE_ID" --to-mailbox archive
posthastectl reply --message "$PH_MESSAGE_ID" --body "Got it, thanks!"
posthastectl send --to a@example.com --subject "Heads up" --body "..."
```

Every write verb sets `Idempotency-Key` for you automatically, derived from
the triggering event's seq (`$PH_EVENT_SEQ`/`$PH_SEQ`) so a redelivery of the
*same* event reproduces the *same* key — see
[the idempotency key](#safe-write-back-the-idempotency-key) below for exactly
how, and `--idempotency-key` / `posthastectl <verb> --help` to override it.

### Advanced: the raw JSON-on-stdin path

For custom logic beyond the five verbs (or another language's HTTP client),
skip `posthastectl` entirely and drive `/v1` yourself — the same env vars and
the full `MessageDetail` JSON on stdin are still all you get, no wrapper
required:

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

This is exactly what `posthastectl tag` does internally — reach for it only
when the sugar doesn't fit (custom body shape, a non-`sh` handler, etc.).

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
setup is required beyond sending the header. **`posthastectl`'s write verbs do
this for you** — they read `PH_IDEMPOTENCY_KEY` / `PH_EVENT_SEQ` / `PH_SEQ`
from the environment and derive a key deterministic in the triggering event
(suffixed per-verb, so calling `tag` then `move` for one event never collides).
Only the raw-REST path above needs to compute it by hand.

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
- `--topic <topic>` — subscribe to a topic other than the default
  `message.updated` (server-side). Pairs with `--rule <name>` below for the
  client-machine-execution pattern (RFC-L2-scripting ruling 19).
- `--rule <name>` — with `--topic rule.fired`, dispatch only the named rule's
  `rule.fired` events. See [Central evaluate, edge
  execute](#central-evaluate-edge-execute-emit) below for the full flow.

> The `--exec` handler runs on attacker-influenced input (email). Treat stdin as
> untrusted; `--keyword` and `--rule` are convenience, not an auth boundary —
> the server evaluates and emits regardless of what a client-side filter
> selects (RFC-L2-scripting ruling 20e).

If the tap's durable history is truncated past your cursor, `watch` receives an
explicit **gap frame** (never a silent drop) and resumes from the live head.

> **One tap, not two** (RFC-L2-scripting D52/S3): `/v1/events` already carries
> everything the authority server itself originates — rule outcomes included.
> The bundled app's runtime shares the AS's own event bus and durable log
> in-process, so a second "AS tap" mount would just duplicate this one. That
> includes `rule.fired`/`rule.delivery.failed`: they append through the same
> durable log this section describes, so a `--rule`/`--topic rule.fired` watch
> that reconnects after a rule fired still sees it (or the gap frame, never a
> silent drop) exactly like any other fact.

## Always-on: register a watch as a service

A foreground `watch --exec` stops when your terminal closes. For always-on
laptop automation — the shape between the ad-hoc foreground run and the in-app
rules engine — wrap it in a **user service unit** with
`posthaste-wizard ctl register-watch` (RFC-L2-scripting ruling 12). It renders
and installs a `systemd --user` unit (Linux) or a launchd LaunchAgent (macOS)
that runs `posthastectl watch` with your flags, restarts on failure, and points
at the `posthastectl` the wizard installed (it locates it via the install
manifest, the install dir, then `PATH`):

```sh
posthaste-wizard ctl register-watch \
  --exec 'sh ./handler.sh' --topic rule.fired --rule flag-for-edge --name agent
```

> **Consent (RFC-L2-scripting ruling 20b).** Registering a watch means local
> code runs on your machine in response to **server-controlled events** (mail /
> rule firings), on attacker-influenced input. So the command prints a one-time
> warning and **requires you to type `yes`** (or pass `--yes`) before it
> installs anything. Only register a handler you trust, and keep the rule
> sender-scoped — see [scripting-security.md](scripting-security.md).

The `--serve-hook` variant wraps `posthastectl hook serve` instead (ruling 17),
for an always-on webhook receiver behind a GUI/`rules.toml` `webhook` rule:

```sh
posthaste-wizard ctl register-watch --serve-hook ./handler.sh --port 8787 --name hook
```

- Filters (`--topic`, `--rule`, `--keyword`, `--account`) pass straight through
  to `watch`; `--name <suffix>` names the unit (defaults to the rule/keyword/
  topic). `--serve-hook` takes `--port`.
- The unit passes discovery environment through (`POSTHASTE_STATE_ROOT`,
  `POSTHASTE_API_URL`, `POSTHASTE_TOKEN` if set), so the service finds
  `daemon.json` exactly as the foreground command would.
- Remove one with `posthaste-wizard ctl unregister-watch [--name <suffix>]`
  (omit `--name` when there is only one). List what's registered any time with
  `posthaste-wizard ctl status`.
- Like everything the wizard does, this is a **user** unit and it **never
  sudos** — if the unit directory isn't writable it refuses and explains.

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

## Zero-code rules (levels 0-1)

Levels 0 and 1 of the ladder need **no script at all**: a declarative rule,
authored in a `rules.toml` file in your config root, runs *inside* the
always-on authority server. It subscribes to the fact stream itself, matches
each triggering message against a WHEN-clause (the same query grammar smart
mailboxes use), and runs an action.

> **Read [scripting-security.md](scripting-security.md) first.** A rule turns
> untrusted email into an action. Two habits keep that safe and appear in every
> example below: **scope the WHEN-clause by sender** (`from:`) so attacker mail
> cannot trigger your rule, and **grant the least capability** the handler needs.

### Create a rule in the app (no file, no restart)

For the safe actions — **tag / move / notify / emit / webhook** — you don't need
`rules.toml` at all: open **Settings → Automations** and create the rule in the
app. It builds the WHEN-clause with the same grammar shown here, defaults the
webhook grant to least-privilege (`read`), and surfaces the sender-scope /
prompt-injection guidance inline. The rule persists to a GUI-owned
`rules.d/<id>.toml` and **starts firing immediately** — no restart. The app
**cannot** create an `exec` rule (that action isn't representable on the write
surface, by design — a GUI-settable exec would be remote code execution); exec
stays `rules.toml`-only, below.

### Author a rule in `rules.toml`

Create `<config-root>/rules.toml` (the config root is `daemon.json`'s directory,
e.g. `~/.config/mail`) and restart the server to load it:

```toml
# Level 0 — a built-in action, applied through the server's own apply surface.
# Scoped by sender: space-separated terms are ANDed (one grammar, shared with
# smart mailboxes), so this fires only on invoices FROM the known vendor.
[[rule]]
id = "tag-invoices"
name = "Tag billing mail"
when = "subject:invoice from:billing@myvendor.com"
enabled = true
action = { kind = "tag", tag = "$billing" }
# Other level-0 actions: { kind = "move", mailboxId = "archive" }
#                        { kind = "notify", title = "Heads up" }

# Level 1 — the RFC §9 worked example: when a message you tagged `instruct`
# arrives FROM YOU, POST it to your agent's webhook with a per-invocation,
# attenuated token. The `from:` scope is load-bearing: an unscoped
# content→agent rule lets any sender drive your agent (a prompt-injection
# surface). See scripting-security.md.
[[rule]]
id = "instruct-agent"
name = "Send instruct-tagged mail to the agent"
when = "tag:instruct from:me@mydomain.com"
enabled = true
# Least-grant: this handler writes a reply tag, so it needs `tag` on TOP of
# `read`. If your handler only reads, grant `["read"]` alone. Every added verb
# widens what a successful prompt-injection could do with the token.
action = { kind = "webhook", url = "https://vm.example/agent", grants = ["read", "tag"], expirySeconds = 3600 }
```

When the rule fires, the engine mints a **fresh capability token** scoped to
exactly `grants` (here `read` + `tag`), confined to **that one message**, and
expiring after `expirySeconds`. It POSTs the webhook:

```json
{
  "ruleId": "instruct-agent",
  "idempotencyKey": "rule:instruct-agent:<eventSeq>",
  "event":   { "seq": 91, "topic": "message.updated", "accountId": "...", "messageId": "..." },
  "message": { /* the MessageSummary */ },
  "token":   "<the attenuated macaroon>"
}
```

Your handler reads the thread and writes back via `apply`/`send` under `token`
— its authority can never exceed the grant (a `[read, tag]` token cannot
`config:reload`, cannot mint, cannot touch another message). Because delivery is
**at-least-once**, pass the payload's `idempotencyKey` as the `Idempotency-Key`
header on every write-back, exactly as in the [idempotency section](#safe-write-back-the-idempotency-key)
above — a redelivery is then deduped, never double-applied. Localhost/loopback
URLs are first-class webhook targets.

Delivery is observable: every firing emits a `rule.fired` fact, and a webhook
whose bounded retries are exhausted dead-letters as a `rule.delivery.failed`
fact — both on the tap.

### The easy listener: `posthastectl hook serve`

A `webhook` action needs *something* listening at its URL. For a
GUI-created rule (or any `rules.toml` webhook pointed at your own machine),
`posthastectl hook serve` **is** that listener — a tiny built-in localhost
receiver (RFC-L2-scripting ruling 17), so you never write an HTTP server just
to catch a delivery:

```sh
posthastectl hook serve --exec ./handler.sh --port 8787 --path /hook
```

Point a `webhook` rule's `url` at `http://127.0.0.1:8787/hook` (in place of
the [worked example](#author-a-rule-in-rulestoml) above's
`https://vm.example/agent`), and every delivery runs `./handler.sh` once,
with the **exact same contract** as the Level-1 `exec` action: the full
`{ ruleId, idempotencyKey, event, message, token }` delivery JSON on stdin,
and these env vars derived from it —

`PH_RULE`, `PH_IDEMPOTENCY_KEY`, `PH_ACCOUNT`, `PH_MESSAGE_ID`, `PH_FROM`,
`PH_SUBJECT`, `PH_KEYWORDS`, `PH_EVENT_SEQ`, `PH_TOPIC`, plus
`POSTHASTE_TOKEN` (the delivery's own per-invocation attenuated token) and,
when a daemon is discoverable on this machine, `POSTHASTE_API_URL` — so the
same two-line handler from [Writing a handler](#writing-a-handler-the-two-line-form)
works unmodified:

```sh
#!/bin/sh
# handler.sh — PH_MESSAGE_ID, POSTHASTE_TOKEN, POSTHASTE_API_URL are already set.
posthastectl tag --message "$PH_MESSAGE_ID" --add reviewed
```

**Payload-is-data, same invariant as `exec`.** The delivery body reaches your
script only as the JSON stdin document (or the `PH_*` env vars) — never
interpolated into a command string or argv, so a malicious sender cannot
inject a command through the webhook body.

- `--port <n>` — default `8787`; `0` lets the OS assign a port (check the
  startup log line for the resolved one).
- `--path </path>` — default `/hook`; must match the rule's `url` path.
- `--token <token>` — require `Authorization: Bearer <token>` on every
  delivery (`401` without it), so only a rule that knows the token can invoke
  this receiver. It gates *who may call the listener*, not a substitute for
  the per-delivery capability token already inside the payload.
- Binds `127.0.0.1` only (RFC-L2-scripting ruling 15) — there is no `--host`
  flag; this command is not meant to be reachable off the box.
- `Ctrl-C` (`SIGINT`) shuts it down cleanly.

Pair `hook serve` with `posthaste-wizard ctl register-watch` (ruling 12) to
wrap it in a user service unit for an always-on receiver, exactly as you would
`watch --exec`.

### Central evaluate, edge execute (`emit`)

The `emit` action does nothing but fire the `rule.fired` fact:

```toml
[[rule]]
id = "flag-for-edge"
name = "Evaluate centrally, execute at the edge"
when = "tag:instruct from:me@mydomain.com"
enabled = true
action = { kind = "emit" }
```

Pair it with a client-side, rule-filtered `watch` on your laptop — the
no-listener counterpart to `hook serve` above, pull-based so nothing needs an
inbound port:

```sh
posthastectl watch --topic rule.fired --rule flag-for-edge --exec ./handler.sh --cursor ./cursor
```

The always-on authority server evaluates the WHEN-clause **once, centrally**,
and emits `rule.fired`; `--topic rule.fired` subscribes to that fact stream
instead of the default `message.updated`, and `--rule flag-for-edge` narrows
dispatch to just this rule's firings (`./handler.sh` gets the usual `PH_*` env,
now including `PH_RULE=flag-for-edge` and the matched message's detail on
stdin — a `rule.fired` watch skips the arrival gate, since a fact isn't a
mailbox arrival). The WHEN logic lives in one place; execution happens
wherever you want it — this is the "emit rule → `watch --rule`" half of the
north-star client-machine-execution pattern (`hook serve` above is the other
half, for `webhook` rules).

**`--rule` (like `--keyword`) is convenience, not a security boundary**
(RFC-L2-scripting ruling 20e): it only filters what your local script *sees* —
the server evaluates the WHEN-clause and emits `rule.fired` regardless of any
client-side filter, so the sender-scoping and least-grant guidance in
[scripting-security.md](scripting-security.md) still applies to the rule
itself, not to `--rule`.

### The exec action is file-only (load-bearing security rule)

A rule can also run a **local script** on the authority-server host:

```toml
[[rule]]
id = "run-local"
name = "Hand off to a local handler"
when = "tag:instruct from:me@mydomain.com"
enabled = true
action = { kind = "exec", command = "/opt/posthaste/handler.sh", grants = ["read", "tag"], expirySeconds = 3600 }
```

`exec` runs `command` — a **fixed host binary** — with the full event+message
JSON payload on **stdin**, the scoped token in `POSTHASTE_TOKEN`, and these
convenience env vars (RFC-L2-scripting ruling 21 — "posthastectl IS the SDK"):

`PH_IDEMPOTENCY_KEY`, `PH_ACCOUNT` (source id), `PH_MESSAGE_ID`, `PH_FROM`,
`PH_SUBJECT`, `PH_KEYWORDS` (comma-separated), `PH_EVENT_SEQ`, `PH_TOPIC`.

Since `posthastectl`'s write verbs already read these, `/opt/posthaste/handler.sh`
can be the two-line form with no JSON parsing at all:

```sh
#!/bin/sh
# handler.sh — PH_MESSAGE_ID, POSTHASTE_TOKEN, etc. are already in the environment.
posthastectl tag --message "$PH_MESSAGE_ID" --add reviewed
```

**Payload-is-data (RFC §7.20).** There is deliberately **no argument template**:
event/message data reaches your script only as the JSON stdin document (or the
`PH_*` env vars above), never interpolated into a command string or argv. A
malicious sender therefore cannot inject a command. For anything beyond the
env vars, parse stdin as JSON and treat every field as untrusted input.

**`exec` is settable ONLY by editing `rules.toml` on the host — never over
REST.** This is a hard, load-bearing invariant (RFC-L2-scripting §7.16): a
REST-settable exec action would be remote code execution. The REST surface for
rules is **read-only** (list + preview); creating or editing a rule — especially
an `exec` rule — is a deliberate, file-system-level act by the host operator.

## Agent via MCP

The north-star agent flow (RFC-L2-scripting ruling 22): a **rule at the
authority server** provides the *trigger*, and a **persistent localhost agent,
connected once over MCP**, provides the *capability*. One connection gives the
agent both — it is pushed each `rule.fired` fact as an MCP notification and acts
through the same typed tools, with **no script and no hand-written types**.

Posthaste ships an MCP server (`apps/mcp` — the same package as `posthastectl`).
It exposes:

- **The action tools** — `list_conversations`, `get_conversation`,
  `get_message`, `search_messages`, `set_keywords`, `move_to_mailbox`, `send_message`,
  `trigger_sync`, `list_mailboxes`, and a first-class **`reply`** tool (give it a
  `sourceId` + `messageId` + `body`; it looks up the reply context — recipient,
  subject, `In-Reply-To`/`References` — and sends, so the agent replies without
  knowing the compose plumbing).
- **The fact subscription** — after connecting, the server opens the `/v1/events`
  tap and forwards every `DomainEvent` to the agent as a standard MCP
  `notifications/message`. The event rides in the notification's structured
  `data` (`{ kind: "event", topic, seq, event }`), tagged `logger: "posthaste"`.
  `rule.fired` and `rule.delivery.failed` are the point of the subscription
  (a failed delivery arrives at `level: "error"`).
  - **Gap frames are distinct.** If the tap's durable history was truncated past
    your resume point, the agent gets a `{ kind: "gap", highestSeq }` payload at
    `level: "warning"` — never a silent drop. Treat it as "reconcile now"
    (re-read state) rather than assuming you saw every event.

### The rule (trigger)

Author an `emit` or `webhook` rule as in [Zero-code rules](#zero-code-rules-levels-0-1)
above — scoped by sender. The simplest pairing for a localhost agent is `emit`
(evaluate centrally, act at the edge):

```toml
[[rule]]
id = "agent-instruct"
name = "Hand instruct-tagged mail to the MCP agent"
when = "tag:instruct from:me@mydomain.com"
enabled = true
action = { kind = "emit" }
```

The agent receives the resulting `rule.fired` notification and decides how to act.

### The agent host config (capability)

An MCP host spawns the stdio server as a child process. Point it at the server
entry and declare the grant set in the environment — this is where **connect-time
per-connection minting** happens: on startup the server attenuates the discovered
bootstrap token into a token scoped to exactly these grants, and uses it for
every tool call and the subscription.

```json
{
  "mcpServers": {
    "posthaste": {
      "command": "/Applications/PosthasteNightly.app/Contents/MacOS/posthastectl",
      "args": ["mcp"],
      "env": {
        "POSTHASTE_MCP_GRANTS": "tap:read,read",
        "POSTHASTE_MCP_TOKEN_EXPIRY": "1h"
      }
    }
  }
}
```

Any `posthastectl` works as the `command` — the app-bundled sidecar above, a
wizard install (`~/.local/bin/posthastectl`), or from a checkout
(`bun run apps/mcp/src/index.ts`). The server auto-discovers `daemon.json`
(no URL/token needed — a fresh scoped token is minted at every connect, so
tokens never need manual refreshing), or honors
`POSTHASTE_API_URL`/`POSTHASTE_TOKEN` if you set them.

Connection environment:

- `POSTHASTE_MCP_GRANTS` — comma-separated grants, the same vocabulary as
  `token mint --grant` (`tap:read`, `read`, `apply`, or raw verbs). **Default:
  `tap:read,read` — read-only + subscribe.** Write verbs are an explicit opt-in.
- `POSTHASTE_MCP_TOKEN_EXPIRY` — a human duration (`1h`, `90m`, `3600`) for the
  minted token.
- `POSTHASTE_MCP_ACCOUNT` — optional, narrows the minted token to one account.
- `POSTHASTE_MCP_AFTER_SEQ` — resume the subscription from a last-seen seq. Omit
  it to attach at the live head (snapshot-attach): read current state, note its
  `asOfSeq`, then reconnect with that seq for a gap-free follow.

### Least-grant is the security boundary

An agent reads message content — which is attacker-authored (anyone can email
you). An agent that **also holds `apply`/`send` and reads untrusted content is
the prompt-injection surface**: a crafted message can instruct the agent, which
then acts with your token. So:

- **Keep the default `tap:read,read`** unless the agent genuinely needs to write.
  A summarizer never grants more than `read`.
- **Scope the rule's WHEN-clause by sender** (`from:`) so only mail you trust
  can trigger the agent at all.
- Only add `apply`/`send` grants once you have accepted that surface.

**Read [scripting-security.md](scripting-security.md) — threat 2 (prompt
injection / confused deputy) — before granting an autonomous agent any write
capability.**

## The ladder

This quickstart's `watch --exec` flow is level 2 of the scripting ladder (the
CLI owns the cursor, reconnect, and auth). Levels 0-1 (the declarative rules
above) run in the authority server with no client at all; the **agent-native
MCP surface** ([Agent via MCP](#agent-via-mcp) above) is the level-"agent-native"
rung — trigger + capability over one connection.

## Keeping it updated (headless / self-host)

Desktop machines update through the app's own updater. A **headless or
self-hosted** node — role binaries, `posthastectl`, and the wizard itself —
updates through `posthaste-wizard update` (RFC-L2-scripting ruling 14). The
wizard is the only actor that owns the service units, so it is the one actor
that can stop a service, swap its binary, and start it again correctly.

Every install records what it placed in an **install manifest** at
`$XDG_STATE_HOME/posthaste/wizard-manifest.toml` (else
`~/.local/state/posthaste/wizard-manifest.toml`) — one entry per component with
its path, version, and channel. `ctl install` and the role `install` populate
it automatically; a host that installed before the manifest existed reads back
as empty and `update` simply reports there is nothing tracked (re-install to
start tracking).

```sh
# See what's out of date (resolves each channel's latest; changes nothing):
posthaste-wizard update --check
```

```
component updates:
  ↑ posthaste-authority-server [nightly] 0.2.0-nightly.44 → 0.2.0-nightly.50
  ✓ posthastectl               [nightly] up to date
```

```sh
# Apply — download + verify (the release SHA256SUMS), stop the service, swap the
# binary (keeping the old one as <path>.bak), restart, record the new version:
posthaste-wizard update --yes
```

- The wizard updates **its own binary last**, in the same pass — on unix a
  rename over a running executable is safe (the running process keeps its open
  inode; the next launch is the new binary).
- **Roll back** a bad update with `posthaste-wizard update --rollback
  <component>` — it swaps the kept `.bak` back and restores the recorded
  version.
- **Opt into auto-updates** with `posthaste-wizard update --install-timer`: it
  renders a user timer (systemd `.timer` / launchd interval agent) that runs
  `update --yes` on a daily schedule. It is a rendered unit, never a
  daemon-resident self-updater, and — like all wizard unit writes — refuses
  rather than sudo if the unit directory isn't writable.

## Reference

- Tap: `GET /v1/events` (SSE) — `--after-seq`, `--topic`, `--account`,
  `--mailbox`.
- `posthaste-wizard ctl register-watch --exec <script> [--topic <t>] [--rule
  <r>] [--keyword <k>] [--account <id>] [--name <suffix>] [--yes]` — wrap
  `watch` in a user service (ruling 12); `--serve-hook <script> [--port <n>]`
  wraps `hook serve` (ruling 17). `ctl unregister-watch [--name <suffix>]`
  removes; `ctl status` lists. Requires consent (ruling 20b); never sudos.
- `posthaste-wizard update [--check] [--yes] [--rollback <component>]
  [--install-timer]` — the headless updater (ruling 14): manifest-tracked,
  checksum-verified swap keeping `<path>.bak`, wizard self-update last.
- `watch` filters (client-side, convenience — not an auth boundary, ruling
  20e): `--account`, `--topic` (default `message.updated`), `--mailbox`,
  `--keyword`, `--rule` (pair with `--topic rule.fired`), `--all-updates`.
- `hook serve`: `posthastectl hook serve --exec <script> [--port <n>]
  [--path </path>] [--token <token>]` — the built-in localhost webhook
  receiver (ruling 17), for GUI/`rules.toml` `webhook` actions. `127.0.0.1`
  only; default `--port 8787`, `--path /hook`.
- Apply (write-back) commands: `POST /v1/sources/{id}/commands/messages/{mid}/…`
  (`set-keywords`, `add-to-mailbox`, `remove-from-mailbox`, `replace-mailboxes`,
  `destroy`) — all accept the `Idempotency-Key` header. `POST
  /v1/sources/{id}/commands/send` does not (yet).
- Write verbs (the SDK surface, RFC-L2-scripting ruling 21) — each resolves
  `--account`/`--message` from `$PH_ACCOUNT`/`$PH_ACCOUNT_ID`/`$PH_MESSAGE_ID`
  when omitted, and auto-derives `Idempotency-Key`; `<verb> --help` for the
  full flag list:
  - `posthastectl tag --message <id> [--account <id>] --add <kw>... --remove <kw>...`
  - `posthastectl move --message <id> [--account <id>] --to-mailbox <role|id>`
  - `posthastectl reply --message <id> [--account <id>] --body <text|-|@file>`
  - `posthastectl send --to <addr>... --subject <s> --body <text|-|@file> [--account <id>]`
  - `posthastectl apply --kind <set-keywords|add-to-mailbox|remove-from-mailbox|replace-mailboxes|destroy> --message <id> [--account <id>] [--body <json|-|@file>]`
    — the escape hatch: any message-command route by name and raw wire shape.
  - All accept `--idempotency-key <key>` to override the auto-derived one.
- `exec` action env vars (Level 1, rule-driven): `PH_IDEMPOTENCY_KEY`,
  `PH_ACCOUNT`, `PH_MESSAGE_ID`, `PH_FROM`, `PH_SUBJECT`, `PH_KEYWORDS`,
  `PH_EVENT_SEQ`, `PH_TOPIC` (plus `POSTHASTE_TOKEN`); full event+message JSON
  on stdin.
- `watch --exec` env vars (Level 2, CLI-driven): `PH_ACCOUNT_ID`,
  `PH_MESSAGE_ID`, `PH_SEQ`, `PH_TOPIC`, `PH_RULE` (empty unless a `--rule`-
  matched `rule.fired` event), `PH_KEYWORDS`, `PH_MAILBOX_IDS`; full
  `MessageDetail` JSON on stdin.
- `hook serve` env vars (client-side receiver, mirrors the `exec` action's
  set above): `PH_RULE`, `PH_IDEMPOTENCY_KEY`, `PH_ACCOUNT`, `PH_MESSAGE_ID`,
  `PH_FROM`, `PH_SUBJECT`, `PH_KEYWORDS`, `PH_EVENT_SEQ`, `PH_TOPIC`, plus
  `POSTHASTE_TOKEN` (the delivery's token) and `POSTHASTE_API_URL` (when a
  daemon is discoverable); the raw delivery body on stdin.
- Token mint: `posthastectl token mint` → `POST /v1/auth/tokens`.
- Design: `docs/eph/RFC-L2-scripting.md`. Security/threat model:
  [scripting-security.md](scripting-security.md).

## Appendix: manual `posthastectl` install

`posthaste-wizard ctl install` (above) is the supported path. If you'd rather
not run it — building from source, or working somewhere the wizard can't
reach — `posthastectl` ships with the `posthaste-mcp` package (bun):

- From a checkout: run it with `bun apps/mcp/src/cli.ts …`, or build the
  standalone binary with `just mcp build-cli` (→ `apps/mcp/dist/posthastectl`).
- From a release: download the `PosthasteCTL[Nightly]-<platform>` asset (and
  `SHA256SUMS` to verify it) from the matching GitHub release, mark it
  executable, and place it on your `PATH` yourself — this is exactly what
  `posthaste-wizard ctl install` automates, including the checksum check and
  (macOS) clearing the quarantine flag.

Either way, `posthastectl` still auto-discovers `daemon.json` with no flags —
only the install step differs.
