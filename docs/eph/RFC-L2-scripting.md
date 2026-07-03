---
scope: L2
summary: "RFC — the Posthaste scripting & automation surface: the tap (fact-carrying channel), the one-vocabulary action path with attenuated capability tokens and deterministic idempotency, and the minimal-code ladder from declarative rules to agent-native MCP. Drafted 2026-07-02; DEFERRED — execution scheduled after the architecture-cleanup follow-through."
modified: 2026-07-02
reviewed: 2026-07-02
lifecycle: ephemeral
type: RFC
state: ratified
depends:
  - path: eph/RFC-L2-architecture-cleanup
  - path: architecture/L2-crate-topology
dependents: []
---

# RFC — Scripting & Automation (draft, deferred)

Companion to RFC-L2-architecture-cleanup (decisions D1–D51 there; D52+ here).
Everything below builds on the M9-wave substrate: the node anatomy (topology
§2.1b), the far-end sub-stores, the near-end engine, `MailOperation`/`apply`,
and macaroon capability tokens.

## 1. Motivation & requirements (user-stated, 2026-07-02)

Make Posthaste maximally easy AND powerful to script: subscribe to anything,
do anything (including modifying mail), with users writing as little code as
possible. Canonical story: *"when a message tagged `instruct` arrives, send it
as a prompt to my agent on my VM — and the agent writes back."* Target: that
story costs one declarative rule and one prompt; zero user protocol code.

## 2. The principle: a script is a client without a replica

Scripts mount exactly two things from the node anatomy: the **tap** (facts in)
and the **Api** (actions out). No kernel, no projector, no link, no
convergence obligations. Total consumer state: one seq cursor. No new
protocol, no parallel automation API that can drift from the product surface —
adding a capability to the app adds it to scripting by construction (XIV).

## 3. Channel-kind taxonomy (completes D50)

| Kind | Example | Recovery semantics |
|---|---|---|
| log-carrying | AS `Base` assertions | replay bounded backlog; collapse acceptable |
| state-carrying | view snapshots/deltas | collapse-always (latest wins) |
| **fact-carrying** (new) | `DomainEvent` | **durable replay; collapse is data loss — gap frame instead** |

For facts, history IS the payload. Recovery = replay from a durable log;
when history is truncated past the consumer's cursor, an explicit **gap
frame** (the `Reset` element reinterpreted) tells the consumer to decide —
never a silent drop (fixes audit N8; `docs/api/L1:232` already specifies
this and the code deviates).

## 4. D52 — the tap

| D52 | **The tap: a read-only far-end.** Factor `posthaste-link-far-end` into its down-channel half (registry, seq stamping, replay, Reset) and up-channel half (dedup, settlement sinks); a tap is an instantiation of the down-half alone: `Tap<DomainEvent, TapSubscriberId>` with (a) the ReplayStore backed by a durable, seq-addressed **FactLog port** (a named per-component responsibility — runtime backs it with `event_log`; AS backs it with its store; not an incidental table), and (b) Collapse replaced by the gap frame. Per-component mounts: **runtime tap** = `/v1/events` reborn (same wire shape, shared internals — supersedes the remount half of D51); **AS tap** = new mount, same machinery over its event bus (consumers: server-side automation, ops tooling, provider-derived facts with no runtime running); **client tap** = mountable but unmounted until a consumer is named (XX). Authz: tap subscriptions are macaroon-scoped like every /v1 surface; filters (topic/account/mailbox) compose with token scope. | XIV; VIII (gap frame); XX | drafted |

## 5. The stateless consumer contract

1. **Cursor-only**: the sole consumer state is one opaque seq. Lose it → "now
   onward". (`posthastectl watch` already implements this with a cursor file.)
2. **At-least-once**, dedupe by identity: every event carries `seq` + stable ids.
3. **Snapshot-attach**: Api reads return the event seq as-of-read (a
   consistency token); a level-triggered script reads state via Api, then
   tails the tap from that seq. Gap-free attach, zero server-side per-consumer
   state. (Api change: reads gain an optional `asOfSeq` response field.)
4. Server-side consumer state = one reaper-managed registry entry (M9a TTL
   machinery, sinkless).

## 6. D53 — the action path

| D53 | **One vocabulary, attenuated capability, deterministic idempotency.** Scripts act through the SAME typed surface as the product: `apply(op: MailOperation)`, compose/send, the read Api — no automation-specific mutation API. Safety: per-invocation macaroon attenuation — a rule firing mints a token scoped to exactly the granted actions and context (e.g. read-this-message + reply-this-thread, 1h expiry); minting is offline (macaroon property), no new auth system. Correctness: at-least-once delivery ⇒ script writes must dedupe — `apply` gains an optional idempotency key; rule-driven invocations stamp a deterministic `ClientMutationId = f(rule_id, event_seq)` so redelivery cannot double-execute. **This resolves P8** (the register's missing REST idempotency ledger) with the dedup sub-store that already exists. | XIV; X; least-privilege | drafted |

## 7. The ladder (as little code as possible)

| Level | User writes | Machinery owns |
|---|---|---|
| 0 declarative rule | a rule (settings/TOML) | in-app engine + built-in actions (tag/move/notify) — extends the existing `preview_automation_rule` machinery |
| 1 rule + hook action | a URL (or script path) | `webhook`/`exec` actions: the engine consumes the tap, POSTs `{event, context, scoped-token}`; no cursor/reconnect/auth in user land; hooks receive **facts, never streams** — one event per invocation |
| 2 `posthastectl watch --exec` | one handler function | CLI owns cursor file, reconnect, backoff (exists today) |
| 3 raw tap + Api | a daemon | full protocol: two endpoints and a cursor |
| agent-native (MCP) | a prompt | `apps/mcp` tool surface (read/apply/send) driven by the scoped token |

## 8. Rules run at the authority server

Headless-first: a rule must fire with every client asleep; the AS is
always-on, sees provider-derived facts first, and executes actions through
its own Api surface in-process. Client-side rules (UI-coupled actions) are a
later, separate mount. Rule-action invocations are facts themselves
(`rule.fired` topic) — scriptable and auditable through the same tap.

## 9. Worked example (the requirement, end to end)

Rule: `when message.tagged:instruct → webhook https://vm/agent, grant:
[read-message, reply-thread], expiry: 1h`. Engine matches at the AS, mints
the attenuated token, POSTs event+message+token. The agent (MCP client)
reads the thread and writes back via `apply`/`send` under that token — scope
cannot exceed the grant; the deterministic idempotency key makes redelivery
safe. User code: one rule, one prompt.

## 10. Migration steps (execution deferred)

| Step | Content | Depends |
|---|---|---|
| S1 | Factor link-far-end into down/up halves; extract `Tap` + `FactLog` port | M9 (done) |
| S2 | Runtime tap replaces `/v1/events` internals (wire shape kept; gap frame lands; N8 closes) — absorbs D51's remount half | S1 |
| S3 | AS tap mount | S1 |
| S4 | `apply` idempotency key (P8 fix) + snapshot-attach `asOfSeq` on reads | — |
| S5 | Rule engine actions (webhook/exec) + per-invocation token minting + `rule.fired` facts | S2, S4 |
| S6 | `apps/mcp` tool surface completion (read/apply/send under scoped tokens) | S4 |
| — | Client tap: deferred until a consumer is named (XX) | — |

Note: D51's **delete** half (sessionless views pair + riders) is NOT part of
this RFC — it stays in the architecture-cleanup track as dead-code removal.

## 11. Open questions (resolve at un-deferral)

1. Name: "tap" vs `ObservationPort` vs other (user's naming call).
2. Does the tap carry the component's own emissions in addition to inbound
   facts? (Working default: yes — inbound + emitted, authz-filtered.)
3. Local transport: unix socket / stdio mount for same-machine scripts
   (transport impl behind the existing seams, not an architecture change) —
   build at S3 or defer?
4. Rule language surface: TOML file vs settings UI vs both; relationship to
   the existing smart-mailbox rule grammar (one grammar? D28's parser is
   reusable).
5. Webhook delivery semantics: retry policy, dead-lettering, and whether
   delivery state is itself a fact stream.

## 7. Rulings (owner, 2026-07-03 — un-deferred for beta)

1. **Q1**: the name is **tap**.
2. **Q2**: a component's tap carries inbound AND its own emitted facts, authz-filtered.
3. **Q3**: unix-socket/local transport deferred past beta (SSE over loopback suffices; pure transport add-on later).
4. **Q4**: ONE grammar for rule WHEN-clauses and smart mailboxes — AND the shared grammar extracts into its own crate (working name `posthaste-query-grammar`): D28's parser (`parse_query_with_scopes`/`ScopeToken` + the tokenizer) moves from domain-service into the new crate; domain-service and the rules engine both consume it. Wasm-purity preserved (it must stay frontier-compatible).
5. **Q5**: webhook delivery = at-least-once, `posthaste-call-policy` BackoffSchedule, bounded attempts, dead-letter AS A FACT (`rule.delivery.failed` on the tap). Delivery state is itself observable.
6. **Beta cut**: slice 1 = S1-S4 + two riders (one-command token-mint UX in posthastectl; a worked laptop-script example shipped as docs + integration test). Slice 2 = S5 levels 0-1 (rules + webhook/exec actions) + S6 minimal MCP trio (read/apply/send). Post-beta: unix socket, client tap, settings-UI rules, level-3 SDK polish.
7. **Slice-1 milestone (owner-corrected 2026-07-03)**: with the FULL DESKTOP APP (PosthasteNightly) running — which embeds posthaste-authority-runtime-server in-process — a laptop script works in <5 minutes without reading source: `posthastectl` auto-discovers the local app, `token mint --grant ...`, `watch --exec ./script`. Durable cursor across script AND app restarts; gap frame on truncation; safe write-back via apply idempotency. Pinned by an e2e test.
7b. **Discovery rider (added by the correction)**: the embedded server writes a well-known discovery file (port + bootstrap capability) into the app's state dir; posthastectl reads it automatically. Without this, "easy" fails at step zero — the injected port/token are webview-only today.
8. **Sequencing**: S1 absorbs lifecycle-M28 (the idle-reaper requirement rides the far-end factoring — one pass over that code).
9. **CLI-distribution rider (slice 2, owner Q 2026-07-03: "will posthastectl be auto-installed?" — today: no, separate artifact)**: bundle posthastectl into the desktop app as a Tauri sidecar (per-platform binaries already built by the release CLI job) + an install-to-PATH affordance (first-run prompt or Settings → Scripting → Install CLI; symlink to ~/.local/bin, fall back to copy; uninstall cleans it). Quickstart updated to lead with it. Until it lands, the quickstart documents the release-artifact download.
10. **Field-test findings (owner, 2026-07-03, live test of nightly.49)**: (a) macOS Gatekeeper rejects the CLI as "damaged" — nightly ships the bare binary unsigned (channel policy's enforce_macos_signing=false covers the app, not the CLI artifact); FIX: ad-hoc codesign minimum (Developer ID + notarization if the identity is available to nightly) in the build-cli release job + package as .tar.gz to preserve the exec bit + MACOS-INSTALL note. (b) **Setup goes through the wizard (owner-ruled)**: posthaste-wizard gains a ctl story — install the binary to PATH (~/.local/bin default), register (shell detection, discovery verification, optional completion), invokable standalone and as the desktop sidecar affordance's engine; supersedes the raw symlink shape of rider 9 — the wizard IS the installer.
11. **Least-default bootstrap (owner field-test finding #2, 2026-07-03, ratified)**: daemon.json's token narrows from full-scope to {mint, tap:read} — reads/watch work out of the box; ALL writes require an explicitly minted token (posthastectl mints transparently against the bootstrap when the user asks for write grants). The full-scope credential stays webview-internal, never written to disk beyond the app's own injection. The slice-1 e2e updates to pin: watch works bootstrap-only; apply WITHOUT a minted token is 401/403.
12. **Watch-as-a-service rider (slice 2)**: `posthaste-wizard ctl register-watch --exec <script> [filters]` renders+installs a USER service unit (launchd/systemd — the wizard's existing unit machinery) around posthastectl watch, for always-on laptop automation between the ad-hoc foreground shape and the in-app rules engine.
13. **Registration semantics (clarified)**: what-to-react-to lives in the watch invocation (slice 1) or the rule's WHEN-clause (slice 2); scripts are pure handlers and never self-register.
