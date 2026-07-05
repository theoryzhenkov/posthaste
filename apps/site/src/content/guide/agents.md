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

There's a standard way to connect an AI agent to an app called **MCP**. When you
connect Posthaste to your agent over MCP, two things happen:

- Your agent gets **tools** — it can now read your mail, tag messages, move them,
  and reply. This is the **capability**.
- Posthaste also **pushes a live feed of events** to the connection — including
  "a rule just fired." This _looks_ like it should wake the agent.

Here's the trap: a normal agent host — think a desktop chat app — treats that
event feed as **logging**. It quietly notes the events. It does **not** start
thinking on its own when one arrives. So mail can be landing, events can be
flowing, and your agent just… sits there. Nothing is broken. The events are
arriving. But **delivery is not the same as waking up.**

A helpful mental model: connecting over MCP hands your agent a **toolbox** and
sits it next to a **news ticker**. It can pick up any tool the moment you ask it
to. But it reads the ticker like background noise — it won't act on a headline
unless something tells it "this is a task, go."

So the real question is: **what wakes the agent?**

## Which is right for me?

There are two working shapes. Pick by how you want the agent to start.

### Shape 1 — You drive it ("triage my inbox")

You connect the MCP tools to your agent host and **you** start the conversation:
"Summarize my unread mail," "Reply to that invoice." The agent uses the tools on
demand. Nothing needs to wake automatically because _you_ are the trigger.

This is the dead-simple path, and it's the next section.

### Shape 2 — Mail wakes the agent ("act automatically when it arrives")

You want a message to _trigger_ the agent with no human in the loop. For that,
something has to do the waking. You have two ways:

- **Let the app call your agent.** A rule fires and Posthaste reaches out —
  through a **webhook**, a local **script**, or a **watcher** — and that runner
  starts your agent. This is the most reliable path and the one we walk in full
  below.
- **Run an always-on agent loop.** A small program you keep running that treats
  each incoming event as a task and acts on it. This is the elegant "just
  connect over MCP and it reacts" story — but only because you built the loop
  that consumes events as work. It doesn't come for free from a stock chat app.

The rest of this page covers all three: the simple driven path, the always-on
loop, and the app-calls-your-agent path — then one worked example.

---

## The simple path: an MCP-capable agent host

If you use an agent host that speaks MCP (many desktop AI apps do), this is all
it takes to give it the mail tools.

Add Posthaste to your host's MCP server configuration:

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

- `command` is the path to `posthastectl`. Above is the copy bundled inside the
  macOS app; if you ran `posthaste-wizard ctl install` it's at
  `~/.local/bin/posthastectl`. Any `posthastectl` works.
- `args` is just `["mcp"]` — that starts the MCP server.
- `POSTHASTE_MCP_GRANTS` is what the agent is allowed to do. The default,
  **`tap:read,read`**, means **read-only** (read your mail and receive the event
  feed). Writing is an explicit opt-in — see [Give it the least access](#give-it-the-least-access-below).

Your agent finds your running Posthaste on its own — no URLs, no tokens to paste.

### Tokens refresh themselves — you never manage a key

This matters, so it's called out plainly: **you never copy, paste, or renew an
access token for the agent.** Every time the agent connects, Posthaste mints a
fresh, limited key scoped to exactly the permissions you granted, and uses it for
that session. The key expires on its own; the next connection gets a new one.
There is nothing to rotate and nothing to leak into a config file. The app hands
your agent a limited, auto-renewing key.

### What tools the agent gets

Once connected, your agent can:

- **Read your mail** — list and search conversations and messages, and open a
  full message.
- **Tag** a message (add or remove keywords).
- **Move** a message to a mailbox.
- **Reply** in-thread — you give it the message and the reply body, and it works
  out the recipient, subject, and threading itself.
- **Send** a new message, and **trigger a sync**.

With the read-only default, only the reading tools do anything; the write tools
appear but are refused until you grant more. That's deliberate.

### Give it the least access (below)

An agent reads mail — and anyone can email you, so some of what it reads is
written by strangers. An agent that _also_ holds write or send permission **and**
reads untrusted mail is the classic prompt-injection risk: a crafted message can
try to instruct the agent, which then acts with your key. So:

- **Keep the default `tap:read,read`** unless the agent truly needs to write. A
  summarizer never needs more.
- Only add write permission (`apply`, or specific verbs like `tag`, `move`,
  `send`) once you've accepted that a stranger's email might try to steer the
  agent.
- Always pair an autonomous agent with a **sender-scoped trigger** (below), so
  only mail you trust can set it off.

Before you grant an autonomous agent any write ability, read **threat 2 (prompt
injection)** in the [security guide](scripting-security.md).

---

## The always-on agent loop

If you want the "just connect and it reacts" experience, you run a small program
that stays connected, blocks waiting for events, and treats each one as work to
do. That's the piece a stock chat app is missing — and once it exists, the event
feed genuinely wakes your agent.

You don't have to design it from scratch: the technical
[scripting quickstart](scripting-quickstart.md#reference-a-wake-on-event-agent-loop) describes a
reference wake-on-event loop you can adapt. The shape is: connect over MCP with
your grants, wait for a `rule.fired` event, then run one agent turn with the
tools. To keep that loop alive across reboots, wrap it as a background
service exactly like any watcher (see [Keeping it running](#keeping-it-running)).

If building and babysitting a long-running loop sounds like more than you want,
use the next path instead — it gets you automatic reactions with no loop to
maintain.

---

## The "app calls my agent" path

This is usually the easiest way to get automatic reactions, because Posthaste
does the waking for you. You write a rule; when it fires, the app runs your
handler; your handler invokes the agent.

- **Webhook:** a **Call a webhook** rule POSTs the message to a URL you run. Use
  `posthastectl hook serve` (from [Automations](automations.md#the-webhook-shape-and-the-easy-listener))
  as the listener, and have it invoke your agent.
- **Watcher:** an **Emit a fact** rule announces "I fired," and a
  `posthastectl watch` on your machine runs your handler in response. No inbound
  port needed. This is the path in the worked example below.

Either way, your handler receives the message and a ready-to-use, message-scoped
access token, invokes your agent, and writes the result back with
`posthastectl`.

---

## Worked example, end to end

**Goal:** _When I get an email I've tagged `todo`, my agent reads it and replies
with a short summary._

We'll use the app-calls-your-agent path with an **emit rule + watcher** — it's
fully supported today, needs no inbound network port, and every piece is
copy-paste correct.

### Step 1 — Create the trigger in the app

1. **Settings → Automations → New rule.**
2. Name it `Summarize todo mail`.
3. **When a message matches:** build `tag:todo from:you@yourdomain.com`. The
   `from:` scope is doing real work — it means only mail from _you_ can ever wake
   the agent, so a stranger can't tag their way into your agent. (You tag the
   message `todo` yourself; the rule reacts to your own tagging.)
4. **Then:** choose **Emit a fact**. On its own this just announces that the rule
   fired — your watcher, next, does the rest.
5. **Enabled**, **Save rule**.

### Step 2 — Write the handler

Save this as `summarize.sh`. It gets the full message as JSON on stdin, asks your
agent for a summary, and replies in-thread. Replace `your-agent` with however you
invoke your own agent (a CLI, a script — anything that reads the message on stdin
and prints a summary):

```sh
#!/bin/sh
# summarize.sh — the message arrives as JSON on stdin.
# PH_MESSAGE_ID and the account are already set in the environment.
summary=$(your-agent summarize)          # reads the message JSON on stdin, prints a summary
posthastectl reply --message "$PH_MESSAGE_ID" --body "$summary"
```

That's the whole handler. You don't pass a URL, a token, or an account —
`posthastectl reply` picks up the message and your running app automatically, and
makes the reply safe to retry.

### Step 3 — Run the watcher

```sh
posthastectl watch \
  --topic rule.fired --rule summarize-todo-mail \
  --exec 'sh ./summarize.sh' --cursor ./cursor
```

- `--topic rule.fired --rule summarize-todo-mail` listens for _your_ rule firing,
  rather than for every new message. (Use the rule's id here. If you're unsure of
  it, this can also be a plain `--keyword todo` watch on new mail — the trigger's
  `from:` scope still does the real gating.)
- `--exec` runs your handler once per firing.
- `--cursor` lets it resume cleanly after a restart.

Tag any message `todo` (from yourself), and within moments your agent's summary
lands as a reply in the thread.

### Step 4 — Keep it running

So it survives a reboot, install it as a background service:

```sh
posthaste-wizard ctl register-watch \
  --topic rule.fired --rule summarize-todo-mail \
  --exec 'sh /full/path/to/summarize.sh' --cursor /full/path/to/cursor \
  --name summarize
```

Confirm once when it asks, and you're done. Remove it later with
`posthaste-wizard ctl unregister-watch --name summarize`.

> This handler **replies**, and that's safe on a redelivery: the app
> de-duplicates `reply`/`send` (as it does `tag`/`move`), so a rare
> double-delivery of the trigger still sends the summary only once.

---

## Keeping it running

Everything that has to stay alive — an always-on agent loop, a watcher, a webhook
listener — uses the same wizard command to become a restart-on-failure
background service:

```sh
posthaste-wizard ctl register-watch --exec 'sh ./handler.sh' --name myagent
```

It's a **user** service (no `sudo`), it asks for a one-time confirmation because
it runs local code in response to server events, and `posthaste-wizard ctl
status` lists everything you've registered. Full details, plus updating the
tools, are in [Automations → Keeping it running](automations.md#keeping-it-running-and-up-to-date).

---

## The short version

- Connecting over MCP gives your agent **tools + an event feed** — capability,
  not a wake-up. A stock host logs the events; it doesn't act on them.
- To **drive** the agent yourself: connect over MCP, keep the read-only default,
  ask it to do things. Done.
- To make mail **wake** the agent automatically: either run an always-on loop
  that consumes events as work, or let the app **call** your agent via a
  webhook / script / watcher rule.
- **Tokens refresh themselves** — you never manage a key.
- **Scope triggers to trusted senders** and **grant the least access** — always,
  and especially before letting an autonomous agent write or send.

For the exact tools, the event contract, and the reference loop, see the
[scripting quickstart](scripting-quickstart.md); for the full threat model,
the [security guide](scripting-security.md).
