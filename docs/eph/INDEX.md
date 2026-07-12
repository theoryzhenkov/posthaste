# docs/eph — the SPECial RFC corpus (index)

These are working decision-records for the L2 program: **RFCs** (decision logs
with D-numbered decisions and M-numbered migration steps), **AUDITs** (read-only
evidence), **DESIGN**/**PLAN** notes, a **DEVIATION** reality ledger, and one
**FEASIBILITY** study. The logs are kept as history; this index is the status
layer on top — what SHIPPED, what's PENDING, what's just a study.

Compiled 2026-07-04. Status determined from each doc's own markers and
cross-checked against the commit history (M-step / S-step / slice landings).

## Status vocabulary

- **SHIPPED** — all migration steps landed.
- **MOSTLY SHIPPED** — most landed; a named tail is deferred/queued.
- **IN PROGRESS** — actively landing; some steps done, some pending.
- **DEFERRED / NOT STARTED** — decided but not begun.
- **EVIDENCE/AUDIT** — read-only investigation, not a plan.
- **REFERENCE** — a design note or ledger (may describe realized machinery).

## The map

| Doc | Purpose | Status |
|---|---|---|
| **RFC-L2-architecture-cleanup** | Crate-topology split, typed `MailOperation` vocabulary, frame/id renames, replica/link/outbox trait seams (the M0–M10 refactor). | **SHIPPED** — M0–M10 (incl. M3b) landed; M9 wave complete (V14–V16), M10 done. |
| **DEVIATION-L2-architecture-cleanup** | Reality ledger for the cleanup: one row per drained divergence between spec end-state and code. | **REFERENCE** (ledger) — complete; all 13 rows closed at M8. |
| **RFC-L2-lifecycle-and-errors** | Ordered shutdown/teardown, deadline discipline, bounded growth, watchdog liveness, typed `Terminality` vocabulary + boundary error hygiene. | **SHIPPED** — M20–M31 (incl. M23b) landed; M28 absorbed into scripting S1. |
| **RFC-L2-provider-reliability** | One outbound-call envelope per provider call, send-exactly-once, push-lifecycle repair, IMAP/sync robustness, supervision + P5 fix, OAuth CAS rotation. | **SHIPPED** — M30–M37 all landed. |
| **RFC-L2-scripting** | The scripting/automation surface: the tap, one-vocabulary action path, capability tokens, the rules→MCP ladder, and CLI distribution. | **MOSTLY SHIPPED** — S1–S6 + distribution wave + rulings 21–23 landed. Remaining: Linux AppImage sidecar (DEFERRED), ruling-24 `/commands/send` idempotency. |
| **RFC-L2-client-resilience** | The web client converges from any state: level-triggered self-healing, no silent drops, one reconcile pass, the one reactive store revamp. | **IN PROGRESS** — landed M40, M46, M47, M48. Pending: M41–M45, M49, M50. |
| **RFC-L2-drafts** | The draft lifecycle: send consumes the draft, discard = hard-delete, idempotent identity-stable saves, edit-draft in the action row. | **SHIPPED** — M60, M61, M62 all landed. |
| **RFC-L2-client-replication-model** | The north-star: `visible = fold(base, intent_log)`, one writer per plane, mutations-as-single-intents, verdict as projection — enforced by a Rust type-system `BaseWrite` capability (Tier 1). §6: the base/overlay/effective substrate (D167–D169) — optimism materialized as the fold's *output* into a `message_overlay` table, every SQL read via the `_effective` view; SQL stays the single predicate engine, replica-core the single fold (one engine, two storage backends). Parent of the send-RFC and the test-taxonomy. | **NS1 COMPLETE (2026-07-11)** — overlay substrate + all reads strangled + live counts + the cutover (mutations→overlay, sync writes raw truth, `_protected`/guard deleted, confirmation-gated retire) + the `BaseWrite` compile-time seal (one legacy production grant: draft-discard, dies with NS2). Next: NS2 send-as-intent → NS3 generalize. Tier-2 linters deferred. |
| **RFC-L2-send-draft-state-machine** | NS2: mail operations as typed INTENTS with effects-as-data on the NS1 substrate — two-output intents (fold effects vs execution plan, D171), authority-side materialization (D170), total phase-aware effects (D172), one-row two-step held sends w/ cross-device drafts (D173), `Readiness` clock un-fusing (D152, the nightly P0), `SendOutcome` kills `moved_to_sent` (D154), registry sole authority + coalescing (D153/D174). | **IN PROGRESS — Slices 0–3 + BE-H2 LANDED** (nightly send restored; last audit HIGH closed; `MailIntent` one-decode-boundary; Slice 3: instant drafts via overlay fold, tombstone discards, D174 coalescing + `depends_on` deleted, last `BaseWrite::legacy` grant + `MessageCommandStore` port deleted — sync is the only base writer) — remaining: 4 send-as-one-intent (`moved_to_sent` dies); 5 verdict + SEND-grid tests. |
| **AUDIT-L2-error-taxonomy** | Error-type census + conversion edge list (info-loss flags), retryability/swallowing/panic findings — the M29/D70–D73 worklist input. | **EVIDENCE/AUDIT** — feeds RFC-L2-lifecycle-and-errors (M29 shipped). |
| **AUDIT-L2-imap-sync-scheduling** | IMAP gateway + sync scheduling/supervision robustness: P1 data-loss window, no-timeout connection layer, no supervision, P5 flake verdict. | **EVIDENCE/AUDIT** — feeds RFC-L2-provider-reliability (M34–M36 shipped). |
| **AUDIT-L2-jmap-push** | JMAP engine + push pipeline robustness: S1 duplicate-send, A1 OAuth lockout, PP1 silent push death, F2 timeout monoculture, PP2 reconnect defect. | **EVIDENCE/AUDIT** — feeds RFC-L2-provider-reliability (M32/M33/M37 shipped). |
| **AUDIT-L2-lifecycle-resources** | Re-verified lifecycle/resource debt register + 22 new rows (N1–N22) + watchdog census + top-10. | **EVIDENCE/AUDIT** — feeds RFC-L2-lifecycle-and-errors (M20–M31 shipped). |
| **DESIGN-L2-deployment-topology** | Forward design: productize the deployment topology over the realized link mechanism (control-pane UI, transport selector, native IPC, hardening). | **REFERENCE** (design note) — link mechanism realized; productization partial. |
| **DESIGN-L2-release-channels** | Release channels as a first-class concept: a single per-channel policy table (identity, updater manifest, devtools, signing, smoke gates). | **REFERENCE** (design note) — machinery realized and in use (tools/release/*). |
| **DESIGN-L2-undo-redo-synced-history** | The model: per-account durable server-authoritative reversible-op log, cursor synced as a view, evaluation stays local-optimistic; cross-device undo. | **REFERENCE** (design note) — Phase 1 shipped; Phase 2 implemented (see revlog-contract). |
| **DESIGN-L2-undo-redo-revlog-contract** | Phase 2 implementable contract: rev_log store table + cursor + RevLog synced view; client proposes idempotent cursor moves, server arbitrates. | **REFERENCE** (design contract) — Slices 1–5c landed; remaining JMAP per-message version + e2e. |
| **PLAN-L2-client-link-unification** | Forward plan: finish unifying the client↔runtime link onto assertion-replication (U1 recompute, U3 replica-default, U4 coverage, U5 one BackendApi). | **REFERENCE** (forward plan) — U2 view-deltas landed; U1/U3/U4/U5 open (overlaps client-resilience Part 2). |
| **PLAN-L2-testkit-roadmap** | Roadmap for posthaste-testkit contracts: runtime-in-harness, view-settlement recorder, declarative fixtures, headless driver, property tests. | **MOSTLY SHIPPED** — P0–P3d landed; P4 headless driver + P5 proptest/profiling remain (P5 exemplar landed: `store/src/tests/prune_floor_guard.rs`). |
| **DESIGN-L2-test-taxonomy** | Layered coverage (unit / hermetic-integration / provider-conformance / e2e) + a per-op CONTRACT REGISTRY for send/move/delete/draft, derived from a 10-class invariant backbone × lifecycle fault points. SEND grid fully derived as the template. | **DESIGN — NOT IMPLEMENTED** — SEND grid done; move/delete/draft grids TODO; L2 fault seam + L4 e2e fixtures + coverage-lint to build. |
| **FEASIBILITY-calendar-platform** | Read-only survey: how hard to build a Calendar app on Posthaste's substrate + cross-domain agent automations. Verdict + five named seams. | **EVIDENCE/FEASIBILITY — NOT STARTED** — recommendation: integration shim first. |

## Program at a glance

The core L2 refactor and reliability program **shipped**: architecture-cleanup
(M0–M10), lifecycle-and-errors (M20–M31), provider-reliability (M30–M37), and
drafts (M60–M62) are all landed. Scripting is **mostly shipped** through the
distribution wave, with only the Linux AppImage sidecar deferred and one
send-route idempotency tail queued. The **live front** is client-resilience:
the hotfix (M40) and the reactive-store foundation (M46–M48) landed; the
self-healing tail (M41–M45) and the revamp completion (M49–M50) are pending.
Undo/redo (Phases 1–2) shipped; the testkit roadmap is mostly done. Calendar is
a study only — not started.
