---
title: Plug your AI agent into Posthaste
description: Connect your AI agent over MCP — the one thing everyone gets wrong, and a full worked example.
sidebar:
  label: Plug in an agent
  order: 3
---

This is the part people are most excited about — and the part most people get
subtly wrong the first time. So we'll name the confusion up front, clear it up,
and then walk one complete example end to end.

If you haven't read [Automations](automations.md), skim its decision guide
first — an agent is just one more kind of action wired to a trigger.

## The one thing everyone gets wrong

> **Connecting your agent gives it the tools to act. It does not, by itself,
> wake your agent up when mail arrives.**

Read that twice. Here's the picture.

There's a standard way to connect an AI agent to an app called **MCP**. When
you connect Posthaste to your agent over MCP, your agent gets **tools** — it
can now read your mail, tag messages, move them, and reply. This is the
**capability**.

What it does _not_ get is a reason to act. An agent host — think a desktop
chat app — sits idle until something starts a turn: usually you, typing. So
mail can be landing, and your agent just… sits there. Nothing is broken.
**Having the tools is not the same as waking up.**

A helpful mental model: connecting over MCP hands your agent a **toolbox**. It
can pick up any tool the moment you ask it to. But nobody rings its doorbell
when mail arrives — unless you wire that up, which is exactly what the rest of
this page does.

So the real question is: **what wakes the agent?**

## Which is right for me?

There are two working shapes. Pick by how you want the agent to start.

### Shape 1 — You drive it ("triage my inbox")

You connect the MCP tools to your agent host and **you** start the
conversation: "Summarize my unread mail," "Reply to that invoice." The agent
uses the tools on demand. Nothing needs to wake automatically because _you_
are the trigger.

This is the dead-simple path, and it's the next section.

### Shape 2 — Mail wakes the agent ("act automatically when it arrives")

You want a message to _trigger_ the agent with no human in the loop. For that,
something has to do the waking:

- **A watcher runs your agent.** `posthastectl watch --exec` runs a handler
  per matching message, and the handler invokes your agent. This is the most
  reliable path and the one we walk in full below.
- **An always-on agent loop.** A small program you keep running that tails
  Posthaste's event stream and treats each event as a task. This is the
  elegant "it just reacts" story — but only because you built the loop that
  consumes events as work. It doesn't come for free from a stock chat app.

The rest of this page covers all three: the simple driven path, the
watcher-wakes-the-agent path, and the always-on loop — then one worked
example.

---

## The simple path: an MCP-capable agent host

If you use an agent host that speaks MCP (many desktop AI apps do), this is
all it takes to give it the mail tools.

Add Posthaste to your host's MCP server configuration:

```json
{
  "mcpServers": {
    "posthaste": {
      "command": "posthastectl",
      "args": ["mcp"]
    }
  }
}
```

- `command` is the path to `posthastectl` — the same single binary as the CLI.
  Use the copy on your `PATH`, or the one bundled inside the desktop app (on
  macOS: `/Applications/Posthaste.app/Contents/MacOS/posthastectl`).
- `args` is just `["mcp"]` — that starts the MCP server on stdio.

Your agent finds your running Posthaste on its own — the server reads the
app's `connection-info.json` discovery file, so there are no URLs and no
tokens to paste. If the app isn't running, the tools say so instead of
hanging.

### What tools the agent gets

Once connected, your agent can:

- **Read your mail** — `list_accounts`, `list_mailboxes`, `list_messages`,
  `search_messages` (the full query grammar: `from:`, `is:unread`, `tag:`,
  free text), `get_message` for a full message, `get_thread` for a whole
  conversation, and `get_blob` for attachment bytes.
- **See what's in flight** — `list_pending_operations` shows queued writes
  and their settlement verdicts, so the agent can check that its own actions
  landed.
- **Tag** a message (`set_keywords` — tags, read state, and flags are all
  keywords).
- **Move** a message to a mailbox (`move_to_mailbox`).
- **Reply** in-thread (`reply`) — the agent gives it the message and the reply
  body, and it works out the recipient, subject, and threading itself.
- **Send** a new message (`send_message`), **manage mailboxes**
  (`create_mailbox`, `rename_mailbox`, `delete_mailbox`), and **trigger a
  sync** (`trigger_sync`).

Read tools are annotated read-only (MCP's `readOnlyHint`), so a host that
distinguishes safe from unsafe tools can show them accordingly. Every write
tool carries a client idempotency id under the hood, so a retried tool call
never double-applies.

### Give it the least access — honest version

An agent reads mail — and anyone can email you, so some of what it reads is
written by strangers. An agent that _also_ holds write or send capability
**and** reads untrusted mail is the classic prompt-injection risk: a crafted
message can try to instruct the agent, which then acts as you.

Here's the honest part: **today the MCP connection carries the app's session
secret, which grants the full surface — read and write, every account.**
Scoped, expiring capability tokens ("this agent may only read") are specified
in the API design and are staged work, not implemented yet. Until they land:

- Connecting an agent means trusting it — and its model — with your mailbox.
- If your agent host supports **per-tool allow-lists**, use them: a summarizer
  needs only the read tools. That's host-side policy, not enforcement in
  Posthaste, but it's real friction against an injected instruction.
- For anything autonomous, **gate the trigger on something only you control**
  (like a tag you apply yourself — see the worked example), so a stranger's
  mail can never start an agent turn in the first place.

Before you let an autonomous agent write or send, read **threat 2 (prompt
injection)** in the [security guide](scripting-security.md).

---

## The watcher path: mail wakes the agent

This is usually the easiest way to get automatic reactions. A
`posthastectl watch --exec` watcher runs your handler per matching message;
the handler invokes your agent; the agent's output is written back with
`posthastectl`. The agent doesn't even need MCP for this — the handler feeds
it the message and handles the write-back itself.

The full worked example is below. The shape:

```
mail arrives → watch matches it → handler runs → agent thinks → posthastectl writes back
```

No inbound network port, no always-on connection to babysit — the watcher is
the only long-lived piece, and it's a few flags.

## The always-on agent loop

If you want a persistently-connected agent that reacts on its own, you run a
small program that stays up, tails the event stream, and treats each event as
work: read `posthastectl events` (NDJSON, one `{generation, event}` object
per line), and on a relevant event, run one agent turn — over MCP tools or
plain `posthastectl` calls — to fetch the message, decide, and act.

Two things your loop must respect, because the stream is honest about them:

- **Events are prompts, not a ledger.** On any doubt (a reconnect, a fresh
  `runId`), reconcile through queries instead of assuming you saw everything.
- **No replay.** Events missed while your loop was down are gone; catch up
  with a query on startup, exactly like a watcher restart.

If building and babysitting a long-running loop sounds like more than you
want, use the watcher path — it gets you automatic reactions with the restart
semantics already handled.

---

## Worked example, end to end

**Goal:** _When I tag an email `todo`, my agent reads it and replies with a
short summary._

We'll use the watcher path. The trigger is a tag **you apply yourself** —
that's doing real security work: a stranger can't tag your mail, so a
stranger's email can never wake the agent. Only messages you deliberately
hand over get processed.

### Step 1 — Write the handler

Save this as `summarize.sh`. It gets the full message detail as JSON on
stdin, asks your agent for a summary, and replies in-thread. Replace
`your-agent` with however you invoke your own agent (a CLI, a script —
anything that reads the message JSON on stdin and prints a summary):

```sh
#!/bin/sh
# summarize.sh — the message detail arrives as JSON on stdin.
# The watcher exports PH_MESSAGE_ID and PH_ACCOUNT_ID for the triggering message.
summary=$(your-agent summarize)   # reads the message JSON on stdin, prints a summary
posthastectl reply "$PH_MESSAGE_ID" --account-id "$PH_ACCOUNT_ID" \
  --body "$summary" --id "summarize:$PH_MESSAGE_ID"
```

That's the whole handler. You don't pass a URL or a token — `posthastectl`
discovers your running app on its own; the triggering message and account
come from the `PH_*` variables the watcher already set. The `--id` derived
from the message makes the reply idempotent: if the handler ever runs twice
for the same message, the summary is sent once.

### Step 2 — Run the watcher

```sh
posthastectl watch --keyword todo --all-updates --exec 'sh ./summarize.sh'
```

- `--keyword todo` fires the handler for messages carrying the `todo` tag.
- `--all-updates` includes tag changes, not just arrivals — you're tagging
  existing mail, so the tagging _is_ the event.
- `--exec` runs your handler once per match, message on stdin.

Tag any message `todo`, and within moments your agent's summary lands as a
reply in the thread.

### Step 3 — Catch up after downtime

Missed events aren't replayed, so start each session (or boot) with a
reconciliation pass before the watch — query for tagged-but-unsummarized
mail and run the handler over it. The idempotent `--id` makes overlap
harmless. The exact pattern is in the
[scripting quickstart](scripting-quickstart.md#restart-semantics-at-most-once-reconcile-via-queries).

### Step 4 — Keep it running

Wrap the watcher in a user service — a `systemd --user` unit (Linux) or a
launchd LaunchAgent (macOS) with restart-on-failure — as described in
[Automations → Keeping it running](automations.md#keeping-it-running). It runs
as you, and never needs `sudo`.

---

## The short version

- Connecting over MCP gives your agent **tools** — capability, not a wake-up.
  A stock host won't act on incoming mail by itself.
- To **drive** the agent yourself: add `posthastectl mcp` to your host's MCP
  config and ask it to do things. Done.
- To make mail **wake** the agent: run a `watch --exec` handler that invokes
  it, or build an always-on loop that consumes the event stream as work.
- **Today the connection grants the full mail surface** — scoped tokens are
  coming, but until then, connecting an agent means trusting it with your
  mailbox. Use host-side tool allow-lists where you can.
- **Gate autonomous triggers on something only you control** (your own tag),
  so strangers' mail can never start a turn.

For the exact commands, the event contract, and the write-back idempotency
story, see the [scripting quickstart](scripting-quickstart.md); for the full
threat model, the [security guide](scripting-security.md).
