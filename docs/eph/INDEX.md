# docs/eph — the SPECial RFC corpus (index)

These are working decision-records for the L2 program: **RFCs** (decision logs
with D-numbered decisions and M-numbered migration steps), **AUDITs** (read-only
evidence), **DESIGN**/**PLAN** notes, a **DEVIATION** reality ledger, and one
**FEASIBILITY** study. The logs are kept as history; this index is the status
layer on top — what SHIPPED, what's PENDING, what's just a study.

Compiled 2026-07-04; status refreshed 2026-07-18 after the split-model
deletion (the seam crates, `legacy/web`, `legacy/desktop`, and `apps/mcp` are
gone; the pre-deletion tree survives on `legacy/split-model-final`). Status
determined from each doc's own markers and cross-checked against the commit
history (M-step / S-step / slice landings).

Added 2026-07-30: `DESIGN-L2-theming` — the client theming rework, implemented.
Its §4 records where the motivating diagnosis was wrong and §5 lists what was
deliberately left undone; no claim it makes about appearance was observed.

Added 2026-07-31: `DESIGN-L2-window-liveness` — the client window-liveness fix,
implemented. Narrower than the theming rework: three steps, one frontend
composition defect. Its §5 is the important half — the frontend suite has no
DOM, so nothing about React composition or real multi-window behaviour is
tested, and the ownership scheme it lands is a lease rather than an election.

Added 2026-07-30: `AUDIT-L2-architecture-review`. Note that its §2.3 and §2.4
contradict two status rows in this table — `RFC-L2-provider-reliability` is
recorded below as **SHIPPED — M30–M37 all landed**, while D98 (in M36's
contents) still reads `proposed` in that RFC with no implementation; and the
`legacy/split-model-final` recovery branch referenced above no longer exists on
the remote (`git ls-remote --heads origin` returns only `main`).

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
| **RFC-L2-scripting** | The scripting/automation surface: the tap, one-vocabulary action path, capability tokens, the rules→MCP ladder, and CLI distribution. | **SHIPPED, NOW DORMANT** — S1–S6 + distribution wave + rulings 21–23 landed in the split model; the CLI/MCP artifacts (`apps/mcp`, `posthastectl`) were deleted with that stack (2026-07-18). The surface returns as the mirror-client Slice-4 rebuild on the one integration surface. |
| **RFC-L2-client-resilience** | The web client converges from any state: level-triggered self-healing, no silent drops, one reconcile pass, the one reactive store revamp. | **SUPERSEDED (pending tail)** by RFC-L2-mirror-client — M40, M46–M48 landed in the split-model web client (now deleted); the pending tail (M41–M45, M49–M50) will not be implemented. |
| **RFC-L2-drafts** | The draft lifecycle: send consumes the draft, discard = hard-delete, idempotent identity-stable saves, edit-draft in the action row. | **SHIPPED** — M60, M61, M62 all landed. |
| **RFC-L2-client-replication-model** | The north-star: `visible = fold(base, intent_log)`, one writer per plane, mutations-as-single-intents, verdict as projection — enforced by a Rust type-system `BaseWrite` capability (Tier 1). §6: the base/overlay/effective substrate (D167–D169) — optimism materialized as the fold's *output* into a `message_overlay` table, every SQL read via the `_effective` view; SQL stays the single predicate engine, replica-core the single fold (one engine, two storage backends). Parent of the send-RFC and the test-taxonomy. | **NS1 COMPLETE (2026-07-11)** — overlay substrate + all reads strangled + live counts + the cutover (mutations→overlay, sync writes raw truth, `_protected`/guard deleted, confirmation-gated retire) + the `BaseWrite` compile-time seal (one legacy production grant: draft-discard, dies with NS2). NS2 send-as-intent landed (see the send RFC row); NS3 generalize next. Tier-2 linters deferred. |
| **RFC-L2-send-draft-state-machine** | NS2: mail operations as typed INTENTS with effects-as-data on the NS1 substrate — two-output intents (fold effects vs execution plan, D171), authority-side materialization (D170), total phase-aware effects (D172), one-row two-step held sends w/ cross-device drafts (D173), `Readiness` clock un-fusing (D152, the nightly P0), `SendOutcome` kills `moved_to_sent` (D154), registry sole authority + coalescing (D153/D174). | **COMPLETE (Slices 0–5, 2026-07-12)** — nightly send restored; instant drafts + sent rows via the overlay fold; sync is the only base writer; send is ONE intent (D170 materialization, D172 phase-aware multi-row fold, gateway-owned consumption, D154 `SendFiling`, D173 two-step held plan + one-intent undo, adoption + S-CONV-2 filing repair, D175 repair); SEND-grid L2 cells landed. Remaining: L3/L4 confirming rows + BE-H3 client bridge (taxonomy grid). |
| **RFC-L2-mirror-client** | The lightweight-client model: the backend is the ONE evaluator, materializing windowed SURFACES from the NS1 effective views into a versioned per-session state document (D180/D181); the TS client is a dumb mirror (subscribe → patch → render) sending NS2-intent commands (D184); recovery = refetch the screen-sized document (D182); a dirty→coalesce→diff recomputer keeps it cheap (D183, the option-iii lesson solved locally); optimism backend-only and invisible (D185); one hackable integration surface for UI/CLI/MCP (D186); local-first, remote-capable, process unification severable (D187). Retires the client-replica seam (~40–50k LOC + 3 roadmap fronts). | **IN PROGRESS** — realized as the integrated app (`apps/client`: one Rust backend as the sole evaluator, TS frontend as mirror). Seam retirement LANDED 2026-07-18: the split-model stack (14 seam/server crates, `legacy/web`, `legacy/desktop`, `apps/mcp`) is deleted; pre-deletion tree on `legacy/split-model-final`. CLI/MCP are dormant pending their rebuild on the one integration surface (Slice 4); artifact consolidation (Slice 5) open. |
| **RFC-L2-view-membership-negotiation** | One predicate, two evaluators, runtime-owned assignment: capability declaration (D176), membership contract as data (D177), fail-toward-re-serve (D178), compiler-proven user-view predicates (D179). | **SUPERSEDED pre-implementation (2026-07-16)** by RFC-L2-mirror-client — one evaluator makes assignment moot. Slice 0's testkit `MailListMirror` bridge is gone (deleted with the seam, 2026-07-18); Slices 1–5 will not be implemented. |
| **AUDIT-L2-error-taxonomy** | Error-type census + conversion edge list (info-loss flags), retryability/swallowing/panic findings — the M29/D70–D73 worklist input. | **EVIDENCE/AUDIT** — feeds RFC-L2-lifecycle-and-errors (M29 shipped). |
| **AUDIT-L2-imap-sync-scheduling** | IMAP gateway + sync scheduling/supervision robustness: P1 data-loss window, no-timeout connection layer, no supervision, P5 flake verdict. | **EVIDENCE/AUDIT** — feeds RFC-L2-provider-reliability (M34–M36 shipped). |
| **AUDIT-L2-jmap-push** | JMAP engine + push pipeline robustness: S1 duplicate-send, A1 OAuth lockout, PP1 silent push death, F2 timeout monoculture, PP2 reconnect defect. | **EVIDENCE/AUDIT** — feeds RFC-L2-provider-reliability (M32/M33/M37 shipped). |
| **AUDIT-L2-lifecycle-resources** | Re-verified lifecycle/resource debt register + 22 new rows (N1–N22) + watchdog census + top-10. | **EVIDENCE/AUDIT** — feeds RFC-L2-lifecycle-and-errors (M20–M31 shipped). |
| **AUDIT-L2-architecture-review** | Post-pivot review of the integrated app: the account-scoped derive read under the global write lock, four guards enforcing less than they document, the unobservable provider breaker + unpaced poll loop, 647/727 dead `@spec` pointers. Sequenced next steps. | **EVIDENCE/AUDIT** (2026-07-30) — 4 confirmed / 2 contested / 4 **unverified** (§4 is leads only). Successor pass to AUDIT-L2-architecture-health; re-measures its open `@spec` lint row (degraded 473 → 647). |
| **DESIGN-L2-deployment-topology** | Forward design: productize the deployment topology over the realized link mechanism (control-pane UI, transport selector, native IPC, hardening). | **REFERENCE** (design note) — link mechanism realized; productization partial. |
| **DESIGN-L2-release-channels** | Release channels as a first-class concept: a single per-channel policy table (identity, updater manifest, devtools, signing, smoke gates). | **REFERENCE** (design note) — machinery realized and in use (tools/release/*). |
| **DESIGN-L2-theming** | Separate palette / material / compositing in the client theme system: six surface roles supplied as typed per-theme data, `backdrop-filter` confined to the floating tiers (killing the glass-only notifications-panel occlusion by construction), themes forbidden from writing selectors, and completeness gates on both the typed materials and the hand-written palette blocks. | **DESIGN — IMPLEMENTED** (2026-07-30) — all four migration steps landed, one commit each; structural claims mutation-tested, visual claims unverified (never rendered). |
| **DESIGN-L2-window-liveness** | Split the one boolean answering two questions in the client's app root: liveness (the stream subscription that keeps a window's mirror fresh) belongs in EVERY window and is now inseparable from the mirror — one provider creates the `QueryClient` and subscribes it, and no bare client is exported — while ownership of the process-wide OS surfaces (Dock badge, new-mail banners) stops being inferred from window identity and becomes a leased claim that survives its holder closing. | **DESIGN — IMPLEMENTED** (2026-07-31) — three migration steps landed, one commit each; the ownership claim is unit-tested (mutation-checked), React composition and multi-window runtime behaviour are untested (no DOM) and unobserved. |
| **DESIGN-L2-undo-redo-synced-history** | The model: per-account durable server-authoritative reversible-op log, cursor synced as a view, evaluation stays local-optimistic; cross-device undo. | **REFERENCE** (design note) — Phase 1 shipped; Phase 2 implemented (see revlog-contract). |
| **DESIGN-L2-undo-redo-revlog-contract** | Phase 2 implementable contract: rev_log store table + cursor + RevLog synced view; client proposes idempotent cursor moves, server arbitrates. | **REFERENCE** (design contract) — Slices 1–5c landed; remaining JMAP per-message version + e2e. |
| **PLAN-L2-client-link-unification** | Forward plan: finish unifying the client↔runtime link onto assertion-replication (U1 recompute, U3 replica-default, U4 coverage, U5 one BackendApi). | **SUPERSEDED** by RFC-L2-mirror-client — the client↔runtime link it unified is deleted (2026-07-18). U2 view-deltas landed pre-retirement; U1/U3/U4/U5 will not be implemented. |
| **PLAN-L2-testkit-roadmap** | Roadmap for posthaste-testkit contracts: runtime-in-harness, view-settlement recorder, declarative fixtures, headless driver, property tests. | **MOSTLY SHIPPED, PARTLY RETIRED** — P0–P3d landed; runtime-in-harness + the view-settlement recorder were deleted with the split-model runtime (2026-07-18). P4 headless driver is dormant with the CLI; P5 proptest/profiling remains (exemplar landed: `store/src/tests/prune_floor_guard.rs`). |
| **DESIGN-L2-test-taxonomy** | Layered coverage (unit / hermetic-integration / provider-conformance / e2e) + a per-op CONTRACT REGISTRY for send/move/delete/draft, derived from a 10-class invariant backbone × lifecycle fault points. SEND grid fully derived as the template. | **DESIGN — NOT IMPLEMENTED** — SEND grid done; move/delete/draft grids TODO; L2 fault seam + L4 e2e fixtures + coverage-lint to build. |
| **FEASIBILITY-calendar-platform** | Read-only survey: how hard to build a Calendar app on Posthaste's substrate + cross-domain agent automations. Verdict + five named seams. | **EVIDENCE/FEASIBILITY — NOT STARTED** — recommendation: integration shim first. |

## Program at a glance

The core L2 refactor and reliability program **shipped**: architecture-cleanup
(M0–M10), lifecycle-and-errors (M20–M31), provider-reliability (M30–M37), and
drafts (M60–M62) are all landed. The **live front** is the mirror-client
pivot: the integrated app (`apps/client`) is the product and ships as
nightly, and the split-model stack — the seam crates, the legacy web/desktop
apps, and the CLI/MCP (`apps/mcp`) — was deleted on 2026-07-18 (pre-deletion
tree: `legacy/split-model-final`). That deletion superseded the
client-resilience tail, link-unification, and view-membership negotiation
outright; scripting is dormant until its Slice-4 rebuild on the one
integration surface. Undo/redo (Phases 1–2) shipped; the testkit roadmap is
mostly done (its runtime-coupled pieces retired with the seam). Calendar is a
study only — not started.
