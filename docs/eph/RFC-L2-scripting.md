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
14. **Update ownership (owner Q 2026-07-03: wizard vs per-component — ruled: one updater per HOST-SHAPE)**: desktop machines update via the app's Tauri updater (one update carries app + embedded server + the ctl sidecar once rider 9 lands; ctl install detects app-managed and defers). Headless/self-host updates via `posthaste-wizard update [--check|--yes|--rollback]`: install manifest recorded at install time (component, path, version, channel); channel-latest resolution via the existing release/checksum machinery; service stop → verified swap → start (the wizard owns the units — the only actor that can do this correctly); previous binary kept as .bak; the wizard updates itself in the same pass (download-verify → atomic rename). Opt-in auto-update = a wizard-rendered timer unit running `wizard update --yes` (never a daemon-resident self-updater). Rationale: updater code is the riskiest shipped code — exactly one implementation per host-shape (XIV); per-component self-updaters rejected. Slice-2 scope.
15. **Localhost hooks are first-class (owner Q 2026-07-03)**: webhook URLs may target 127.0.0.1 — the rule file/privileged rule creation IS the host-trust decision; the executor must not block user-configured hosts. SSRF posture re-reviewed when GUI rule creation lands (creation stays Manage-scoped).
16. **GUI rule editing pulled into beta (owner-ruled)**: REST CRUD for rules (built-in + webhook actions ONLY — the exec-is-file-only invariant is load-bearing: GUI/REST-settable exec = RCE) persisting into the config-root rules store; a settings-page editor in the app. Rules clarified: they live at the AUTHORITY SERVER (always-on; in-process for the desktop bundle) — the GUI edits via /v1.
17. **`posthastectl hook serve --exec <script>` (ratified rider)**: a built-in localhost webhook receiver running a script per delivery — the easy listener for GUI-created rules; pairs with register-watch for always-on. Exec-action rules remain the no-listener path for file-managed automation.
18. **Ruling 16 REVISED (owner, 2026-07-03): no GUI — rules are config-file-only for beta.** All rules live in the AS config root's rules file; the exec-RCE tension dissolves (nothing rule-shaped is wire-settable). REST stays read-only list+preview. GUI editing returns, if ever, as a post-beta decision.
19. **Client-machine execution: evaluate centrally, execute at the edge (owner Q 2026-07-03)**: a rule with the `emit` action (matches + fires rule.fired, nothing else) + a client-side `posthastectl watch --topic rule.fired --rule <name> --exec <script>` — pull-based (NAT-friendly, no listener, no inbound port), always-on via register-watch. One evaluator (the AS, the grammar); execution wherever a tap consumer runs. Requires: the Emit action variant (S5, trivial), a --rule filter on watch (mcp follow-up with hook serve). This is also the hook-serve/register-watch pairing: register-watch wraps any long-running posthastectl consumer in a user service unit.
20. **Security posture (owner Q 2026-07-03 — threat model ratified, docs/scripting-security.md)**. Beta-critical, MUST land before .52 ships the scenario:
  (a) **Payload-is-data**: watch --exec + webhook + exec deliver the payload as JSON on stdin, NEVER shell-interpolated / never as a built command. Framework-enforced; a security review of the exec/watch path gates .52.
  (b) **Consent**: watch --exec + register-watch print a one-time warning (runs local code in response to server-controlled events); the wizard's register-watch requires an explicit confirm.
  (c) **Sender-scoped triggers are the documented default**: quickstart rule examples use `from:` scoping; an unscoped content→agent rule is called out as an injection surface. The grammar already supports from: (smart-mailbox parity).
  (d) **Least-grant default**: doc examples grant read-only unless the action needs more; the RCE-via-agent (prompt injection, threat 2) is the headline risk, documented loudly.
  (e) **exec = config-file-only** stays load-bearing (§7.16/18). **Filter flags (--rule/--topic) are convenience, NOT a security boundary** — documented as such.
  Post-beta: handler sandbox profiles; per-rule sender allowlist UX; is fact-signing worth it (low — TLS covers MITM, AS-compromise owns the key anyway).

## North star (owner, 2026-07-03)
"An agent doing something with your message = (1) GUI event configuration, (2) launching a persistent agent on localhost, (3) it communicating via MCP." The three rulings below serve this; every scripting unit is measured against it.

21. **posthastectl IS the SDK (ratified)** — dissolves the "manual types" problem. READS: the exec path exports common fields as env vars (extend the PH_* set: PH_MESSAGE_ID/PH_FROM/PH_SUBJECT/PH_KEYWORDS/PH_ACCOUNT/PH_EVENT_SEQ/PH_TOPIC…), full JSON stays on stdin. WRITES: `posthastectl {tag,move,reply,send,apply}` construct the typed op, auto-derive the idempotency key from the triggering event (PH_EVENT_SEQ + rule), use the injected token — the handler never touches REST, MailOperation, or idempotency math. A two-line bash handler is the whole "script". Secondary rider: generated TS/Python SDK packages from the existing schema for structured power users.
22. **MCP is the agent path (pulled into beta)** — the Posthaste MCP server exposes BOTH (a) the action trio (read_thread/tag/move/reply/send — typed + idempotent by construction) AND (b) a fact subscription (rule.fired / events — the tap surfaced as MCP notifications). A persistent localhost agent connects once and gets trigger + capability. Auth: per-connection scoped token via the mint machinery (least-grant; the threat-model applies — an agent with apply reading untrusted content is the prompt-injection surface). Dissolves both problems for the agent persona (no script, no types).
23. **Safe-actions Automations GUI (ratified — scoped reversal of ruling 18)** — in-app rule creation for tag/move/notify/webhook/emit ONLY; exec stays config-file-only (the RCE invariant is intact — the GUI cannot create the one dangerous action). Kills setup friction for the common case: no config-root hunting, no TOML. The GUI surfaces grant selection + inline least-grant/sender-scope security guidance (the scripting-security.md checklist). Plus `posthastectl init` (detect app → mint → scaffold starter rule+handler) + a recipe gallery as smaller no-reversal wins.
