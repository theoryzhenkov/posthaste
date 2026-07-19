---
title: Scripting security & threat model
description: The threat model behind Posthaste scripting — the trust boundary, the threats stated honestly, and the mitigations you own.
sidebar:
  label: Security
  order: 5
---

Posthaste scripting lets mail trigger code. That is powerful and therefore
dangerous. This page states the threats honestly and the mitigations you are
responsible for. Read it before you run `watch --exec` or connect an agent.

## The trust relationships

- **Everything runs on your machine.** The Posthaste app embeds its backend
  in-process and serves one API on a loopback port. There is no server of
  ours, no account in a cloud — the trust boundary is your own computer.
- **Possession of `connection-info.json` is the boundary.** On launch the
  backend writes the port and a session secret to a discovery file in its
  state root, owner-readable only (`0600`), and rotates the secret every
  launch. Anything that can read that file — or the `POSTHASTE_TOKEN`
  environment you exported — is "logged in" as you.
- **The secret grants the full surface — there is no lesser grant.** Scoped,
  expiring capability tokens ("this script may only read", "this agent may
  only touch account X") are specified in the API design and are staged
  work. **They are not implemented.** Today every holder of the session
  secret can read, write, and send across every account. This is stronger
  than it sounds: it means "give the summarizer a read-only token" is not an
  available mitigation yet, and every mitigation below has to work without it.
- **You do NOT trust message content.** Anyone can send you email. Any
  automation whose input includes message content is feeding your handler
  attacker-authored data.

## Threat 1 — a booby-trapped email tries to run code via your watcher

`posthastectl watch --exec 'sh ./handler.sh'` runs a fixed local command per
matching message. A stranger's email cannot change _which_ command runs, but
it fully controls the _content_ your handler receives.

- **The watcher never interpolates the payload into a command line.** The
  message detail reaches your handler only as JSON on **stdin** and as
  `PH_*` environment variables (`PH_MESSAGE_ID`, `PH_ACCOUNT_ID`,
  `PH_KEYWORDS`, ...). Your handler must keep that discipline: parse the
  JSON as data, never `eval` it, never paste fields into a shell command.
- **The `--account`/`--mailbox`/`--keyword` filters are convenience, not a
  security fence.** They narrow what your handler sees; they do not make the
  content it sees trustworthy.

## Threat 2 — malicious content steers an agent (prompt injection)

**This needs no compromise of anything.** If an automation feeds mail to an
AI agent, anyone who can make a message match the trigger can inject
instructions into the agent — and the agent acts with the full session
secret: it can reply, send, and delete as you.

Mitigations you must apply, because token scoping cannot save you yet:

- **Gate autonomous triggers on something only you control.** A tag you
  apply yourself (`--keyword todo`) means a stranger's mail can never start
  an agent turn. An unscoped content trigger is an open injection surface.
- **Use host-side tool allow-lists.** If your agent host can restrict which
  MCP tools a session may call, give a summarizer only the read tools.
  That is host policy, not enforcement in Posthaste — but it is real
  friction against an injected "now send this to…".
- **Treat agent output as suggestions, not commands**, whenever the agent
  has read untrusted content. Keep a human between "the agent drafted a
  reply" and "the reply was sent" unless you have deliberately accepted the
  risk.

## Threat 3 — the secret leaks off the machine

The session secret is only as local as you keep it.

- **Never put the token on a command line.** `posthastectl` has no `--token`
  flag by design — argv is visible in process listings and shell history.
  It also never echoes the token into logs or error output.
- **Never copy the token into a config file, a repo, or another machine.**
  If you tunnel the API somewhere, you have extended the trust boundary to
  everything on the far end.
- Rotation limits the damage window: the secret changes on every app
  launch, so a leaked token dies with the session.

## What Posthaste deliberately does not do

- **Built-in rules cannot execute code or call the network.** Rule actions
  are mail actions only (tag, flag, read state, move). Custom logic always
  means _you_ run the code, on your machine, as a watcher — so "a stranger's
  email matched my rule" can never become "a stranger's email ran a program"
  without you writing that program.
- **The event stream keeps no per-client state and replays nothing.**
  A watcher that was down missed those events; it catches up through
  queries. There is no stored ledger of triggers for an attacker to mine.

## Checklist before you ship an automation

- [ ] Is the trigger scoped to something you control (your own tag) or to
      senders you trust?
- [ ] Does the handler parse the payload as data and never execute or
      interpolate it?
- [ ] If it feeds an agent: have you accepted that the agent holds the
      **full** mail surface, and restricted its tools host-side?
- [ ] Is the token still only in the discovery file / environment — not in
      argv, a config file, or a repo?
