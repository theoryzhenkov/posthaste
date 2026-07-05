---
title: Scripting security & threat model
description: The full threat model behind Posthaste scripting — the threats stated honestly, and the mitigations you own.
sidebar:
  label: Security
  order: 5
---

Posthaste scripting lets server-side events trigger code. That is powerful and
therefore dangerous. This page states the threats honestly and the mitigations
you are responsible for. Read it before you run `watch --exec` or write an
`exec`/`webhook` rule.

## The trust relationships

- **You trust your authority server.** It holds your mail and its root signing
  key. A compromised AS already means compromised mail. Scripting adds one
  marginal risk on top: it can trigger code on machines that watch it (below).
- **You do NOT trust message content.** Anyone can send you email. Any rule
  whose payload includes message content is feeding your handler
  attacker-authored data.
- **The network is trusted only via TLS + a pinned token.** A network attacker
  cannot impersonate your AS. A compromised AS is a different, higher threat.

## Threat 1 — compromised AS triggers your laptop handler

`posthastectl watch --exec ./handler.sh` runs a fixed local script in response
to server events. A compromised AS cannot change which script runs, but it can
send arbitrary payloads to it and trigger it at will.

- **This is RCE only if your handler treats input as code.** The framework
  passes the payload as JSON on **stdin**, never as shell arguments and never
  interpolated into a command. Your handler must do the same: parse the JSON,
  never `eval` it.
- **The `--rule`/`--topic` filters are convenience, not security.** A
  compromised AS can forge any rule name or topic. Do not rely on them to keep
  a handler from firing.
- Consent: `watch --exec` and `register-watch` print a one-time warning that
  they run local code in response to server-controlled events.

## Threat 2 — malicious message content (prompt injection / confused deputy)

**This needs no server compromise.** A rule like "tag:instruct → send the
message to my agent" means _anyone who can make an email match_ can inject
instructions into your agent, which then acts with your minted token.

Mitigations you must apply:

- **Scope the trigger to trusted senders.** The WHEN-clause is the full query
  grammar — use it: `when = "tag:instruct AND from:me@mydomain.com"`. An
  unscoped content rule is an open injection surface.
- **Grant the least token.** The rule's minted token bounds a hijacked agent's
  blast radius. A summarizer gets `grants = ["read"]`. Never grant `apply`/send
  to a rule that feeds untrusted content to an autonomous agent unless you have
  accepted that the agent may be instructed by strangers.
- **Treat agent output as suggestions, not commands**, when the agent read
  untrusted content.

## Threat 3 — `exec` rules are config-file-only, by design

An `exec` action runs a local command on the **authority-server** machine.
It is settable **only** by editing the config-root rules file — never over
REST, never over any GUI. Whoever can write that file already has AS
filesystem access. This invariant is load-bearing: a wire-settable exec action
would be remote code execution on your server. Do not build one.

## Checklist before you ship a rule

- [ ] Is the WHEN-clause scoped to senders/mailboxes you trust?
- [ ] Is the token grant the minimum the action needs?
- [ ] Does the handler parse the payload as data and never execute it?
- [ ] If it feeds an agent, have you accepted the prompt-injection surface?
- [ ] Is the webhook URL one you control (localhost or your own host)?
