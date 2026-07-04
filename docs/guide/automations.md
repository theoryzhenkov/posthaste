# Automations

Posthaste can act on your mail automatically. This page walks you from the
zero-code version (click a few things in Settings) up to running your own
script. If your goal is "let my AI agent handle it," read this page for the
foundations, then jump to **[Agents](agents.md)**.

## Start here: what do you want to happen?

Pick the row that matches what you're trying to do. Each points at the section
that does it.

| When mail matches something, you want to… | You need | Effort |
|---|---|---|
| **Tag it, move it, or get notified** | A built-in rule in **Settings → Automations** | None. Anyone can do this. |
| **Run your own logic** (log it, hit an API, do a custom thing) | A **script** the app calls, or a watcher you run | A few lines of shell |
| **Let your AI agent read and handle it** | An agent connection — see **[Agents](agents.md)** | A config block, plus one decision |

A useful way to think about it: a **rule** is the trigger ("when a message
matches this search"). The **action** is what happens next. Built-in actions
(tag / move / notify) need nothing from you. Everything more custom is just a
different action — a webhook, a script, or your agent — wired to the same
trigger.

Two safety habits show up at every level. They're worth learning once, up front:

1. **Scope your trigger to senders you trust.** A rule that fires on
   `from:boss@work.com` is safe. A rule that fires on any message containing a
   word is something a stranger can set off by emailing you. More in
   [Staying safe](#staying-safe).
2. **Give each action the least access it needs.** A rule that only reads mail
   should never hold permission to send mail.

---

## Level 1 — built-in automations (no code)

This is the whole feature for most people. You build a rule in the app; it runs
on the server, so it keeps working **even with every Posthaste window closed**.

### Open the Automations pane

1. Open **Settings**.
2. In the left rail, click **Automations** (the one described "Rules that react
   to your mail — tag, move, notify, webhook").
3. You'll see your rules, or an empty state that says **"No automations yet."**
   Click **New rule**.

### Build a rule

The editor has three parts, top to bottom:

- **Name** — anything you'll recognize later, e.g. `Tag receipts`.
- **When a message matches** — this is your trigger. It's a visual builder for a
  search, using the same search language as smart mailboxes. A trigger like
  `from:billing@myvendor.com subject:invoice` means "from that vendor **and** the
  subject mentions invoice" (terms are combined with AND). Right under it, the
  app reminds you to scope to senders you trust — take that seriously.
- **Then** — this is the action. Pick one from the **Do this** menu:
  - **Add a tag** — tag the message (e.g. `reviewed`).
  - **Move to mailbox** — file it into one mailbox by its id (e.g. `Archive`).
  - **Notify** — raise an in-app notification with a title (and optional body).
    No external call, nothing leaves your machine.
  - **Emit a fact** — announce "this rule fired" without doing anything else. On
    its own it looks like it does nothing; it's the hand-off point for a script
    or agent that's listening. You'll use this in [Level 2](#level-2-run-your-own-script)
    and [Agents](agents.md).
  - **Call a webhook** — POST the message to a web address you choose. Covered
    in Level 2 below.

Tick **Enabled**, click **Save rule**, and it starts firing immediately — no
restart, no waiting.

### A worked no-code example

Say you want every invoice from a known vendor tagged `billing`:

1. **New rule**, name it `Tag vendor invoices`.
2. **When a message matches**: build `from:billing@myvendor.com subject:invoice`.
3. **Then**: **Add a tag**, tag = `billing`.
4. **Enabled**, **Save rule**. Done.

From now on, matching mail is tagged the moment it arrives.

### One thing the app won't let you do (on purpose)

The app can create the **safe** actions above, but it deliberately **cannot**
create a rule that runs a program on your computer. That kind of rule (called
`exec`) is powerful enough to be dangerous if it could be set from a screen, so
it can only be created by editing a file on the machine that runs the server —
never from the app. If you already have such a rule, the Automations pane shows
it as **read-only** with a **config file** badge. This is a safety line, not a
limitation to work around.

---

## Level 2 — run your own script

When the built-in actions aren't enough — you want to log the message, call your
own service, or do something custom — you run your own code. There are two basic
shapes. Pick by where your code lives.

### The two shapes

**Shape A — the app calls your web address (a webhook).**
Your code runs as a small web server. Posthaste POSTs each matching message to
its URL. Good when your handler already lives behind a URL, or you want the app
to reach out.

**Shape B — you run a local program (a watcher or listener).**
Your code is a script on your own machine. You either **watch** for matching
mail and run the script per message, or run a tiny **listener** that catches
webhook deliveries. Good for "everything stays on my laptop," and it needs no
inbound network port.

Both hand your script the **same thing**: the message as JSON on standard input,
plus a set of `PH_*` environment variables (the message id, sender, subject, and
so on) already filled in. And both follow one firm rule: **the message is only
ever data.** It arrives on stdin or in environment variables — it is never
pasted into a command — so a booby-trapped email can't run commands on your
machine. (You still shouldn't `eval` the content yourself.)

### First: get the `posthastectl` command

The examples below use `posthastectl`, Posthaste's command-line helper. It comes
bundled inside the desktop app, and it finds your running app automatically — no
URLs, no tokens to copy. To put it on your `PATH` so you can just type
`posthastectl`, run the setup wizard once:

```sh
posthaste-wizard ctl install
```

That installs it to `~/.local/bin/posthastectl` and prints a ✓/✗ checklist. If a
row fails (for example, the directory isn't on your `PATH`), it tells you exactly
what to fix.

### Hello world: a script that just logs the message

Save this as `handler.sh`. It reads the message JSON on stdin and appends a line
to a log:

```sh
#!/bin/sh
# handler.sh — the full message arrives as JSON on stdin.
# PH_MESSAGE_ID, PH_FROM, PH_SUBJECT are already set in the environment.
printf '%s\t%s\t%s\n' "$PH_FROM" "$PH_SUBJECT" "$PH_MESSAGE_ID" >> ~/mail.log
```

Now run a watcher that executes it once per new message:

```sh
posthastectl watch --exec 'sh ./handler.sh' --cursor ./cursor
```

- `--exec` runs your script per matching message.
- `--cursor` remembers where you left off in a small file, so if the watcher (or
  the app) restarts, it resumes instead of re-reading everything.
- The first time you run this, it prints a one-time warning that it runs your
  local code in response to server events. That's expected — you're the one who
  wrote the script.

You can narrow what fires the watcher with `--account`, `--mailbox`, or
`--keyword`. (These are conveniences for what your script *sees* — they are not a
security fence. Keep the real gate in the rule's trigger.)

### Writing back: tag or reply from a script

`posthastectl` is also the write tool, so your handler doesn't have to speak any
protocol. Inside a handler, the message you're acting on is already in the
environment, so these are complete, one-line actions:

```sh
posthastectl tag   --message "$PH_MESSAGE_ID" --add reviewed
posthastectl move  --message "$PH_MESSAGE_ID" --to-mailbox archive
posthastectl reply --message "$PH_MESSAGE_ID" --body "Got it, thanks!"
posthastectl send  --to a@example.com --subject "Heads up" --body "..."
```

You don't have to worry about the same message being handled twice (which can
happen after a restart) — these write commands make repeat runs safe
automatically.

> One caveat to know about `reply` and `send`: today, a rare double-delivery of
> the same trigger could send the reply twice (unlike `tag`/`move`, which are
> fully de-duplicated). It's uncommon, but if a duplicated reply would be
> embarrassing, keep that in mind. Tagging and moving have no such caveat.

### The webhook shape, and the easy listener

If you'd rather have the **app call you** (Shape A), use a **Call a webhook**
action instead of a watcher. Point it at a URL, and Posthaste POSTs each matching
message there with a fresh, limited access key scoped to just that message.

You still need *something* listening at that URL. You don't have to write a web
server — `posthastectl` has one built in:

```sh
posthastectl hook serve --exec ./handler.sh --port 8787 --path /hook
```

Then set the rule's webhook URL to `http://127.0.0.1:8787/hook`. Every delivery
runs `handler.sh` with the exact same contract as a watcher: message JSON on
stdin, `PH_*` variables set, and a ready-to-use access token in the environment.
The listener only accepts connections from your own machine.

### Which shape should I use?

- **Just my laptop, nothing inbound** → a watcher (`watch --exec`), or `emit` +
  a watcher if you want the trigger managed in the app. This is the simplest.
- **The app should call out to a URL I run** → a **webhook** rule, plus
  `hook serve` as the listener.
- **My handler already lives at a public URL** → a **webhook** rule pointed
  straight at it.

---

## Staying safe

Automations turn incoming mail into actions. That's the whole point — and it's
exactly why a few habits matter. Here are the three a human actually needs. For
the full reasoning, see the
[scripting security & threat model](../scripting-security.md).

1. **Scope your triggers to senders you trust.** Anyone can email you, so any
   rule that a stranger's message can match is a rule a stranger can trigger. Add
   a `from:` term for someone you trust (`from:you@yourdomain.com`,
   `from:boss@work.com`). The Automations pane nudges you to do this for a
   reason.

2. **Give each action the least access it needs.** When a rule calls a webhook or
   an agent, it mints a temporary access key. Grant only what the handler
   actually uses. A summarizer needs `read` and nothing else. Every extra
   permission is something a hijacked handler could misuse. In the webhook
   editor, the grants default to `read`, and if you add more — or point at a
   non-local address — the app shows a prompt-injection warning. That warning is
   worth reading.

3. **Treat message content as untrusted.** Anything that runs a local script or
   feeds an AI an email can be abused if the email was crafted to abuse it. Your
   handler should parse the message as data and never execute any part of it. If
   you feed mail to an autonomous agent that can also write or send, understand
   that a crafted message could try to steer it — which is the whole subject of
   the [Agents](agents.md) page.

---

## Keeping it running and up to date

**Keep a watcher alive across reboots.** A watcher you start in a terminal stops
when you close the terminal. To make it a background service that restarts
itself, wrap it with the wizard:

```sh
posthaste-wizard ctl register-watch \
  --exec 'sh ./handler.sh' --cursor ./cursor --name mylogger
```

It installs a proper user service (systemd on Linux, launchd on macOS), asks you
to confirm once (because it runs local code in response to server events), and
never needs `sudo`. Remove it later with
`posthaste-wizard ctl unregister-watch --name mylogger`, and list what's
registered with `posthaste-wizard ctl status`. The same command wraps a webhook
listener with `--serve-hook ./handler.sh --port 8787` instead of `--exec`.

**Update the tools.** The desktop app updates itself. If you installed
`posthastectl` or the wizard on their own (a headless or self-hosted setup),
update them in one line:

```sh
posthaste-wizard update --check   # show what's out of date, change nothing
posthaste-wizard update --yes     # download, verify, and swap in the new versions
```

You can roll back a bad update with `posthaste-wizard update --rollback
<component>`, or opt into a daily auto-update with
`posthaste-wizard update --install-timer`.

---

Ready to plug in an AI agent? That's the next page: **[Agents](agents.md)**.
