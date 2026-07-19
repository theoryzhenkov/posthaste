---
title: Automations
description: From zero-code built-in rules to running your own script — how to make Posthaste react to your mail.
sidebar:
  order: 2
---

Posthaste can act on your mail automatically. This page walks you from the
zero-code version (click a few things in Settings) up to running your own
script. If your goal is "let my AI agent handle it," read this page for the
foundations, then jump to **[Agents](agents.md)**.

## Start here: what do you want to happen?

Pick the row that matches what you're trying to do. Each points at the section
that does it.

| When mail matches something, you want to…                      | You need                                          | Effort                            |
| -------------------------------------------------------------- | ------------------------------------------------- | --------------------------------- |
| **Tag it, file it, flag it, or mark it read**                  | A built-in rule in **Settings → Automations**     | None. Anyone can do this.         |
| **Run your own logic** (log it, hit an API, do a custom thing) | A **watcher** script on your machine              | A few lines of shell              |
| **Let your AI agent read and handle it**                       | An agent connection — see **[Agents](agents.md)** | A config block, plus one decision |

A useful way to think about it: a **rule** is the trigger ("when a message
matches this search"). The **action** is what happens next. Built-in actions
(tag / move / flag / mark read) need nothing from you. Everything more custom
is a watcher — your own script reacting to the same mail, often using a rule's
tag as the hand-off.

Two safety habits show up at every level. They're worth learning once, up
front:

1. **Scope your trigger to senders you trust.** A rule that fires on
   `from:boss@work.com` is safe. A rule that fires on any message containing a
   word is something a stranger can set off by emailing you. More in
   [Staying safe](#staying-safe).
2. **Treat message content as data, never as instructions.** Especially once a
   script or an agent is reading it.

---

## Level 1 — built-in automations (no code)

This is the whole feature for most people. You build a rule in the app, and it
runs inside the app's own backend the moment matching mail arrives.

### Open the Automations pane

1. Open **Settings**.
2. In the left rail, click **Automations**.
3. You'll see your rules, or an empty state. Click **New rule**.

### Build a rule

The editor has three parts, top to bottom:

- **Name** — anything you'll recognize later, e.g. `Tag receipts`.
- **When a message matches** — this is your trigger. It's a visual builder for
  a search condition, using the same query language as smart mailboxes. A
  condition like `from:billing@myvendor.com subject:invoice` means "from that
  vendor **and** the subject mentions invoice" (terms are combined with AND).
  Scope it to senders you trust — take that seriously.
- **Then** — the actions. Pick one or more:
  - **Add a tag** / **Remove a tag** — tag the message (e.g. `billing`), or
    strip a tag.
  - **Mark read** / **Mark unread**.
  - **Flag** / **Unflag**.
  - **Move to mailbox** — file it into one mailbox (e.g. `Archive`).

A **preview** shows which of today's messages the condition would match, so
you can check the trigger before anything fires. Rules can also **backfill** —
apply to the mail you already have, not just what arrives next.

Save the rule and it starts firing immediately — no restart, no waiting. The
rule's writes go through the exact same command path as your own clicks, so
everything a rule does shows up in the pending-operations view like any other
change.

### A worked no-code example

Say you want every invoice from a known vendor tagged `billing`:

1. **New rule**, name it `Tag vendor invoices`.
2. **When a message matches**: build `from:billing@myvendor.com subject:invoice`.
3. **Then**: **Add a tag**, tag = `billing`.
4. **Save**. Done.

From now on, matching mail is tagged the moment it arrives.

### What built-in rules deliberately don't do

A rule's actions are mail actions — tag, flag, read state, move. A rule
**cannot run a program or call out to the network**. That's a safety line, not
a gap: a rule that could execute code would turn "a stranger's email matched
my rule" into "a stranger's email ran something." When you want custom logic,
_you_ run the code, on your machine, as a watcher — next section — and the
rule's tag is the hand-off.

---

## Level 2 — run your own script

When the built-in actions aren't enough — you want to log the message, call
your own service, or do something custom — you run a **watcher**: a script on
your own machine that `posthastectl` runs once per matching message.
Everything stays on your laptop, and nothing needs an inbound network port.

### First: get the `posthastectl` command

The examples below use `posthastectl`, Posthaste's command-line tool. It comes
bundled inside the desktop app and as a standalone release download, and it
finds your running app automatically — no URLs, no tokens to copy. Install
details are in the
[scripting quickstart](scripting-quickstart.md#install-posthastectl).

### Hello world: a script that just logs the message

Save this as `handler.sh`. It reads the message JSON on stdin and appends a
line to a log:

```sh
#!/bin/sh
# handler.sh — the full message detail arrives as JSON on stdin.
# PH_MESSAGE_ID, PH_ACCOUNT_ID, PH_KEYWORDS are already set in the environment.
printf '%s\t%s\n' "$PH_MESSAGE_ID" "$PH_KEYWORDS" >> ~/mail.log
```

Now run a watcher that executes it once per new message:

```sh
posthastectl watch --exec 'sh ./handler.sh'
```

You can narrow what fires the watcher with `--account`, `--mailbox`, or
`--keyword`, or widen it to every change with `--all-updates`. (These narrow
what your script _sees_ — they are not a security fence.)

One firm rule holds throughout: **the message is only ever data.** It arrives
on stdin or in environment variables — it is never pasted into a command — so
a booby-trapped email can't run commands on your machine. (You still shouldn't
`eval` the content yourself.)

### The hand-off pattern: a rule tags, your watcher acts

The cleanest way to wire "when _this specific thing_ happens, run my script"
is to let a built-in rule do the matching and use its tag as the signal:

1. In **Settings → Automations**, create a rule: when
   `from:billing@myvendor.com subject:invoice`, **add tag** `needs-filing`.
2. On your machine, watch for that tag:

```sh
posthastectl watch --keyword needs-filing --exec 'sh ./file-invoice.sh'
```

The rule's condition — evaluated in the app, with the full query grammar — is
the real gate; the watcher just reacts to its verdict. This also means the tag
is visible in the app, so you can always _see_ what your automation selected.

### Writing back: tag, move, or reply from a script

`posthastectl` is also the write tool, so your handler doesn't have to speak
any protocol. The watcher exports the triggering message as `PH_MESSAGE_ID`
and `PH_ACCOUNT_ID`, so inside a handler these are complete, one-line
actions:

```sh
posthastectl tag "$PH_MESSAGE_ID" --account-id "$PH_ACCOUNT_ID" --add reviewed --id "reviewer:$PH_MESSAGE_ID"
posthastectl move "$PH_MESSAGE_ID" --account-id "$PH_ACCOUNT_ID" --mailbox-ids <archiveMailboxId> --id "filer:$PH_MESSAGE_ID"
posthastectl reply "$PH_MESSAGE_ID" --account-id "$PH_ACCOUNT_ID" --body "Got it, thanks!" --id "acker:$PH_MESSAGE_ID"
posthastectl send --account-id "$PH_ACCOUNT_ID" --to a@example.com --subject "Heads up" --body "..."
```

(`move` takes mailbox **ids** — look them up once with
`posthastectl mailboxes list`.)

The `--id` is the safety net: every write carries an idempotency id, and
retrying the **same id** applies once. Derive it from the trigger (as above,
from `$PH_MESSAGE_ID`) and a handler that runs twice for the same message —
a manual re-run, a catch-up pass — tags or replies only once.

### Restarts: catch up with a query, then follow

A watcher only sees events while it's running — missed events are not
replayed. The robust shape is **reconcile, then follow**: on startup, query
for the backlog and handle it, then start the watch for the live tail. The
[scripting quickstart](scripting-quickstart.md#restart-semantics-at-most-once-reconcile-via-queries)
shows the exact two-command pattern. With trigger-derived `--id`s, overlap
between the two phases is harmless.

---

## Staying safe

Automations turn incoming mail into actions. That's the whole point — and it's
exactly why a few habits matter. Here are the three a human actually needs.
For the full reasoning, see the
[scripting security & threat model](scripting-security.md).

1. **Scope your triggers to senders you trust.** Anyone can email you, so any
   rule that a stranger's message can match is a rule a stranger can trigger.
   Add a `from:` term for someone you trust (`from:you@yourdomain.com`,
   `from:boss@work.com`).

2. **Treat message content as untrusted.** Anything that runs a local script
   or feeds an AI an email can be abused if the email was crafted to abuse it.
   Your handler should parse the message as data and never execute any part of
   it. If you feed mail to an autonomous agent that can also write or send,
   understand that a crafted message could try to steer it — which is the
   whole subject of the [Agents](agents.md) page.

3. **Know what your script holds.** Today a script talks to Posthaste with the
   app's session token, which grants the full mail surface — reading _and_
   writing, every account. Scoped, expiring tokens ("this handler may only
   read") are designed and on the way, but until they land, only run handlers
   you'd trust with your mailbox, because that is literally what you're doing.

---

## Keeping it running

A watcher you start in a terminal stops when you close the terminal. To keep
one alive across reboots, run it under your OS's **user** service manager —
no `sudo`, no daemons of ours:

- **Linux**: a `systemd --user` unit with
  `ExecStart=%h/.local/bin/posthastectl watch --keyword needs-filing --exec 'sh %h/bin/handler.sh'`
  and `Restart=on-failure`, enabled with `systemctl --user enable --now`.
- **macOS**: a launchd LaunchAgent in `~/Library/LaunchAgents` with
  `KeepAlive` set, loaded with `launchctl load`.

Point the unit's environment at a custom `POSTHASTE_STATE_ROOT` if you use
one; otherwise discovery works exactly as it does in your terminal.

---

Ready to plug in an AI agent? That's the next page: **[Agents](agents.md)**.
